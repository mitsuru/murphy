//! `Rails/ThreeStateBooleanColumn` — boolean columns need default + NOT NULL.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/ThreeStateBooleanColumn
//! upstream_version_checked: 2.35.0
//! version_added: "2.19"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_send`
//!   (`RESTRICT_ON_SEND = %i[add_column column boolean]`): bare
//!   `add_column table, column, :boolean/:\"boolean\" [, opts]`,
//!   `recv.column column, :boolean [, opts]`, and
//!   `recv.boolean column [, opts]` flag unless the trailing options
//!   hash satisfies `required_options?`
//!   (`default: <non-nil>` plus `null: false`). The
//!   `change_column_null(table, column, false)` exemption inside the
//!   enclosing `def` (with `table_node` from the first arg or the
//!   enclosing `create_table`/`change_table` block) is implemented via
//!   source-equality on the table/column arguments. Offense is the
//!   whole send; no autocorrect. Disabled upstream by default
//!   (`Enabled: pending`). File scope (`Include: ['db/**/*.rb']`) is
//!   enforced via the murphy-rails pack default.yml (engine
//!   `cop_applies_to_file` gate).
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ThreeStateBooleanColumn;

const MSG: &str = "Boolean columns should always have a default value and a `NOT NULL` constraint.";

#[cop(
    name = "Rails/ThreeStateBooleanColumn",
    description = "Add a default value and a `NOT NULL` constraint to boolean columns.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl ThreeStateBooleanColumn {
    #[on_node(kind = "send", methods = ["add_column", "column", "boolean"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
        return;
    }
    let Some(method) = cx.method_name(node).map(|s| s.to_owned()) else {
        return;
    };
    let has_receiver = cx.call_receiver(node).get().is_some();
    let args = cx.call_arguments(node).to_vec();
    // Upstream shapes: `add_column` bare; `column`/`boolean` with receiver.
    let (column_node, options_node): (NodeId, Option<NodeId>) = match method.as_str() {
        "add_column" => {
            if has_receiver {
                return;
            }
            match args.len() {
                3 => {
                    if !is_boolean_type(cx, args[2]) {
                        return;
                    }
                    (args[1], None)
                }
                4 => {
                    if !is_boolean_type(cx, args[2]) {
                        return;
                    }
                    (args[1], Some(args[3]))
                }
                _ => return,
            }
        }
        "column" => {
            if !has_receiver {
                return;
            }
            match args.len() {
                2 => {
                    if !is_boolean_type(cx, args[1]) {
                        return;
                    }
                    (args[0], None)
                }
                3 => {
                    if !is_boolean_type(cx, args[1]) {
                        return;
                    }
                    (args[0], Some(args[2]))
                }
                _ => return,
            }
        }
        "boolean" => {
            if !has_receiver {
                return;
            }
            match args.len() {
                1 => (args[0], None),
                2 => (args[0], Some(args[1])),
                _ => return,
            }
        }
        _ => return,
    };

    if let Some(opts) = options_node
        && required_options(cx, opts)
    {
        return;
    }

    // Upstream exemption: inside a `def`, skip when there is no table
    // or a matching `change_column_null(table, column, false)` exists.
    if let Some(def_node) = enclosing_def(cx, node) {
        let table_node = table_node(cx, node, &method);
        match table_node {
            None => return,
            Some(table) => {
                if change_column_null_in(cx, def_node, table, column_node) {
                    return;
                }
            }
        }
    }

    cx.emit_offense(cx.range(node), MSG, None);
}

/// `(sym :boolean)` or `(str "boolean")`.
fn is_boolean_type(cx: &Cx<'_>, id: NodeId) -> bool {
    match *cx.kind(id) {
        NodeKind::Sym(sym) => cx.symbol_str(sym) == "boolean",
        NodeKind::Str(sid) => cx.string_str(sid) == "boolean",
        _ => false,
    }
}

/// Upstream `required_options?`: hash containing `default: <non-nil>`
/// and `null: false`.
fn required_options(cx: &Cx<'_>, id: NodeId) -> bool {
    if !matches!(*cx.kind(id), NodeKind::Hash(_)) {
        return false;
    }
    let mut has_default = false;
    let mut has_null_false = false;
    for pair in cx.hash_pairs(id) {
        let NodeKind::Pair { key, value } = *cx.kind(pair) else {
            continue;
        };
        let NodeKind::Sym(sym) = *cx.kind(key) else {
            continue;
        };
        match cx.symbol_str(sym) {
            "default" => {
                if !matches!(*cx.kind(value), NodeKind::Nil) {
                    has_default = true;
                }
            }
            "null" => {
                if matches!(*cx.kind(value), NodeKind::False_) {
                    has_null_false = true;
                }
            }
            _ => {}
        }
    }
    has_default && has_null_false
}

/// Nearest enclosing `def`/`defs` ancestor (`any_def` upstream).
fn enclosing_def(cx: &Cx<'_>, node: NodeId) -> Option<NodeId> {
    for anc in cx.ancestors(node) {
        if matches!(*cx.kind(anc), NodeKind::Def { .. } | NodeKind::Defs { .. }) {
            return Some(anc);
        }
    }
    None
}

/// Upstream `table_node`: `add_column` first arg, else the enclosing
/// `create_table`/`change_table` block's first arg.
fn table_node(cx: &Cx<'_>, node: NodeId, method: &str) -> Option<NodeId> {
    if method == "add_column" {
        return cx.call_arguments(node).first().copied();
    }
    for anc in cx.ancestors(node) {
        let NodeKind::Block { call, .. } = *cx.kind(anc) else {
            continue;
        };
        let m = cx.method_name(call);
        if m == Some("create_table") || m == Some("change_table") {
            return cx.call_arguments(call).first().copied();
        }
    }
    None
}

/// Upstream `change_column_null?` search: bare
/// `change_column_null(table, column, false)` inside `def_node` with
/// source-equal table/column arguments.
fn change_column_null_in(cx: &Cx<'_>, def_node: NodeId, table: NodeId, column: NodeId) -> bool {
    let table_src = cx.raw_source(cx.range(table));
    let column_src = cx.raw_source(cx.range(column));
    for id in cx.descendants(def_node) {
        if !matches!(*cx.kind(id), NodeKind::Send { .. }) {
            continue;
        }
        if cx.method_name(id) != Some("change_column_null") {
            continue;
        }
        if cx.call_receiver(id).get().is_some() {
            continue;
        }
        let args = cx.call_arguments(id);
        if args.len() != 3 {
            continue;
        }
        if !matches!(*cx.kind(args[2]), NodeKind::False_) {
            continue;
        }
        if cx.raw_source(cx.range(args[0])) != table_src {
            continue;
        }
        if cx.raw_source(cx.range(args[1])) != column_src {
            continue;
        }
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::ThreeStateBooleanColumn;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_add_column_without_options() {
        test::<ThreeStateBooleanColumn>().expect_offense(indoc! {r#"
            add_column :users, :active, :boolean
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Boolean columns should always have a default value and a `NOT NULL` constraint.
        "#});
    }

    #[test]
    fn flags_add_column_string_type() {
        test::<ThreeStateBooleanColumn>().expect_offense(indoc! {r#"
            add_column :users, :active, "boolean"
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Boolean columns should always have a default value and a `NOT NULL` constraint.
        "#});
    }

    #[test]
    fn flags_column_without_options() {
        test::<ThreeStateBooleanColumn>().expect_offense(indoc! {r#"
            t.column :active, :boolean
            ^^^^^^^^^^^^^^^^^^^^^^^^^^ Boolean columns should always have a default value and a `NOT NULL` constraint.
        "#});
    }

    #[test]
    fn flags_boolean_without_options() {
        test::<ThreeStateBooleanColumn>().expect_offense(indoc! {r#"
            t.boolean :active
            ^^^^^^^^^^^^^^^^^ Boolean columns should always have a default value and a `NOT NULL` constraint.
        "#});
    }

    #[test]
    fn flags_partial_options_missing_null() {
        test::<ThreeStateBooleanColumn>().expect_offense(indoc! {r#"
            add_column :users, :active, :boolean, default: true
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Boolean columns should always have a default value and a `NOT NULL` constraint.
        "#});
    }

    #[test]
    fn flags_default_nil() {
        test::<ThreeStateBooleanColumn>().expect_offense(indoc! {r#"
            add_column :users, :active, :boolean, default: nil, null: false
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Boolean columns should always have a default value and a `NOT NULL` constraint.
        "#});
    }

    #[test]
    fn allows_full_options() {
        test::<ThreeStateBooleanColumn>().expect_no_offenses(
            "add_column :users, :active, :boolean, default: true, null: false\n",
        );
    }

    #[test]
    fn allows_full_options_false_default() {
        test::<ThreeStateBooleanColumn>().expect_no_offenses(
            "t.boolean :active, default: false, null: false\n",
        );
    }

    #[test]
    fn does_not_flag_non_boolean() {
        test::<ThreeStateBooleanColumn>()
            .expect_no_offenses("add_column :users, :name, :string\n");
    }

    #[test]
    fn does_not_flag_receiver_add_column() {
        test::<ThreeStateBooleanColumn>()
            .expect_no_offenses("x.add_column :users, :active, :boolean\n");
    }

    #[test]
    fn allows_change_column_null_in_def() {
        test::<ThreeStateBooleanColumn>().expect_no_offenses(indoc! {r#"
            def change
              add_column :users, :active, :boolean
              change_column_null :users, :active, false
            end
        "#});
    }

    #[test]
    fn flags_without_change_column_null_in_def() {
        test::<ThreeStateBooleanColumn>().expect_offense(indoc! {r#"
            def change
              add_column :users, :active, :boolean
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Boolean columns should always have a default value and a `NOT NULL` constraint.
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(ThreeStateBooleanColumn);
