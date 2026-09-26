//! `Rails/AddColumnIndex` — `add_column` does not accept `index`, use `add_index`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/AddColumnIndex
//! upstream_version_checked: 2.35.0
//! version_added: "2.11"
//! version_changed: "2.20"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_send` (RESTRICT_ON_SEND
//!   `[:add_column]`, bare `send nil? :add_column` only): first two args
//!   are table/column, then any trailing `Hash` arg is scanned for a
//!   `pair` whose key is `(sym :index)` or `(str "index")`. Offense is
//!   the pair only; autocorrect removes it via `index_range` (left
//!   surrounding-space with newlines + left surrounding-comma) and
//!   inserts `\nadd_index <table>, <column>[, <inner opts>]` after the
//!   send, where inner opts are the stripped contents of the index
//!   value when it is a `Hash` (e.g. `index: { unique: true }` →
//!   `add_index :t, :c, unique: true`). String keys (`'index' => true`)
//!   match; interpolated keys do not. `MigratedSchemaVersion` skipping
//!   and `MigrationsHelper#in_migration?` gating are absent upstream
//!   for this cop (no `in_migration?` call); file scope (`Include:
//!   db/**/*.rb`) is enforced via the murphy-rails pack default.yml.
//! ```
//!
//! ## Matched shapes
//!
//! - `add_column :t, :c, :integer, index: true` → remove `, index: true`,
//!   insert `\nadd_index :t, :c`
//! - `add_column :t, :c, :integer, index: { unique: true }` →
//!   `add_index :t, :c, unique: true`
//! - `add_column :t, :c, :integer, 'index' => true` → same as `index: true`
//! - `add_column :t, :c, :integer` / `foo.add_column(...)` → no offense

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, RangeSide, SpaceRangeOptions, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct AddColumnIndex;

const MSG: &str = "`add_column` does not accept an `index` key, use `add_index` instead.";

#[cop(
    name = "Rails/AddColumnIndex",
    description = "`add_column` does not accept an `index` key, use `add_index` instead.",
    default_enabled = false,
    options = NoOptions,
)]
impl AddColumnIndex {
    // Mirrors upstream `RESTRICT_ON_SEND = %i[add_column]`.
    #[on_node(kind = "send", methods = ["add_column"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn is_index_key(cx: &Cx<'_>, key: NodeId) -> bool {
    match *cx.kind(key) {
        NodeKind::Sym(sym) => cx.symbol_str(sym) == "index",
        NodeKind::Str(sid) => cx.string_str(sid) == "index",
        _ => false,
    }
}

fn find_index_pair(cx: &Cx<'_>, hash: NodeId) -> Option<(NodeId, NodeId)> {
    for pair in cx.hash_pairs(hash) {
        let NodeKind::Pair { key, value } = *cx.kind(pair) else {
            continue;
        };
        if is_index_key(cx, key) {
            return Some((pair, value));
        }
    }
    None
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
        return;
    }
    // Upstream `(send nil? :add_column ...)` — bare calls only.
    if cx.call_receiver(node).get().is_some() {
        return;
    }
    let args = cx.call_arguments(node).to_vec();
    if args.len() < 3 {
        return;
    }
    let table = args[0];
    let column = args[1];
    // Upstream `<(hash <$(pair ...) ...>) ...>`: any Hash after table/column
    // containing an index pair. Scan trailing args for the first match.
    let mut found: Option<(NodeId, NodeId)> = None;
    for &arg in &args[2..] {
        if !matches!(*cx.kind(arg), NodeKind::Hash(_)) {
            continue;
        }
        if let Some((pair, value)) = find_index_pair(cx, arg) {
            found = Some((pair, value));
            break;
        }
    }
    let Some((pair, value)) = found else {
        return;
    };
    cx.emit_offense(cx.range(pair), MSG, None);
    // Autocorrect: remove `index_range(pair)` + insert `add_index` after.
    cx.emit_edit(index_range(cx, cx.range(pair)), "");
    let table_src = cx.raw_source(cx.range(table)).to_owned();
    let column_src = cx.raw_source(cx.range(column)).to_owned();
    let mut add_index = format!("add_index {table_src}, {column_src}");
    if matches!(*cx.kind(value), NodeKind::Hash(_)) {
        let inner = hash_inner_source(cx, value);
        if !inner.is_empty() {
            add_index.push_str(&format!(", {inner}"));
        }
    }
    let node_end = cx.range(node).end;
    cx.emit_edit(
        Range {
            start: node_end,
            end: node_end,
        },
        &format!("\n{add_index}"),
    );
}

/// Mirrors upstream `index_range`: `range_with_surrounding_comma(
/// range_with_surrounding_space(pair, side: :left), :left)`.
fn index_range(cx: &Cx<'_>, pair_range: Range) -> Range {
    let expanded = cx.range_with_surrounding_space(
        pair_range,
        SpaceRangeOptions {
            side: RangeSide::Left,
            newlines: true,
            whitespace: false,
            continuations: false,
        },
    );
    let src = cx.source().as_bytes();
    let mut start = expanded.start as usize;
    // `range_with_surrounding_comma(..., :left)`: consume preceding commas.
    while start > 0 && src[start - 1] == b',' {
        start -= 1;
    }
    Range {
        start: start as u32,
        end: expanded.end,
    }
}

/// Upstream `value.source_range.adjust(begin_pos: 1, end_pos: -1).source.strip`:
/// strip outer braces and surrounding whitespace. Handles both braced
/// (`{ unique: true }`) and bare (`unique: true` — defensive) sources.
fn hash_inner_source(cx: &Cx<'_>, hash: NodeId) -> String {
    let src = cx.raw_source(cx.range(hash)).trim().to_owned();
    if src.starts_with('{') && src.ends_with('}') && src.len() >= 2 {
        src[1..src.len() - 1].trim().to_owned()
    } else {
        src
    }
}

#[cfg(test)]
mod tests {
    use super::AddColumnIndex;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_index_true_and_corrects() {
        test::<AddColumnIndex>().expect_correction(
            indoc! {r#"
                add_column :table, :column, :integer, default: 0, index: true
                                                                  ^^^^^^^^^^^ `add_column` does not accept an `index` key, use `add_index` instead.
            "#},
            "add_column :table, :column, :integer, default: 0\nadd_index :table, :column\n",
        );
    }

    #[test]
    fn flags_index_hash_value_and_corrects() {
        test::<AddColumnIndex>().expect_correction(
            indoc! {r#"
                add_column :table, :column, :integer, default: 0, index: { unique: true, name: 'my_unique_index' }
                                                                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `add_column` does not accept an `index` key, use `add_index` instead.
            "#},
            "add_column :table, :column, :integer, default: 0\nadd_index :table, :column, unique: true, name: 'my_unique_index'\n",
        );
    }

    #[test]
    fn corrects_when_index_before_other_keys() {
        test::<AddColumnIndex>().expect_correction(
            indoc! {r#"
                add_column :table, :column, :integer, index: true, default: 0
                                                      ^^^^^^^^^^^ `add_column` does not accept an `index` key, use `add_index` instead.
            "#},
            "add_column :table, :column, :integer, default: 0\nadd_index :table, :column\n",
        );
    }

    #[test]
    fn flags_string_key_and_corrects() {
        test::<AddColumnIndex>().expect_correction(
            indoc! {r#"
                add_column :table, :column, :integer, 'index' => true, default: 0
                                                      ^^^^^^^^^^^^^^^ `add_column` does not accept an `index` key, use `add_index` instead.
            "#},
            "add_column :table, :column, :integer, default: 0\nadd_index :table, :column\n",
        );
    }

    #[test]
    fn corrects_multiline() {
        test::<AddColumnIndex>().expect_correction(
            indoc! {r#"
                add_column :table, :column, :integer,
                           index: true,
                           ^^^^^^^^^^^ `add_column` does not accept an `index` key, use `add_index` instead.
                           default: 0
            "#},
            "add_column :table, :column, :integer,\n           default: 0\nadd_index :table, :column\n",
        );
    }

    #[test]
    fn allows_without_index() {
        test::<AddColumnIndex>()
            .expect_no_offenses("add_column :table, :column, :integer, default: 0\n");
    }

    #[test]
    fn ignores_receiver_calls() {
        test::<AddColumnIndex>()
            .expect_no_offenses("foo.add_column :table, :column, :integer, index: true\n");
    }
}
murphy_plugin_api::submit_cop!(AddColumnIndex);
