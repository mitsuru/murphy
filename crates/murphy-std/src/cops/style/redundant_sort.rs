//! `Style/RedundantSort` — replace `sort.first`, `sort.last`, `sort[0]`, etc.
//! with `min`, `max`, `min_by`, `max_by`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantSort
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   This cop is marked unsafe in RuboCop: `sort.last` and `max` may return
//!   different elements when there are multiple elements where `a <=> b == 0`
//!   (sort.last returns the last stable element, max returns the first).
//!   Murphy implements it with default_enabled=true matching RuboCop's default.
//!
//!   Covered patterns:
//!     - sort.first / sort.last
//!     - sort[0] / sort[-1] / sort.at(0) / sort.at(-1) / sort.slice(0) / sort.slice(-1)
//!     - sort_by(&:foo).first / sort_by { block }.first (and last/[0]/[-1]/at/slice variants)
//!     - Block, Numblock, and Itblock receivers for sort_by
//!
//!   Gap: The logical-operator autocorrect reorganization (moving `||`/`&&`/`or`/`and`
//!   from after `.first` to after the suggestion on multiline expressions) is not
//!   implemented. The offense is still reported and the rename+delete autocorrect
//!   is applied correctly for the accessor; only the operator position is not moved.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad (→ min/min_by)
//! [2, 1, 3].sort.first
//! [2, 1, 3].sort[0]
//! [2, 1, 3].sort.at(0)
//! [2, 1, 3].sort.slice(0)
//! arr.sort_by(&:foo).first
//! arr.sort_by { |x| x.foo }.first
//!
//! # bad (→ max/max_by)
//! [2, 1, 3].sort.last
//! [2, 1, 3].sort[-1]
//! [2, 1, 3].sort.at(-1)
//! arr.sort_by(&:foo).last
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantSort;

const MSG: &str = "Use `%suggestion%` instead of `%sorter%...%accessor_source%`.";

#[cop(
    name = "Style/RedundantSort",
    description = "Use `min`/`max` instead of `sort` followed by `.first`/`.last`/`[0]`/`[-1]`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RedundantSort {
    #[on_node(kind = "send", methods = ["first", "last", "[]", "at", "slice"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Csend { method, .. } = *cx.kind(node) else {
            return;
        };
        if matches!(cx.symbol_str(method), "first" | "last" | "[]" | "at" | "slice") {
            check(node, cx);
        }
    }
}

// ---------------------------------------------------------------------------
// Accessor argument analysis
// ---------------------------------------------------------------------------

/// Whether this accessor is a "first" (→ min) or "last" (→ max) position.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Position {
    First,
    Last,
}

/// Check if the accessor node indicates a first (0) or last (-1) position.
/// Returns `None` if the accessor is not a redundant-sort pattern.
fn accessor_position(node: NodeId, cx: &Cx<'_>) -> Option<Position> {
    let (method_name, args) = match *cx.kind(node) {
        NodeKind::Send { method, args, .. } => (cx.symbol_str(method), cx.list(args)),
        NodeKind::Csend { method, args, .. } => (cx.symbol_str(method), cx.list(args)),
        _ => return None,
    };

    match method_name {
        "first" => {
            if args.is_empty() {
                Some(Position::First)
            } else {
                None
            }
        }
        "last" => {
            if args.is_empty() {
                Some(Position::Last)
            } else {
                None
            }
        }
        "[]" | "at" | "slice" => {
            if args.len() != 1 {
                return None;
            }
            match *cx.kind(args[0]) {
                NodeKind::Int(0) => Some(Position::First),
                NodeKind::Int(-1) => Some(Position::Last),
                _ => None,
            }
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Receiver (sort side) analysis
// ---------------------------------------------------------------------------

/// The inner sort call node and whether it is `sort` or `sort_by`.
#[derive(Clone, Copy)]
enum SortKind {
    Sort,
    SortBy,
}

/// For a node that is the receiver of the accessor call, extract:
/// - the sort call node (the `sort` or `sort_by` send node)
/// - which variant (`Sort` or `SortBy`)
///
/// Receiver shapes handled:
/// 1. Plain `send :sort` with no args → `(sort_node, Sort)` (csend excluded, see below)
/// 2. Plain `send :sort_by` with exactly one block-pass arg → `(sort_by_node, SortBy)` (csend excluded)
/// 3. Block/Numblock/Itblock wrapping a `sort_by` call → `(sort_by_call_inside_block, SortBy)`
/// Note: Block wrapping `sort` (comparison block) is intentionally excluded —
///       `sort { |a, b| b <=> a }.first` is NOT equivalent to `min { |a, b| b <=> a }`.
fn extract_sort_receiver(receiver: NodeId, cx: &Cx<'_>) -> Option<(NodeId, SortKind)> {
    match *cx.kind(receiver) {
        // Case 1 & 2: plain send only (NOT csend).
        // `obj&.sort.first` is NOT equivalent to `obj&.min`: when `obj` is nil,
        // `obj&.sort` returns nil and `.first` raises NoMethodError, but `obj&.min`
        // would silently return nil. That is a behaviour change, so csend receivers
        // are excluded.
        NodeKind::Send { method, args, .. } => {
            let method_name = cx.symbol_str(method);
            let args_list = cx.list(args);
            match method_name {
                "sort" if args_list.is_empty() => Some((receiver, SortKind::Sort)),
                "sort_by" if args_list.len() == 1 => {
                    // Must be a block-pass argument (&:method).
                    if matches!(cx.kind(args_list[0]), NodeKind::BlockPass(_)) {
                        Some((receiver, SortKind::SortBy))
                    } else {
                        None
                    }
                }
                _ => None,
            }
        }
        // Case 3: block wrapping sort_by (sort with comparison block is NOT handled —
        // `sort { |a, b| b <=> a }.first` is NOT equivalent to `min { |a, b| b <=> a }`)
        // csend inside the block (`obj&.sort_by { ... }.first`) is also excluded for
        // the same reason as plain csend: behaviour change when obj is nil.
        NodeKind::Block { call, .. } | NodeKind::Numblock { send: call, .. } => {
            // Reject if the inner call is csend.
            if matches!(cx.kind(call), NodeKind::Csend { .. }) {
                return None;
            }
            let method_name = cx.method_name(call)?;
            match method_name {
                "sort_by" if cx.call_arguments(call).is_empty() => {
                    Some((call, SortKind::SortBy))
                }
                _ => None,
            }
        }
        NodeKind::Itblock { send, .. } => {
            // Reject if the inner call is csend.
            if matches!(cx.kind(send), NodeKind::Csend { .. }) {
                return None;
            }
            let method_name = cx.method_name(send)?;
            match method_name {
                "sort_by" if cx.call_arguments(send).is_empty() => {
                    Some((send, SortKind::SortBy))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Suggestion string
// ---------------------------------------------------------------------------

fn suggestion(sort_kind: SortKind, position: Position) -> &'static str {
    match (sort_kind, position) {
        (SortKind::Sort, Position::First) => "min",
        (SortKind::Sort, Position::Last) => "max",
        (SortKind::SortBy, Position::First) => "min_by",
        (SortKind::SortBy, Position::Last) => "max_by",
    }
}

fn sorter_str(sort_kind: SortKind) -> &'static str {
    match sort_kind {
        SortKind::Sort => "sort",
        SortKind::SortBy => "sort_by",
    }
}

// ---------------------------------------------------------------------------
// Main check
// ---------------------------------------------------------------------------

fn check(accessor: NodeId, cx: &Cx<'_>) {
    // The accessor must indicate a position.
    let Some(position) = accessor_position(accessor, cx) else {
        return;
    };

    // The receiver of the accessor must be a sort/sort_by call.
    let receiver = match cx.call_receiver(accessor).get() {
        Some(r) => r,
        None => return,
    };
    let Some((sort_call, sort_kind)) = extract_sort_receiver(receiver, cx) else {
        return;
    };

    let sort_selector = cx.selector(sort_call);
    let accessor_end = cx.range(accessor).end;

    let offense_range = Range {
        start: sort_selector.start,
        end: accessor_end,
    };

    // Build accessor source for message: from the selector of the accessor
    // to end of the accessor node (without the leading dot).
    // For `sort.first`, accessor_source = "first".
    // For `sort[0]`, accessor_source = "[0]".
    // For `sort.at(0)`, accessor_source = "at(0)".
    let accessor_selector_start = cx.selector(accessor).start;
    let accessor_source = cx.raw_source(Range {
        start: accessor_selector_start,
        end: accessor_end,
    });

    let sugg = suggestion(sort_kind, position);
    let sorter = sorter_str(sort_kind);
    let msg = MSG
        .replace("%suggestion%", sugg)
        .replace("%sorter%", sorter)
        .replace("%accessor_source%", accessor_source);

    cx.emit_offense(offense_range, &msg, None);

    // Autocorrect (two surgical edits):
    // Edit 1: rename `sort` / `sort_by` selector to suggestion.
    cx.emit_edit(sort_selector, sugg);

    // Edit 2: delete from end of receiver to end of accessor.
    // This removes `.first`, `[0]`, `.at(0)`, etc.
    let delete_range = Range {
        start: cx.range(receiver).end,
        end: accessor_end,
    };
    cx.emit_edit(delete_range, "");
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::RedundantSort;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- sort.first -----

    #[test]
    fn flags_sort_first() {
        test::<RedundantSort>().expect_offense(indoc! {"
            [2, 1, 3].sort.first
                      ^^^^^^^^^^ Use `min` instead of `sort...first`.
        "});
    }

    #[test]
    fn corrects_sort_first() {
        test::<RedundantSort>().expect_correction(
            indoc! {"
                [2, 1, 3].sort.first
                          ^^^^^^^^^^ Use `min` instead of `sort...first`.
            "},
            "[2, 1, 3].min\n",
        );
    }

    // ----- sort.last -----

    #[test]
    fn flags_sort_last() {
        test::<RedundantSort>().expect_offense(indoc! {"
            [2, 1, 3].sort.last
                      ^^^^^^^^^ Use `max` instead of `sort...last`.
        "});
    }

    #[test]
    fn corrects_sort_last() {
        test::<RedundantSort>().expect_correction(
            indoc! {"
                [2, 1, 3].sort.last
                          ^^^^^^^^^ Use `max` instead of `sort...last`.
            "},
            "[2, 1, 3].max\n",
        );
    }

    // ----- sort[0] -----

    #[test]
    fn flags_sort_index_zero() {
        test::<RedundantSort>().expect_offense(indoc! {"
            [2, 1, 3].sort[0]
                      ^^^^^^^ Use `min` instead of `sort...[0]`.
        "});
    }

    #[test]
    fn corrects_sort_index_zero() {
        test::<RedundantSort>().expect_correction(
            indoc! {"
                [2, 1, 3].sort[0]
                          ^^^^^^^ Use `min` instead of `sort...[0]`.
            "},
            "[2, 1, 3].min\n",
        );
    }

    // ----- sort[-1] -----

    #[test]
    fn flags_sort_index_neg_one() {
        test::<RedundantSort>().expect_offense(indoc! {"
            [2, 1, 3].sort[-1]
                      ^^^^^^^^ Use `max` instead of `sort...[-1]`.
        "});
    }

    #[test]
    fn corrects_sort_index_neg_one() {
        test::<RedundantSort>().expect_correction(
            indoc! {"
                [2, 1, 3].sort[-1]
                          ^^^^^^^^ Use `max` instead of `sort...[-1]`.
            "},
            "[2, 1, 3].max\n",
        );
    }

    // ----- sort.at(0) -----

    #[test]
    fn flags_sort_at_zero() {
        test::<RedundantSort>().expect_offense(indoc! {"
            [2, 1, 3].sort.at(0)
                      ^^^^^^^^^^ Use `min` instead of `sort...at(0)`.
        "});
    }

    #[test]
    fn corrects_sort_at_zero() {
        test::<RedundantSort>().expect_correction(
            indoc! {"
                [2, 1, 3].sort.at(0)
                          ^^^^^^^^^^ Use `min` instead of `sort...at(0)`.
            "},
            "[2, 1, 3].min\n",
        );
    }

    // ----- sort.slice(0) -----

    #[test]
    fn flags_sort_slice_zero() {
        test::<RedundantSort>().expect_offense(indoc! {"
            [2, 1, 3].sort.slice(0)
                      ^^^^^^^^^^^^^ Use `min` instead of `sort...slice(0)`.
        "});
    }

    #[test]
    fn corrects_sort_slice_zero() {
        test::<RedundantSort>().expect_correction(
            indoc! {"
                [2, 1, 3].sort.slice(0)
                          ^^^^^^^^^^^^^ Use `min` instead of `sort...slice(0)`.
            "},
            "[2, 1, 3].min\n",
        );
    }

    // ----- sort_by(&:foo).first -----

    #[test]
    fn flags_sort_by_block_pass_first() {
        test::<RedundantSort>().expect_offense(indoc! {"
            arr.sort_by(&:foo).first
                ^^^^^^^^^^^^^^^^^^^^ Use `min_by` instead of `sort_by...first`.
        "});
    }

    #[test]
    fn corrects_sort_by_block_pass_first() {
        test::<RedundantSort>().expect_correction(
            indoc! {"
                arr.sort_by(&:foo).first
                    ^^^^^^^^^^^^^^^^^^^^ Use `min_by` instead of `sort_by...first`.
            "},
            "arr.min_by(&:foo)\n",
        );
    }

    // ----- sort_by(&:foo).last -----

    #[test]
    fn flags_sort_by_block_pass_last() {
        test::<RedundantSort>().expect_offense(indoc! {"
            arr.sort_by(&:foo).last
                ^^^^^^^^^^^^^^^^^^^ Use `max_by` instead of `sort_by...last`.
        "});
    }

    #[test]
    fn corrects_sort_by_block_pass_last() {
        test::<RedundantSort>().expect_correction(
            indoc! {"
                arr.sort_by(&:foo).last
                    ^^^^^^^^^^^^^^^^^^^ Use `max_by` instead of `sort_by...last`.
            "},
            "arr.max_by(&:foo)\n",
        );
    }

    // ----- sort_by { block }.first -----

    #[test]
    fn flags_sort_by_block_first() {
        test::<RedundantSort>().expect_offense(indoc! {"
            arr.sort_by { |x| x.foo }.first
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `min_by` instead of `sort_by...first`.
        "});
    }

    #[test]
    fn corrects_sort_by_block_first() {
        test::<RedundantSort>().expect_correction(
            indoc! {"
                arr.sort_by { |x| x.foo }.first
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `min_by` instead of `sort_by...first`.
            "},
            "arr.min_by { |x| x.foo }\n",
        );
    }

    // ----- sort_by { _1.foo }.first (numblock) -----

    #[test]
    fn flags_sort_by_numblock_first() {
        test::<RedundantSort>().expect_offense(indoc! {"
            arr.sort_by { _1.foo }.first
                ^^^^^^^^^^^^^^^^^^^^^^^^ Use `min_by` instead of `sort_by...first`.
        "});
    }

    #[test]
    fn corrects_sort_by_numblock_first() {
        test::<RedundantSort>().expect_correction(
            indoc! {"
                arr.sort_by { _1.foo }.first
                    ^^^^^^^^^^^^^^^^^^^^^^^^ Use `min_by` instead of `sort_by...first`.
            "},
            "arr.min_by { _1.foo }\n",
        );
    }

    // ----- sort_by { it.foo }.first (itblock) -----

    #[test]
    fn flags_sort_by_itblock_first() {
        test::<RedundantSort>().expect_offense(indoc! {"
            arr.sort_by { it.foo }.first
                ^^^^^^^^^^^^^^^^^^^^^^^^ Use `min_by` instead of `sort_by...first`.
        "});
    }

    #[test]
    fn corrects_sort_by_itblock_first() {
        test::<RedundantSort>().expect_correction(
            indoc! {"
                arr.sort_by { it.foo }.first
                    ^^^^^^^^^^^^^^^^^^^^^^^^ Use `min_by` instead of `sort_by...first`.
            "},
            "arr.min_by { it.foo }\n",
        );
    }

    // ----- Negative cases -----

    #[test]
    fn accepts_plain_sort() {
        test::<RedundantSort>().expect_no_offenses("[2, 1, 3].sort\n");
    }

    #[test]
    fn accepts_sort_first_with_arg() {
        // .first(1) is not equivalent to [0]
        test::<RedundantSort>().expect_no_offenses("[2, 1, 3].sort.first(1)\n");
    }

    #[test]
    fn accepts_sort_last_with_arg() {
        test::<RedundantSort>().expect_no_offenses("[2, 1, 3].sort.last(1)\n");
    }

    #[test]
    fn accepts_sort_index_nonzero() {
        test::<RedundantSort>().expect_no_offenses("[2, 1, 3].sort[1]\n");
    }

    #[test]
    fn accepts_sort_index_neg_two() {
        test::<RedundantSort>().expect_no_offenses("[2, 1, 3].sort[-2]\n");
    }

    #[test]
    fn accepts_sort_by_without_accessor() {
        test::<RedundantSort>().expect_no_offenses("arr.sort_by { |x| x.foo }\n");
    }

    #[test]
    fn accepts_first_without_sort() {
        test::<RedundantSort>().expect_no_offenses("[1, 2, 3].first\n");
    }

    #[test]
    fn accepts_min_already_used() {
        test::<RedundantSort>().expect_no_offenses("[1, 2, 3].min\n");
    }

    #[test]
    fn accepts_sort_with_comparison_block() {
        // sort { |a, b| b <=> a }.first is NOT equivalent to min { |a, b| b <=> a }
        // (the comparison block reverses order, making sort produce descending results)
        test::<RedundantSort>().expect_no_offenses("array.sort { |a, b| b <=> a }.first\n");
    }

    #[test]
    fn accepts_sort_with_comparison_block_last() {
        test::<RedundantSort>().expect_no_offenses("array.sort { |a, b| b <=> a }.last\n");
    }

    #[test]
    fn accepts_csend_sort_first() {
        // obj&.sort.first is NOT equivalent to obj&.min:
        // when obj is nil, &.sort returns nil and .first raises NoMethodError,
        // but &.min would silently return nil -- a behaviour change.
        test::<RedundantSort>().expect_no_offenses("obj&.sort.first\n");
    }

    #[test]
    fn accepts_csend_sort_by_block_first() {
        test::<RedundantSort>().expect_no_offenses("obj&.sort_by { |x| x.foo }.first\n");
    }
}
murphy_plugin_api::submit_cop!(RedundantSort);