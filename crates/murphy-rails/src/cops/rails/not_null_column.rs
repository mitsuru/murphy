//! `Rails/NotNullColumn` — do not add NOT NULL column without default.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/NotNullColumn
//! upstream_version_checked: 2.35.0
//! version_added: "0.43"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND [add_column
//!   add_reference], add_column (bare, last-arg Hash, type-gated virtual /
//!   mysql-text), add_reference (bare, last-arg Hash), change_table Block
//!   (single Arg table var, Begin-aware body scan) with `column`
//!   (col/type/hash), shortcut (`t.string :col, hash`, method-as-type
//!   virtual/mysql-text gating), and `references`/`add_reference`
//!   (hash-bearing) branches, plus `null: false` (False_) offense on the
//!   pair and `default: non-nil` exemption (`default: nil` still flags).
//!   Database option (mysql skips text) is honored; yaml/env auto-detection
//!   and MigratedSchemaVersion skipping remain absent; Include
//!   (`db/**/*.rb`) is enforced via the murphy-rails pack default.yml
//!   (engine `cop_applies_to_file` gate, verified vs rubocop-rails 2.38.0
//!   default.yml, murphy-4gd.1.15).
//! ```
//!
//! Checks for add_column calls with a NOT NULL constraint without a
//! default value.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Symbol, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct NotNullColumn;

#[derive(CopOptions)]
pub struct NotNullColumnOptions {
    #[option(
        name = "Database",
        description = "Database adapter name (e.g. mysql); text columns skip when mysql."
    )]
    pub database: Option<String>,
}

#[cop(
    name = "Rails/NotNullColumn",
    description = "Do not add a NOT NULL column without a default value.",
    default_severity = "warning",
    default_enabled = true,
    options = NotNullColumnOptions,
)]
impl NotNullColumn {
    #[on_node(kind = "send", methods = ["add_column", "add_reference"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check_send(node, cx);
    }

    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check_change_table(node, cx);
    }

    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        let _ = (node, cx);
    }

    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        let _ = (node, cx);
    }
}

const MSG: &str = "Do not add a NOT NULL column without a default value.";

fn check_send(node: NodeId, cx: &Cx<'_>) {
    let method = match cx.method_name(node) {
        Some(m) => m.to_owned(),
        None => return,
    };
    // Upstream `(send nil? ...)` — bare calls only.
    if cx.call_receiver(node).get().is_some() {
        return;
    }
    let args = cx.call_arguments(node).to_vec();
    match method.as_str() {
        "add_column" => {
            // `(send nil? :add_column _ _ $_ (hash $...))`
            if args.len() != 4 {
                return;
            }
            let type_node = args[2];
            let hash_node = args[3];
            if !matches!(*cx.kind(hash_node), NodeKind::Hash(_)) {
                return;
            }
            let pairs = hash_pairs(cx, hash_node);
            check_column_node(cx, type_node, &pairs);
        }
        "add_reference" => {
            // `(send nil? :add_reference _ _ (hash $...))`
            if args.len() != 3 {
                return;
            }
            let hash_node = args[2];
            if !matches!(*cx.kind(hash_node), NodeKind::Hash(_)) {
                return;
            }
            check_pairs(cx, &hash_pairs(cx, hash_node));
        }
        _ => {}
    }
}

fn check_change_table(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Block { call, args, body } = *cx.kind(node) else {
        return;
    };
    if cx.method_name(call) != Some("change_table") {
        return;
    }
    if cx.call_receiver(call).get().is_some() {
        return;
    }
    let table_sym = match block_single_arg(cx, args) {
        Some(s) => s,
        None => return,
    };
    let Some(body_id) = body.get() else {
        return;
    };
    // Begin-aware: `begin ? children : [body]`.
    let children: Vec<NodeId> = match *cx.kind(body_id) {
        NodeKind::Begin(list) => cx.list(list).to_vec(),
        _ => vec![body_id],
    };
    for child in children {
        check_change_table_child(cx, child, table_sym);
    }
}

fn check_change_table_child(cx: &Cx<'_>, child: NodeId, table: Symbol) {
    let NodeKind::Send { receiver, method, .. } = *cx.kind(child) else {
        return;
    };
    let Some(recv) = receiver.get() else {
        return;
    };
    // `(lvar $_)` with matching table var.
    let NodeKind::Lvar(sym) = *cx.kind(recv) else {
        return;
    };
    if cx.symbol_str(sym) != cx.symbol_str(table) {
        return;
    }
    let method_name = cx.symbol_str(method).to_owned();
    let args = cx.call_arguments(child).to_vec();
    match method_name.as_str() {
        "column" => {
            // `(send (lvar $_) :column _ $_ (hash $...))`
            if args.len() != 3 {
                return;
            }
            let type_node = args[1];
            let hash_node = args[2];
            if !matches!(*cx.kind(hash_node), NodeKind::Hash(_)) {
                return;
            }
            check_column_node(cx, type_node, &hash_pairs(cx, hash_node));
        }
        "add_reference" => {
            if args.is_empty() {
                return;
            }
            let Some(&last) = args.last() else {
                return;
            };
            if !matches!(*cx.kind(last), NodeKind::Hash(_)) {
                return;
            }
            check_pairs(cx, &hash_pairs(cx, last));
        }
        "references" => {
            if args.is_empty() {
                return;
            }
            let Some(&last) = args.last() else {
                return;
            };
            if !matches!(*cx.kind(last), NodeKind::Hash(_)) {
                return;
            }
            check_pairs(cx, &hash_pairs(cx, last));
        }
        _ => {
            // Shortcut: `t.string :name, null: false` — method is the type.
            if args.len() != 2 {
                return;
            }
            let hash_node = args[1];
            if !matches!(*cx.kind(hash_node), NodeKind::Hash(_)) {
                return;
            }
            check_column_method(cx, &method_name, &hash_pairs(cx, hash_node));
        }
    }
}

fn block_single_arg(cx: &Cx<'_>, args_id: NodeId) -> Option<Symbol> {
    let NodeKind::Args(list) = *cx.kind(args_id) else {
        return None;
    };
    let params = cx.list(list);
    if params.len() != 1 {
        return None;
    }
    if let NodeKind::Arg(sym) = *cx.kind(params[0]) {
        Some(sym)
    } else {
        None
    }
}

fn hash_pairs(cx: &Cx<'_>, hash: NodeId) -> Vec<NodeId> {
    let NodeKind::Hash(list) = *cx.kind(hash) else {
        return Vec::new();
    };
    cx.list(list).to_vec()
}

fn is_database_mysql(cx: &Cx<'_>) -> bool {
    let opts = cx.options_or_default::<NotNullColumnOptions>();
    matches!(opts.database.as_deref(), Some("mysql"))
}

fn type_is_virtual_node(cx: &Cx<'_>, id: NodeId) -> bool {
    match *cx.kind(id) {
        NodeKind::Sym(sym) => cx.symbol_str(sym) == "virtual",
        NodeKind::Str(sid) => cx.string_str(sid) == "virtual",
        _ => false,
    }
}

fn type_is_text_node(cx: &Cx<'_>, id: NodeId) -> bool {
    match *cx.kind(id) {
        NodeKind::Sym(sym) => cx.symbol_str(sym) == "text",
        NodeKind::Str(sid) => cx.string_str(sid) == "text",
        _ => false,
    }
}

fn check_column_node(cx: &Cx<'_>, type_node: NodeId, pairs: &[NodeId]) {
    if type_is_virtual_node(cx, type_node) {
        return;
    }
    if type_is_text_node(cx, type_node) && is_database_mysql(cx) {
        return;
    }
    check_pairs(cx, pairs);
}

fn check_column_method(cx: &Cx<'_>, method: &str, pairs: &[NodeId]) {
    if method == "virtual" {
        return;
    }
    if method == "text" && is_database_mysql(cx) {
        return;
    }
    check_pairs(cx, pairs);
}

fn pair_key(cx: &Cx<'_>, pair: NodeId) -> Option<String> {
    let NodeKind::Pair { key, .. } = *cx.kind(pair) else {
        return None;
    };
    match *cx.kind(key) {
        NodeKind::Sym(sym) => Some(cx.symbol_str(sym).to_owned()),
        _ => None,
    }
}

fn pair_value(cx: &Cx<'_>, pair: NodeId) -> Option<NodeId> {
    let NodeKind::Pair { value, .. } = *cx.kind(pair) else {
        return None;
    };
    Some(value)
}

fn is_null_false(cx: &Cx<'_>, pair: NodeId) -> bool {
    if pair_key(cx, pair).as_deref() != Some("null") {
        return false;
    }
    matches!(*cx.kind(pair_value(cx, pair).unwrap()), NodeKind::False_)
}

fn has_non_nil_default(cx: &Cx<'_>, pairs: &[NodeId]) -> bool {
    for &p in pairs {
        if pair_key(cx, p).as_deref() != Some("default") {
            continue;
        }
        let Some(v) = pair_value(cx, p) else {
            continue;
        };
        // `(pair (sym :default) !nil)` — default: nil does NOT exempt.
        if !matches!(*cx.kind(v), NodeKind::Nil) {
            return true;
        }
    }
    false
}

fn check_pairs(cx: &Cx<'_>, pairs: &[NodeId]) {
    if has_non_nil_default(cx, pairs) {
        return;
    }
    let Some(null_pair) = pairs.iter().find(|&&p| is_null_false(cx, p)) else {
        return;
    };
    cx.emit_offense(cx.range(*null_pair), MSG, None);
}

#[cfg(test)]
mod tests {
    use super::{NotNullColumn, NotNullColumnOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_add_column_null_false() {
        test::<NotNullColumn>().expect_offense(indoc! {r#"
            add_column :users, :name, :string, null: false
                                               ^^^^^^^^^^^ Do not add a NOT NULL column without a default value.
        "#});
    }

    #[test]
    fn allows_add_column_with_default() {
        test::<NotNullColumn>().expect_no_offenses(
            "add_column :users, :name, :string, null: false, default: \"\"\n",
        );
    }

    #[test]
    fn allows_variable_type_with_default() {
        test::<NotNullColumn>()
            .expect_no_offenses("add_column(:users, :name, type, default: 'default')\n");
    }

    #[test]
    fn flags_add_column_default_nil() {
        test::<NotNullColumn>().expect_offense(indoc! {r#"
            add_column :users, :name, :string, null: false, default: nil
                                               ^^^^^^^^^^^ Do not add a NOT NULL column without a default value.
        "#});
    }

    #[test]
    fn allows_virtual_columns() {
        test::<NotNullColumn>().expect_no_offenses(indoc! {r#"
            add_column :users, :height_in, :virtual, as: "height_cm / 2.54", null: false, default: nil
        "#});
        test::<NotNullColumn>().expect_no_offenses(indoc! {r#"
            add_column :users, :height_in, 'virtual', as: "height_cm / 2.54", null: false, default: nil
        "#});
    }

    #[test]
    fn allows_null_true() {
        test::<NotNullColumn>()
            .expect_no_offenses("add_column :users, :name, :string, null: true\n");
    }

    #[test]
    fn allows_no_options() {
        test::<NotNullColumn>().expect_no_offenses("add_column :users, :name, :string\n");
    }

    #[test]
    fn allows_change_column() {
        test::<NotNullColumn>().expect_no_offenses(indoc! {r#"
            add_column :users, :name, :string
            User.update_all(name: "dummy")
            change_column :users, :name, :string, null: false
        "#});
    }

    #[test]
    fn allows_create_table() {
        test::<NotNullColumn>().expect_no_offenses(indoc! {r#"
            class CreateUsersTable < ActiveRecord::Migration
              def change
                create_table :users do |t|
                  t.string :name, null: false
                  t.timestamps null: false
                end
              end
            end
        "#});
    }

    #[test]
    fn allows_empty_change_table() {
        test::<NotNullColumn>().expect_no_offenses(indoc! {r#"
            class ExampleMigration < ActiveRecord::Migration[7.0]
              def change
                change_table :invoices do |t|
                end
              end
            end
        "#});
    }

    #[test]
    fn flags_change_table_shortcut() {
        test::<NotNullColumn>().expect_offense(indoc! {r#"
            def change
              change_table :users do |t|
                t.string :name, null: false
                                ^^^^^^^^^^^ Do not add a NOT NULL column without a default value.
              end
            end
        "#});
    }

    #[test]
    fn flags_change_table_multiple() {
        test::<NotNullColumn>().expect_offense(indoc! {r#"
            def change
              change_table :users do |t|
                t.string :name, null: false
                                ^^^^^^^^^^^ Do not add a NOT NULL column without a default value.
                t.string :address, null: false
                                   ^^^^^^^^^^^ Do not add a NOT NULL column without a default value.
              end
            end
        "#});
    }

    #[test]
    fn allows_change_table_with_default() {
        test::<NotNullColumn>().expect_no_offenses(indoc! {r#"
            def change
              change_table :users do |t|
                t.string :name, null: false, default: ""
              end
            end
        "#});
    }

    #[test]
    fn allows_change_table_no_options() {
        test::<NotNullColumn>().expect_no_offenses(indoc! {r#"
            def change
              change_table :users do |t|
                t.string :name
              end
            end
        "#});
    }

    #[test]
    fn flags_change_table_column() {
        test::<NotNullColumn>().expect_offense(indoc! {r#"
            def change
              change_table :users do |t|
                t.column :name, :string, null: false
                                         ^^^^^^^^^^^ Do not add a NOT NULL column without a default value.
              end
            end
        "#});
    }

    #[test]
    fn allows_change_table_column_with_default() {
        test::<NotNullColumn>().expect_no_offenses(indoc! {r#"
            def change
              change_table :users do |t|
                t.column :name, :string, null: false, default: ""
              end
            end
        "#});
    }

    #[test]
    fn flags_change_table_references() {
        test::<NotNullColumn>().expect_offense(indoc! {r#"
            def change
              change_table :users do |t|
                t.references :address, null: false
                                       ^^^^^^^^^^^ Do not add a NOT NULL column without a default value.
              end
            end
        "#});
    }

    #[test]
    fn allows_change_table_references_no_options() {
        test::<NotNullColumn>().expect_no_offenses(indoc! {r#"
            def change
              change_table :users do |t|
                t.references :address
              end
            end
        "#});
    }

    #[test]
    fn flags_add_reference() {
        test::<NotNullColumn>().expect_offense(indoc! {r#"
            add_reference :products, :category, null: false
                                                ^^^^^^^^^^^ Do not add a NOT NULL column without a default value.
        "#});
    }

    #[test]
    fn allows_add_reference_with_default() {
        test::<NotNullColumn>().expect_no_offenses(
            "add_reference :products, :category, null: false, default: 1\n",
        );
    }

    #[test]
    fn allows_add_reference_no_options() {
        test::<NotNullColumn>().expect_no_offenses("add_reference :products, :category\n");
    }

    #[test]
    fn allows_text_with_mysql() {
        let opts = NotNullColumnOptions {
            database: Some("mysql".to_owned()),
        };
        test::<NotNullColumn>()
            .with_options(&opts)
            .expect_no_offenses("add_column :articles, :content, :text, null: false\n");
        test::<NotNullColumn>()
            .with_options(&opts)
            .expect_no_offenses("add_column :articles, :content, 'text', null: false\n");
    }

    #[test]
    fn flags_text_with_postgres() {
        let opts = NotNullColumnOptions {
            database: Some("postgresql".to_owned()),
        };
        test::<NotNullColumn>().with_options(&opts).expect_offense(indoc! {r#"
            add_column :articles, :content, :text, null: false
                                                   ^^^^^^^^^^^ Do not add a NOT NULL column without a default value.
        "#});
    }

    #[test]
    fn flags_text_by_default() {
        test::<NotNullColumn>().expect_offense(indoc! {r#"
            add_column :articles, :content, :text, null: false
                                                   ^^^^^^^^^^^ Do not add a NOT NULL column without a default value.
        "#});
    }
}
murphy_plugin_api::submit_cop!(NotNullColumn);
