//! `Rails/UnusedIgnoredColumns` — remove non-existent columns from `ignored_columns`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/UnusedIgnoredColumns
//! upstream_version_checked: 2.35.0
//! version_added: "2.11"
//! version_changed: "2.25"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND [:ignored_columns=] gating
//!   plus `op-asgn` (`+=`) handling, `return unless schema` (no schema → no
//!   offense), table lookup via `table_name` (explicit `self.table_name =` or
//!   `tableize`), per-column offense on the str/sym literal. Non-literal
//!   arrays and missing tables emit nothing. File scope
//!   (`Include: ['**/app/models/**/*.rb']`) is enforced via the murphy-rails
//!   pack default.yml (engine `cop_applies_to_file` gate).
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct UnusedIgnoredColumns;

#[cop(
    name = "Rails/UnusedIgnoredColumns",
    description = "Remove a column that does not exist from `ignored_columns`.",
    default_enabled = false,
    options = NoOptions,
)]
impl UnusedIgnoredColumns {
    // Mirrors upstream `RESTRICT_ON_SEND = %i[ignored_columns=]` plus
    // `alias on_op_asgn on_send` for `+=`.
    #[on_node(kind = "send", methods = ["ignored_columns="])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check_send(node, cx);
    }

    #[on_node(kind = "op_asgn")]
    fn check_op_asgn(&self, node: NodeId, cx: &Cx<'_>) {
        check_op_asgn(node, cx);
    }
}

fn check_send(node: NodeId, cx: &Cx<'_>) {
    if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
        return;
    }
    // Upstream `(send self :ignored_columns= $array)` — self receiver only.
    if !cx.is_self_receiver(node) {
        return;
    }
    let args = cx.call_arguments(node);
    let [array] = args else {
        return;
    };
    if !matches!(*cx.kind(*array), NodeKind::Array(_)) {
        return;
    }
    check_array(node, *array, cx);
}

fn check_op_asgn(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::OpAsgn { target, op, value } = *cx.kind(node) else {
        return;
    };
    // Upstream `(op-asgn (send self :ignored_columns) :+ $array)`.
    if cx.symbol_str(op) != "+" {
        return;
    }
    if !matches!(*cx.kind(target), NodeKind::Send { .. }) {
        return;
    }
    if cx.method_name(target) != Some("ignored_columns") {
        return;
    }
    if !cx.is_self_receiver(target) {
        return;
    }
    if !matches!(*cx.kind(value), NodeKind::Array(_)) {
        return;
    }
    check_array(node, value, cx);
}

fn check_array(_send_or_opasgn: NodeId, array: NodeId, cx: &Cx<'_>) {
    let Some(schema) = cx.rails_schema() else {
        return;
    };
    let Some(class_node) = find_class_ancestor(cx, _send_or_opasgn) else {
        return;
    };
    let table_name = table_name_for(cx, class_node);
    let Some(table) = schema.table_by(&table_name) else {
        return;
    };
    for &col_node in cx.array_elements(array) {
        let Some(name) = literal_string_value(cx, col_node) else {
            continue;
        };
        if table.with_column(&name) {
            continue;
        }
        cx.emit_offense(
            cx.range(col_node),
            &format!("Remove `{name}` from `ignored_columns` because the column does not exist."),
            None,
        );
    }
}

/// Nearest enclosing `class` ancestor (modules don't count — upstream
/// `class_node` searches `each_ancestor.find(&:class_type?)`).
fn find_class_ancestor(cx: &Cx<'_>, node: NodeId) -> Option<NodeId> {
    for anc in cx.ancestors(node) {
        if matches!(*cx.kind(anc), NodeKind::Class { .. }) {
            return Some(anc);
        }
    }
    None
}

/// Table name: explicit `self.table_name = 'x'` wins, else `tableize` of the
/// full class path (namespaces joined with `::`).
fn table_name_for(cx: &Cx<'_>, class_node: NodeId) -> String {
    if let Some(explicit) = find_set_table_name(cx, class_node) {
        return explicit;
    }
    let full = class_full_path(cx, class_node).unwrap_or_default();
    murphy_plugin_api::rails_schema::tableize(&full)
}

fn find_set_table_name(cx: &Cx<'_>, class_node: NodeId) -> Option<String> {
    // Search class body + descendants for `self.table_name = ...`.
    let body = match *cx.kind(class_node) {
        NodeKind::Class { body, .. } => body.get()?,
        _ => return None,
    };
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
    // Ancestor modules/classes, outermost first.
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

#[cfg(test)]
mod tests {
    use super::UnusedIgnoredColumns;
    use murphy_plugin_api::test_support::{indoc, test};

    const SCHEMA: &str = r#"
        ActiveRecord::Schema.define(version: 2020_02_02_075409) do
          create_table "users", force: :cascade do |t|
            t.string "account", null: false
          end
        end
    "#;

    #[test]
    fn does_nothing_without_schema() {
        test::<UnusedIgnoredColumns>().expect_no_offenses(indoc! {r#"
            class User < ApplicationRecord
              self.ignored_columns = [:real_name]
            end
        "#});
    }

    #[test]
    fn flags_unused_symbol_column() {
        test::<UnusedIgnoredColumns>()
            .with_rails_schema(SCHEMA)
            .expect_offense(indoc! {r#"
                class User < ApplicationRecord
                  self.ignored_columns = [:real_name]
                                          ^^^^^^^^^^ Remove `real_name` from `ignored_columns` because the column does not exist.
                end
            "#});
    }

    #[test]
    fn flags_unused_string_column() {
        test::<UnusedIgnoredColumns>()
            .with_rails_schema(SCHEMA)
            .expect_offense(indoc! {r#"
                class User < ApplicationRecord
                  self.ignored_columns = ['real_name']
                                          ^^^^^^^^^^^ Remove `real_name` from `ignored_columns` because the column does not exist.
                end
            "#});
    }

    #[test]
    fn allows_existent_symbol_column() {
        test::<UnusedIgnoredColumns>()
            .with_rails_schema(SCHEMA)
            .expect_no_offenses(indoc! {r#"
                class User < ApplicationRecord
                  self.ignored_columns = [:account]
                end
            "#});
    }

    #[test]
    fn allows_existent_string_column() {
        test::<UnusedIgnoredColumns>()
            .with_rails_schema(SCHEMA)
            .expect_no_offenses(indoc! {r#"
                class User < ApplicationRecord
                  self.ignored_columns = ['account']
                end
            "#});
    }

    #[test]
    fn flags_only_nonexistent_in_mixed() {
        test::<UnusedIgnoredColumns>()
            .with_rails_schema(SCHEMA)
            .expect_offense(indoc! {r#"
                class User < ApplicationRecord
                  self.ignored_columns = [:real_name, :account]
                                          ^^^^^^^^^^ Remove `real_name` from `ignored_columns` because the column does not exist.
                end
            "#});
    }

    #[test]
    fn ignores_non_literal_array() {
        test::<UnusedIgnoredColumns>()
            .with_rails_schema(SCHEMA)
            .expect_no_offenses(indoc! {r#"
                class User < ApplicationRecord
                  self.ignored_columns = array
                end
            "#});
    }

    #[test]
    fn flags_append_assignment() {
        test::<UnusedIgnoredColumns>()
            .with_rails_schema(SCHEMA)
            .expect_offense(indoc! {r#"
                class User < ApplicationRecord
                  self.ignored_columns += ['real_name']
                                           ^^^^^^^^^^^ Remove `real_name` from `ignored_columns` because the column does not exist.
                end
            "#});
    }

    #[test]
    fn ignores_mixin_module() {
        test::<UnusedIgnoredColumns>()
            .with_rails_schema(SCHEMA)
            .expect_no_offenses(indoc! {r#"
                module Abc
                  self.ignored_columns = [:real_name]
                end
            "#});
    }

    #[test]
    fn ignores_missing_table() {
        let empty = r#"
            ActiveRecord::Schema.define(version: 2020_02_02_075409) do
            end
        "#;
        test::<UnusedIgnoredColumns>()
            .with_rails_schema(empty)
            .expect_no_offenses(indoc! {r#"
                class User < ApplicationRecord
                  self.ignored_columns = [:real_name]
                end
            "#});
    }

    #[test]
    fn ignores_extension_only_schema() {
        let schema = r#"
            ActiveRecord::Schema.define(version: 2020_02_02_075409) do
              enable_extension 'plpgsql'
            end
        "#;
        test::<UnusedIgnoredColumns>()
            .with_rails_schema(schema)
            .expect_no_offenses(indoc! {r#"
                class User < ApplicationRecord
                  self.ignored_columns = [:real_name]
                end
            "#});
    }
}
murphy_plugin_api::submit_cop!(UnusedIgnoredColumns);
