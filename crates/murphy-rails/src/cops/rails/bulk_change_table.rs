//! `Rails/BulkChangeTable` — combine alter queries with `bulk: true`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/BulkChangeTable
//! upstream_version_checked: 2.35.0
//! version_added: "0.57"
//! version_changed: "2.20"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: `def change/up/down` body scan for
//!   consecutive combinable alter methods on the same literal table
//!   (first-of-group offense), and `change_table` without `bulk:`
//!   flagging >1 combinable transformations (`t.remove` counts non-hash
//!   args). Database-gated via the `Database` option (`mysql` always;
//!   `postgresql` needs TargetRailsVersion >= 5.2; `change_null` /
//!   `change_column_null` need >= 6.1); unset or unsupported Database is
//!   silent. `database.yml`/`DATABASE_URL` auto-detection remains absent
//!   (cf. NotNullColumn). Include (`db/**/*.rb`) is enforced via the
//!   murphy-rails pack default.yml (engine `cop_applies_to_file` gate).
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct BulkChangeTable;

#[derive(CopOptions)]
pub struct BulkChangeTableOptions {
    #[option(
        name = "Database",
        description = "Database adapter name (`mysql` or `postgresql`); unset disables the cop."
    )]
    pub database: Option<String>,
}

#[cop(
    name = "Rails/BulkChangeTable",
    description = "Check whether alter queries are combinable.",
    default_severity = "warning",
    default_enabled = true,
    options = BulkChangeTableOptions,
)]
impl BulkChangeTable {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        check_def(node, cx);
    }

    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, body, .. } = *cx.kind(node) else {
            return;
        };
        check_change_table_block(cx, call, body.get());
    }

    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Numblock { send, body, .. } = *cx.kind(node) else {
            return;
        };
        check_change_table_block(cx, send, body.get());
    }

    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Itblock { send, body } = *cx.kind(node) else {
            return;
        };
        check_change_table_block(cx, send, body.get());
    }
}

const MSG_FOR_CHANGE_TABLE: &str = "You can combine alter queries using `bulk: true` options.";

fn alter_message(table: &str) -> String {
    format!("You can use `change_table :{table}, bulk: true` to combine alter queries.")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Database {
    Mysql,
    Postgresql,
}

fn resolve_database(cx: &Cx<'_>) -> Option<Database> {
    let opts = cx.options_or_default::<BulkChangeTableOptions>();
    match opts.database.as_deref()?.to_ascii_lowercase().as_str() {
        "mysql" | "mysql2" | "trilogy" => Some(Database::Mysql),
        "postgresql" | "postgres" | "postgis" => Some(Database::Postgresql),
        _ => None,
    }
}

fn supports_bulk_alter(cx: &Cx<'_>, db: Database) -> bool {
    match db {
        Database::Mysql => true,
        // Bulk alter support for PostgreSQL landed in Rails 5.2
        // (https://github.com/rails/rails/pull/31331). Unset
        // TargetRailsVersion assumes newest (`rails_version_at_least`
        // is `is_none_or`), matching RuboCop's recent default.
        Database::Postgresql => cx.rails_version_at_least(5, 2),
    }
}

fn is_combinable_alter(cx: &Cx<'_>, db: Database, method: &str) -> bool {
    match method {
        "add_column" | "remove_column" | "remove_columns" | "change_column" | "add_timestamps"
        | "remove_timestamps" => true,
        "rename_column" | "add_index" | "remove_index" if db == Database::Mysql => true,
        "change_column_default" if db == Database::Postgresql => true,
        "change_column_null"
            if db == Database::Postgresql && cx.rails_version_at_least(6, 1) =>
        {
            true
        }
        _ => false,
    }
}

fn is_combinable_transform(cx: &Cx<'_>, db: Database, method: &str) -> bool {
    match method {
        "primary_key" | "column" | "string" | "text" | "integer" | "bigint" | "float"
        | "decimal" | "numeric" | "datetime" | "timestamp" | "time" | "date" | "binary"
        | "boolean" | "json" | "virtual" | "remove" | "change" | "timestamps"
        | "remove_timestamps" => true,
        "rename" | "index" | "remove_index" if db == Database::Mysql => true,
        "change_default" if db == Database::Postgresql => true,
        "change_null" if db == Database::Postgresql && cx.rails_version_at_least(6, 1) => true,
        _ => false,
    }
}

fn check_def(node: NodeId, cx: &Cx<'_>) {
    let Some(db) = resolve_database(cx) else {
        return;
    };
    if !supports_bulk_alter(cx, db) {
        return;
    }
    let NodeKind::Def {
        receiver, name, body, ..
    } = *cx.kind(node)
    else {
        return;
    };
    if receiver.get().is_some() {
        return;
    }
    let method = cx.symbol_str(name).to_owned();
    if !matches!(method.as_str(), "change" | "up" | "down") {
        return;
    }
    let Some(body_id) = body.get() else {
        return;
    };
    // Upstream `node.body.child_nodes`: direct children of the def body.
    let children: Vec<NodeId> = match *cx.kind(body_id) {
        NodeKind::Begin(list) | NodeKind::Kwbegin(list) => cx.list(list).to_vec(),
        _ => vec![body_id],
    };
    // Upstream `AlterMethodsRecorder`: consecutive combinable alter runs on
    // the same literal table; only the first node of a run (size > 1) flags.
    // Anything else (non-send, non-combinable, non-literal table) flushes.
    let mut run: Vec<NodeId> = Vec::new();
    let flush = |run: &mut Vec<NodeId>, cx: &Cx<'_>| {
        if run.len() > 1 {
            add_offense_for_alter_method(cx, run[0]);
        }
        run.clear();
    };
    for child in children {
        if is_combinable_alter_send(cx, db, child) {
            match table_key(cx, child) {
                Some(key)
                    if run.iter().all(|&n| table_key(cx, n).as_deref() == Some(key.as_str())) =>
                {
                    run.push(child);
                }
                Some(_) => {
                    flush(&mut run, cx);
                    run.push(child);
                }
                None => flush(&mut run, cx),
            }
        } else {
            flush(&mut run, cx);
        }
    }
    flush(&mut run, cx);
}

/// Upstream `call_to_combinable_alter_method?`: `send` type (not `csend`)
/// with a combinable method name. No receiver gate upstream.
fn is_combinable_alter_send(cx: &Cx<'_>, db: Database, node: NodeId) -> bool {
    if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
        return false;
    }
    let method = cx.method_name(node).unwrap_or("").to_owned();
    is_combinable_alter(cx, db, &method)
}

/// Literal table name (`sym`/`str`), normalized to its string value so
/// mixed `:users`/`"users"` styles compare equal (upstream `value.to_s`).
/// Non-literal first args (variables, dynamic strings) yield `None` and
/// flush the recorder upstream.
fn table_key(cx: &Cx<'_>, send: NodeId) -> Option<String> {
    let args = cx.call_arguments(send);
    let first = *args.first()?;
    match *cx.kind(first) {
        NodeKind::Sym(sym) => Some(cx.symbol_str(sym).to_owned()),
        NodeKind::Str(sid) => Some(cx.string_str(sid).to_owned()),
        _ => None,
    }
}

fn add_offense_for_alter_method(cx: &Cx<'_>, node: NodeId) {
    let Some(table) = table_key(cx, node) else {
        return;
    };
    cx.emit_offense(cx.range(node), &alter_message(&table), None);
}

fn check_change_table_block(cx: &Cx<'_>, call: NodeId, body: Option<NodeId>) {
    let Some(db) = resolve_database(cx) else {
        return;
    };
    if !supports_bulk_alter(cx, db) {
        return;
    }
    if !matches!(*cx.kind(call), NodeKind::Send { .. }) {
        return;
    }
    // Upstream `node.command?(:change_table)` — bare call.
    if cx.method_name(call) != Some("change_table") {
        return;
    }
    if cx.call_receiver(call).get().is_some() {
        return;
    }
    if has_bulk_option(cx, call) {
        return;
    }
    let Some(body_id) = body else {
        return;
    };
    // Upstream `body.send_type? ? [body] : body.each_child_node(:send)`.
    let sends: Vec<NodeId> = if matches!(*cx.kind(body_id), NodeKind::Send { .. }) {
        vec![body_id]
    } else {
        cx.children(body_id)
            .into_iter()
            .filter(|&c| matches!(*cx.kind(c), NodeKind::Send { .. }))
            .collect()
    };
    if count_transformations(cx, db, &sends) > 1 {
        cx.emit_offense(change_table_offense_range(cx, call), MSG_FOR_CHANGE_TABLE, None);
    }
}

/// Upstream flags the `change_table` send (`change_table :users`), but the
/// arena `Send` inside a `Block` wrapper spans through the block header
/// (`change_table :users do |t|`), so trim the range to the last argument.
fn change_table_offense_range(cx: &Cx<'_>, call: NodeId) -> Range {
    let start = cx.range(call).start;
    let end = cx
        .call_arguments(call)
        .iter()
        .map(|&a| cx.range(a).end)
        .max()
        .unwrap_or_else(|| cx.loc(call).name.end);
    Range { start, end }
}

fn count_transformations(cx: &Cx<'_>, db: Database, sends: &[NodeId]) -> usize {
    sends
        .iter()
        .map(|&node| {
            let method = cx.method_name(node).unwrap_or("").to_owned();
            if method == "remove" {
                // `t.remove :a, :b` counts columns; trailing options hash excluded.
                cx.call_arguments(node)
                    .iter()
                    .filter(|&&a| !matches!(*cx.kind(a), NodeKind::Hash(_)))
                    .count()
            } else if is_combinable_transform(cx, db, &method) {
                1
            } else {
                0
            }
        })
        .sum()
}

/// Upstream `include_bulk_options?`: second argument is a hash holding a
/// `:bulk` key (either `bulk: true` or `bulk: false` exempts).
fn has_bulk_option(cx: &Cx<'_>, call: NodeId) -> bool {
    let args = cx.call_arguments(call);
    if args.len() < 2 {
        return false;
    }
    let opts = args[1];
    if !matches!(*cx.kind(opts), NodeKind::Hash(_)) {
        return false;
    }
    let NodeKind::Hash(list) = *cx.kind(opts) else {
        return false;
    };
    cx.list(list).iter().any(|&pair| {
        if !matches!(*cx.kind(pair), NodeKind::Pair { .. }) {
            return false;
        }
        let NodeKind::Pair { key, .. } = *cx.kind(pair) else {
            return false;
        };
        matches!(*cx.kind(key), NodeKind::Sym(sym) if cx.symbol_str(sym) == "bulk")
    })
}

#[cfg(test)]
mod tests {
    use super::{BulkChangeTable, BulkChangeTableOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    fn mysql() -> BulkChangeTableOptions {
        BulkChangeTableOptions {
            database: Some("mysql".to_owned()),
        }
    }

    fn postgresql() -> BulkChangeTableOptions {
        BulkChangeTableOptions {
            database: Some("postgresql".to_owned()),
        }
    }

    #[test]
    fn silent_without_database_change_table() {
        test::<BulkChangeTable>().expect_no_offenses(indoc! {r#"
            def change
              change_table :users do |t|
                t.string :name, null: false
                t.string :address, null: true
              end
            end
        "#});
    }

    #[test]
    fn silent_without_database_alter_methods() {
        test::<BulkChangeTable>().expect_no_offenses(indoc! {r#"
            def change
              add_column :users, :name, :string, null: false
              remove_column :users, :nickname
            end
        "#});
    }

    #[test]
    fn silent_with_unsupported_database() {
        let opts = BulkChangeTableOptions {
            database: Some("sqlite".to_owned()),
        };
        test::<BulkChangeTable>().with_options(&opts).expect_no_offenses(indoc! {r#"
            def change
              change_table :users do |t|
                t.string :name, null: false
                t.string :address, null: true
              end
            end
        "#});
    }

    #[test]
    fn mysql_flags_change_table() {
        test::<BulkChangeTable>().with_options(&mysql()).expect_offense(indoc! {r#"
            def change
              change_table :users do |t|
              ^^^^^^^^^^^^^^^^^^^ You can combine alter queries using `bulk: true` options.
                t.string :name, null: false
                t.string :address, null: true
              end
            end
        "#});
    }

    #[test]
    fn mysql_flags_alter_methods() {
        test::<BulkChangeTable>().with_options(&mysql()).expect_offense(indoc! {r#"
            def change
              add_column :users, :name, :string, null: false
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ You can use `change_table :users, bulk: true` to combine alter queries.
              remove_column :users, :nickname
            end
        "#});
    }

    #[test]
    fn mysql_groups_by_table() {
        test::<BulkChangeTable>().with_options(&mysql()).expect_offense(indoc! {r#"
            def change
              add_reference :users, :team
              add_column :users, :name, :string, null: false
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ You can use `change_table :users, bulk: true` to combine alter queries.
              remove_column :users, :nickname
              remove_column :users, :flag
              add_column :teams, :owner_name, :string, null: false
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ You can use `change_table :teams, bulk: true` to combine alter queries.
              add_column :teams, :member_count, :integer, null: false
              User.reset_column_information
              User.all.each do |user|
                user.refresh!
              end
              remove_column :users, :name
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^ You can use `change_table :users, bulk: true` to combine alter queries.
              remove_column :users, :metadata
            end
        "#});
    }

    #[test]
    fn mysql_bulk_true_exempts() {
        test::<BulkChangeTable>().with_options(&mysql()).expect_no_offenses(indoc! {r#"
            def change
              change_table :users, bulk: true do |t|
                t.string :name, null: false
                t.string :address, null: true
              end
            end
        "#});
    }

    #[test]
    fn mysql_bulk_false_exempts() {
        test::<BulkChangeTable>().with_options(&mysql()).expect_no_offenses(indoc! {r#"
            def change
              change_table :users, bulk: false do |t|
                t.string :name, null: false
                t.string :address, null: true
              end
            end
        "#});
    }

    #[test]
    fn mysql_single_transformation_no_offense() {
        test::<BulkChangeTable>().with_options(&mysql()).expect_no_offenses(indoc! {r#"
            def change
              change_table :users do |t|
                t.string :name, null: false
              end
            end
        "#});
    }

    #[test]
    fn mysql_non_combinable_transformation_no_offense() {
        test::<BulkChangeTable>().with_options(&mysql()).expect_no_offenses(indoc! {r#"
            def change
              change_table :users do |t|
                t.belongs_to :team
                t.string :name, null: false
              end
            end
        "#});
    }

    #[test]
    fn mysql_single_remove_one_column_no_offense() {
        test::<BulkChangeTable>().with_options(&mysql()).expect_no_offenses(indoc! {r#"
            def change
              change_table :users do |t|
                t.remove :name
              end
            end
        "#});
    }

    #[test]
    fn mysql_single_remove_multi_column_offense() {
        test::<BulkChangeTable>().with_options(&mysql()).expect_offense(indoc! {r#"
            def change
              change_table :users do |t|
              ^^^^^^^^^^^^^^^^^^^ You can combine alter queries using `bulk: true` options.
                t.remove :name, :metadata
              end
            end
        "#});
    }

    #[test]
    fn mysql_single_remove_one_column_with_options_no_offense() {
        test::<BulkChangeTable>().with_options(&mysql()).expect_no_offenses(indoc! {r#"
            def change
              change_table :users do |t|
                t.remove :name, type: :string
              end
            end
        "#});
    }

    #[test]
    fn mysql_change_table_no_block_no_offense() {
        test::<BulkChangeTable>().with_options(&mysql()).expect_no_offenses(indoc! {r#"
            def up
              change_table(:users)
            end
        "#});
    }

    #[test]
    fn mysql_change_table_empty_block_no_offense() {
        test::<BulkChangeTable>().with_options(&mysql()).expect_no_offenses(indoc! {r#"
            def up
              change_table(:users) do
              end
            end
        "#});
    }

    #[test]
    fn mysql_non_combinable_alter_between_no_offense() {
        test::<BulkChangeTable>().with_options(&mysql()).expect_no_offenses(indoc! {r#"
            def change
              add_column :users, :name, :string, null: false
              add_reference :users, :team
              remove_column :users, :nickname
            end
        "#});
    }

    #[test]
    fn mysql_different_table_no_offense() {
        test::<BulkChangeTable>().with_options(&mysql()).expect_no_offenses(indoc! {r#"
            def change
              add_reference :users, :team
              add_column :users, :name, :string, null: false
              remove_column :teams, :owner_name
            end
        "#});
    }

    #[test]
    fn mysql_block_between_flushes() {
        test::<BulkChangeTable>().with_options(&mysql()).expect_no_offenses(indoc! {r#"
            def change
              add_column :users, :name, :string, null: false
              User.find_each do |user|
                user.update(name: user.nickname)
              end
              remove_column :users, :nickname
            end
        "#});
    }

    #[test]
    fn mysql_single_alter_no_offense() {
        test::<BulkChangeTable>().with_options(&mysql()).expect_no_offenses(indoc! {r#"
            def change
              add_column :users, :name, :string, null: false
            end
        "#});
    }

    #[test]
    fn mysql_string_table_offense() {
        test::<BulkChangeTable>().with_options(&mysql()).expect_offense(indoc! {r#"
            def change
              remove_index "users", :name
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^ You can use `change_table :users, bulk: true` to combine alter queries.
              remove_index "users", :address
            end
        "#});
    }

    #[test]
    fn mysql_mixed_table_style_offense() {
        test::<BulkChangeTable>().with_options(&mysql()).expect_offense(indoc! {r#"
            def change
              remove_index "users", :name
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^ You can use `change_table :users, bulk: true` to combine alter queries.
              remove_index :users, :address
            end
        "#});
    }

    #[test]
    fn mysql_variable_table_no_offense() {
        test::<BulkChangeTable>().with_options(&mysql()).expect_no_offenses(indoc! {r#"
            def change
              %w[owners members].each do |table|
                add_column table, :name, :string, null: false
              end
            end
        "#});
    }

    #[test]
    fn mysql_index_transformations_offense() {
        test::<BulkChangeTable>().with_options(&mysql()).expect_offense(indoc! {r#"
            def change
              change_table :users do |t|
              ^^^^^^^^^^^^^^^^^^^ You can combine alter queries using `bulk: true` options.
                t.index :name
                t.index :address
              end
            end
        "#});
    }

    #[test]
    fn mysql_remove_index_alter_offense() {
        test::<BulkChangeTable>().with_options(&mysql()).expect_offense(indoc! {r#"
            def change
              remove_index :users, :name
              ^^^^^^^^^^^^^^^^^^^^^^^^^^ You can use `change_table :users, bulk: true` to combine alter queries.
              remove_index :users, :address
            end
        "#});
    }

    #[test]
    fn mysql_ignores_postgres_only() {
        test::<BulkChangeTable>().with_options(&mysql()).expect_no_offenses(indoc! {r#"
            def change
              change_column_null :users, :name, false
              change_column_null :users, :address, false
            end
        "#});
        test::<BulkChangeTable>().with_options(&mysql()).expect_no_offenses(indoc! {r#"
            def change
              change_table :users do |t|
                t.change_null :name, false
                t.change_null :address, false
              end
            end
        "#});
    }

    #[test]
    fn mysql_empty_migration_no_offense() {
        test::<BulkChangeTable>().with_options(&mysql()).expect_no_offenses(indoc! {r#"
            class EmptyMigration < ActiveRecord::Migration[5.1]
              def change; end
            end
        "#});
    }

    #[test]
    fn mysql_other_def_names_ignored() {
        test::<BulkChangeTable>().with_options(&mysql()).expect_no_offenses(indoc! {r#"
            def helper
              add_column :users, :name, :string, null: false
              remove_column :users, :nickname
            end
        "#});
    }

    #[test]
    fn postgresql52_flags_base() {
        test::<BulkChangeTable>()
            .with_options(&postgresql())
            .with_target_rails_version(5, 2)
            .expect_offense(indoc! {r#"
                def change
                  change_table :users do |t|
                  ^^^^^^^^^^^^^^^^^^^ You can combine alter queries using `bulk: true` options.
                    t.string :name, null: false
                    t.string :address, null: true
                  end
                end
            "#});
    }

    #[test]
    fn postgresql52_flags_change_default() {
        test::<BulkChangeTable>()
            .with_options(&postgresql())
            .with_target_rails_version(5, 2)
            .expect_offense(indoc! {r#"
                def change
                  change_table :users do |t|
                  ^^^^^^^^^^^^^^^^^^^ You can combine alter queries using `bulk: true` options.
                    t.change_default :name, 'unknown'
                    t.change_default :address, nil
                  end
                end
            "#});
        test::<BulkChangeTable>()
            .with_options(&postgresql())
            .with_target_rails_version(5, 2)
            .expect_offense(indoc! {r#"
                def change
                  change_column_default :users, :name, false
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ You can use `change_table :users, bulk: true` to combine alter queries.
                  change_column_default :users, :address, false
                end
            "#});
    }

    #[test]
    fn postgresql52_ignores_change_null() {
        test::<BulkChangeTable>()
            .with_options(&postgresql())
            .with_target_rails_version(5, 2)
            .expect_no_offenses(indoc! {r#"
                def change
                  change_column_null :users, :name, false
                  change_column_null :users, :address, false
                end
            "#});
        test::<BulkChangeTable>()
            .with_options(&postgresql())
            .with_target_rails_version(5, 2)
            .expect_no_offenses(indoc! {r#"
                def change
                  change_table :users do |t|
                    t.change_null :name, false
                    t.change_null :address, false
                  end
                end
            "#});
    }

    #[test]
    fn postgresql52_ignores_mysql_only() {
        test::<BulkChangeTable>()
            .with_options(&postgresql())
            .with_target_rails_version(5, 2)
            .expect_no_offenses(indoc! {r#"
                def change
                  change_table :users do |t|
                    t.index :name
                    t.index :address
                  end
                end
            "#});
        test::<BulkChangeTable>()
            .with_options(&postgresql())
            .with_target_rails_version(5, 2)
            .expect_no_offenses(indoc! {r#"
                def change
                  remove_index :users, :name
                  remove_index :users, :address
                end
            "#});
    }

    #[test]
    fn postgresql51_silent() {
        test::<BulkChangeTable>()
            .with_options(&postgresql())
            .with_target_rails_version(5, 1)
            .expect_no_offenses(indoc! {r#"
                def change
                  change_table :users do |t|
                    t.string :name, null: false
                    t.string :address, null: true
                  end
                end
            "#});
        test::<BulkChangeTable>()
            .with_options(&postgresql())
            .with_target_rails_version(5, 1)
            .expect_no_offenses(indoc! {r#"
                def change
                  add_column :users, :name, :string, null: false
                  remove_column :users, :nickname
                end
            "#});
    }

    #[test]
    fn postgresql61_flags_change_null() {
        test::<BulkChangeTable>()
            .with_options(&postgresql())
            .with_target_rails_version(6, 1)
            .expect_offense(indoc! {r#"
                def change
                  change_column_null :users, :name, false
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ You can use `change_table :users, bulk: true` to combine alter queries.
                  change_column_null :users, :address, false
                end
            "#});
        test::<BulkChangeTable>()
            .with_options(&postgresql())
            .with_target_rails_version(6, 1)
            .expect_offense(indoc! {r#"
                def change
                  change_table :users do |t|
                  ^^^^^^^^^^^^^^^^^^^ You can combine alter queries using `bulk: true` options.
                    t.change_null :name, false
                    t.change_null :address, false
                  end
                end
            "#});
    }

    #[test]
    fn postgresql61_bulk_true_exempts_change_null() {
        test::<BulkChangeTable>()
            .with_options(&postgresql())
            .with_target_rails_version(6, 1)
            .expect_no_offenses(indoc! {r#"
                def change
                  change_table :users, bulk: true do |t|
                    t.change_null :name, false
                    t.change_null :address, false
                  end
                end
            "#});
    }
}
murphy_plugin_api::submit_cop!(BulkChangeTable);
