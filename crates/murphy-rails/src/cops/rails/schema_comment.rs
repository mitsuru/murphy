//! `Rails/SchemaComment` — require `comment` on new tables and columns.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/SchemaComment
//! upstream_version_checked: 2.35.0
//! version_added: "2.13"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: bare `add_column` (3-4 args) and
//!   `create_table` (1+ args) without a `comment:`/`"comment"` hash entry
//!   flag whole sends (`New database column/table without `comment``). A
//!   `create_table` with a comment and a block scans the block body for
//!   column-definition sends (`RAILS_ABSTRACT_SCHEMA_DEFINITIONS` +
//!   helpers + postgres + mysql sets) without comments. `comment: nil`
//!   and `comment: ""` count as missing, matching upstream
//!   `!{nil (str blank?)}`. When the table itself lacks a comment only
//!   the table offense fires (upstream `elsif` chain). No autocorrect.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

const COLUMN_MSG: &str = "New database column without `comment`.";
const TABLE_MSG: &str = "New database table without `comment`.";

const COLUMN_METHODS: &[&str] = &[
    // RAILS_ABSTRACT_SCHEMA_DEFINITIONS
    "bigint", "binary", "boolean", "date", "datetime", "decimal", "float", "integer", "json",
    "string", "text", "time", "timestamp", "virtual",
    // RAILS_ABSTRACT_SCHEMA_DEFINITIONS_HELPERS
    "column", "references", "belongs_to", "primary_key", "numeric",
    // POSTGRES_SCHEMA_DEFINITIONS
    "bigserial", "bit", "bit_varying", "cidr", "citext", "daterange", "hstore", "inet",
    "interval", "int4range", "int8range", "jsonb", "ltree", "macaddr", "money", "numrange", "oid",
    "point", "line", "lseg", "box", "path", "polygon", "circle", "serial", "tsrange", "tstzrange",
    "tsvector", "uuid", "xml",
    // MYSQL_SCHEMA_DEFINITIONS
    "blob", "tinyblob", "mediumblob", "longblob", "tinytext", "mediumtext", "longtext",
    "unsigned_integer", "unsigned_bigint", "unsigned_float", "unsigned_decimal",
];

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SchemaComment;

#[cop(
    name = "Rails/SchemaComment",
    description = "Enforces the use of the `comment` option when adding a new table or column.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl SchemaComment {
    // Mirrors upstream `RESTRICT_ON_SEND = %i[add_column create_table]`.
    #[on_node(kind = "send", methods = ["add_column", "create_table"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
        return;
    }
    // Upstream `(send nil? ...)`: bare calls only.
    if cx.call_receiver(node).get().is_some() {
        return;
    }
    let method = match cx.method_name(node) {
        Some(m) => m.to_owned(),
        None => return,
    };
    if method == "add_column" {
        if add_column_without_comment(cx, node) {
            cx.emit_offense(cx.range(node), COLUMN_MSG, None);
        }
    } else if method == "create_table" {
        if create_table_without_comment(cx, node) {
            // The arena send range covers an attached block; upstream
            // reports the `create_table` send alone.
            let end = send_only_end(cx, node);
            cx.emit_offense(
                murphy_plugin_api::Range {
                    start: cx.range(node).start,
                    end,
                },
                TABLE_MSG,
                None,
            );
        } else if let Some(block) = cx.block_node(node).get() {
            check_columns_in_block(cx, block);
        }
    }
}

/// Upstream `add_column?` (3-4 args) without `add_column_with_comment?`.
fn add_column_without_comment(cx: &Cx<'_>, node: NodeId) -> bool {
    let args = cx.call_arguments(node);
    if args.len() < 3 || args.len() > 4 {
        return false;
    }
    if args.len() == 3 {
        return true;
    }
    !hash_has_comment(cx, args[3])
}

/// Upstream `create_table?` (1+ args) without `create_table_with_comment?`.
fn create_table_without_comment(cx: &Cx<'_>, node: NodeId) -> bool {
    let args = cx.call_arguments(node);
    if args.is_empty() {
        return false;
    }
    for &arg in args {
        if matches!(*cx.kind(arg), NodeKind::Hash(_)) && hash_has_comment(cx, arg) {
            return false;
        }
    }
    true
}

/// Upstream `comment_present?`: hash contains `(pair {(sym :comment)
/// (str "comment")} !{nil (str blank?)})`.
fn hash_has_comment(cx: &Cx<'_>, hash: NodeId) -> bool {
    if !matches!(*cx.kind(hash), NodeKind::Hash(_)) {
        return false;
    }
    for child in cx.children(hash) {
        if !matches!(*cx.kind(child), NodeKind::Pair { .. }) {
            continue;
        }
        let NodeKind::Pair { key, value } = *cx.kind(child) else {
            continue;
        };
        let is_comment_key = match *cx.kind(key) {
            NodeKind::Sym(sym) => cx.symbol_str(sym) == "comment",
            NodeKind::Str(sid) => cx.string_str(sid) == "comment",
            _ => false,
        };
        if !is_comment_key {
            continue;
        }
        // `!{nil (str blank?)}`: nil and blank-string values count as missing.
        if matches!(*cx.kind(value), NodeKind::Nil) {
            continue;
        }
        if let NodeKind::Str(sid) = *cx.kind(value)
            && cx.string_str(sid).is_empty()
        {
            continue;
        }
        return true;
    }
    false
}

/// Upstream `check_column_within_create_table_block(node.parent.body)`.
fn check_columns_in_block(cx: &Cx<'_>, block: NodeId) {
    let body = match *cx.kind(block) {
        NodeKind::Block { body, .. } => body.get(),
        NodeKind::Numblock { body, .. } => body.get(),
        NodeKind::Itblock { body, .. } => body.get(),
        _ => None,
    };
    let Some(body) = body else {
        return;
    };
    // Single statement or begin-wrapped list.
    let stmts: Vec<NodeId> = match *cx.kind(body) {
        NodeKind::Begin(list) => cx.list(list).to_vec(),
        _ => vec![body],
    };
    for stmt in stmts {
        if t_column_without_comment(cx, stmt) {
            cx.emit_offense(cx.range(stmt), COLUMN_MSG, None);
        }
    }
}

/// Upstream `t_column?` without `t_column_with_comment?`.
fn t_column_without_comment(cx: &Cx<'_>, node: NodeId) -> bool {
    if !matches!(
        *cx.kind(node),
        NodeKind::Send { .. } | NodeKind::Csend { .. }
    ) {
        return false;
    }
    let method = match cx.method_name(node) {
        Some(m) => m,
        None => return false,
    };
    if !COLUMN_METHODS.contains(&method) {
        return false;
    }
    let args = cx.call_arguments(node);
    if args.is_empty() {
        return false;
    }
    // `t_column_with_comment?`: `(send _ METHODS _column _type? #comment_present?)`.
    // Comment hash may be the last arg (after column and optional type).
    for &arg in args {
        if matches!(*cx.kind(arg), NodeKind::Hash(_)) && hash_has_comment(cx, arg) {
            return false;
        }
    }
    true
}

/// Send-only end: the arena send range covers an attached block, while
/// upstream reports the `create_table` send alone.
fn send_only_end(cx: &Cx<'_>, node: NodeId) -> u32 {
    if cx.block_node(node).get().is_none() {
        return cx.range(node).end;
    }
    if let Some(&last) = cx.call_arguments(node).last() {
        return cx.range(last).end;
    }
    cx.loc(node).name.end
}

#[cfg(test)]
mod tests {
    use super::SchemaComment;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_add_column_without_comment() {
        test::<SchemaComment>().expect_offense(indoc! {r#"
            add_column :table, :column, :integer
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ New database column without `comment`.
        "#});
    }

    #[test]
    fn allows_add_column_with_comment() {
        test::<SchemaComment>().expect_no_offenses(
            "add_column :table, :column, :integer, comment: 'Number of offenses'\n",
        );
    }

    #[test]
    fn flags_add_column_nil_comment() {
        test::<SchemaComment>().expect_offense(indoc! {r#"
            add_column :table, :column, :integer, comment: nil
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ New database column without `comment`.
        "#});
    }

    #[test]
    fn flags_create_table_without_comment() {
        test::<SchemaComment>().expect_offense(indoc! {r#"
            create_table :table do |t|
            ^^^^^^^^^^^^^^^^^^^ New database table without `comment`.
              t.string :name
            end
        "#});
    }

    #[test]
    fn allows_create_table_with_comment() {
        test::<SchemaComment>().expect_no_offenses(indoc! {r#"
            create_table :table, comment: 'Table of offenses data' do |t|
              t.string :name, comment: 'Number of offenses'
            end
        "#});
    }

    #[test]
    fn flags_column_without_comment_in_block() {
        test::<SchemaComment>().expect_offense(indoc! {r#"
            create_table :table, comment: 'Table' do |t|
              t.string :name
              ^^^^^^^^^^^^^^ New database column without `comment`.
            end
        "#});
    }

    #[test]
    fn allows_column_with_comment_in_block() {
        test::<SchemaComment>().expect_no_offenses(indoc! {r#"
            create_table :table, comment: 'Table' do |t|
              t.string :name, comment: 'Name'
            end
        "#});
    }

    #[test]
    fn allows_receiver_form() {
        test::<SchemaComment>()
            .expect_no_offenses("foo.add_column :table, :column, :integer\n");
    }
}
murphy_plugin_api::submit_cop!(SchemaComment);
