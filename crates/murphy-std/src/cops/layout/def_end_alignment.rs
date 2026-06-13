//! `Layout/DefEndAlignment` — flags `end` keywords of method definitions that
//! are not aligned with the configured anchor.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/DefEndAlignment
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Port of RuboCop's `DefEndAlignment` (which mixes in `EndKeywordAlignment`).
//!   Murphy has no cross-node visitor state, so the check is driven entirely
//!   from the `def`/`defs` node rather than RuboCop's split `on_def` / `on_send`
//!   handlers. For a bare `def`, the anchor is the `def` keyword. For a
//!   def-modifier chain (`private def foo` / `foo bar def baz`), the anchor
//!   depends on `EnforcedStyleAlignWith`: `start_of_line` (default) anchors on
//!   the outermost modifier send's start column with source text
//!   `<modifiers> def` (RuboCop's `range_between(send.begin, def_keyword.end)`),
//!   while `def` anchors on the `def` keyword. An offense is registered iff the
//!   `end` keyword is on a different line than the anchor AND their columns
//!   differ — RuboCop's `same_line? || column_offset.zero?` acceptance.
//!   Endless / single-line defs (no `end` keyword) are skipped. Autocorrect
//!   rewrites the `end`'s leading indentation to the anchor column, which is
//!   idempotent.
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, OptNodeId, Range, cop};

/// Stateless unit struct (ADR 0035 const-metadata cop pattern).
#[derive(Default)]
pub struct DefEndAlignment;

#[derive(CopOptions)]
pub struct DefEndAlignmentOptions {
    #[option(
        name = "EnforcedStyleAlignWith",
        default = "start_of_line",
        description = "Whether `end` aligns with the line start (incl. `private`/`public` prefixes) or the `def` keyword."
    )]
    pub enforced_style_align_with: AlignWith,
}

/// `SupportedStylesAlignWith: [start_of_line, def]`.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlignWith {
    #[option(value = "start_of_line")]
    StartOfLine,
    #[option(value = "def")]
    Def,
}

#[cop(
    name = "Layout/DefEndAlignment",
    description = "Align ends corresponding to defs correctly.",
    default_severity = "warning",
    default_enabled = true,
    options = DefEndAlignmentOptions
)]
impl DefEndAlignment {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(def: NodeId, cx: &Cx<'_>) {
    let end_kw = cx.loc(def).end_keyword();
    // Endless / single-line def (`def foo = 1`) — no `end` to align.
    if end_kw == Range::ZERO {
        return;
    }

    let opts = cx.options_or_default::<DefEndAlignmentOptions>();
    let def_keyword = cx.loc(def).keyword();
    if def_keyword == Range::ZERO {
        return;
    }

    // Resolve the anchor (the range `end` should line up with) and the
    // message's `source` text.
    let outermost_modifier = outermost_def_modifier(def, cx);
    let anchor = match (opts.enforced_style_align_with, outermost_modifier.get()) {
        // `start_of_line` with a modifier chain: anchor on the line start —
        // RuboCop's `range_between(send.begin_pos, def.keyword.end_pos)`.
        (AlignWith::StartOfLine, Some(send)) => Range {
            start: cx.range(send).start,
            end: def_keyword.end,
        },
        // No modifier, or `def` style: anchor on the `def` keyword.
        _ => def_keyword,
    };

    let (anchor_line, anchor_col) = line_and_column(cx, anchor.start);
    let (end_line, end_col) = line_and_column(cx, end_kw.start);

    // RuboCop's acceptance: aligned if on the same line OR same column.
    if anchor_line == end_line || anchor_col == end_col {
        return;
    }

    let source = cx.raw_source(anchor);
    let message = format!(
        "`end` at {end_line}, {end_col} is not aligned with `{source}` at {anchor_line}, {anchor_col}."
    );
    cx.emit_offense(end_kw, &message, None);

    // Autocorrect: rewrite the `end` line's leading whitespace so the `end`
    // keyword starts at `anchor_col`. Idempotent.
    let end_line_start = end_kw.start
        - cx.source().as_bytes()[..end_kw.start as usize]
            .iter()
            .rev()
            .take_while(|&&b| b != b'\n')
            .count() as u32;
    // Only rewrite when nothing but whitespace precedes `end` on its line.
    // For a non-own-line `end` (e.g. `def foo\n  work; end`) replacing
    // `[line_start, end)` would delete the leading code, so emit the offense
    // without an autocorrect in that case.
    let prefix = &cx.source()[end_line_start as usize..end_kw.start as usize];
    if !prefix.bytes().all(|b| b == b' ' || b == b'\t') {
        return;
    }
    cx.emit_edit(
        Range {
            start: end_line_start,
            end: end_kw.start,
        },
        &" ".repeat(anchor_col),
    );
}

/// Walk up the parent chain from `def`, following def-modifier sends
/// (`private def …`, `foo bar def …`), and return the outermost such send.
/// `OptNodeId::NONE` when `def` is not part of a modifier chain.
fn outermost_def_modifier(def: NodeId, cx: &Cx<'_>) -> OptNodeId {
    let mut result = OptNodeId::NONE;
    let mut current = def;
    while let Some(parent) = cx.parent(current).get() {
        // The parent must be a def-modifier send whose modifier target is
        // ultimately this def (it is, since we walked up from `def`).
        if cx.is_def_modifier(parent) {
            result = OptNodeId::some(parent);
            current = parent;
        } else {
            break;
        }
    }
    result
}

/// 1-based line and 0-based column (byte offset from line start) of `offset`.
fn line_and_column(cx: &Cx<'_>, offset: u32) -> (usize, usize) {
    let src = cx.source().as_bytes();
    let off = offset as usize;
    let line_start = src[..off]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |pos| pos + 1);
    let line = src[..off].iter().filter(|&&b| b == b'\n').count() + 1;
    let column = off - line_start;
    (line, column)
}

murphy_plugin_api::submit_cop!(DefEndAlignment);

#[cfg(test)]
mod tests {
    use super::{AlignWith, DefEndAlignment, DefEndAlignmentOptions};
    use murphy_plugin_api::test_support::{
        run_cop, run_cop_with_edits, run_cop_with_options, run_cop_with_options_and_edits,
    };

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

    fn def_style() -> DefEndAlignmentOptions {
        DefEndAlignmentOptions {
            enforced_style_align_with: AlignWith::Def,
        }
    }

    // ---- EnforcedStyleAlignWith: start_of_line (default) ----

    #[test]
    fn flags_misaligned_end_default() {
        let src = "def test\n  end\n";
        let run = run_cop_with_edits::<DefEndAlignment>(src);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(
            run.offenses[0].message,
            "`end` at 2, 2 is not aligned with `def` at 1, 0."
        );
        assert_eq!(apply(src, &run.edits), "def test\nend\n");
    }

    #[test]
    fn flags_misaligned_defs_default() {
        let src = "def Test.test\n  end\n";
        let run = run_cop::<DefEndAlignment>(src);
        assert_eq!(run.len(), 1);
        assert_eq!(
            run[0].message,
            "`end` at 2, 2 is not aligned with `def` at 1, 0."
        );
    }

    #[test]
    fn accepts_aligned_end_default() {
        assert!(run_cop::<DefEndAlignment>("def test\nend\n").is_empty());
    }

    #[test]
    fn misaligned_end_sharing_its_line_is_not_autocorrected() {
        // `end` is not the first token on its line; rewriting `[line_start,
        // end)` would delete `work; `. The offense must still fire, but with
        // no autocorrect edit (regression: previously corrupted the body).
        let src = "def foo\n  work; end\n";
        let run = run_cop_with_edits::<DefEndAlignment>(src);
        assert_eq!(run.offenses.len(), 1);
        assert!(
            run.edits.is_empty(),
            "must not autocorrect an `end` that shares its line with code"
        );
    }

    #[test]
    fn accepts_aligned_defs_default() {
        assert!(run_cop::<DefEndAlignment>("def Test.test\nend\n").is_empty());
    }

    #[test]
    fn accepts_prefix_aligned_at_line_start_default() {
        // `foo def test` / `end` aligned at column 0 — no offense.
        assert!(run_cop::<DefEndAlignment>("foo def test\nend\n").is_empty());
    }

    #[test]
    fn accepts_longer_prefix_aligned_default() {
        assert!(run_cop::<DefEndAlignment>("foo bar def test\nend\n").is_empty());
    }

    #[test]
    fn flags_prefix_misaligned_default() {
        let src = "foo def test\n    end\n";
        let run = run_cop_with_edits::<DefEndAlignment>(src);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(
            run.offenses[0].message,
            "`end` at 2, 4 is not aligned with `foo def` at 1, 0."
        );
        assert_eq!(apply(src, &run.edits), "foo def test\nend\n");
    }

    #[test]
    fn accepts_private_def_aligned_default() {
        // `private def foo` with `end` at column 0 — aligned, no offense.
        assert!(run_cop::<DefEndAlignment>("private def foo\nend\n").is_empty());
    }

    // ---- EnforcedStyleAlignWith: def ----

    #[test]
    fn flags_misaligned_end_def_style() {
        let src = "def test\n  end\n";
        let run = run_cop_with_options::<DefEndAlignment>(src, &def_style());
        assert_eq!(run.len(), 1);
        assert_eq!(
            run[0].message,
            "`end` at 2, 2 is not aligned with `def` at 1, 0."
        );
    }

    #[test]
    fn accepts_prefix_aligned_with_def_keyword_def_style() {
        // `def` style: `end` aligns under the `def` keyword (column 4).
        assert!(
            run_cop_with_options::<DefEndAlignment>("foo def test\n    end\n", &def_style())
                .is_empty()
        );
    }

    #[test]
    fn flags_prefix_at_line_start_def_style() {
        let src = "foo def test\nend\n";
        let run = run_cop_with_options_and_edits::<DefEndAlignment>(src, &def_style());
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(
            run.offenses[0].message,
            "`end` at 2, 0 is not aligned with `def` at 1, 4."
        );
        assert_eq!(apply(src, &run.edits), "foo def test\n    end\n");
    }

    // ---- skip cases ----

    #[test]
    fn skips_endless_def() {
        assert!(run_cop::<DefEndAlignment>("def foo = 1\n").is_empty());
    }

    #[test]
    fn autocorrect_is_idempotent() {
        let src = "def test\n  end\n";
        let run = run_cop_with_edits::<DefEndAlignment>(src);
        let fixed = apply(src, &run.edits);
        let run2 = run_cop_with_edits::<DefEndAlignment>(&fixed);
        assert!(run2.offenses.is_empty(), "second pass must be clean");
    }
}
