//! `Layout/EndAlignment` — flags `end` keywords that are not aligned with the
//! keyword (or, in other styles, the variable / start-of-line) that opens the
//! construct.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/EndAlignment
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: [murphy-ewdz]
//! notes: >
//!   Ports the default `EnforcedStyleAlignWith: keyword`. Handlers fire on
//!   `class`/`sclass`/`module`/`if` (non-ternary)/`while`/`until`/`case`/
//!   `case_match`. For the `keyword` style, RuboCop aligns `end` with the
//!   construct's opening keyword (`inner_node.loc.keyword` in both
//!   `check_other_alignment` and `check_asgn_alignment`), so the assignment vs.
//!   non-assignment split is irrelevant to this style and is not ported.
//!
//!   Alignment rule (`matching_ranges` for the `keyword` key): the `end` is
//!   correct when it is on the same line as the keyword OR shares the keyword's
//!   (0-based, character-counted) column. Otherwise an offense is reported on
//!   the `end` keyword with RuboCop's message
//!   ``\`end\` at L, C is not aligned with \`<kw>\` at L, C.`` and an
//!   autocorrect re-indents the `end` line to the keyword's column.
//!
//!   Gaps vs. upstream (tracked in murphy-ewdz):
//!   - `EnforcedStyleAlignWith: variable` is not implemented — it requires the
//!     assignment LHS range (`asgn_variable_align_with`), which has no direct
//!     `NodeLoc` surface. The cop ignores the configured style and always
//!     enforces `keyword`.
//!   - `EnforcedStyleAlignWith: start_of_line` is likewise not implemented.
//!   These need a config-time SupportedStylesAlignWith surface plus the
//!   variable/line-range computation; only the default ships here.
//!
//!   ABI note: `LocRef::keyword()` and `LocRef::end_keyword()` provide the two
//!   ranges directly. `Sclass` and `CaseMatch` are not keyword-bearing in
//!   `LocRef::keyword()`, so their `class`/`case` keyword range is recovered
//!   with a token scan at the node start.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

#[derive(Default)]
pub struct EndAlignment;

#[cop(
    name = "Layout/EndAlignment",
    description = "Align ends correctly.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl EndAlignment {
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "sclass")]
    fn check_sclass(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "module")]
    fn check_module(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        // `on_if(node)` runs `check_other_alignment unless node.ternary?`.
        // Ternaries and modifier-form `if`/`unless` have no `end` keyword, so
        // the `end_keyword == ZERO` guard in `check` filters them out.
        check(node, cx);
    }

    #[on_node(kind = "while")]
    fn check_while(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "until")]
    fn check_until(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "case")]
    fn check_case(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "case_match")]
    fn check_case_match(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let Some(end_kw) = end_keyword_range(node, cx) else {
        // Modifier-form / ternary / no `end` — nothing to align.
        return;
    };
    let Some(kw) = keyword_range(node, cx) else {
        // Keyword range not recoverable (e.g. nested `elsif`) — skip; the
        // construct is reported (if at all) through its outer node.
        return;
    };

    let (kw_line, kw_col) = line_col(kw.start, cx);
    let (end_line, end_col) = line_col(end_kw.start, cx);

    // `matching_ranges` for the keyword style: aligned when on the same line as
    // the keyword OR at the same column.
    if kw_line == end_line || kw_col == end_col {
        return;
    }

    let source = cx.raw_source(kw);
    let msg = format!(
        "`end` at {end_line}, {end_col} is not aligned with `{source}` at {kw_line}, {kw_col}."
    );
    cx.emit_offense(end_kw, &msg, None);

    // Autocorrect: re-indent the `end` line to the keyword's column. Only when
    // `end` is the first non-whitespace on its line (otherwise rewriting the
    // leading whitespace would corrupt inline code).
    if let Some(line_start) = line_start_if_end_leads(end_kw.start, cx) {
        let indent = " ".repeat(kw_col);
        cx.emit_edit(
            Range {
                start: line_start,
                end: end_kw.start,
            },
            &indent,
        );
    }
}

/// The `end` keyword range of the construct, or `None` for modifier-form /
/// ternary nodes that have no `end`.
fn end_keyword_range(node: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let r = cx.loc(node).end_keyword();
    (r != Range::ZERO).then_some(r)
}

/// The opening keyword range of the construct. Uses `LocRef::keyword()` for the
/// keyword-bearing nodes; for `Sclass` / `CaseMatch` (which `keyword()` does
/// not classify as keyword-bearing) it recovers the keyword token at the node
/// start. Returns `None` when no keyword token starts at the node start (e.g. a
/// nested `elsif`, whose leading token is `elsif`, not `if`).
fn keyword_range(node: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let kw = cx.loc(node).keyword();
    if kw != Range::ZERO {
        return Some(kw);
    }
    // Fallback for Sclass / CaseMatch: the keyword token sits at the node
    // start. Reject anything that is not the expected opening keyword so a
    // nested `elsif`/`else`/`when` does not masquerade as a top keyword.
    if !matches!(*cx.kind(node), NodeKind::Sclass { .. } | NodeKind::CaseMatch { .. }) {
        return None;
    }
    let start = cx.range(node).start;
    cx.token_after(start).and_then(|t| {
        if t.kind == SourceTokenKind::Other && t.range.start == start {
            let text = cx.raw_source(t.range);
            if matches!(text, "class" | "case") {
                return Some(t.range);
            }
        }
        None
    })
}

/// 1-based line and 0-based character column of `offset`.
fn line_col(offset: u32, cx: &Cx<'_>) -> (usize, usize) {
    let src = cx.source();
    let upper = (offset as usize).min(src.len());
    let line = src[..upper].bytes().filter(|&b| b == b'\n').count() + 1;
    let line_start = src[..upper]
        .rfind('\n')
        .map_or(0, |pos| pos + 1);
    let col = src[line_start..upper].chars().count();
    (line, col)
}

/// If the `end` keyword at `end_start` is the first non-whitespace character on
/// its line, return the byte offset of that line's start; otherwise `None`.
fn line_start_if_end_leads(end_start: u32, cx: &Cx<'_>) -> Option<u32> {
    let src = cx.source();
    let end_start = end_start as usize;
    let line_start = src[..end_start].rfind('\n').map_or(0, |pos| pos + 1);
    if src[line_start..end_start].bytes().all(|b| b == b' ' || b == b'\t') {
        Some(line_start as u32)
    } else {
        None
    }
}

murphy_plugin_api::submit_cop!(EndAlignment);

#[cfg(test)]
mod tests {
    use super::EndAlignment as Cop;
    use murphy_plugin_api::test_support::{run_cop, run_cop_with_edits, test, CapturedEdit};

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
    fn accepts_aligned_class() {
        test::<Cop>().expect_no_offenses("class Foo\n  x = 1\nend\n");
    }

    #[test]
    fn accepts_aligned_module() {
        test::<Cop>().expect_no_offenses("module Foo\n  x = 1\nend\n");
    }

    #[test]
    fn accepts_single_line_if() {
        test::<Cop>().expect_no_offenses("if x then y end\n");
    }

    #[test]
    fn accepts_modifier_if() {
        // Modifier-form `if` has no `end`.
        test::<Cop>().expect_no_offenses("foo if bar\n");
    }

    #[test]
    fn accepts_ternary() {
        test::<Cop>().expect_no_offenses("x = a ? b : c\n");
    }

    #[test]
    fn accepts_aligned_if() {
        test::<Cop>().expect_no_offenses("if cond\n  foo\nend\n");
    }

    #[test]
    fn accepts_aligned_elsif_chain() {
        test::<Cop>().expect_no_offenses("if a\n  1\nelsif b\n  2\nend\n");
    }

    #[test]
    fn accepts_aligned_while() {
        test::<Cop>().expect_no_offenses("while cond\n  foo\nend\n");
    }

    #[test]
    fn accepts_aligned_until() {
        test::<Cop>().expect_no_offenses("until cond\n  foo\nend\n");
    }

    #[test]
    fn accepts_aligned_case() {
        test::<Cop>().expect_no_offenses("case x\nwhen 1\n  a\nend\n");
    }

    #[test]
    fn accepts_aligned_case_match() {
        test::<Cop>().expect_no_offenses("case x\nin 1\n  a\nend\n");
    }

    #[test]
    fn accepts_aligned_sclass() {
        test::<Cop>().expect_no_offenses("class << self\n  x = 1\nend\n");
    }

    #[test]
    fn flags_misaligned_end_in_assignment() {
        // `x = if c ... end` — keyword style wants `end` under `if`, not `x`.
        let src = "x = if c\n  foo\n      end\n";
        let offenses = run_cop::<Cop>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert!(
            offenses[0].message.contains("is not aligned with `if`"),
            "msg: {}",
            offenses[0].message
        );
    }

    #[test]
    fn flags_misaligned_class_end() {
        let src = "class Foo\n  x = 1\n  end\n";
        let offenses = run_cop::<Cop>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "`end` at 3, 2 is not aligned with `class` at 1, 0."
        );
    }

    #[test]
    fn flags_misaligned_if_end() {
        let src = "if cond\n  foo\n  end\n";
        let offenses = run_cop::<Cop>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "`end` at 3, 2 is not aligned with `if` at 1, 0."
        );
    }

    #[test]
    fn corrects_misaligned_class_end() {
        let src = "class Foo\n  x = 1\n  end\n";
        let run = run_cop_with_edits::<Cop>(src);
        assert_eq!(apply(src, &run.edits), "class Foo\n  x = 1\nend\n");
    }

    #[test]
    fn corrects_misaligned_nested_end() {
        // `end` of inner `if` should align with the inner `if` keyword column.
        let src = "def foo\n  if c\n    x\n      end\nend\n";
        let run = run_cop_with_edits::<Cop>(src);
        assert_eq!(apply(src, &run.edits), "def foo\n  if c\n    x\n  end\nend\n");
    }

    #[test]
    fn correction_is_idempotent() {
        let src = "class Foo\n  x = 1\n      end\n";
        let run = run_cop_with_edits::<Cop>(src);
        let fixed = apply(src, &run.edits);
        assert!(run_cop::<Cop>(&fixed).is_empty(), "not idempotent: {fixed:?}");
    }
}
