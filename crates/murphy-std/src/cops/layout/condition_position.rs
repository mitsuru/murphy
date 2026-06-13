//! `Layout/ConditionPosition` — flags a condition placed on a different line
//! from its `if`/`unless`/`elsif`/`while`/`until` keyword.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/ConditionPosition
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Direct port of RuboCop's `on_if` / `on_while` (aliased `on_until`). The
//!   cop fires when a block-form conditional's condition begins on a line after
//!   the keyword. Skip conditions mirror upstream's `check`: ternaries
//!   (`on_if` returns early on `node.ternary?`), modifier form
//!   (`node.modifier_form?`), and single-line conditions
//!   (`node.single_line_condition?`, i.e. the keyword and the condition's first
//!   line coincide). The keyword line/source is taken from
//!   `cx.if_keyword_loc` for `if`/`unless`/`elsif` and `cx.loc(node).keyword()`
//!   for block-form `while`/`until`. Autocorrect inserts ` <condition source>`
//!   after the keyword token and deletes the condition's original whole lines,
//!   matching RuboCop's `insert_after(keyword, " #{source}")` +
//!   `remove(range_by_whole_lines(condition, include_final_newline: true))`.
//! ```

use murphy_plugin_api::{Cx, NodeId, Range, cop};

/// Stateless unit struct (ADR 0035 const-metadata cop pattern).
#[derive(Default)]
pub struct ConditionPosition;

#[cop(
    name = "Layout/ConditionPosition",
    description = "Checks for condition placed in a confusing position relative to the keyword.",
    default_severity = "warning",
    default_enabled = true
)]
impl ConditionPosition {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        // RuboCop `on_if`: `return if node.ternary?`.
        if cx.is_ternary(node) {
            return;
        }
        let keyword = cx.if_keyword_loc(node);
        check(self, node, cx, keyword);
    }

    #[on_node(kind = "while")]
    fn check_while(&self, node: NodeId, cx: &Cx<'_>) {
        let keyword = cx.loc(node).keyword();
        check(self, node, cx, keyword);
    }

    #[on_node(kind = "until")]
    fn check_until(&self, node: NodeId, cx: &Cx<'_>) {
        let keyword = cx.loc(node).keyword();
        check(self, node, cx, keyword);
    }
}

/// RuboCop's `check(node)`.
fn check(_cop: &ConditionPosition, node: NodeId, cx: &Cx<'_>, keyword: Range) {
    // `return if node.modifier_form? || node.single_line_condition?`.
    if cx.is_modifier_form(node) {
        return;
    }
    let Some(condition) = condition_of(node, cx) else {
        return;
    };
    // No keyword token resolved (malformed / unsupported shape) — nothing to do.
    if keyword == Range::ZERO {
        return;
    }

    let keyword_line = line_of(cx, keyword.start);
    let condition_line = line_of(cx, cx.range(condition).start);

    // `single_line_condition?` — keyword and condition share their first line.
    if keyword_line == condition_line {
        return;
    }

    let keyword_src = cx.raw_source(keyword);
    let message = format!("Place the condition on the same line as `{keyword_src}`.");
    cx.emit_offense(cx.range(condition), &message, None);

    // Autocorrect: insert " <condition>" after the keyword, then remove the
    // condition's original whole lines (including the trailing newline).
    let condition_src = cx.raw_source(cx.range(condition));
    cx.emit_edit(
        Range {
            start: keyword.end,
            end: keyword.end,
        },
        &format!(" {condition_src}"),
    );
    let whole = cx.range_by_whole_lines(cx.range(condition), true);
    cx.emit_edit(whole, "");
}

/// The condition child of an `if`/`while`/`until` node.
fn condition_of(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    use murphy_plugin_api::NodeKind;
    match *cx.kind(node) {
        NodeKind::If { cond, .. } => Some(cond),
        NodeKind::While { cond, .. } | NodeKind::Until { cond, .. } => Some(cond),
        _ => None,
    }
}

/// 1-based source line of byte `offset`.
fn line_of(cx: &Cx<'_>, offset: u32) -> usize {
    cx.source()[..offset as usize].matches('\n').count() + 1
}

murphy_plugin_api::submit_cop!(ConditionPosition);

#[cfg(test)]
mod tests {
    use super::ConditionPosition;
    use murphy_plugin_api::test_support::{run_cop, run_cop_with_edits};

    fn apply(source: &str, edits: &[murphy_plugin_api::test_support::CapturedEdit]) -> String {
        let mut out = String::with_capacity(source.len());
        let mut last = 0usize;
        let mut ordered: Vec<_> = edits.iter().collect();
        ordered.sort_by_key(|e| e.range.start);
        for e in ordered {
            out.push_str(&source[last..e.range.start as usize]);
            out.push_str(&e.replacement);
            last = e.range.end as usize;
        }
        out.push_str(&source[last..]);
        out
    }

    #[test]
    fn flags_if_condition_on_next_line() {
        let src = "if\nx == 10\nend\n";
        let run = run_cop_with_edits::<ConditionPosition>(src);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(
            run.offenses[0].message,
            "Place the condition on the same line as `if`."
        );
        assert_eq!(apply(src, &run.edits), "if x == 10\nend\n");
    }

    #[test]
    fn flags_while_condition_on_next_line() {
        let src = "while\nx == 10\nend\n";
        let run = run_cop::<ConditionPosition>(src);
        assert_eq!(run.len(), 1);
        assert_eq!(
            run[0].message,
            "Place the condition on the same line as `while`."
        );
    }

    #[test]
    fn flags_until_condition_on_next_line() {
        let src = "until\nx == 10\nend\n";
        let run = run_cop::<ConditionPosition>(src);
        assert_eq!(run.len(), 1);
        assert_eq!(
            run[0].message,
            "Place the condition on the same line as `until`."
        );
    }

    #[test]
    fn accepts_condition_on_same_line() {
        assert!(run_cop::<ConditionPosition>("if x == 10\n bala\nend\n").is_empty());
    }

    #[test]
    fn accepts_modifier_form() {
        assert!(
            run_cop::<ConditionPosition>("do_something if\n  something && something_else\n")
                .is_empty()
        );
    }

    #[test]
    fn flags_elsif_condition_on_next_line() {
        let src = "if something\n  test\nelsif\n  something\n  test\nend\n";
        let run = run_cop_with_edits::<ConditionPosition>(src);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(
            run.offenses[0].message,
            "Place the condition on the same line as `elsif`."
        );
        assert_eq!(
            apply(src, &run.edits),
            "if something\n  test\nelsif something\n  test\nend\n"
        );
    }

    #[test]
    fn accepts_ternary() {
        assert!(run_cop::<ConditionPosition>("x ? a : b\n").is_empty());
    }

    #[test]
    fn accepts_unless_condition_same_line() {
        assert!(run_cop::<ConditionPosition>("unless x == 10\n  bala\nend\n").is_empty());
    }

    #[test]
    fn flags_unless_condition_on_next_line() {
        let src = "unless\nx == 10\nend\n";
        let run = run_cop::<ConditionPosition>(src);
        assert_eq!(run.len(), 1);
        assert_eq!(
            run[0].message,
            "Place the condition on the same line as `unless`."
        );
    }
}
