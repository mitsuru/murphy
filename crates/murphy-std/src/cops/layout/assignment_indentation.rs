//! `Layout/AssignmentIndentation` — checks the indentation of the first line of
//! the right-hand side of a multi-line assignment.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/AssignmentIndentation
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: [murphy-vafs]
//! notes: >
//!   Ports `check_assignment` over the `CheckAssignment` mixin's node set:
//!   every `*asgn` write (`lvasgn`/`ivasgn`/`cvasgn`/`gvasgn`/`casgn`/`masgn`/
//!   `op_asgn`/`or_asgn`/`and_asgn`) plus setter sends (`obj.foo = rhs`,
//!   guarded to a literal `=` operator). The RHS is extracted per `extract_rhs`
//!   (the assignment's value, or a setter send's last argument).
//!
//!   The cop is skipped unless there is an RHS, an assignment operator, and the
//!   operator and RHS are on different lines (RuboCop's
//!   `return if same_line?(node.loc.operator, rhs)`). When the RHS *does* start
//!   on its own line, its column must equal `base + IndentationWidth`, where
//!   `base` is the (0-based) column of the leftmost assignment on the same line
//!   (RuboCop's `leftmost_multiple_assignment`). Otherwise an offense is
//!   reported on the RHS's first line.
//!
//!   Autocorrect: re-indents the RHS's first line to the expected column
//!   (RuboCop's `AlignmentCorrector.correct` with the column delta). Only the
//!   first physical line is moved; following lines are left to
//!   `Layout/IndentationConsistency` / `Layout/EndAlignment`, mirroring
//!   upstream. The edit is only emitted when the RHS is the first
//!   non-whitespace on its line; idempotent.
//!
//!   `IndentationWidth` matches RuboCop's resolution: this cop's own
//!   `IndentationWidth` override is honoured, and when unset the width falls
//!   back to the run-wide resolved `Layout/IndentationWidth: Width` via
//!   `cx.indentation_width()` (default 2) — murphy-kke2.
//!
//!   Gaps vs upstream:
//!   - Column is counted by Unicode scalar (`chars().count()`), not RuboCop's
//!     `display_column` (which counts East-Asian wide glyphs as width 2). Wide
//!     characters before the assignment are an edge gap.
//! ```
//!
//! ## Matched shapes
//!
//! `*asgn` writes and setter sends whose multi-line RHS begins on a line of its
//! own that is not indented `IndentationWidth` past the leftmost assignment.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, OptNodeId, Range, cop};

const MSG: &str = "Indent the first line of the right-hand-side of a multi-line assignment.";

/// Stateless unit struct (ADR 0035).
#[derive(Default)]
pub struct AssignmentIndentation;

/// Options for [`AssignmentIndentation`].
#[derive(CopOptions)]
pub struct AssignmentIndentationOptions {
    // `Option<i64>` so the bundled default `IndentationWidth: ~` (JSON null) and
    // an unset key both decode to `None`, which falls back to the run-wide
    // resolved `Layout/IndentationWidth.Width` via `cx.indentation_width()`.
    #[option(
        name = "IndentationWidth",
        description = "Number of spaces for one indentation level (null/unset falls back to Layout/IndentationWidth's Width, default 2)."
    )]
    pub indentation_width: Option<i64>,
}

#[cop(
    name = "Layout/AssignmentIndentation",
    description = "Checks the indentation of the first line of the right-hand-side of a multi-line assignment.",
    default_severity = "warning",
    default_enabled = true,
    options = AssignmentIndentationOptions
)]
impl AssignmentIndentation {
    #[on_node(kind = "lvasgn")]
    fn check_lvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "ivasgn")]
    fn check_ivasgn(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "cvasgn")]
    fn check_cvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "gvasgn")]
    fn check_gvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "casgn")]
    fn check_casgn(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "masgn")]
    fn check_masgn(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "op_asgn")]
    fn check_op_asgn(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "or_asgn")]
    fn check_or_asgn(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "and_asgn")]
    fn check_and_asgn(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let is_send = matches!(cx.kind(node), NodeKind::Send { .. });

    // `return unless rhs`.
    let Some(rhs) = extract_rhs(node, is_send, cx) else {
        return;
    };

    // `return unless node.loc.operator`. Setter sends additionally require a
    // literal `=` operator.
    let op = if is_send {
        let Some(eq) = setter_operator_range(node, rhs, cx) else {
            return;
        };
        eq
    } else {
        assignment_op_range(node, cx)
    };
    if op == Range::ZERO {
        return;
    }

    // `return if same_line?(node.loc.operator, rhs)`. The operator always
    // precedes the RHS, so they share a line iff no newline lies between them.
    let src = cx.source();
    let bytes = src.as_bytes();
    let rhs_start = cx.range(rhs).start;
    if !gap_has_newline(bytes, op.start, rhs_start) {
        return;
    }

    // `base = display_column(leftmost_multiple_assignment(node).source_range)`.
    let leftmost = leftmost_multiple_assignment(node, cx);
    let base = column_of(cx, cx.range(leftmost).start);

    let opts = cx.options_or_default::<AssignmentIndentationOptions>();
    let expected = base
        + opts
            .indentation_width
            .unwrap_or(cx.indentation_width())
            .max(0) as usize;

    // `check_alignment([rhs], expected)`: the RHS's first-line column must equal
    // `expected`.
    let actual = column_of(cx, rhs_start);
    if actual == expected {
        return;
    }

    cx.emit_offense(first_line_range(rhs, cx), MSG, None);

    // Autocorrect: re-indent the RHS's first line to `expected`. Only when the
    // RHS is the first non-whitespace on its line. Idempotent.
    if let Some(line_start) = line_start_if_leads(rhs_start, cx) {
        let indent = " ".repeat(expected);
        cx.emit_edit(
            Range {
                start: line_start,
                end: rhs_start,
            },
            &indent,
        );
    }
}

/// `extract_rhs`: a setter send's last argument, or an assignment's value.
fn extract_rhs(node: NodeId, is_send: bool, cx: &Cx<'_>) -> Option<NodeId> {
    if is_send {
        return cx.call_arguments(node).last().copied();
    }
    assignment_value(node, cx).get()
}

/// The RHS value of an `*asgn` node.
fn assignment_value(node: NodeId, cx: &Cx<'_>) -> OptNodeId {
    match *cx.kind(node) {
        NodeKind::Lvasgn { value, .. }
        | NodeKind::Ivasgn { value, .. }
        | NodeKind::Cvasgn { value, .. }
        | NodeKind::Gvasgn { value, .. }
        | NodeKind::Casgn { value, .. } => value,
        NodeKind::Masgn { rhs, .. } => OptNodeId::some(rhs),
        NodeKind::OpAsgn { value, .. }
        | NodeKind::OrAsgn { value, .. }
        | NodeKind::AndAsgn { value, .. } => OptNodeId::some(value),
        _ => OptNodeId::NONE,
    }
}

/// `node.assignment?` — whether `node` is any assignment kind.
fn is_assignment(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        *cx.kind(node),
        NodeKind::Lvasgn { .. }
            | NodeKind::Ivasgn { .. }
            | NodeKind::Cvasgn { .. }
            | NodeKind::Gvasgn { .. }
            | NodeKind::Casgn { .. }
            | NodeKind::Masgn { .. }
            | NodeKind::OpAsgn { .. }
            | NodeKind::OrAsgn { .. }
            | NodeKind::AndAsgn { .. }
    )
}

/// RuboCop's `leftmost_multiple_assignment`: walk up the parent chain while the
/// parent is on the same line as the node AND the parent is itself an
/// assignment. Returns the leftmost such assignment (for `a = b = c = rhs` the
/// `a = …` node).
fn leftmost_multiple_assignment(node: NodeId, cx: &Cx<'_>) -> NodeId {
    let mut current = node;
    let bytes = cx.source().as_bytes();
    while let Some(parent) = cx.parent(current).get() {
        if is_assignment(parent, cx)
            && !gap_has_newline(bytes, cx.range(parent).start, cx.range(current).start)
        {
            current = parent;
        } else {
            break;
        }
    }
    current
}

/// The setter `=` operator of a send whose RHS is `rhs` (its last argument).
/// `None` if the send is not a setter. Handles `obj.foo = x` and `foo[:x] = y`.
fn setter_operator_range(node: NodeId, rhs: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let search_from = cx
        .call_receiver(node)
        .get()
        .map_or(cx.range(node).start, |r| cx.range(r).end);
    let rhs_start = cx.range(rhs).start;
    let toks = cx.sorted_tokens();
    let lo = toks.partition_point(|t| t.range.start < search_from);
    let hi = toks.partition_point(|t| t.range.end <= rhs_start);
    if lo >= hi {
        return None;
    }
    toks[lo..hi]
        .iter()
        .rev()
        .find(|t| cx.raw_source(t.range) == "=")
        .map(|t| t.range)
}

/// The assignment operator (`=`, `+=`, `||=`, …) of an `*asgn` node, or
/// `Range::ZERO` if it cannot be located.
fn assignment_op_range(node: NodeId, cx: &Cx<'_>) -> Range {
    let Some(rhs) = assignment_value(node, cx).get() else {
        return Range::ZERO;
    };
    let node_start = cx.range(node).start as usize;
    let rhs_start = cx.range(rhs).start as usize;
    let src = cx.source().as_bytes();
    let gap = &src[node_start..rhs_start];
    gap.iter()
        .rposition(|&b| b == b'=')
        .map_or(Range::ZERO, |idx| {
            let pos = (node_start + idx) as u32;
            Range {
                start: pos,
                end: pos + 1,
            }
        })
}

/// 0-based character column of `offset` (RuboCop's `column`).
fn column_of(cx: &Cx<'_>, offset: u32) -> usize {
    let src = cx.source();
    let upper = (offset as usize).min(src.len());
    let line_start = src[..upper].rfind('\n').map_or(0, |pos| pos + 1);
    src[line_start..upper].chars().count()
}

/// The portion of `node`'s source range up to (but excluding) the first
/// newline — its first physical line.
fn first_line_range(node: NodeId, cx: &Cx<'_>) -> Range {
    let range = cx.range(node);
    let start = range.start as usize;
    let end = cx
        .source()
        .as_bytes()
        .get(start..range.end as usize)
        .and_then(|line| line.iter().position(|&b| b == b'\n'))
        .map_or(range.end as usize, |idx| start + idx);
    Range {
        start: range.start,
        end: end as u32,
    }
}

/// Whether `[start, end)` contains a newline.
fn gap_has_newline(src: &[u8], start: u32, end: u32) -> bool {
    let lo = (start as usize).min(src.len());
    let hi = (end as usize).min(src.len()).max(lo);
    src[lo..hi].contains(&b'\n')
}

/// If the token at `offset` is the first non-whitespace on its line, return the
/// line's start byte offset; otherwise `None`.
fn line_start_if_leads(offset: u32, cx: &Cx<'_>) -> Option<u32> {
    let src = cx.source();
    let offset = offset as usize;
    let line_start = src[..offset].rfind('\n').map_or(0, |pos| pos + 1);
    if src[line_start..offset].bytes().all(|b| b == b' ' || b == b'\t') {
        Some(line_start as u32)
    } else {
        None
    }
}

murphy_plugin_api::submit_cop!(AssignmentIndentation);

#[cfg(test)]
mod tests {
    use super::AssignmentIndentation as Cop;
    use murphy_plugin_api::test_support::{CapturedEdit, run_cop, run_cop_with_edits, test};

    fn apply(source: &str, edits: &[CapturedEdit]) -> String {
        let mut sorted: Vec<&CapturedEdit> = edits.iter().collect();
        sorted.sort_by_key(|e| e.range.start);
        let mut out = String::new();
        let mut cursor = 0usize;
        for e in sorted {
            out.push_str(&source[cursor..e.range.start as usize]);
            out.push_str(&e.replacement);
            cursor = e.range.end as usize;
        }
        out.push_str(&source[cursor..]);
        out
    }

    #[test]
    fn flags_unindented_rhs() {
        let src = "a =\nif b ; end\n";
        let offenses = run_cop::<Cop>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(offenses[0].message, super::MSG);
    }

    #[test]
    fn accepts_properly_indented_rhs() {
        test::<Cop>().expect_no_offenses("a =\n  if b ; end\n");
    }

    /// Cross-cop fallback (murphy-kke2): with this cop's own `IndentationWidth`
    /// unset, the RHS indent comes from the run-wide resolved
    /// `Layout/IndentationWidth.Width`. At width 4 an RHS indented 4 is accepted;
    /// under the old hardcoded 2 it was flagged as over-indented.
    #[test]
    fn falls_back_to_layout_indentation_width() {
        test::<Cop>()
            .with_indentation_width(4)
            .expect_no_offenses("a =\n    if b ; end\n");
    }

    #[test]
    fn accepts_single_line_assignment() {
        test::<Cop>().expect_no_offenses("a = if b ; end\n");
    }

    #[test]
    fn accepts_rhs_on_operator_line() {
        // RHS does not start on a new line — not checked.
        test::<Cop>().expect_no_offenses("a = b +\n  c\n");
    }

    #[test]
    fn flags_overindented_rhs() {
        // base 0 + width 2 = 2; RHS at column 4.
        let src = "a =\n    if b ; end\n";
        let offenses = run_cop::<Cop>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
    }

    #[test]
    fn flags_multi_lhs() {
        let src = "a,\nb =\nif b ; end\n";
        let offenses = run_cop::<Cop>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
    }

    #[test]
    fn accepts_multi_lhs_indented() {
        test::<Cop>().expect_no_offenses("a,\nb =\n  if b ; end\n");
    }

    #[test]
    fn ignores_comparison_operators() {
        test::<Cop>().expect_no_offenses("a ==\n  b\n");
    }

    #[test]
    fn flags_chained_assignment_base_at_leftmost() {
        // `foo = bar = <rhs>`: base is `foo`'s column (0), expected 2.
        let src = "foo = bar =\nbaz_method\n";
        let offenses = run_cop::<Cop>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
    }

    #[test]
    fn accepts_chained_assignment_indented() {
        test::<Cop>().expect_no_offenses("foo = bar =\n  baz_method\n");
    }

    #[test]
    fn corrects_unindented_rhs() {
        let src = "a =\nif b ; end\n";
        let run = run_cop_with_edits::<Cop>(src);
        assert_eq!(apply(src, &run.edits), "a =\n  if b ; end\n");
    }

    #[test]
    fn corrects_overindented_rhs() {
        let src = "a =\n    if b ; end\n";
        let run = run_cop_with_edits::<Cop>(src);
        assert_eq!(apply(src, &run.edits), "a =\n  if b ; end\n");
    }

    #[test]
    fn correction_is_idempotent() {
        let src = "a =\n      if b ; end\n";
        let run = run_cop_with_edits::<Cop>(src);
        let fixed = apply(src, &run.edits);
        assert!(run_cop::<Cop>(&fixed).is_empty(), "not idempotent: {fixed:?}");
    }

    #[test]
    fn flags_setter_send() {
        let src = "obj.foo =\nbar_method\n";
        let offenses = run_cop::<Cop>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
    }
}
