//! `Style/RedundantFilterChain` — replace `select.any?`, `select.empty?`,
//! `select.none?`, `select.one?` with the predicate method directly.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantFilterChain
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Covered patterns:
//!     - `select { block }.any?` → `any? { block }`
//!     - `select { block }.empty?` → `none? { block }`
//!     - `select { block }.none?` → `none? { block }`
//!     - `select { block }.one?` → `one? { block }`
//!     - Same for `filter` and `find_all` as the filter method.
//!     - Block-pass form: `select(&:foo).any?` → `any?(&:foo)`.
//!     - Both `send` and `csend` for the predicate call.
//!   Gaps:
//!     - `many?` and `present?` (ActiveSupport extensions) are not handled.
//!       They require `AllCops.ActiveSupportExtensionsEnabled: true`, but Murphy
//!       does not expose that AllCops setting to cops in the plugin API.
//!   Safety:
//!     - Autocorrect is unsafe: `array.select.any?` evaluates all elements
//!       through the filter, while `array.any?` uses short-circuit evaluation.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! arr.select { |x| x > 1 }.any?
//! arr.select { |x| x > 1 }.empty?
//! arr.select { |x| x > 1 }.none?
//! arr.select { |x| x > 1 }.one?
//! arr.select(&:odd?).any?
//!
//! # good
//! arr.any? { |x| x > 1 }
//! arr.none? { |x| x > 1 }
//! relation.select(:name).any?  # non-block select
//! arr.select { |x| x > 1 }.any?(&:odd?)  # predicate has args
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantFilterChain;

/// The message format: `%1` = preferred method, `%2` = filter method, `%3` = predicate method.
const MSG: &str = "Use `%s` instead of `%s.%s`.";

/// Methods that serve as the filter step.
const FILTER_METHODS: &[&str] = &["select", "filter", "find_all"];

/// Methods that serve as the predicate step (excluding many?/present? which need ActiveSupport).
const PREDICATE_METHODS: &[&str] = &["any?", "empty?", "none?", "one?"];

/// Map from predicate method to the replacement method name.
fn replacement_for(predicate: &str) -> &'static str {
    match predicate {
        "empty?" => "none?",
        "any?" => "any?",
        "none?" => "none?",
        "one?" => "one?",
        _ => "any?", // fallback (unreachable given PREDICATE_METHODS guard)
    }
}

#[cop(
    name = "Style/RedundantFilterChain",
    description = "Identifies `select`/`filter`/`find_all` chained with `any?`, `empty?`, `none?`, or `one?`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
    safe_autocorrect = false,
)]
impl RedundantFilterChain {
    #[on_node(kind = "send", methods = ["any?", "empty?", "none?", "one?"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Csend { method, .. } = *cx.kind(node) else {
            return;
        };
        if !matches!(cx.symbol_str(method), "any?" | "empty?" | "none?" | "one?") {
            return;
        }
        check(node, cx);
    }
}

fn check(predicate_node: NodeId, cx: &Cx<'_>) {
    // Predicate must have no arguments.
    let pred_args = cx.call_arguments(predicate_node);
    if !pred_args.is_empty() {
        return;
    }

    // Predicate must not have a block (i.e., must not be the `call` inside a Block).
    if cx.block_node(predicate_node).get().is_some() {
        return;
    }

    let predicate_name = cx.method_name(predicate_node).unwrap_or("");
    if !PREDICATE_METHODS.contains(&predicate_name) {
        return;
    }

    // Get the receiver of the predicate call.
    let receiver = match cx.call_receiver(predicate_node).get() {
        Some(r) => r,
        None => return,
    };

    // The receiver must be a filter call (select/filter/find_all).
    // Two shapes:
    //   Shape 1: `block(select_call, ...)` — select with a block
    //   Shape 2: `select_call(block_pass_arg)` — select with a block-pass
    if let Some((select_call, block_is_wrapper)) = extract_filter_call(receiver, cx) {
        let select_name = cx.method_name(select_call).unwrap_or("");
        if !FILTER_METHODS.contains(&select_name) {
            return;
        }

        let replacement = replacement_for(predicate_name);

        // Offense range: from select selector start to predicate selector end.
        let offense_range = Range {
            start: cx.selector(select_call).start,
            end: cx.selector(predicate_node).end,
        };

        let msg = MSG
            .replacen("%s", replacement, 1)
            .replacen("%s", select_name, 1)
            .replacen("%s", predicate_name, 1);

        cx.emit_offense(offense_range, &msg, None);

        // Autocorrect (two surgical edits):
        // Edit 1: rename the filter method selector to the replacement.
        cx.emit_edit(cx.selector(select_call), replacement);

        // Edit 2: delete from the end of the receiver (the block or select call)
        //         to the end of the predicate selector.
        // This removes the `.any?` / `.empty?` / etc. part.
        let delete_start = cx.range(receiver).end;
        let delete_end = cx.selector(predicate_node).end;
        cx.emit_edit(
            Range {
                start: delete_start,
                end: delete_end,
            },
            "",
        );

        let _ = block_is_wrapper; // used for documentation, not needed at runtime
    }
}

/// Extract the inner filter (select/filter/find_all) call node from a receiver.
///
/// Returns `(select_call_node, block_is_wrapper)`:
/// - Shape 1: `Block(select_call, ...)` — select wrapped in a block.
///   Returns `(select_call, true)`.
/// - Shape 1b: `Numblock(select_call, ...)` — same but with numbered params.
///   Returns `(select_call, true)`.
/// - Shape 2: `Send(select_call, block_pass_arg)` — select with a block-pass.
///   Returns `(select_call, false)` where `select_call == receiver`.
///
/// Returns `None` if receiver is not a filter call in a recognized shape.
fn extract_filter_call(receiver: NodeId, cx: &Cx<'_>) -> Option<(NodeId, bool)> {
    match *cx.kind(receiver) {
        // Shape 1: block wrapping a select call (send or csend inside the block).
        NodeKind::Block { call, .. } => {
            if cx.method_name(call).map_or(false, |n| FILTER_METHODS.contains(&n)) {
                Some((call, true))
            } else {
                None
            }
        }
        // Shape 1b: numblock wrapping a select call.
        NodeKind::Numblock { send: call, .. } => {
            if cx.method_name(call).map_or(false, |n| FILTER_METHODS.contains(&n)) {
                Some((call, true))
            } else {
                None
            }
        }
        // Shape 1c: itblock wrapping a select call.
        NodeKind::Itblock { send: call, .. } => {
            if cx.method_name(call).map_or(false, |n| FILTER_METHODS.contains(&n)) {
                Some((call, true))
            } else {
                None
            }
        }
        // Shape 2: plain send/csend with a block-pass argument.
        // e.g. `arr.select(&:odd?).any?`
        // The receiver itself IS the select call, and its single arg must be a BlockPass.
        NodeKind::Send { args, .. } | NodeKind::Csend { args, .. } => {
            let args_list = cx.list(args);
            // Must have exactly one block-pass argument (or zero args with no block).
            // Zero args: `arr.select.any?` — select with no block (not a recognized pattern).
            // One block-pass arg: `arr.select(&:foo).any?` — recognized.
            if args_list.len() == 1
                && matches!(cx.kind(args_list[0]), NodeKind::BlockPass(_))
                && cx.method_name(receiver).map_or(false, |n| FILTER_METHODS.contains(&n))
            {
                Some((receiver, false))
            } else {
                None
            }
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::RedundantFilterChain;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- Offense: any? ---

    #[test]
    fn flags_select_any() {
        test::<RedundantFilterChain>().expect_offense(indoc! {"
            arr.select { |x| x > 1 }.any?
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Use `any?` instead of `select.any?`.
        "});
    }

    #[test]
    fn corrects_select_any() {
        test::<RedundantFilterChain>().expect_correction(
            indoc! {"
                arr.select { |x| x > 1 }.any?
                    ^^^^^^^^^^^^^^^^^^^^^^^^^ Use `any?` instead of `select.any?`.
            "},
            "arr.any? { |x| x > 1 }\n",
        );
    }

    // --- Offense: empty? → none? ---

    #[test]
    fn flags_select_empty() {
        test::<RedundantFilterChain>().expect_offense(indoc! {"
            arr.select { |x| x > 1 }.empty?
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `none?` instead of `select.empty?`.
        "});
    }

    #[test]
    fn corrects_select_empty() {
        test::<RedundantFilterChain>().expect_correction(
            indoc! {"
                arr.select { |x| x > 1 }.empty?
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `none?` instead of `select.empty?`.
            "},
            "arr.none? { |x| x > 1 }\n",
        );
    }

    // --- Offense: none? ---

    #[test]
    fn flags_select_none() {
        test::<RedundantFilterChain>().expect_offense(indoc! {"
            arr.select { |x| x > 1 }.none?
                ^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `none?` instead of `select.none?`.
        "});
    }

    #[test]
    fn corrects_select_none() {
        test::<RedundantFilterChain>().expect_correction(
            indoc! {"
                arr.select { |x| x > 1 }.none?
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `none?` instead of `select.none?`.
            "},
            "arr.none? { |x| x > 1 }\n",
        );
    }

    // --- Offense: one? ---

    #[test]
    fn flags_select_one() {
        test::<RedundantFilterChain>().expect_offense(indoc! {"
            arr.select { |x| x > 1 }.one?
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Use `one?` instead of `select.one?`.
        "});
    }

    #[test]
    fn corrects_select_one() {
        test::<RedundantFilterChain>().expect_correction(
            indoc! {"
                arr.select { |x| x > 1 }.one?
                    ^^^^^^^^^^^^^^^^^^^^^^^^^ Use `one?` instead of `select.one?`.
            "},
            "arr.one? { |x| x > 1 }\n",
        );
    }

    // --- filter and find_all aliases ---

    #[test]
    fn flags_filter_any() {
        test::<RedundantFilterChain>().expect_offense(indoc! {"
            arr.filter { |x| x > 1 }.any?
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Use `any?` instead of `filter.any?`.
        "});
    }

    #[test]
    fn corrects_filter_any() {
        test::<RedundantFilterChain>().expect_correction(
            indoc! {"
                arr.filter { |x| x > 1 }.any?
                    ^^^^^^^^^^^^^^^^^^^^^^^^^ Use `any?` instead of `filter.any?`.
            "},
            "arr.any? { |x| x > 1 }\n",
        );
    }

    #[test]
    fn flags_find_all_any() {
        test::<RedundantFilterChain>().expect_offense(indoc! {"
            arr.find_all { |x| x > 1 }.any?
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `any?` instead of `find_all.any?`.
        "});
    }

    #[test]
    fn corrects_find_all_any() {
        test::<RedundantFilterChain>().expect_correction(
            indoc! {"
                arr.find_all { |x| x > 1 }.any?
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `any?` instead of `find_all.any?`.
            "},
            "arr.any? { |x| x > 1 }\n",
        );
    }

    // --- Block-pass form: select(&:foo).any? ---

    #[test]
    fn flags_select_block_pass_any() {
        test::<RedundantFilterChain>().expect_offense(indoc! {"
            arr.select(&:odd?).any?
                ^^^^^^^^^^^^^^^^^^^ Use `any?` instead of `select.any?`.
        "});
    }

    #[test]
    fn corrects_select_block_pass_any() {
        test::<RedundantFilterChain>().expect_correction(
            indoc! {"
                arr.select(&:odd?).any?
                    ^^^^^^^^^^^^^^^^^^^ Use `any?` instead of `select.any?`.
            "},
            "arr.any?(&:odd?)\n",
        );
    }

    // --- Negative cases (no offense) ---

    #[test]
    fn accepts_select_with_non_block_arg() {
        // relation.select(:name).any? — select arg is a symbol (column name), not block/block-pass
        test::<RedundantFilterChain>().expect_no_offenses("relation.select(:name).any?\n");
    }

    #[test]
    fn accepts_predicate_with_arg() {
        // arr.select { |x| x > 1 }.any?(&:odd?) — predicate has an argument
        test::<RedundantFilterChain>()
            .expect_no_offenses("arr.select { |x| x > 1 }.any?(&:odd?)\n");
    }

    #[test]
    fn accepts_predicate_with_block() {
        // arr.select { |x| x > 1 }.any? { |x| x.odd? } — predicate has a block
        test::<RedundantFilterChain>()
            .expect_no_offenses("arr.select { |x| x > 1 }.any? { |x| x.odd? }\n");
    }

    #[test]
    fn accepts_any_without_select() {
        test::<RedundantFilterChain>().expect_no_offenses("arr.any? { |x| x > 1 }\n");
    }

    #[test]
    fn accepts_select_alone() {
        test::<RedundantFilterChain>().expect_no_offenses("arr.select { |x| x > 1 }\n");
    }

    #[test]
    fn accepts_select_with_map_chain() {
        // select followed by map is not this cop's concern
        test::<RedundantFilterChain>()
            .expect_no_offenses("arr.select { |x| x > 1 }.map { |x| x * 2 }\n");
    }

    #[test]
    fn flags_select_numblock_any() {
        // select with numbered block params
        test::<RedundantFilterChain>().expect_offense(indoc! {"
            arr.select { _1 > 1 }.any?
                ^^^^^^^^^^^^^^^^^^^^^^ Use `any?` instead of `select.any?`.
        "});
    }

    #[test]
    fn corrects_select_numblock_any() {
        test::<RedundantFilterChain>().expect_correction(
            indoc! {"
                arr.select { _1 > 1 }.any?
                    ^^^^^^^^^^^^^^^^^^^^^^ Use `any?` instead of `select.any?`.
            "},
            "arr.any? { _1 > 1 }\n",
        );
    }
}

murphy_plugin_api::submit_cop!(RedundantFilterChain);
