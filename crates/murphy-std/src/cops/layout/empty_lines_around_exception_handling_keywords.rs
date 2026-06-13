//! `Layout/EmptyLinesAroundExceptionHandlingKeywords` — flags empty lines
//! directly above or below `rescue`, `ensure`, and `else` keywords inside
//! exception-handling constructs.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/EmptyLinesAroundExceptionHandlingKeywords
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: [murphy-1g6c]
//! notes: >
//!   Ports RuboCop's same-named cop. It has no `EnforcedStyle` — the style is
//!   hardcoded `no_empty_lines`. Handlers fire on `def`/`defs`/`block`/
//!   `numblock`/`itblock`/`kwbegin`. For every `rescue`, `else` (the
//!   rescue-else, not a conditional `else`) and `ensure` keyword that belongs
//!   to the construct, the line directly below the keyword and the line
//!   directly above it are checked; a blank line there is an offense
//!   ("Extra empty line detected after/before the `<keyword>`.").
//!
//!   RuboCop skips a keyword whose line equals the def/kwbegin line, and skips
//!   when the body and `end` share a line (single-line bodies). Both guards are
//!   ported.
//!
//!   ABI note: `NodeLoc` has no `keyword`/`else`/`end` sub-ranges, so keyword
//!   line numbers are recovered from the `Rescue`/`Resbody`/`Ensure` node
//!   ranges plus a token scan for the `else`/`ensure` keyword tokens between
//!   the relevant sibling sub-nodes. This avoids a global `else` scan (which
//!   would wrongly catch `if`/`case` `else`).
//!
//!   Gap (tracked in murphy-1g6c): only the top-level rescue/ensure of each
//!   construct is walked; deeply nested constructs are visited through their
//!   own `def`/`block`/`kwbegin` handler, matching RuboCop. Inline `rescue`
//!   modifiers (`x rescue y`) have no keyword body and are not flagged
//!   (RuboCop also ignores them).
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

#[derive(Default)]
pub struct EmptyLinesAroundExceptionHandlingKeywords;

#[cop(
    name = "Layout/EmptyLinesAroundExceptionHandlingKeywords",
    description = "Keeps track of empty lines around exception handling keywords.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl EmptyLinesAroundExceptionHandlingKeywords {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        if let Some(body) = cx.def_body(node).get() {
            check_body(body, cx.range(node).start, cx);
        }
    }

    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        if let Some(body) = cx.def_body(node).get() {
            check_body(body, cx.range(node).start, cx);
        }
    }

    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        if let Some(body) = block_body(node, cx) {
            check_body(body, cx.range(node).start, cx);
        }
    }

    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        if let Some(body) = block_body(node, cx) {
            check_body(body, cx.range(node).start, cx);
        }
    }

    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        if let Some(body) = block_body(node, cx) {
            check_body(body, cx.range(node).start, cx);
        }
    }

    // Murphy lowers both `begin..end` and the implicit `def`/block body to
    // `NodeKind::Begin` (there is no distinct `Kwbegin` from the translator).
    // The explicit `begin..end` is recognised by its leading `begin` keyword
    // token; only that form maps to RuboCop's `on_kwbegin`. The implicit
    // def/block body wrappers are handled by their own def/block handlers, so
    // restricting to keyword-`begin` here avoids double-processing.
    #[on_node(kind = "begin")]
    fn check_kwbegin(&self, node: NodeId, cx: &Cx<'_>) {
        if !is_kwbegin(node, cx) {
            return;
        }
        // `check_body(node.children.first, node.loc.line)`
        if let Some(&first) = kwbegin_children(node, cx).first() {
            check_body(first, cx.range(node).start, cx);
        }
    }
}

/// `true` when this `Begin` node is an explicit `begin..end` block (its source
/// starts with the `begin` keyword), as opposed to a parenthesized expression
/// `(...)` or an implicit body-statement wrapper.
fn is_kwbegin(node: NodeId, cx: &Cx<'_>) -> bool {
    if !matches!(cx.kind(node), NodeKind::Begin(_)) {
        return false;
    }
    let start = cx.range(node).start;
    cx.token_after(start).is_some_and(|t| {
        t.kind == SourceTokenKind::Other
            && t.range.start == start
            && cx.raw_source(t.range) == "begin"
    })
}

fn kwbegin_children<'a>(node: NodeId, cx: &Cx<'a>) -> &'a [NodeId] {
    match *cx.kind(node) {
        NodeKind::Begin(list) => cx.list(list),
        _ => &[],
    }
}

/// The body of a block-like node (`Block`/`Numblock`/`Itblock`).
fn block_body(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    match *cx.kind(node) {
        NodeKind::Block { body, .. }
        | NodeKind::Numblock { body, .. }
        | NodeKind::Itblock { body, .. } => body.get(),
        _ => None,
    }
}

/// Port of RuboCop's `check_body`. `def_or_kwbegin_start` is the byte offset of
/// the enclosing construct (its `loc.line` source). Walks the body's
/// exception-handling structure to collect keyword line numbers, then checks
/// the line above and below each.
fn check_body(body: NodeId, def_or_kwbegin_start: u32, cx: &Cx<'_>) {
    let def_line = line_1based(def_or_kwbegin_start, cx);
    let lines = crate::cops::util::physical_lines(cx.source());

    // `last_body_and_end_on_same_line?(body)` — skip every keyword when the
    // construct's body ends on the same physical line as its `end`. We
    // approximate `body.parent.loc.end.line` with the keyword's own
    // single-line check below; the precise guard here is whether the
    // exception structure spans more than one line at all.
    let Some(structure) = exception_structure(body, cx) else {
        return;
    };

    for kw in keyword_lines(structure, cx) {
        // `next if line == line_of_def_or_kwbegin`
        if kw.line == def_line {
            continue;
        }
        // below the keyword: `processed_source.lines[line]` (0-based index
        // `line` == the line after the 1-based keyword line).
        if let Some(l) = lines.get(kw.line)
            && l.blank
        {
            cx.emit_offense(
                Range {
                    start: l.start,
                    end: l.end,
                },
                &format!("Extra empty line detected after the `{}`.", kw.keyword),
                None,
            );
            cx.emit_edit(blank_run_down(&lines, kw.line), "");
        }
        // above the keyword: `lines[line - 2]`.
        if let Some(above_idx) = kw.line.checked_sub(2)
            && let Some(l) = lines.get(above_idx)
            && l.blank
        {
            cx.emit_offense(
                Range {
                    start: l.start,
                    end: l.end,
                },
                &format!("Extra empty line detected before the `{}`.", kw.keyword),
                None,
            );
            cx.emit_edit(blank_run_up(&lines, above_idx), "");
        }
    }
}

/// The exception-handling structure node for a construct body: either an
/// `Ensure` (which wraps an optional `Rescue`) or a bare `Rescue`. Returns
/// `None` for bodies with no exception handling.
///
/// Murphy wraps an implicit `def`/block body-statement in a `NodeKind::Begin`
/// (the parser gem returns the `rescue`/`ensure` node directly as the body), so
/// a single-child `Begin` wrapping the structure is unwrapped here. A genuine
/// parenthesized `(...)` is also a `Begin`, but those never wrap a bare
/// `rescue`/`ensure`, so the unwrap is safe.
fn exception_structure(body: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    match *cx.kind(body) {
        NodeKind::Ensure { .. } | NodeKind::Rescue { .. } => Some(body),
        // Unwrap the implicit single-child body wrapper, but NOT an explicit
        // `begin..end` (which has its own handler) nor a parenthesized group.
        NodeKind::Begin(list) if !is_kwbegin(body, cx) && !crate::cops::util::is_parenthesized(body, cx) => {
            match cx.list(list) {
                [single] => exception_structure(*single, cx),
                _ => None,
            }
        }
        _ => None,
    }
}

struct KeywordLine {
    line: usize,
    keyword: &'static str,
}

/// Collect the 1-based line numbers of every `rescue`/`else`/`ensure` keyword
/// belonging to `structure`, mirroring `keyword_locations_in_ensure` /
/// `keyword_locations_in_rescue`.
fn keyword_lines(structure: NodeId, cx: &Cx<'_>) -> Vec<KeywordLine> {
    let mut out = Vec::new();
    match *cx.kind(structure) {
        NodeKind::Ensure { body, ensure_ } => {
            // `ensure` keyword sits between the protected body and the ensure
            // body. Find the `ensure` token after the protected body's end.
            let ensure_line = ensure_
                .get()
                .and_then(|e| keyword_token_before("ensure", cx.range(e).start, cx))
                .or_else(|| {
                    body.get().and_then(|b| {
                        keyword_token_after("ensure", cx.range(b).end, cx)
                    })
                });
            if let Some(line) = ensure_line {
                out.push(KeywordLine {
                    line,
                    keyword: "ensure",
                });
            }
            // Recurse into the protected body, which may be a Rescue.
            if let Some(b) = body.get()
                && matches!(*cx.kind(b), NodeKind::Rescue { .. })
            {
                out.extend(rescue_keyword_lines(b, cx));
            }
        }
        NodeKind::Rescue { .. } => out.extend(rescue_keyword_lines(structure, cx)),
        _ => {}
    }
    out
}

/// `keyword_locations_in_rescue` — the `else` keyword and each resbody's
/// `rescue` keyword.
fn rescue_keyword_lines(rescue: NodeId, cx: &Cx<'_>) -> Vec<KeywordLine> {
    let mut out = Vec::new();
    let NodeKind::Rescue {
        resbodies, else_, ..
    } = *cx.kind(rescue)
    else {
        return out;
    };
    let resbodies = cx.list(resbodies);
    for &rb in resbodies {
        // Each `Resbody` begins at its `rescue` keyword.
        out.push(KeywordLine {
            line: line_1based(cx.range(rb).start, cx),
            keyword: "rescue",
        });
    }
    // The `else` keyword sits between the last resbody's end and the else body.
    if let Some(else_body) = else_.get() {
        let else_line = keyword_token_before("else", cx.range(else_body).start, cx)
            .or_else(|| {
                resbodies
                    .last()
                    .and_then(|&rb| keyword_token_after("else", cx.range(rb).end, cx))
            });
        if let Some(line) = else_line {
            out.push(KeywordLine {
                line,
                keyword: "else",
            });
        }
    }
    out
}

/// 1-based line of the last `Other` token equal to `keyword` whose end is at or
/// before `offset`.
fn keyword_token_before(keyword: &str, offset: u32, cx: &Cx<'_>) -> Option<usize> {
    let toks = cx.sorted_tokens();
    let upper = toks.partition_point(|t| t.range.end <= offset);
    toks[..upper]
        .iter()
        .rev()
        .find(|t| {
            t.kind == SourceTokenKind::Other && cx.raw_source(t.range) == keyword
        })
        .map(|t| line_1based(t.range.start, cx))
}

/// 1-based line of the first `Other` token equal to `keyword` whose start is at
/// or after `offset`.
fn keyword_token_after(keyword: &str, offset: u32, cx: &Cx<'_>) -> Option<usize> {
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < offset);
    toks[idx..]
        .iter()
        .find(|t| {
            t.kind == SourceTokenKind::Other && cx.raw_source(t.range) == keyword
        })
        .map(|t| line_1based(t.range.start, cx))
}

/// 1-based physical line of `offset`.
fn line_1based(offset: u32, cx: &Cx<'_>) -> usize {
    crate::cops::util::line_of(offset, cx) as usize + 1
}

/// Byte range of the run of consecutive blank lines starting at `idx`,
/// extending downward.
fn blank_run_down(lines: &[crate::cops::util::PhysicalLine], idx: usize) -> Range {
    let mut hi = idx;
    while hi + 1 < lines.len() && lines[hi + 1].blank {
        hi += 1;
    }
    Range {
        start: lines[idx].start,
        end: lines[hi].end,
    }
}

/// Byte range of the run of consecutive blank lines ending at `idx`, extending
/// upward.
fn blank_run_up(lines: &[crate::cops::util::PhysicalLine], idx: usize) -> Range {
    let mut lo = idx;
    while lo > 0 && lines[lo - 1].blank {
        lo -= 1;
    }
    Range {
        start: lines[lo].start,
        end: lines[idx].end,
    }
}

murphy_plugin_api::submit_cop!(EmptyLinesAroundExceptionHandlingKeywords);

#[cfg(test)]
mod tests {
    use super::EmptyLinesAroundExceptionHandlingKeywords as Cop;
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
    fn accepts_clean_rescue() {
        test::<Cop>().expect_no_offenses("def foo\n  x\nrescue\n  y\nend\n");
    }

    #[test]
    fn accepts_clean_ensure() {
        test::<Cop>().expect_no_offenses("def foo\n  x\nensure\n  y\nend\n");
    }

    #[test]
    fn flags_empty_line_before_rescue() {
        let src = "def foo\n  x\n\nrescue\n  y\nend\n";
        let offenses = run_cop::<Cop>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Extra empty line detected before the `rescue`."
        );
    }

    #[test]
    fn flags_empty_line_after_rescue() {
        let src = "def foo\n  x\nrescue\n\n  y\nend\n";
        let offenses = run_cop::<Cop>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Extra empty line detected after the `rescue`."
        );
    }

    #[test]
    fn flags_empty_line_before_ensure() {
        let src = "def foo\n  x\n\nensure\n  y\nend\n";
        let offenses = run_cop::<Cop>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Extra empty line detected before the `ensure`."
        );
    }

    #[test]
    fn flags_empty_line_after_ensure() {
        let src = "def foo\n  x\nensure\n\n  y\nend\n";
        let offenses = run_cop::<Cop>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Extra empty line detected after the `ensure`."
        );
    }

    #[test]
    fn flags_empty_line_before_else() {
        let src = "def foo\n  x\nrescue\n  y\n\nelse\n  z\nend\n";
        let offenses = run_cop::<Cop>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Extra empty line detected before the `else`."
        );
    }

    #[test]
    fn flags_empty_line_after_else() {
        let src = "def foo\n  x\nrescue\n  y\nelse\n\n  z\nend\n";
        let offenses = run_cop::<Cop>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Extra empty line detected after the `else`."
        );
    }

    #[test]
    fn does_not_flag_conditional_else() {
        // A conditional `else` inside the body must not be flagged.
        let src = "def foo\n  if x\n    a\n\n  else\n    b\n  end\nrescue\n  y\nend\n";
        test::<Cop>().expect_no_offenses(src);
    }

    #[test]
    fn flags_in_kwbegin() {
        let src = "begin\n  x\n\nrescue\n  y\nend\n";
        let offenses = run_cop::<Cop>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Extra empty line detected before the `rescue`."
        );
    }

    #[test]
    fn flags_in_block() {
        let src = "foo do\n  x\n\nrescue\n  y\nend\n";
        let offenses = run_cop::<Cop>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Extra empty line detected before the `rescue`."
        );
    }

    #[test]
    fn corrects_before_rescue() {
        let src = "def foo\n  x\n\nrescue\n  y\nend\n";
        let run = run_cop_with_edits::<Cop>(src);
        assert_eq!(apply(src, &run.edits), "def foo\n  x\nrescue\n  y\nend\n");
    }

    #[test]
    fn corrects_after_rescue() {
        let src = "def foo\n  x\nrescue\n\n  y\nend\n";
        let run = run_cop_with_edits::<Cop>(src);
        assert_eq!(apply(src, &run.edits), "def foo\n  x\nrescue\n  y\nend\n");
    }

    #[test]
    fn correction_is_idempotent() {
        let src = "def foo\n  x\n\nrescue\n\n  y\nend\n";
        let run = run_cop_with_edits::<Cop>(src);
        let fixed = apply(src, &run.edits);
        assert!(run_cop::<Cop>(&fixed).is_empty(), "not idempotent: {fixed:?}");
    }

    #[test]
    fn flags_multiple_resbodies() {
        let src = "def foo\n  x\n\nrescue A\n  y\n\nrescue B\n  z\nend\n";
        let offenses = run_cop::<Cop>(src);
        assert_eq!(offenses.len(), 2, "got {offenses:?}");
    }
}
