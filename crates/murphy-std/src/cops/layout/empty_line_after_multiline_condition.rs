//! `Layout/EmptyLineAfterMultilineCondition` — requires a blank line after a
//! multiline condition in `if`/`unless`/`while`/`until`/`case`-`when`/
//! `rescue`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/EmptyLineAfterMultilineCondition
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Ports `on_if`/`on_while`/`on_until`/`on_while_post`/`on_until_post`/
//!   `on_case`/`on_rescue` and `check_condition`. RuboCop's eight Ruby
//!   entry points fold into Murphy's per-node-kind dispatch on
//!   `if`/`while`/`until`/`case`/`rescue`. Skips ternaries; modifier-form
//!   `if` is only checked when it has a following sibling, and do-while
//!   (`post`) loops likewise. A `when` branch is flagged when its
//!   conditions span multiple lines and the next line is not blank; a
//!   `resbody` likewise when its (>= 2) exception classes span lines.
//!   Message: "Use empty line after multiline condition." Disabled by
//!   default (matches RuboCop `Enabled: false`). Autocorrect inserts a
//!   newline after the condition's whole-line range.
//! ```
//!
//! ## Algorithm
//!
//! For each control-flow node the cop locates the relevant condition (the
//! `if`/loop condition, the `when` conditions, or the `resbody` exception
//! list). When that condition spans more than one physical line and the
//! line directly after it is not blank, an offense is raised and a blank
//! line inserted after the condition's final line.

use crate::cops::util::{line_is_blank, line_of, nth_line_start, whole_line_range_with_newline};
use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const MSG: &str = "Use empty line after multiline condition.";

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct EmptyLineAfterMultilineCondition;

#[cop(
    name = "Layout/EmptyLineAfterMultilineCondition",
    description = "Enforces empty line after multiline condition.",
    default_severity = "warning",
    // RuboCop ships this cop `Enabled: false` (opt-in); `default.yml` also
    // disables it. This fallback keeps every config path faithful.
    default_enabled = false,
    options = NoOptions
)]
impl EmptyLineAfterMultilineCondition {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        // `return if node.ternary?`
        if cx.is_ternary(node) {
            return;
        }
        let Some(cond) = cx.if_condition(node).get() else {
            return;
        };
        if cx.is_modifier_form(node) {
            // `check_condition(node.condition) if node.right_sibling`
            if cx.right_sibling(node).get().is_some() {
                check_condition(cond, cx);
            }
        } else {
            check_condition(cond, cx);
        }
    }

    #[on_node(kind = "while")]
    fn check_while(&self, node: NodeId, cx: &Cx<'_>) {
        check_loop(node, cx);
    }

    #[on_node(kind = "until")]
    fn check_until(&self, node: NodeId, cx: &Cx<'_>) {
        check_loop(node, cx);
    }

    #[on_node(kind = "case")]
    fn check_case(&self, node: NodeId, cx: &Cx<'_>) {
        for &when_node in cx.case_when_branches(node) {
            let Some(&last) = cx.when_conditions(when_node).last() else {
                continue;
            };
            if !multiline_when_condition(when_node, cx) {
                continue;
            }
            let last_line = line_of(cx.range(last).end.saturating_sub(1), cx);
            if next_line_empty(last_line, cx) {
                continue;
            }
            cx.emit_offense(cx.range(when_node), MSG, None);
            autocorrect_after(last, cx);
        }
    }

    #[on_node(kind = "rescue")]
    fn check_rescue(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Rescue { resbodies, .. } = *cx.kind(node) else {
            return;
        };
        for &resbody in cx.list(resbodies) {
            let NodeKind::Resbody { exceptions, .. } = *cx.kind(resbody) else {
                continue;
            };
            let exception_nodes = cx.list(exceptions);
            if !multiline_rescue_exceptions(exception_nodes, cx) {
                continue;
            }
            let last = *exception_nodes.last().expect("non-empty: size checked >= 2");
            let last_line = line_of(cx.range(last).end.saturating_sub(1), cx);
            if next_line_empty(last_line, cx) {
                continue;
            }
            cx.emit_offense(cx.range(resbody), MSG, None);
            autocorrect_after(last, cx);
        }
    }
}

/// Shared `on_while`/`on_until` + `on_while_post`/`on_until_post` body. The
/// `post` (do-while) form requires a right sibling; the standard form is
/// checked unconditionally.
fn check_loop(node: NodeId, cx: &Cx<'_>) {
    let (cond, post) = match *cx.kind(node) {
        NodeKind::While { cond, post, .. } | NodeKind::Until { cond, post, .. } => (cond, post),
        _ => return,
    };
    if post && cx.right_sibling(node).get().is_none() {
        return;
    }
    check_condition(cond, cx);
}

/// `check_condition` — flag a multiline condition whose following line is not
/// blank.
fn check_condition(condition: NodeId, cx: &Cx<'_>) {
    if !is_multiline(condition, cx) {
        return;
    }
    let last_line = line_of(cx.range(condition).end.saturating_sub(1), cx);
    if next_line_empty(last_line, cx) {
        return;
    }
    cx.emit_offense(cx.range(condition), MSG, None);
    autocorrect_after(condition, cx);
}

/// `node.multiline?` — the node spans more than one physical line.
fn is_multiline(node: NodeId, cx: &Cx<'_>) -> bool {
    let range = cx.range(node);
    line_of(range.start, cx) != line_of(range.end.saturating_sub(1), cx)
}

/// `next_line_empty?(line)` — `processed_source[line].blank?`. RuboCop's
/// 1-based `condition.last_line` indexes the 0-based `processed_source` array
/// at the line *after* the condition; our `last_line` is 0-based, so that is
/// `last_line + 1`.
fn next_line_empty(last_line: u32, cx: &Cx<'_>) -> bool {
    line_is_blank(cx, last_line + 1)
}

/// `multiline_when_condition?` — first condition's first line differs from the
/// last condition's last line.
fn multiline_when_condition(when_node: NodeId, cx: &Cx<'_>) -> bool {
    let conditions = cx.when_conditions(when_node);
    let (Some(&first), Some(&last)) = (conditions.first(), conditions.last()) else {
        return false;
    };
    line_of(cx.range(first).start, cx) != line_of(cx.range(last).end.saturating_sub(1), cx)
}

/// `multiline_rescue_exceptions?` — at least two exception classes, with the
/// first and last on different lines.
fn multiline_rescue_exceptions(exception_nodes: &[NodeId], cx: &Cx<'_>) -> bool {
    if exception_nodes.len() <= 1 {
        return false;
    }
    let first = exception_nodes[0];
    let last = *exception_nodes.last().expect("len > 1 checked above");
    line_of(cx.range(first).start, cx) != line_of(cx.range(last).end.saturating_sub(1), cx)
}

/// `range = range_by_whole_lines(node.source_range); corrector.insert_after(range, "\n")`.
fn autocorrect_after(node: NodeId, cx: &Cx<'_>) {
    let last_line = line_of(cx.range(node).end.saturating_sub(1), cx);
    let Some(line_start) = nth_line_start(cx, last_line) else {
        return;
    };
    let whole = whole_line_range_with_newline(line_start, cx);
    cx.emit_edit(
        Range {
            start: whole.end,
            end: whole.end,
        },
        "\n",
    );
}

#[cfg(test)]
mod tests {
    use super::EmptyLineAfterMultilineCondition;
    use murphy_plugin_api::test_support::{run_cop, run_cop_with_edits, test};

    #[test]
    fn flags_multiline_if_without_empty_line() {
        let src = "if multiline &&\n  condition\n  do_something\nend\n";
        let offenses = run_cop::<EmptyLineAfterMultilineCondition>(src);
        assert_eq!(offenses.len(), 1, "expected 1 offense, got {offenses:?}");
        assert_eq!(offenses[0].message, "Use empty line after multiline condition.");
    }

    #[test]
    fn accepts_multiline_if_with_empty_line() {
        test::<EmptyLineAfterMultilineCondition>()
            .expect_no_offenses("if multiline &&\n  condition\n\n  do_something\nend\n");
    }

    #[test]
    fn accepts_single_line_if() {
        test::<EmptyLineAfterMultilineCondition>()
            .expect_no_offenses("if condition\n  do_something\nend\n");
    }

    #[test]
    fn accepts_ternary() {
        test::<EmptyLineAfterMultilineCondition>().expect_no_offenses("x = a ? b : c\n");
    }

    #[test]
    fn flags_multiline_while() {
        let src = "while a &&\n  b\n  c\nend\n";
        let offenses = run_cop::<EmptyLineAfterMultilineCondition>(src);
        assert_eq!(offenses.len(), 1, "expected 1 offense, got {offenses:?}");
    }

    #[test]
    fn flags_multiline_until() {
        let src = "until a &&\n  b\n  c\nend\n";
        let offenses = run_cop::<EmptyLineAfterMultilineCondition>(src);
        assert_eq!(offenses.len(), 1, "expected 1 offense, got {offenses:?}");
    }

    #[test]
    fn flags_multiline_case_when() {
        let src = "case x\nwhen foo,\n  bar\n  do_something\nend\n";
        let offenses = run_cop::<EmptyLineAfterMultilineCondition>(src);
        assert_eq!(offenses.len(), 1, "expected 1 offense, got {offenses:?}");
    }

    #[test]
    fn accepts_multiline_case_when_with_empty_line() {
        test::<EmptyLineAfterMultilineCondition>()
            .expect_no_offenses("case x\nwhen foo,\n  bar\n\n  do_something\nend\n");
    }

    #[test]
    fn flags_multiline_rescue() {
        let src = "begin\n  do_something\nrescue FooError,\n  BarError\n  handle_error\nend\n";
        let offenses = run_cop::<EmptyLineAfterMultilineCondition>(src);
        assert_eq!(offenses.len(), 1, "expected 1 offense, got {offenses:?}");
    }

    #[test]
    fn accepts_multiline_rescue_with_empty_line() {
        test::<EmptyLineAfterMultilineCondition>().expect_no_offenses(
            "begin\n  do_something\nrescue FooError,\n  BarError\n\n  handle_error\nend\n",
        );
    }

    #[test]
    fn accepts_single_exception_rescue() {
        // Only one exception class on one line → not a multiline condition.
        test::<EmptyLineAfterMultilineCondition>()
            .expect_no_offenses("begin\n  do_something\nrescue FooError\n  handle_error\nend\n");
    }

    #[test]
    fn corrects_multiline_if() {
        let src = "if multiline &&\n  condition\n  do_something\nend\n";
        let result = run_cop_with_edits::<EmptyLineAfterMultilineCondition>(src);
        assert_eq!(result.offenses.len(), 1);
        let edit = &result.edits[0];
        assert_eq!(edit.replacement, "\n");
        // Inserted right after the `  condition\n` line.
        let inserted_at = edit.range.start as usize;
        assert_eq!(&src[..inserted_at], "if multiline &&\n  condition\n");
    }
}

murphy_plugin_api::submit_cop!(EmptyLineAfterMultilineCondition);
