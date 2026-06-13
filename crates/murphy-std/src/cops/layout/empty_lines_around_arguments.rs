//! `Layout/EmptyLinesAroundArguments` — flags blank lines inside the
//! argument list of a multi-line method invocation.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/EmptyLinesAroundArguments
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Ports `on_send`/`on_csend`, `extra_lines`, and
//!   `empty_range_for_starting_point`. Skips single-line calls, calls
//!   with no arguments, and calls whose receiver ends on a different
//!   line than the selector. For each argument (and the closing paren)
//!   the cop expands left through surrounding whitespace; when the
//!   resulting range spans more than one blank line it flags the blank
//!   line and removes it. Message: "Empty line detected around
//!   arguments." Autocorrect removes the offending blank line.
//! ```
//!
//! ## Algorithm
//!
//! Mirrors RuboCop's `empty_range_for_starting_point`: for each argument
//! start (and the closing-paren start), expand left through whitespace
//! including newlines. If that range crosses more than one line boundary,
//! a blank line exists immediately before it — the offense covers the
//! whole second-to-last line of the expanded range (including its
//! trailing newline), which is the blank line itself.

use crate::cops::util::{line_of, nth_line_start, whole_line_range_with_newline};
use murphy_plugin_api::{Cx, NoOptions, NodeId, Range, RangeSide, SpaceRangeOptions, cop};

const MSG: &str = "Empty line detected around arguments.";

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct EmptyLinesAroundArguments;

#[cop(
    name = "Layout/EmptyLinesAroundArguments",
    description = "Keeps track of empty lines around method arguments.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl EmptyLinesAroundArguments {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check_call(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check_call(node, cx);
    }
}

fn check_call(node: NodeId, cx: &Cx<'_>) {
    let args = cx.call_arguments(node);
    if args.is_empty() {
        return;
    }
    // `node.single_line?` — the whole call fits on one line.
    let node_range = cx.range(node);
    if line_of(node_range.start, cx) == line_of(node_range.end.saturating_sub(1), cx) {
        return;
    }
    if receiver_and_method_call_on_different_lines(node, cx) {
        return;
    }

    // For each argument, then the closing paren, expand left through
    // whitespace and flag any crossed blank line.
    for &arg in args {
        if let Some(range) = empty_range_for_starting_point(cx.range(arg).start, cx) {
            cx.emit_offense(range, MSG, None);
            cx.emit_edit(range, "");
        }
    }
    let end_loc = cx.loc(node).end();
    if end_loc != Range::ZERO
        && let Some(range) = empty_range_for_starting_point(end_loc.start, cx)
    {
        cx.emit_offense(range, MSG, None);
        cx.emit_edit(range, "");
    }
}

/// `node.receiver && node.receiver.loc.last_line != node.loc.selector&.line`
fn receiver_and_method_call_on_different_lines(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(receiver) = cx.call_receiver(node).get() else {
        return false;
    };
    let selector = cx.selector(node);
    if selector == Range::ZERO {
        // No selector (e.g. operator/implicit call) — RuboCop's `&.line`
        // is nil, so `receiver.last_line != nil` is always true.
        return true;
    }
    line_of(cx.range(receiver).end.saturating_sub(1), cx) != line_of(selector.start, cx)
}

/// Port of RuboCop's `empty_range_for_starting_point`:
///
/// ```ruby
/// range = range_with_surrounding_space(start, whitespace: true, side: :left)
/// return unless range.last_line - range.first_line > 1
/// yield range.source_buffer.line_range(range.last_line - 1).adjust(end_pos: 1)
/// ```
fn empty_range_for_starting_point(start: u32, cx: &Cx<'_>) -> Option<Range> {
    let expanded = cx.range_with_surrounding_space(
        Range { start, end: start },
        SpaceRangeOptions {
            side: RangeSide::Left,
            whitespace: true,
            newlines: true,
            continuations: false,
        },
    );
    let first_line = line_of(expanded.start, cx);
    let last_line = line_of(expanded.end, cx);
    if last_line.saturating_sub(first_line) <= 1 {
        return None;
    }
    // The blank line is the second-to-last line of the expanded range.
    // RuboCop yields `line_range(last_line - 1)` (whole line) plus one
    // byte so the trailing newline is removed too.
    let blank_line_offset = nth_line_start(cx, last_line - 1)?;
    Some(whole_line_range_with_newline(blank_line_offset, cx))
}

#[cfg(test)]
mod tests {
    use super::EmptyLinesAroundArguments;
    use murphy_plugin_api::test_support::{run_cop, run_cop_with_edits, test};

    #[test]
    fn accepts_single_line_call() {
        test::<EmptyLinesAroundArguments>().expect_no_offenses("do_something(foo, bar)\n");
    }

    #[test]
    fn accepts_no_args() {
        test::<EmptyLinesAroundArguments>().expect_no_offenses("do_something()\n");
    }

    #[test]
    fn accepts_multiline_without_blank_lines() {
        test::<EmptyLinesAroundArguments>().expect_no_offenses("do_something(\n  foo\n)\n");
    }

    #[test]
    fn flags_blank_line_before_closing_paren() {
        let src = "do_something(\n  foo\n\n)\n";
        let offenses = run_cop::<EmptyLinesAroundArguments>(src);
        assert_eq!(offenses.len(), 1, "expected 1 offense, got {offenses:?}");
        assert_eq!(offenses[0].message, "Empty line detected around arguments.");
    }

    #[test]
    fn flags_blank_line_after_first_argument() {
        // process(bar,\n\n        baz: qux,\n        thud: fred)
        let src = "process(bar,\n\n        baz: qux,\n        thud: fred)\n";
        let offenses = run_cop::<EmptyLinesAroundArguments>(src);
        assert_eq!(offenses.len(), 1, "expected 1 offense, got {offenses:?}");
    }

    #[test]
    fn flags_blank_line_after_opening_paren() {
        let src = "some_method(\n\n  [1, 2, 3],\n  x: y\n)\n";
        let offenses = run_cop::<EmptyLinesAroundArguments>(src);
        assert_eq!(offenses.len(), 1, "expected 1 offense, got {offenses:?}");
    }

    #[test]
    fn corrects_blank_line_before_closing_paren() {
        let src = "do_something(\n  foo\n\n)\n";
        let result = run_cop_with_edits::<EmptyLinesAroundArguments>(src);
        assert_eq!(result.offenses.len(), 1);
        let edit = &result.edits[0];
        assert_eq!(edit.replacement, "");
        // The removed range is the blank line (just "\n").
        assert_eq!(&src[edit.range.start as usize..edit.range.end as usize], "\n");
    }

    #[test]
    fn accepts_blank_line_between_single_line_calls() {
        // Two separate single-line calls separated by a blank line — no offense.
        test::<EmptyLinesAroundArguments>()
            .expect_no_offenses("do_something(foo)\n\ndo_other(bar)\n");
    }

    #[test]
    fn flags_csend_blank_line() {
        let src = "obj&.do_something(\n  foo\n\n)\n";
        let offenses = run_cop::<EmptyLinesAroundArguments>(src);
        assert_eq!(offenses.len(), 1, "expected 1 offense, got {offenses:?}");
    }
}

murphy_plugin_api::submit_cop!(EmptyLinesAroundArguments);
