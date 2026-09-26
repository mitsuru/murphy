//! `Rails/UniqueValidationWithoutIndex` — uniqueness validation needs a unique index.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/UniqueValidationWithoutIndex
//! upstream_version_checked: 2.35.0
//! version_added: "2.5"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND [:validates] gating,
//!   `return unless schema` (no schema → no offense), falsey `uniqueness:`
//!   (`false`/`nil`) and `if:`/`unless:`/`conditions:` exemptions, table lookup
//!   via explicit `self.table_name =` or `tableize`, scope expansion
//!   (`scope: :x`, `scope: [...]`, `.freeze`), `belongs_to` + `foreign_key:`
//!   + polymorphic resolution, unique-index coverage (column-set equality or
//!   expression-index substring), and `add_index` merging. Offense is the whole
//!   `validates` send. File scope (`Include: ['**/app/models/**/*.rb']`) is
//!   enforced via the murphy-rails pack default.yml.
//! ```

use std::collections::HashSet;

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct UniqueValidationWithoutIndex;

const MSG: &str = "Uniqueness validation should have a unique index on the database column.";

#[cop(
    name = "Rails/UniqueValidationWithoutIndex",
    description = "Uniqueness validation should have a unique index on the database column.",
    default_enabled = true,
    options = NoOptions,
)]
impl UniqueValidationWithoutIndex {
    // Mirrors upstream `RESTRICT_ON_SEND = %i[validates]`.
    #[on_node(kind = "send", methods = ["validates"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
        return;
    }
    let Some(schema) = cx.rails_schema() else {
        return;
    };
    let Some(uniqueness) = uniqueness_part(cx, node) else {
        return;
    };
    // Falsey `uniqueness: false/nil` → no offense.
    if is_falsey(cx, uniqueness) {
        return;
    }
    if condition_part(cx, node, uniqueness) {
        return;
    }
    let Some(class_node) = find_class_ancestor(cx, node) else {
        return;
    };
    let table_name = table_name_for(cx, class_node);
    let Some(table) = schema.table_by(&table_name) else {
        return;
    };
    let Some(names) = column_names(cx, node, uniqueness, class_node, table) else {
        return;
    };
    if with_unique_index(table, &names) {
        return;
    }
    cx.emit_offense(cx.range(node), MSG, None);
}

/// Last-arg hash's `uniqueness:` value, if present.
fn uniqueness_part(cx: &Cx<'_>, node: NodeId) -> Option<NodeId> {
    let args = cx.call_arguments(node);
    let last = *args.last()?;
    if !matches!(*cx.kind(last), NodeKind::Hash(_)) {
        return None;
    }
    for pair in cx.hash_pairs(last) {
        let NodeKind::Pair { key, value } = *cx.kind(pair) else {
            continue;
        };
        if let NodeKind::Sym(sym) = *cx.kind(key)
            && cx.symbol_str(sym) == "uniqueness"
        {
            return Some(value);
        }
    }
    None
}

fn is_falsey(cx: &Cx<'_>, id: NodeId) -> bool {
    matches!(
        *cx.kind(id),
        NodeKind::False_ | NodeKind::Nil
    )
}

/// Upstream `condition_part?`: `if:`/`unless:` on the outer `validates` hash,
/// or `if:`/`unless:`/`conditions:` on the `uniqueness:` hash.
fn condition_part(cx: &Cx<'_>, node: NodeId, uniqueness: NodeId) -> bool {
    let args = cx.call_arguments(node);
    let Some(&last) = args.last() else {
        return false;
    };
    if !matches!(*cx.kind(last), NodeKind::Hash(_)) {
        return false;
    }
    if hash_has_keys(cx, last, &["if", "unless"]) {
        return true;
    }
    if !matches!(*cx.kind(uniqueness), NodeKind::Hash(_)) {
        return false;
    }
    hash_has_keys(cx, uniqueness, &["if", "unless", "conditions"])
}

fn hash_has_keys(cx: &Cx<'_>, hash: NodeId, wants: &[&str]) -> bool {
    for pair in cx.hash_pairs(hash) {
        let NodeKind::Pair { key, .. } = *cx.kind(pair) else {
            continue;
        };
        if let NodeKind::Sym(sym) = *cx.kind(key)
            && wants.contains(&cx.symbol_str(sym))
        {
            return true;
        }
    }
    false
}

fn find_class_ancestor(cx: &Cx<'_>, node: NodeId) -> Option<NodeId> {
    for anc in cx.ancestors(node) {
        if matches!(*cx.kind(anc), NodeKind::Class { .. }) {
            return Some(anc);
        }
    }
    None
}

fn table_name_for(cx: &Cx<'_>, class_node: NodeId) -> String {
    if let Some(explicit) = find_set_table_name(cx, class_node) {
        return explicit;
    }
    let full = class_full_path(cx, class_node).unwrap_or_default();
    murphy_plugin_api::rails_schema::tableize(&full)
}

fn find_set_table_name(cx: &Cx<'_>, class_node: NodeId) -> Option<String> {
    let NodeKind::Class { body, .. } = *cx.kind(class_node) else {
        return None;
    };
    let body = body.get()?;
    let mut stack = vec![body];
    stack.extend(cx.descendants(body));
    for id in stack {
        if !matches!(*cx.kind(id), NodeKind::Send { .. }) {
            continue;
        }
        if cx.method_name(id) != Some("table_name=") {
            continue;
        }
        if !cx.is_self_receiver(id) {
            continue;
        }
        let args = cx.call_arguments(id);
        let first = *args.first()?;
        if let Some(v) = literal_string_value(cx, first) {
            return Some(v);
        }
    }
    None
}

fn class_full_path(cx: &Cx<'_>, class_node: NodeId) -> Option<String> {
    let NodeKind::Class { name, .. } = *cx.kind(class_node) else {
        return None;
    };
    let own = cx.const_name(name)?;
    let mut namespaces: Vec<String> = Vec::new();
    for anc in cx.ancestors(class_node) {
        match *cx.kind(anc) {
            NodeKind::Class { name, .. } | NodeKind::Module { name, .. } => {
                if let Some(n) = cx.const_name(name) {
                    namespaces.push(n);
                }
            }
            _ => {}
        }
    }
    namespaces.reverse();
    namespaces.push(own);
    Some(namespaces.join("::"))
}

fn literal_string_value(cx: &Cx<'_>, id: NodeId) -> Option<String> {
    match *cx.kind(id) {
        NodeKind::Sym(sym) => Some(cx.symbol_str(sym).to_owned()),
        NodeKind::Str(sid) => Some(cx.string_str(sid).to_owned()),
        _ => None,
    }
}

/// Resolve the validated column set (base + scope), expanding relations via
/// `belongs_to`. `None` means "cannot resolve" → no offense (upstream returns
/// `nil` from `column_names` when `ret.include?(nil)`).
fn column_names(
    cx: &Cx<'_>,
    node: NodeId,
    uniqueness: NodeId,
    class_node: NodeId,
    table: &murphy_plugin_api::RailsTable,
) -> Option<HashSet<String>> {
    let args = cx.call_arguments(node);
    let first = *args.first()?;
    let base = literal_string_value(cx, first)?;
    let mut names = vec![base];
    if let Some(mut scope_names) = scope_column_names(cx, uniqueness) {
        names.append(&mut scope_names);
    }
    let mut resolved: Vec<String> = Vec::new();
    for name in names {
        match resolve_relation_into_column(cx, &name, class_node, table) {
            Some(cols) => resolved.extend(cols),
            None => return None,
        }
    }
    Some(resolved.into_iter().collect())
}

fn scope_column_names(cx: &Cx<'_>, uniqueness: NodeId) -> Option<Vec<String>> {
    if !matches!(*cx.kind(uniqueness), NodeKind::Hash(_)) {
        return None;
    }
    let scope_value = find_hash_value(cx, uniqueness, "scope")?;
    let scope_value = unfreeze_scope(cx, scope_value);
    match *cx.kind(scope_value) {
        NodeKind::Sym(sym) => Some(vec![cx.symbol_str(sym).to_owned()]),
        NodeKind::Str(sid) => Some(vec![cx.string_str(sid).to_owned()]),
        NodeKind::Array(_) => {
            let mut out = Vec::new();
            for &e in cx.array_elements(scope_value) {
                if let Some(v) = literal_string_value(cx, e) {
                    out.push(v);
                }
                // Non-literal elements are skipped (upstream `array_node_to_array`
                // returns nil → no scope concat; skipping is the safe subset).
            }
            Some(out)
        }
        _ => None,
    }
}

fn find_hash_value(cx: &Cx<'_>, hash: NodeId, want: &str) -> Option<NodeId> {
    for pair in cx.hash_pairs(hash) {
        let NodeKind::Pair { key, value } = *cx.kind(pair) else {
            continue;
        };
        if let NodeKind::Sym(sym) = *cx.kind(key)
            && cx.symbol_str(sym) == want
        {
            return Some(value);
        }
    }
    None
}

/// `[:a, :b].freeze` → the array; otherwise the node itself.
fn unfreeze_scope(cx: &Cx<'_>, id: NodeId) -> NodeId {
    if matches!(*cx.kind(id), NodeKind::Send { .. })
        && cx.method_name(id) == Some("freeze")
        && let Some(recv) = cx.call_receiver(id).get()
    {
        return recv;
    }
    id
}

/// Upstream `resolve_relation_into_column`: direct column wins; else resolve
/// via `belongs_to` (foreign_key or `name_id`), expanding polymorphic to
/// `[fk, name_type]`. `None` when unresolvable.
fn resolve_relation_into_column(
    cx: &Cx<'_>,
    name: &str,
    class_node: NodeId,
    table: &murphy_plugin_api::RailsTable,
) -> Option<Vec<String>> {
    if table.with_column(name) {
        return Some(vec![name.to_owned()]);
    }
    for belongs_to in find_belongs_to(cx, class_node) {
        let args = cx.call_arguments(belongs_to);
        let first = *args.first()?;
        if literal_string_value(cx, first).as_deref() != Some(name) {
            continue;
        }
        let fk = foreign_key_of(cx, belongs_to).unwrap_or_else(|| format!("{name}_id"));
        if !table.with_column(&fk) {
            continue;
        }
        if is_polymorphic(cx, belongs_to) {
            return Some(vec![fk, format!("{name}_type")]);
        }
        return Some(vec![fk]);
    }
    None
}

fn find_belongs_to(cx: &Cx<'_>, class_node: NodeId) -> Vec<NodeId> {
    let NodeKind::Class { body, .. } = *cx.kind(class_node) else {
        return Vec::new();
    };
    let Some(body) = body.get() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut stack = vec![body];
    stack.extend(cx.descendants(body));
    for id in stack {
        if !matches!(*cx.kind(id), NodeKind::Send { .. }) {
            continue;
        }
        if cx.method_name(id) != Some("belongs_to") {
            continue;
        }
        out.push(id);
    }
    out
}

fn foreign_key_of(cx: &Cx<'_>, belongs_to: NodeId) -> Option<String> {
    let args = cx.call_arguments(belongs_to);
    let last = *args.last()?;
    if !matches!(*cx.kind(last), NodeKind::Hash(_)) {
        return None;
    }
    let v = find_hash_value(cx, last, "foreign_key")?;
    literal_string_value(cx, v)
}

fn is_polymorphic(cx: &Cx<'_>, belongs_to: NodeId) -> bool {
    let args = cx.call_arguments(belongs_to);
    let Some(&last) = args.last() else {
        return false;
    };
    if !matches!(*cx.kind(last), NodeKind::Hash(_)) {
        return false;
    }
    for pair in cx.hash_pairs(last) {
        let NodeKind::Pair { key, value } = *cx.kind(pair) else {
            continue;
        };
        if let NodeKind::Sym(sym) = *cx.kind(key)
            && cx.symbol_str(sym) == "polymorphic"
            && matches!(*cx.kind(value), NodeKind::True_)
        {
            return true;
        }
    }
    false
}

fn with_unique_index(
    table: &murphy_plugin_api::RailsTable,
    names: &HashSet<String>,
) -> bool {
    table.indices.iter().any(|idx| {
        if !idx.unique {
            return false;
        }
        if idx.columns.iter().collect::<HashSet<_>>() == names.iter().collect::<HashSet<_>>() {
            return true;
        }
        if let Some(expr) = &idx.expression {
            return names.iter().all(|c| expr.contains(c));
        }
        false
    })
}

#[cfg(test)]
mod tests {
    use super::UniqueValidationWithoutIndex;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn does_nothing_without_schema() {
        test::<UniqueValidationWithoutIndex>().expect_no_offenses(indoc! {r#"
            class User < ApplicationRecord
              validates :account, uniqueness: true
            end
        "#});
        test::<UniqueValidationWithoutIndex>().expect_no_offenses(indoc! {r#"
            class User < ApplicationRecord
              validates :name, presence: true
            end
        "#});
    }

    #[test]
    fn flags_when_no_index() {
        let schema = r#"
            ActiveRecord::Schema.define(version: 2020_02_02_075409) do
              create_table "users", force: :cascade do |t|
                t.string "account", null: false
              end
            end
        "#;
        test::<UniqueValidationWithoutIndex>()
            .with_rails_schema(schema)
            .expect_offense(indoc! {r#"
                class User
                  validates :account, uniqueness: true
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Uniqueness validation should have a unique index on the database column.
                end
            "#});
    }

    #[test]
    fn ignores_false_and_nil() {
        let schema = r#"
            ActiveRecord::Schema.define(version: 2020_02_02_075409) do
              create_table "users", force: :cascade do |t|
                t.string "account", null: false
              end
            end
        "#;
        test::<UniqueValidationWithoutIndex>()
            .with_rails_schema(schema)
            .expect_no_offenses(indoc! {r#"
                class User
                  validates :account, uniqueness: false
                end
            "#});
        test::<UniqueValidationWithoutIndex>()
            .with_rails_schema(schema)
            .expect_no_offenses(indoc! {r#"
                class User
                  validates :account, uniqueness: nil
                end
            "#});
    }

    #[test]
    fn ignores_bare_validates() {
        let schema = r#"
            ActiveRecord::Schema.define(version: 2020_02_02_075409) do
              create_table "users", force: :cascade do |t|
                t.string "account", null: false
              end
            end
        "#;
        test::<UniqueValidationWithoutIndex>()
            .with_rails_schema(schema)
            .expect_no_offenses("class User\n  validates\nend\n");
    }

    #[test]
    fn flags_non_unique_index() {
        let schema = r#"
            ActiveRecord::Schema.define(version: 2020_02_02_075409) do
              create_table "users", force: :cascade do |t|
                t.string "account", null: false
                t.index ["account"], name: "index_users_on_account"
              end
            end
        "#;
        test::<UniqueValidationWithoutIndex>()
            .with_rails_schema(schema)
            .expect_offense(indoc! {r#"
                class User
                  validates :account, uniqueness: true
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Uniqueness validation should have a unique index on the database column.
                end
            "#});
    }

    #[test]
    fn allows_unique_index() {
        let schema = r#"
            ActiveRecord::Schema.define(version: 2020_02_02_075409) do
              create_table "users", force: :cascade do |t|
                t.string "account", null: false
                t.index ["account"], name: "index_users_on_account", unique: true
              end
            end
        "#;
        test::<UniqueValidationWithoutIndex>()
            .with_rails_schema(schema)
            .expect_no_offenses(indoc! {r#"
                class User
                  validates :account, uniqueness: true
                end
            "#});
    }

    #[test]
    fn allows_presence_only() {
        let schema = r#"
            ActiveRecord::Schema.define(version: 2020_02_02_075409) do
              create_table "users", force: :cascade do |t|
                t.string "account", null: false
                t.index ["account"], name: "index_users_on_account"
              end
            end
        "#;
        test::<UniqueValidationWithoutIndex>()
            .with_rails_schema(schema)
            .expect_no_offenses(indoc! {r#"
                class User
                  validates :account, presence: true
                end
            "#});
    }

    #[test]
    fn ignores_check_constraint_nil() {
        let schema = r#"
            ActiveRecord::Schema.define(version: 2020_02_02_075409) do
              create_table "users", force: :cascade do |t|
                t.string "account", null: false
                t.index ["account"], name: "index_users_on_account", unique: true
                t.check_constraint nil, 'expression', name: "constraint_name"
              end
            end
        "#;
        test::<UniqueValidationWithoutIndex>()
            .with_rails_schema(schema)
            .expect_no_offenses(indoc! {r#"
                class User
                  validates :account, uniqueness: true
                end
            "#});
    }

    #[test]
    fn flags_two_columns_without_proper_index() {
        let schema = r#"
            ActiveRecord::Schema.define(version: 2020_02_02_075409) do
              create_table "written_articles", force: :cascade do |t|
                t.bigint "user_id", null: false
                t.bigint "article_id", null: false
                t.index ["user_id"], name: "idx_uid", unique: true
                t.index ["article_id"], name: "idx_aid", unique: true
              end
            end
        "#;
        test::<UniqueValidationWithoutIndex>()
            .with_rails_schema(schema)
            .expect_offense(indoc! {r#"
                class WrittenArticles
                  validates :user_id, uniqueness: { scope: :article_id }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Uniqueness validation should have a unique index on the database column.
                end
            "#});
    }

    #[test]
    fn allows_two_columns_with_proper_index() {
        let schema = r#"
            ActiveRecord::Schema.define(version: 2020_02_02_075409) do
              create_table "written_articles", force: :cascade do |t|
                t.bigint "user_id", null: false
                t.bigint "article_id", null: false
                t.index ["user_id", "article_id"], name: "idx_uid_aid", unique: true
              end
            end
        "#;
        test::<UniqueValidationWithoutIndex>()
            .with_rails_schema(schema)
            .expect_no_offenses(indoc! {r#"
                class WrittenArticles
                  validates :user_id, uniqueness: { scope: :article_id }
                end
            "#});
    }

    #[test]
    fn ignores_conditions() {
        let schema = r#"
            ActiveRecord::Schema.define(version: 2020_02_02_075409) do
              create_table "articles", force: :cascade do |t|
                t.bigint "user_id", null: false
              end
            end
        "#;
        for src in [
            "class Article\n  belongs_to :user\n  validates :user, uniqueness: true, if: -> { false }\nend\n",
            "class Article\n  belongs_to :user\n  validates :user, uniqueness: true, unless: -> { true }\nend\n",
            "class Article\n  belongs_to :user\n  validates :user, uniqueness: { if: -> { false } }\nend\n",
            "class Article\n  belongs_to :user\n  validates :user, uniqueness: { unless: -> { false } }\nend\n",
            "class Article\n  belongs_to :user\n  enum :status, [:draft, :published]\n  validates :user, uniqueness: { conditions: -> { published } }\nend\n",
        ] {
            test::<UniqueValidationWithoutIndex>()
                .with_rails_schema(schema)
                .expect_no_offenses(src);
        }
    }

    #[test]
    fn resolves_belongs_to_relation() {
        let with_unique = r#"
            ActiveRecord::Schema.define(version: 2020_02_02_075409) do
              create_table "articles", force: :cascade do |t|
                t.bigint "user_id", null: false
                t.index ["user_id"], name: "idx_user_id", unique: true
              end
            end
        "#;
        test::<UniqueValidationWithoutIndex>()
            .with_rails_schema(with_unique)
            .expect_no_offenses(indoc! {r#"
                class Article
                  belongs_to :user
                  validates :user, uniqueness: true
                end
            "#});
        let without_unique = r#"
            ActiveRecord::Schema.define(version: 2020_02_02_075409) do
              create_table "articles", force: :cascade do |t|
                t.bigint "user_id", null: false
                t.index ["user_id"], name: "idx_user_id"
              end
            end
        "#;
        test::<UniqueValidationWithoutIndex>()
            .with_rails_schema(without_unique)
            .expect_offense(indoc! {r#"
                class Article
                  belongs_to :user
                  validates :user, uniqueness: true
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Uniqueness validation should have a unique index on the database column.
                end
            "#});
    }

    #[test]
    fn handles_table_name_override_and_namespaces() {
        let schema = r#"
            ActiveRecord::Schema.define(version: 2020_02_02_075409) do
              create_table "members", force: :cascade do |t|
                t.string "account", null: false
              end
            end
        "#;
        test::<UniqueValidationWithoutIndex>()
            .with_rails_schema(schema)
            .expect_offense(indoc! {r#"
                class User
                  self.table_name = 'members'
                  validates :account, uniqueness: true
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Uniqueness validation should have a unique index on the database column.
                end
            "#});
        let admin_schema = r#"
            ActiveRecord::Schema.define(version: 2020_02_02_075409) do
              create_table "admin_users", force: :cascade do |t|
                t.string "account", null: false
              end
            end
        "#;
        test::<UniqueValidationWithoutIndex>()
            .with_rails_schema(admin_schema)
            .expect_offense(indoc! {r#"
                module Admin
                  class User
                    validates :account, uniqueness: true
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Uniqueness validation should have a unique index on the database column.
                  end
                end
            "#});
        test::<UniqueValidationWithoutIndex>()
            .with_rails_schema(admin_schema)
            .expect_offense(indoc! {r#"
                class Admin::User
                  validates :account, uniqueness: true
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Uniqueness validation should have a unique index on the database column.
                end
            "#});
    }

    #[test]
    fn handles_expression_and_add_index() {
        let expr_ok = r#"
            ActiveRecord::Schema.define(version: 2020_02_02_075409) do
              create_table 'emails', force: :cascade do |t|
                t.string 'address', null: false
                t.index 'lower(address)', name: 'index_emails_on_lower_address', unique: true
              end
            end
        "#;
        test::<UniqueValidationWithoutIndex>()
            .with_rails_schema(expr_ok)
            .expect_no_offenses(indoc! {r#"
                class Email < ApplicationRecord
                  validates :address, presence: true, uniqueness: { case_sensitive: false }, email: true
                end
            "#});
        let add_index_ok = r#"
            ActiveRecord::Schema.define(version: 2020_02_02_075409) do
              create_table "users", force: :cascade do |t|
                t.string "account", null: false
              end
              add_index "users", ["account"], name: "index_users_on_account", unique: true
            end
        "#;
        test::<UniqueValidationWithoutIndex>()
            .with_rails_schema(add_index_ok)
            .expect_no_offenses(indoc! {r#"
                class User
                  validates :account, uniqueness: true
                end
            "#});
    }

    #[test]
    fn ignores_missing_table_and_module_and_empty() {
        let schema = r#"
            ActiveRecord::Schema.define(version: 2020_02_02_075409) do
              create_table "users", force: :cascade do |t|
                t.string "account", null: false, unique: true
              end
            end
        "#;
        test::<UniqueValidationWithoutIndex>()
            .with_rails_schema(schema)
            .expect_no_offenses(indoc! {r#"
                class Article
                  validates :account, uniqueness: true
                end
            "#});
        test::<UniqueValidationWithoutIndex>()
            .with_rails_schema(schema)
            .expect_no_offenses(indoc! {r#"
                module User
                  extend ActiveSupport::Concern
                  included do
                    validates :account, uniqueness: true
                  end
                end
            "#});
        test::<UniqueValidationWithoutIndex>()
            .with_rails_schema("")
            .expect_no_offenses(indoc! {r#"
                class User
                  validates :account, uniqueness: true
                end
            "#});
    }
}
murphy_plugin_api::submit_cop!(UniqueValidationWithoutIndex);
