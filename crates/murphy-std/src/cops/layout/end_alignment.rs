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
//! status: verified
//! gap_issues: []
//! notes: >
//!   Ports all three `EnforcedStyleAlignWith` styles (`keyword` default,
//!   `variable`, `start_of_line`). Handlers fire on
//!   `class`/`sclass`/`module`/`if` (non-ternary)/`while`/`until`/`case`/
//!   `case_match`.
//!
//!   Alignment rule (`matching_ranges`): the `end` is correct when it is on the
//!   same line as the anchor OR shares the anchor's (0-based, character-counted)
//!   column. Otherwise an offense is reported on the `end` keyword with
//!   RuboCop's message
//!   ``\`end\` at L, C is not aligned with \`<source>\` at L, C.`` and an
//!   autocorrect re-indents the `end` line to the anchor's column.
//!
//!   Style anchors (`align_anchor`):
//!   - `keyword`: the construct's opening keyword (`node.loc.keyword`).
//!   - `start_of_line`: `start_line_range(node)` — the keyword line's leading
//!     content. Its column is the line's indentation (NOT the keyword column —
//!     `puts(if true` aligns `end` to col 0, not the `if` at col 5), and its
//!     source is the trimmed line content.
//!   - `variable`: the keyword EXCEPT when the construct is the RHS of an
//!     assignment / command call on the same line, then the range
//!     `[outer.begin, keyword.end)` (`asgn_variable_align_with`). A line break
//!     before the keyword falls back to the keyword.
//!
//!   `variable` routing without cross-handler `ignore_node` state: `asgn_outer_node`
//!   walks up the **leftmost** position through parenthesized `Begin` / `Or` /
//!   `And` wrappers (mirroring RuboCop's `rhs = rhs.child_nodes.first while
//!   rhs.type?(:begin, :or, :and)` unwrap) and accepts the parent when it is one
//!   of the nine assignment kinds whose value is the chain, or a `Send`/`Csend`
//!   whose last argument is the chain (setter / command call). A `case`/`case_match`
//!   in argument position anchors on its parent directly (`on_case`'s
//!   `node.argument?` branch). Because only one handler fires per construct, no
//!   `ignore_node` deduplication is needed. Verified against RuboCop 1.86.2 across
//!   `x = if`, `x ||= while`, `x = (if`, `x = foo || if` (falls back to keyword),
//!   `x = if … end || bar`, `obj.foo = if`, `foo bar, if`, `foo(case …)`, and
//!   the line-break-before-keyword case.
//!
//!   ABI note: `LocRef::keyword()` and `LocRef::end_keyword()` provide the two
//!   ranges directly. `Sclass` and `CaseMatch` are not keyword-bearing in
//!   `LocRef::keyword()`, so their `class`/`case` keyword range is recovered
//!   with a token scan at the node start.
//! ```

use murphy_plugin_api::{
    CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, SourceTokenKind, cop,
};

#[derive(Default)]
pub struct EndAlignment;

#[derive(CopOptions)]
pub struct EndAlignmentOptions {
    #[option(
        name = "EnforcedStyleAlignWith",
        default = "keyword",
        description = "Whether `end` aligns with the construct's keyword, the assignment variable, or the start of the keyword's line."
    )]
    pub enforced_style_align_with: AlignWith,
}

/// `SupportedStylesAlignWith: [keyword, variable, start_of_line]`.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlignWith {
    #[option(value = "keyword")]
    Keyword,
    #[option(value = "variable")]
    Variable,
    #[option(value = "start_of_line")]
    StartOfLine,
}

#[cop(
    name = "Layout/EndAlignment",
    description = "Align ends correctly.",
    default_severity = "warning",
    default_enabled = true,
    options = EndAlignmentOptions
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

    let style = cx
        .options_or_default::<EndAlignmentOptions>()
        .enforced_style_align_with;

    // The range `end` should align with, per the configured style. Its `.start`
    // gives the alignment line/column; its source is the message's `source`.
    let anchor = align_anchor(node, kw, style, cx);

    let (anchor_line, anchor_col) = line_col(anchor.start, cx);
    let (end_line, end_col) = line_col(end_kw.start, cx);

    // `matching_ranges`: aligned when `end` is on the same line as the anchor OR
    // at the same column.
    if anchor_line == end_line || anchor_col == end_col {
        return;
    }

    let source = cx.raw_source(anchor);
    let msg = format!(
        "`end` at {end_line}, {end_col} is not aligned with `{source}` at {anchor_line}, {anchor_col}."
    );
    cx.emit_offense(end_kw, &msg, None);

    // Autocorrect: re-indent the `end` line to the anchor's column. Only when
    // `end` is the first non-whitespace on its line (otherwise rewriting the
    // leading whitespace would corrupt inline code).
    if let Some(line_start) = line_start_if_end_leads(end_kw.start, cx) {
        let indent = " ".repeat(anchor_col);
        cx.emit_edit(
            Range {
                start: line_start,
                end: end_kw.start,
            },
            &indent,
        );
    }
}

/// The alignment anchor range for the configured style. The default `keyword`
/// style anchors on the construct's keyword. `start_of_line` anchors on the
/// keyword line's leading content (`start_line_range`). `variable` anchors on
/// the keyword EXCEPT when the construct is the right-hand side of an
/// assignment / command call on the same line — then it anchors on the range
/// from the assignment's start to the keyword's end (`asgn_variable_align_with`).
fn align_anchor(node: NodeId, kw: Range, style: AlignWith, cx: &Cx<'_>) -> Range {
    match style {
        AlignWith::Keyword => kw,
        AlignWith::StartOfLine => start_line_range(kw.start, cx),
        AlignWith::Variable => match asgn_outer_node(node, cx) {
            // `asgn_variable_align_with`: when there's no line break before the
            // keyword, anchor spans `[outer.begin, keyword.end)`. With a line
            // break (keyword on a later line than the assignment), fall back to
            // the keyword.
            Some(outer) if !line_break_before_keyword(cx.range(outer).start, kw.start, cx) => {
                Range {
                    start: cx.range(outer).start,
                    end: kw.end,
                }
            }
            // No assignment, or a line break before the keyword: `variable`
            // behaves like `keyword` (RuboCop's `check_other_alignment` sets
            // `variable: node.loc.keyword`).
            _ => kw,
        },
    }
}

/// RuboCop's `start_line_range(node)`: the range from the first non-whitespace
/// character of `offset`'s line to the start of the line's trailing whitespace.
/// Used as the `start_of_line` anchor — its column is the line's indentation and
/// its source is the line's trimmed content.
fn start_line_range(offset: u32, cx: &Cx<'_>) -> Range {
    let src = cx.source().as_bytes();
    let off = offset as usize;
    let line_start = src[..off].iter().rposition(|&b| b == b'\n').map_or(0, |p| p + 1);
    let line_end = src[line_start..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(src.len(), |p| line_start + p);
    let first_nonws = line_start
        + src[line_start..line_end]
            .iter()
            .position(|&b| b != b' ' && b != b'\t')
            .unwrap_or(0);
    let last_nonws = line_start
        + src[line_start..line_end]
            .iter()
            .rposition(|&b| b != b' ' && b != b'\t')
            .map_or(0, |p| p + 1);
    Range {
        start: first_nonws as u32,
        end: last_nonws.max(first_nonws) as u32,
    }
}

/// `line_break_before_keyword?(whole_expression, rhs)` — true when the keyword
/// (`rhs.first_line`) is on a later physical line than the assignment start
/// (`whole_expression.line`).
fn line_break_before_keyword(expr_start: u32, kw_start: u32, cx: &Cx<'_>) -> bool {
    line_col(kw_start, cx).0 > line_col(expr_start, cx).0
}

/// The assignment / command-call node the construct is the right-hand side of —
/// RuboCop's `outer_node` for `check_asgn_alignment`. `None` when the construct
/// is not in assignment-RHS position.
///
/// Replicates `CheckAssignment`'s routing without cross-handler `ignore_node`
/// state. From the construct, walk up the **leftmost** position through
/// parenthesized `Begin` / `Or` / `And` wrappers (mirroring RuboCop's
/// `rhs = rhs.child_nodes.first while rhs.type?(:begin, :or, :and)` unwrap), and
/// accept the parent when it is:
///
/// - one of the nine assignment kinds whose value (last child) is the chain, or
/// - a `Send`/`Csend` whose last argument is the chain (setter / command call,
///   RuboCop's `extract_rhs(node) == node.last_argument`).
///
/// Additionally, a `case`/`case_match` in argument position anchors on its
/// parent directly (`on_case`'s `node.argument?` branch).
fn asgn_outer_node(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    // `on_case` / `on_case_match`: `check_asgn_alignment(node.parent, node)` when
    // the case is an argument (the parent is the outer node directly).
    if matches!(*cx.kind(node), NodeKind::Case { .. } | NodeKind::CaseMatch { .. })
        && cx.is_argument(node)
    {
        return cx.parent(node).get();
    }

    // Walk up the leftmost-child chain through paren-`Begin` / `Or` / `And`.
    let mut current = node;
    loop {
        let parent = cx.parent(current).get()?;

        // Assignment whose value (last child) is `current`.
        if cx.is_assignment(parent) {
            return (cx.children(parent).last() == Some(&current)).then_some(parent);
        }

        // Send/Csend whose last argument is `current` (setter or command call).
        if matches!(*cx.kind(parent), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
            return (cx.call_arguments(parent).last() == Some(&current)).then_some(parent);
        }

        // Transparent wrappers: continue up only when `current` is the leftmost
        // child (RuboCop unwraps `child_nodes.first`).
        let leftmost_wrapper = match *cx.kind(parent) {
            NodeKind::Or { .. } | NodeKind::And { .. } => {
                cx.children(parent).first() == Some(&current)
            }
            NodeKind::Begin(_) if crate::cops::util::is_parenthesized(parent, cx) => {
                cx.children(parent).first() == Some(&current)
            }
            _ => false,
        };
        if !leftmost_wrapper {
            return None;
        }
        current = parent;
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
    use super::{AlignWith, EndAlignment as Cop, EndAlignmentOptions};
    use murphy_plugin_api::test_support::{
        run_cop, run_cop_with_edits, run_cop_with_options, run_cop_with_options_and_edits, test,
        CapturedEdit,
    };

    fn variable() -> EndAlignmentOptions {
        EndAlignmentOptions {
            enforced_style_align_with: AlignWith::Variable,
        }
    }

    fn start_of_line() -> EndAlignmentOptions {
        EndAlignmentOptions {
            enforced_style_align_with: AlignWith::StartOfLine,
        }
    }

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

    // ---- EnforcedStyleAlignWith: variable ----

    /// `x = if c ... end` misaligned under `variable`: the `end` should align
    /// with `x` (col 0), and the message names the `x = if` range. Verified
    /// against RuboCop 1.86.2.
    #[test]
    fn variable_flags_assignment_rhs_if() {
        let src = "x = if c\n  foo\n    end\n";
        let run = run_cop_with_options::<Cop>(src, &variable());
        assert_eq!(run.len(), 1, "got {run:?}");
        assert_eq!(
            run[0].message,
            "`end` at 3, 4 is not aligned with `x = if` at 1, 0."
        );
    }

    /// `x = if c ... end` with `end` under `x` (col 0) is accepted under
    /// `variable`.
    #[test]
    fn variable_accepts_end_under_variable() {
        let src = "x = if c\n  foo\nend\n";
        assert!(run_cop_with_options::<Cop>(src, &variable()).is_empty());
    }

    /// `variable` autocorrect re-indents `end` to the variable column.
    #[test]
    fn variable_corrects_to_variable_column() {
        let src = "x = if c\n  foo\n    end\n";
        let run = run_cop_with_options_and_edits::<Cop>(src, &variable());
        assert_eq!(apply(src, &run.edits), "x = if c\n  foo\nend\n");
    }

    /// A bare `if` (no assignment) under `variable` behaves like `keyword`:
    /// aligns with the `if` keyword. Verified against RuboCop 1.86.2.
    #[test]
    fn variable_bare_if_aligns_with_keyword() {
        let src = "if c\n  foo\n  end\n";
        let run = run_cop_with_options::<Cop>(src, &variable());
        assert_eq!(run.len(), 1, "got {run:?}");
        assert_eq!(
            run[0].message,
            "`end` at 3, 2 is not aligned with `if` at 1, 0."
        );
    }

    /// `x ||= while c ... end` (or-asgn) under `variable`: message names
    /// `x ||= while`. Verified against RuboCop 1.86.2.
    #[test]
    fn variable_or_asgn_while() {
        let src = "x ||= while c\n  foo\n    end\n";
        let run = run_cop_with_options::<Cop>(src, &variable());
        assert_eq!(run.len(), 1, "got {run:?}");
        assert_eq!(
            run[0].message,
            "`end` at 3, 4 is not aligned with `x ||= while` at 1, 0."
        );
    }

    /// `x = (if c ... end)` parenthesized RHS under `variable`: message names
    /// `x = (if`. Verified against RuboCop 1.86.2.
    #[test]
    fn variable_parenthesized_rhs() {
        let src = "x = (if c\n  foo\n    end)\n";
        let run = run_cop_with_options::<Cop>(src, &variable());
        assert_eq!(run.len(), 1, "got {run:?}");
        assert_eq!(
            run[0].message,
            "`end` at 3, 4 is not aligned with `x = (if` at 1, 0."
        );
    }

    /// `x = foo || if c ... end` — the conditional is NOT the leftmost RHS leaf,
    /// so RuboCop's `child_nodes.first` unwrap never reaches it: `variable`
    /// falls back to the keyword. Verified against RuboCop 1.86.2 (`if` at col 11).
    #[test]
    fn variable_or_rhs_non_leftmost_falls_back_to_keyword() {
        let src = "x = foo || if c\n  foo\n    end\n";
        let run = run_cop_with_options::<Cop>(src, &variable());
        assert_eq!(run.len(), 1, "got {run:?}");
        assert_eq!(
            run[0].message,
            "`end` at 3, 4 is not aligned with `if` at 1, 11."
        );
    }

    /// `x = if c ... end || bar` — the conditional IS the leftmost RHS leaf, so
    /// `variable` anchors on `x = if`. Verified against RuboCop 1.86.2.
    #[test]
    fn variable_if_leftmost_of_or() {
        let src = "x = if c\n  foo\n    end || bar\n";
        let run = run_cop_with_options::<Cop>(src, &variable());
        assert_eq!(run.len(), 1, "got {run:?}");
        assert_eq!(
            run[0].message,
            "`end` at 3, 4 is not aligned with `x = if` at 1, 0."
        );
    }

    /// `obj.foo = if c ... end` setter send under `variable`: message names
    /// `obj.foo = if`. Verified against RuboCop 1.86.2.
    #[test]
    fn variable_setter_send() {
        let src = "obj.foo = if c\n  1\n    end\n";
        let run = run_cop_with_options::<Cop>(src, &variable());
        assert_eq!(run.len(), 1, "got {run:?}");
        assert_eq!(
            run[0].message,
            "`end` at 3, 4 is not aligned with `obj.foo = if` at 1, 0."
        );
    }

    /// `foo bar, if c ... end` command call (conditional is the last arg) under
    /// `variable`: message names `foo bar, if`. Verified against RuboCop 1.86.2.
    #[test]
    fn variable_command_last_arg() {
        let src = "foo bar, if c\n  1\n    end\n";
        let run = run_cop_with_options::<Cop>(src, &variable());
        assert_eq!(run.len(), 1, "got {run:?}");
        assert_eq!(
            run[0].message,
            "`end` at 3, 4 is not aligned with `foo bar, if` at 1, 0."
        );
    }

    /// `x = case y ... end` case as assignment RHS under `variable`: anchors on
    /// `x = case` (the assignment-RHS branch, not the `is_argument` case branch).
    /// Verified against RuboCop 1.86.2.
    #[test]
    fn variable_case_as_assignment_rhs() {
        let src = "x = case y\nwhen 1\n  a\n    end\n";
        let run = run_cop_with_options::<Cop>(src, &variable());
        assert_eq!(run.len(), 1, "got {run:?}");
        assert_eq!(
            run[0].message,
            "`end` at 4, 4 is not aligned with `x = case` at 1, 0."
        );
    }

    /// `foo(case y when 1 ... end)` case-as-argument under `variable`: anchors on
    /// the parent send (`foo(case`). Verified against RuboCop 1.86.2.
    #[test]
    fn variable_case_as_argument() {
        let src = "foo(case y\nwhen 1\n  a\n    end)\n";
        let run = run_cop_with_options::<Cop>(src, &variable());
        assert_eq!(run.len(), 1, "got {run:?}");
        assert_eq!(
            run[0].message,
            "`end` at 4, 4 is not aligned with `foo(case` at 1, 0."
        );
    }

    /// `x =\n  if c ... end` — line break before the keyword, so `variable`
    /// falls back to the keyword. With `end` aligned under the `if` keyword
    /// (col 2) it is accepted. Verified against RuboCop 1.86.2 (no offense).
    #[test]
    fn variable_line_break_before_keyword_falls_back() {
        let src = "x =\n  if c\n    foo\n  end\n";
        assert!(run_cop_with_options::<Cop>(src, &variable()).is_empty());
    }

    // ---- EnforcedStyleAlignWith: start_of_line ----

    /// `puts(if true ... end)` under `start_of_line`: `end` should align with
    /// the line start (col 0), and the message names the whole first line. The
    /// keyword `if` is at col 5, so this is distinct from the keyword style.
    /// Verified against RuboCop 1.86.2.
    #[test]
    fn start_of_line_flags_indented_keyword() {
        let src = "puts(if true\n     end)\n";
        let run = run_cop_with_options::<Cop>(src, &start_of_line());
        assert_eq!(run.len(), 1, "got {run:?}");
        assert_eq!(
            run[0].message,
            "`end` at 2, 5 is not aligned with `puts(if true` at 1, 0."
        );
    }

    /// `puts(if true ... end)` with `end` at col 0 is accepted under
    /// `start_of_line`.
    #[test]
    fn start_of_line_accepts_end_at_line_start() {
        let src = "puts(if true\nend)\n";
        assert!(run_cop_with_options::<Cop>(src, &start_of_line()).is_empty());
    }

    /// Indented construct under `start_of_line`: `end` aligns with the keyword
    /// line's first non-whitespace column (col 2 here), and the message names the
    /// trimmed line content (`x = if c`). Verified against RuboCop 1.86.2.
    #[test]
    fn start_of_line_uses_line_indentation() {
        let src = "def m\n  x = if c\n    foo\n      end\nend\n";
        let run = run_cop_with_options::<Cop>(src, &start_of_line());
        assert_eq!(run.len(), 1, "got {run:?}");
        assert_eq!(
            run[0].message,
            "`end` at 4, 6 is not aligned with `x = if c` at 2, 2."
        );
    }

    /// `start_of_line` autocorrect re-indents `end` to the line-start column.
    #[test]
    fn start_of_line_corrects_to_line_start() {
        let src = "puts(if true\n     end)\n";
        let run = run_cop_with_options_and_edits::<Cop>(src, &start_of_line());
        assert_eq!(apply(src, &run.edits), "puts(if true\nend)\n");
    }
}
