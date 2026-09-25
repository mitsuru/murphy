//! `Rails/ReversibleMigration` — `change` method must be reversible.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/ReversibleMigration
//! upstream_version_checked: 2.35.0
//! version_added: "0.47"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: bare-receiver `change_column`/`execute`
//!   always flag; `drop_table` flags without block/block-pass; `remove_column`
//!   flags with <3 args; `remove_foreign_key` (2-arg, hash without to_table),
//!   `remove_columns` (Rails 6.1 type gating), `remove_index` (2-arg, hash
//!   without column), and `change`/`remove`/`change_default`/
//!   `change_column_default`/`change_table_comment`/`change_column_comment`
//!   without from/to (bare only to avoid double-flagging inner `t.*`);
//!   `change_table` blocks flag irreversible inner calls with Rails 6.1
//!   `t.remove` type gating. Gated on migration class + `def change` +
//!   not inside `reversible`/`up_only`. No autocorrect. `MigratedSchemaVersion`
//!   skipping remains absent (cf. MigrationClassName). Upstream Include
//!   (`db/**/*.rb`) is enforced via the murphy-rails pack default.yml.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ReversibleMigration;

#[cop(
    name = "Rails/ReversibleMigration",
    description = "Checks whether the change method of the migration file is reversible.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl ReversibleMigration {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check_send(node, cx);
    }

    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check_block(node, cx);
    }

    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        check_numblock(node, cx);
    }

    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        check_itblock(node, cx);
    }
}

fn check_send(node: NodeId, cx: &Cx<'_>) {
    if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
        return;
    }
    if !in_migration(cx, node) || !within_change(cx, node) {
        return;
    }
    if within_reversible_or_up_only(cx, node) {
        return;
    }
    // All top-level checks are bare-receiver only (upstream `nil?`).
    let is_bare = cx.call_receiver(node).get().is_none();
    if !is_bare {
        return;
    }
    check_irreversible(node, cx);
    check_drop_table(node, cx);
    check_reversible_hash(node, cx);
    check_remove_column(node, cx);
    check_remove_foreign_key(node, cx);
    check_remove_columns(node, cx);
    check_remove_index(node, cx);
}

fn check_irreversible(node: NodeId, cx: &Cx<'_>) {
    match cx.method_name(node) {
        Some("change_column") | Some("execute") => {}
        _ => return,
    }
    let action = cx.method_name(node).unwrap_or("").to_owned();
    cx.emit_offense(
        cx.range(node),
        &format!("{action} is not reversible."),
        None,
    );
}

fn check_drop_table(node: NodeId, cx: &Cx<'_>) {
    if cx.method_name(node) != Some("drop_table") {
        return;
    }
    let Some(last) = cx.last_argument(node).get() else {
        return;
    };
    // `drop_table :users do |t| ... end` — parent is the block.
    if let Some(parent) = cx.parent(node).get()
        && matches!(
            *cx.kind(parent),
            NodeKind::Block { .. } | NodeKind::Numblock { .. } | NodeKind::Itblock { .. }
        )
    {
        return;
    }
    if matches!(*cx.kind(last), NodeKind::BlockPass(_)) {
        return;
    }
    cx.emit_offense(
        cx.range(node),
        "drop_table(without block) is not reversible.",
        None,
    );
}

fn check_reversible_hash(node: NodeId, cx: &Cx<'_>) {
    if reversible_change_table_call(cx, node) {
        return;
    }
    // Only the from/to family + change/remove reach here as non-reversible;
    // other methods return true above. Restrict to those method names to
    // avoid flagging unrelated bare calls.
    let method = cx.method_name(node).unwrap_or("").to_owned();
    if !matches!(
        method.as_str(),
        "change" | "remove" | "change_default" | "change_column_default" | "change_table_comment" | "change_column_comment"
    ) {
        return;
    }
    cx.emit_offense(
        cx.range(node),
        &format!("{method}(without :from and :to) is not reversible."),
        None,
    );
}

fn check_remove_column(node: NodeId, cx: &Cx<'_>) {
    if cx.method_name(node) != Some("remove_column") {
        return;
    }
    if cx.call_arguments(node).len() < 3 {
        cx.emit_offense(
            cx.range(node),
            "remove_column(without type) is not reversible.",
            None,
        );
    }
}

fn check_remove_foreign_key(node: NodeId, cx: &Cx<'_>) {
    if cx.method_name(node) != Some("remove_foreign_key") {
        return;
    }
    let args = cx.call_arguments(node);
    if args.len() != 2 {
        return;
    }
    let second = args[1];
    if !matches!(*cx.kind(second), NodeKind::Hash(_)) {
        return;
    }
    if !hash_has_keys(cx, second, &["to_table"]) {
        cx.emit_offense(
            cx.range(node),
            "remove_foreign_key(without table) is not reversible.",
            None,
        );
    }
}

fn check_remove_columns(node: NodeId, cx: &Cx<'_>) {
    if cx.method_name(node) != Some("remove_columns") {
        return;
    }
    let Some(last) = cx.last_argument(node).get() else {
        return;
    };
    if hash_has_keys(cx, last, &["type"]) && cx.rails_version_at_least(6, 1) {
        return;
    }
    let action = if cx.rails_version_at_least(6, 1) {
        "remove_columns(without type)"
    } else {
        "remove_columns"
    };
    cx.emit_offense(cx.range(node), &format!("{action} is not reversible."), None);
}

fn check_remove_index(node: NodeId, cx: &Cx<'_>) {
    if cx.method_name(node) != Some("remove_index") {
        return;
    }
    let args = cx.call_arguments(node);
    if args.len() != 2 {
        return;
    }
    let second = args[1];
    if !matches!(*cx.kind(second), NodeKind::Hash(_)) {
        return;
    }
    if !hash_has_keys(cx, second, &["column"]) {
        cx.emit_offense(
            cx.range(node),
            "remove_index(without column) is not reversible.",
            None,
        );
    }
}

fn check_block(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Block { call, body, .. } = *cx.kind(node) else {
        return;
    };
    if !in_migration(cx, node) || !within_change(cx, node) {
        return;
    }
    if within_reversible_or_up_only(cx, node) {
        return;
    }
    let Some(body_id) = body.get() else {
        return;
    };
    check_change_table_call(cx, call, body_id);
}

fn check_numblock(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Numblock { send, body, .. } = *cx.kind(node) else {
        return;
    };
    if !in_migration(cx, node) || !within_change(cx, node) {
        return;
    }
    if within_reversible_or_up_only(cx, node) {
        return;
    }
    let Some(body_id) = body.get() else {
        return;
    };
    check_change_table_call(cx, send, body_id);
}

fn check_itblock(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Itblock { send, body } = *cx.kind(node) else {
        return;
    };
    if !in_migration(cx, node) || !within_change(cx, node) {
        return;
    }
    if within_reversible_or_up_only(cx, node) {
        return;
    }
    let Some(body_id) = body.get() else {
        return;
    };
    check_change_table_call(cx, send, body_id);
}

fn check_change_table_call(cx: &Cx<'_>, call: NodeId, body: NodeId) {
    if !matches!(*cx.kind(call), NodeKind::Send { .. }) {
        return;
    }
    if cx.method_name(call) != Some("change_table") {
        return;
    }
    if cx.call_receiver(call).get().is_some() {
        return;
    }
    if cx.call_arguments(call).is_empty() {
        return;
    }
    if matches!(*cx.kind(body), NodeKind::Send { .. }) {
        check_change_table_offense(cx, body);
    } else if let NodeKind::Begin(list) = *cx.kind(body) {
        for &child in cx.list(list) {
            if matches!(*cx.kind(child), NodeKind::Send { .. }) {
                check_change_table_offense(cx, child);
            }
        }
    } else {
        // Other bodies (e.g. single lvasgn + send are wrapped in Begin;
        // anything else has no direct sends).
        for child in cx.children(body) {
            if matches!(*cx.kind(child), NodeKind::Send { .. }) {
                check_change_table_offense(cx, child);
            }
        }
    }
}

fn check_change_table_offense(cx: &Cx<'_>, node: NodeId) {
    if reversible_change_table_call(cx, node) {
        return;
    }
    let method = cx.method_name(node).unwrap_or("").to_owned();
    let action = if method == "remove" {
        if cx.rails_version_at_least(6, 1) {
            "t.remove (without type)".to_owned()
        } else {
            "t.remove".to_owned()
        }
    } else {
        format!("change_table(with {method})")
    };
    cx.emit_offense(cx.range(node), &format!("{action} is not reversible."), None);
}

fn reversible_change_table_call(cx: &Cx<'_>, node: NodeId) -> bool {
    let method = cx.method_name(node).unwrap_or("").to_owned();
    match method.as_str() {
        "change" => false,
        "remove" => {
            if !cx.rails_version_at_least(6, 1) {
                return false;
            }
            match cx.last_argument(node).get() {
                Some(last) => hash_has_keys(cx, last, &["type"]),
                None => false,
            }
        }
        "change_default" | "change_column_default" | "change_table_comment" | "change_column_comment" => {
            match cx.last_argument(node).get() {
                Some(last) => hash_has_keys(cx, last, &["from", "to"]),
                None => false,
            }
        }
        _ => true,
    }
}

fn hash_has_keys(cx: &Cx<'_>, hash_id: NodeId, keys: &[&str]) -> bool {
    if !matches!(*cx.kind(hash_id), NodeKind::Hash(_)) {
        return false;
    }
    let NodeKind::Hash(list) = *cx.kind(hash_id) else {
        return false;
    };
    let mut found = Vec::new();
    for &pair in cx.list(list) {
        if !matches!(*cx.kind(pair), NodeKind::Pair { .. }) {
            continue;
        }
        let NodeKind::Pair { key, .. } = *cx.kind(pair) else {
            continue;
        };
        let name = match *cx.kind(key) {
            NodeKind::Sym(s) => cx.symbol_str(s).to_owned(),
            NodeKind::Str(id) => cx.string_str(id).to_owned(),
            _ => continue,
        };
        found.push(name);
    }
    keys.iter().all(|k| found.iter().any(|f| f == k))
}

fn is_migration_class(cx: &Cx<'_>, class_id: NodeId) -> bool {
    let NodeKind::Class { name, superclass, .. } = *cx.kind(class_id) else {
        return false;
    };
    // `(const {nil? cbase} _)` — top-level only.
    let is_top = match *cx.kind(name) {
        NodeKind::Const { scope, .. } => match scope.get() {
            None => true,
            Some(s) => matches!(*cx.kind(s), NodeKind::Cbase),
        },
        _ => false,
    };
    if !is_top {
        return false;
    }
    let Some(super_id) = superclass.get() else {
        return false;
    };
    if !matches!(*cx.kind(super_id), NodeKind::Send { .. }) {
        return false;
    }
    if cx.method_name(super_id) != Some("[]") {
        return false;
    }
    let Some(recv) = cx.call_receiver(super_id).get() else {
        return false;
    };
    if cx.const_name(recv).as_deref() != Some("ActiveRecord::Migration") {
        return false;
    }
    let args = cx.call_arguments(super_id);
    if args.len() != 1 {
        return false;
    }
    matches!(*cx.kind(args[0]), NodeKind::Float(_))
}

fn in_migration(cx: &Cx<'_>, node: NodeId) -> bool {
    for anc in cx.ancestors(node) {
        if matches!(*cx.kind(anc), NodeKind::Class { .. }) && is_migration_class(cx, anc) {
            return true;
        }
    }
    false
}

fn within_change(cx: &Cx<'_>, node: NodeId) -> bool {
    for anc in cx.ancestors(node) {
        if let NodeKind::Def { name, .. } = *cx.kind(anc)
            && cx.symbol_str(name) == "change"
        {
            return true;
        }
    }
    false
}

fn block_call_method(cx: &Cx<'_>, block_id: NodeId) -> Option<String> {
    match *cx.kind(block_id) {
        NodeKind::Block { call, .. } => cx.method_name(call).map(|s| s.to_owned()),
        NodeKind::Numblock { send, .. } => cx.method_name(send).map(|s| s.to_owned()),
        NodeKind::Itblock { send, .. } => cx.method_name(send).map(|s| s.to_owned()),
        _ => None,
    }
}

fn within_reversible_or_up_only(cx: &Cx<'_>, node: NodeId) -> bool {
    for anc in cx.ancestors(node) {
        if !matches!(
            *cx.kind(anc),
            NodeKind::Block { .. } | NodeKind::Numblock { .. } | NodeKind::Itblock { .. }
        ) {
            continue;
        }
        if let Some(m) = block_call_method(cx, anc)
            && (m == "reversible" || m == "up_only")
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::ReversibleMigration;
    use murphy_plugin_api::test_support::{indoc, test};

    fn wrap(code: &str) -> String {
        format!(
            "class ExampleMigration < ActiveRecord::Migration[7.0]\n  def change\n    {code}\n  end\nend\n"
        )
    }

    #[test]
    fn accepts_create_table() {
        test::<ReversibleMigration>().expect_no_offenses(wrap(
            "create_table :users do |t|\n      t.string :name\n    end",
        ).as_str());
    }

    #[test]
    fn flags_execute() {
        let src = wrap("execute \"ALTER TABLE pages ADD UNIQUE (page_id)\"");
        let offenses = murphy_plugin_api::test_support::run_cop::<ReversibleMigration>(&src);
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].message, "execute is not reversible.");
    }

    #[test]
    fn accepts_up_only_execute() {
        test::<ReversibleMigration>()
            .expect_no_offenses(wrap("up_only { execute \"UPDATE posts SET published = 'true'\" }").as_str());
    }

    #[test]
    fn flags_drop_table_without_block() {
        let src = wrap("drop_table :users");
        let offenses = murphy_plugin_api::test_support::run_cop::<ReversibleMigration>(&src);
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].message, "drop_table(without block) is not reversible.");
    }

    #[test]
    fn accepts_drop_table_with_block() {
        test::<ReversibleMigration>().expect_no_offenses(wrap(
            "drop_table :users do |t|\n      t.string :name\n    end",
        ).as_str());
    }

    #[test]
    fn accepts_drop_table_with_block_pass() {
        test::<ReversibleMigration>()
            .expect_no_offenses(wrap("drop_table :users, &:timestamps").as_str());
    }

    #[test]
    fn flags_change_column() {
        let src = wrap("change_column(:posts, :state, :string)");
        let offenses = murphy_plugin_api::test_support::run_cop::<ReversibleMigration>(&src);
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].message, "change_column is not reversible.");
    }

    #[test]
    fn flags_change_column_default_without_from_to() {
        let src = wrap("change_column_default(:suppliers, :qualification, 'new')");
        let offenses = murphy_plugin_api::test_support::run_cop::<ReversibleMigration>(&src);
        assert!(offenses.iter().any(|o| o.message == "change_column_default(without :from and :to) is not reversible."));
    }

    #[test]
    fn accepts_change_column_default_with_from_to() {
        test::<ReversibleMigration>().expect_no_offenses(
            wrap("change_column_default(:posts, :state, from: nil, to: \"draft\")").as_str(),
        );
    }

    #[test]
    fn flags_remove_column_without_type() {
        let src = wrap("remove_column(:suppliers, :qualification)");
        let offenses = murphy_plugin_api::test_support::run_cop::<ReversibleMigration>(&src);
        assert!(offenses.iter().any(|o| o.message == "remove_column(without type) is not reversible."));
    }

    #[test]
    fn accepts_remove_column_with_type() {
        test::<ReversibleMigration>().expect_no_offenses(
            wrap("remove_column(:suppliers, :qualification, :string)").as_str(),
        );
    }

    #[test]
    fn flags_remove_foreign_key_without_table() {
        let src = wrap("remove_foreign_key :accounts, column: :owner_id");
        let offenses = murphy_plugin_api::test_support::run_cop::<ReversibleMigration>(&src);
        assert!(offenses.iter().any(|o| o.message == "remove_foreign_key(without table) is not reversible."));
    }

    #[test]
    fn accepts_remove_foreign_key_with_table() {
        test::<ReversibleMigration>()
            .expect_no_offenses(wrap("remove_foreign_key :accounts, :branches").as_str());
    }

    #[test]
    fn accepts_remove_foreign_key_with_to_table() {
        test::<ReversibleMigration>().expect_no_offenses(
            wrap("remove_foreign_key :accounts, to_table: :branches").as_str(),
        );
    }

    #[test]
    fn flags_change_table_with_change() {
        let src = wrap("change_table :users do |t|\n      t.change :description, :text\n    end");
        let offenses = murphy_plugin_api::test_support::run_cop::<ReversibleMigration>(&src);
        assert!(offenses.iter().any(|o| o.message == "change_table(with change) is not reversible."));
    }

    #[test]
    fn accepts_change_table_reversible() {
        test::<ReversibleMigration>().expect_no_offenses(wrap(
            "change_table :users do |t|\n      t.column :name, :string\n    end",
        ).as_str());
    }

    #[test]
    fn flags_change_table_remove_without_type() {
        let src = wrap("change_table :users do |t|\n      t.remove :name\n    end");
        let offenses = murphy_plugin_api::test_support::run_cop::<ReversibleMigration>(&src);
        assert!(offenses.iter().any(|o| o.message.contains("t.remove")));
    }

    #[test]
    fn flags_remove_columns_without_type() {
        let src = wrap("remove_columns :users, :name, :email");
        let offenses = murphy_plugin_api::test_support::run_cop::<ReversibleMigration>(&src);
        assert!(offenses.iter().any(|o| o.message.contains("remove_columns")));
    }

    #[test]
    fn accepts_remove_columns_with_type_on_rails61() {
        test::<ReversibleMigration>()
            .with_target_rails_version(6, 1)
            .expect_no_offenses(wrap("remove_columns(:posts, :title, :body, type: :text)").as_str());
    }

    #[test]
    fn flags_remove_index_without_column() {
        let src = wrap("remove_index :users, name: :index_users_on_email");
        let offenses = murphy_plugin_api::test_support::run_cop::<ReversibleMigration>(&src);
        assert!(offenses.iter().any(|o| o.message == "remove_index(without column) is not reversible."));
    }

    #[test]
    fn accepts_remove_index_with_column() {
        test::<ReversibleMigration>()
            .expect_no_offenses(wrap("remove_index(:posts, column: :body)").as_str());
    }

    #[test]
    fn allows_outside_change() {
        test::<ReversibleMigration>().expect_no_offenses(indoc! {r#"
            class ExampleMigration < ActiveRecord::Migration[7.0]
              def up
                execute "SELECT 1"
              end
            end
        "#});
    }

    #[test]
    fn allows_outside_migration() {
        test::<ReversibleMigration>().expect_no_offenses(indoc! {r#"
            class Foo
              def change
                execute "SELECT 1"
              end
            end
        "#});
    }

    #[test]
    fn allows_inside_reversible() {
        test::<ReversibleMigration>().expect_no_offenses(indoc! {r#"
            class ExampleMigration < ActiveRecord::Migration[7.0]
              def change
                reversible do |dir|
                  dir.up do
                    execute "SELECT 1"
                  end
                end
              end
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(ReversibleMigration);
