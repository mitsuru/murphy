//! `Layout/CaseIndentation` — checks how `when`/`in` clauses of a `case`
//! expression are indented relative to the `case` or `end` keyword.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/CaseIndentation
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: [murphy-vagp]
//! notes: >
//!   Direct port of RuboCop's `on_case` / `on_case_match`. A separate offense
//!   is registered for each misaligned `when`/`in` keyword. The check is:
//!     `when_column == base_column(case_node, style) + indentation_width`
//!   where `base_column` is the `case` keyword column (`EnforcedStyle: case`,
//!   default) or the `end` keyword column (`EnforcedStyle: end`), and
//!   `indentation_width` is `IndentationWidth` when `IndentOneStep: true` else
//!   `0`. Single-line `case`/`case_match` nodes are skipped, matching
//!   `node.single_line?`.
//!
//!   Message is RuboCop's verbatim `MSG`:
//!   `Indent `<branch>` <depth> `<base>`.` with `depth` = `as deep as`
//!   (`IndentOneStep: false`) or `one step more than` (`true`), and `base` =
//!   `case` / `end`.
//!
//!   Autocorrect mirrors RuboCop's `incorrect_style`: the leading whitespace
//!   before the `when`/`in` keyword is replaced with the correct indentation,
//!   but only when that leading run is whitespace-only (RuboCop's
//!   `whitespace.source.strip.empty?` guard — never rewrites when something
//!   else shares the line before the keyword).
//!
//!   ABI note: `NodeLoc` exposes no `keyword`/`end` sub-range, so keyword
//!   columns are recovered via the token API (`cx.loc(node).keyword()` /
//!   `cx.loc(node).end_keyword()`), which is RuboCop-faithful for these nodes.
//!
//!   `IndentationWidth` is modelled as `Option<i64>`: the bundled default
//!   `IndentationWidth: ~` merges to JSON `null`, which a plain `i64` field
//!   would reject — erroring the whole option struct and silently discarding
//!   the user's `EnforcedStyle`/`IndentOneStep`. Under `IndentOneStep: true`
//!   the width matches RuboCop's resolution: this cop's own `IndentationWidth`
//!   override is honoured, and when unset the width falls back to the run-wide
//!   resolved `Layout/IndentationWidth: Width` via `cx.indentation_width()`
//!   (default 2) — murphy-kke2. (When `IndentOneStep` is false the width is
//!   inert.)
//!
//!   Gaps (documented, not bypassed):
//!     * `correct_style_detected` / `opposite_style_detected` style-tracking
//!       (RuboCop's `ConfigurableEnforcedStyle` learning) is not modelled;
//!       Murphy is stateless per-file and only reports offenses.
//!     * (murphy-vagp) `end_and_last_conditional_same_line?` — consulted only
//!       under `EnforcedStyle: end` — approximates the same-line skip using
//!       node ranges (the else/last-conditional node start lines) rather than
//!       the `else`/`then` keyword lines RuboCop reads (`NodeLoc` exposes
//!       neither). Diverges only when `end` shares a line with an else or last
//!       conditional; the common `end`-on-its-own-line case is correct.
//! ```

use crate::cops::util::nth_line_start;
use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, Range, cop};

/// Stateless unit struct (ADR 0035 const-metadata cop pattern).
#[derive(Default)]
pub struct CaseIndentation;

#[derive(CopOptions)]
pub struct CaseIndentationOptions {
    #[option(
        name = "EnforcedStyle",
        default = "case",
        description = "Whether to indent `when`/`in` relative to `case` or align with `end`."
    )]
    pub enforced_style: CaseIndentationStyle,

    #[option(
        name = "IndentOneStep",
        default = false,
        description = "Whether `when`/`in` should be indented one step deeper than the base keyword."
    )]
    pub indent_one_step: bool,

    // `Option<i64>` (not `i64`) so the bundled default `IndentationWidth: ~`
    // (which merges to JSON `null`) decodes to `None` instead of erroring the
    // whole option struct and silently discarding the user's other keys. `None`
    // resolves to the fallback width 2 in `indentation_width`.
    #[option(
        name = "IndentationWidth",
        description = "Number of spaces for one indentation level (null/unset falls back to RuboCop's default of 2)."
    )]
    pub indentation_width: Option<i64>,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum CaseIndentationStyle {
    /// `when`/`in` are indented relative to the `case` keyword.
    #[option(value = "case")]
    Case,
    /// `when`/`in` are aligned with the `end` keyword.
    #[option(value = "end")]
    End,
}

#[cop(
    name = "Layout/CaseIndentation",
    description = "Checks how the `when` and `in` clauses of a `case` expression are indented.",
    default_severity = "warning",
    default_enabled = true,
    options = CaseIndentationOptions,
)]
impl CaseIndentation {
    #[on_node(kind = "case")]
    fn check_case(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<CaseIndentationOptions>();
        // `return if case_node.single_line?`
        if cx.is_single_line(node) {
            return;
        }
        // `return if enforced_style_end? && end_and_last_conditional_same_line?`
        if opts.enforced_style == CaseIndentationStyle::End
            && end_and_last_conditional_same_line(node, cx)
        {
            return;
        }
        for &when_node in cx.case_when_branches(node) {
            check_when(when_node, node, "when", &opts, cx);
        }
    }

    #[on_node(kind = "case_match")]
    fn check_case_match(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<CaseIndentationOptions>();
        if cx.is_single_line(node) {
            return;
        }
        if opts.enforced_style == CaseIndentationStyle::End
            && end_and_last_conditional_same_line(node, cx)
        {
            return;
        }
        for &in_node in cx.in_pattern_branches(node) {
            check_when(in_node, node, "in", &opts, cx);
        }
    }
}

/// `check_when(when_node, branch_type)` — compares the `when`/`in` keyword
/// column to the base column plus the configured `indentation_width`.
fn check_when(
    when_node: NodeId,
    case_node: NodeId,
    branch_type: &str,
    opts: &CaseIndentationOptions,
    cx: &Cx<'_>,
) {
    // The `when`/`in` keyword begins exactly at the branch node's
    // `expression.start` (true for both `When` and `InPattern`). `cx.loc().keyword()`
    // is unavailable for `InPattern` (not keyword-bearing in the ABI), so use
    // the node range start directly — it is the keyword position.
    let keyword_start = cx.range(when_node).start;
    let when_column = column_of(keyword_start, cx);
    let Some(base) = base_column(case_node, opts.enforced_style, cx) else {
        return;
    };
    let width = indentation_width(opts, cx.indentation_width());

    if when_column == base + width {
        return;
    }
    incorrect_style(when_node, case_node, keyword_start, branch_type, opts, cx);
}

/// `incorrect_style` — registers the offense and (when the leading run is
/// whitespace-only) rewrites the indentation.
fn incorrect_style(
    when_node: NodeId,
    case_node: NodeId,
    keyword_start: u32,
    branch_type: &str,
    opts: &CaseIndentationOptions,
    cx: &Cx<'_>,
) {
    let _ = when_node;
    let depth = if opts.indent_one_step {
        "one step more than"
    } else {
        "as deep as"
    };
    let base = style_str(opts.enforced_style);
    let message = format!("Indent `{branch_type}` {depth} `{base}`.");
    // Offense range covers the keyword token (RuboCop's `when_node.loc.keyword`).
    let keyword = Range {
        start: keyword_start,
        end: keyword_start + branch_type.len() as u32,
    };
    cx.emit_offense(keyword, &message, None);

    // `whitespace_range(node)` = `[keyword_begin - column, keyword_begin)`.
    let line_start = match nth_line_start(cx, line_of(keyword_start, cx)) {
        Some(s) => s,
        None => return,
    };
    let whitespace = Range {
        start: line_start,
        end: keyword_start,
    };
    // `corrector.replace(whitespace, replacement) if whitespace.source.strip.empty?`
    if !cx.raw_source(whitespace).trim().is_empty() {
        return;
    }
    // `replacement` = base column (always derived from the *configured* style)
    // plus `indentation_width`, as spaces.
    let Some(base_col) = base_column(case_node, opts.enforced_style, cx) else {
        return;
    };
    let target = base_col + indentation_width(opts, cx.indentation_width());
    cx.emit_edit(whitespace, &" ".repeat(target));
}

/// `base_column(case_node, base)` — `case` keyword column for `:case`, `end`
/// keyword column for `:end`. Returns `None` when the keyword is unrecoverable.
fn base_column(case_node: NodeId, style: CaseIndentationStyle, cx: &Cx<'_>) -> Option<usize> {
    let start = match style {
        // The `case` keyword begins at the node's `expression.start` (true for
        // both `Case` and `CaseMatch`, the latter not being keyword-bearing in
        // the ABI).
        CaseIndentationStyle::Case => cx.range(case_node).start,
        CaseIndentationStyle::End => {
            let end = cx.loc(case_node).end_keyword();
            if end == Range::ZERO {
                return None;
            }
            end.start
        }
    };
    Some(column_of(start, cx))
}

/// `indentation_width` — `IndentationWidth` when `IndentOneStep`, else `0`.
/// An unset (`null` / `~`) `IndentationWidth` falls back to `fallback_width`,
/// the run-wide resolved `Layout/IndentationWidth.Width` (`cx.indentation_width()`,
/// default 2) — murphy-kke2. Taken as a parameter (not `cx`) so the helper stays
/// pure and unit-testable.
fn indentation_width(opts: &CaseIndentationOptions, fallback_width: i64) -> usize {
    if opts.indent_one_step {
        opts.indentation_width.unwrap_or(fallback_width).max(0) as usize
    } else {
        0
    }
}

/// `end_and_last_conditional_same_line?(node)` — the `end` keyword line equals
/// the `else` line (when present) or the last branch's body `begin` line. Only
/// consulted under `EnforcedStyle: end`.
fn end_and_last_conditional_same_line(node: NodeId, cx: &Cx<'_>) -> bool {
    let end_kw = cx.loc(node).end_keyword();
    if end_kw == Range::ZERO {
        return false;
    }
    let end_line = line_of(end_kw.start, cx);

    let else_branch = cx
        .case_else_branch(node)
        .get()
        .or_else(|| cx.case_match_else_branch(node).get());
    let last_line = if let Some(else_node) = else_branch {
        line_of(cx.range(else_node).start, cx)
    } else {
        // `node.child_nodes.last.loc.begin&.line` — the last branch node.
        let last_branch = cx
            .case_when_branches(node)
            .last()
            .or_else(|| cx.in_pattern_branches(node).last())
            .copied();
        match last_branch {
            Some(b) => line_of(cx.range(b).start, cx),
            None => return false,
        }
    };
    end_line == last_line
}

/// 0-based byte column of `offset` (distance from its line start). Indentation
/// is ASCII whitespace, so byte and character columns coincide here.
fn column_of(offset: u32, cx: &Cx<'_>) -> usize {
    let line_start = nth_line_start(cx, line_of(offset, cx)).unwrap_or(0);
    (offset - line_start) as usize
}

fn line_of(offset: u32, cx: &Cx<'_>) -> u32 {
    crate::cops::util::line_of(offset, cx)
}

fn style_str(style: CaseIndentationStyle) -> &'static str {
    match style {
        CaseIndentationStyle::Case => "case",
        CaseIndentationStyle::End => "end",
    }
}

murphy_plugin_api::submit_cop!(CaseIndentation);

#[cfg(test)]
mod tests {
    use super::{CaseIndentation, CaseIndentationOptions, CaseIndentationStyle};
    use murphy_plugin_api::test_support::{
        run_cop, run_cop_with_edits, run_cop_with_options, run_cop_with_options_and_edits, test,
        CapturedEdit,
    };
    use murphy_plugin_api::CopOptions;

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

    // --- default style: case, IndentOneStep: false ---

    #[test]
    fn accepts_when_aligned_with_case() {
        test::<CaseIndentation>().expect_no_offenses("case n\nwhen 0\n  x\nelse\n  y\nend\n");
    }

    #[test]
    fn flags_when_indented_from_case() {
        let src = "case n\n  when 0\n    x\n  else\n    y\nend\n";
        let offenses = run_cop::<CaseIndentation>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(offenses[0].message, "Indent `when` as deep as `case`.");
    }

    #[test]
    fn flags_each_misaligned_when_separately() {
        let src = "case n\n  when 0\n    x\n  when 1\n    y\nend\n";
        let offenses = run_cop::<CaseIndentation>(src);
        assert_eq!(offenses.len(), 2, "got {offenses:?}");
    }

    #[test]
    fn accepts_single_line_case() {
        test::<CaseIndentation>().expect_no_offenses("case n; when 0 then x; else y; end\n");
    }

    #[test]
    fn accepts_aligned_in_pattern() {
        test::<CaseIndentation>().expect_no_offenses("case n\nin 0\n  x\nelse\n  y\nend\n");
    }

    #[test]
    fn flags_misaligned_in_pattern() {
        let src = "case n\n  in 0\n    x\nend\n";
        let offenses = run_cop::<CaseIndentation>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(offenses[0].message, "Indent `in` as deep as `case`.");
    }

    #[test]
    fn accepts_assigned_case_aligned_with_case_keyword() {
        // `a = case n` — the `case` keyword is at column 4, `when` must match.
        test::<CaseIndentation>()
            .expect_no_offenses("a = case n\n    when 0\n      x\n    else\n      y\nend\n");
    }

    #[test]
    fn corrects_misaligned_when() {
        let src = "case n\n  when 0\n    x\nend\n";
        let run = run_cop_with_edits::<CaseIndentation>(src);
        assert_eq!(apply(src, &run.edits), "case n\nwhen 0\n    x\nend\n");
    }

    #[test]
    fn corrects_assigned_case_to_case_column() {
        let src = "a = case n\nwhen 0\n  x\nend\n";
        let run = run_cop_with_edits::<CaseIndentation>(src);
        // `when` must align with `case` (column 4).
        assert_eq!(apply(src, &run.edits), "a = case n\n    when 0\n  x\nend\n");
    }

    #[test]
    fn correction_is_idempotent() {
        let src = "case n\n  when 0\n    x\n  when 1\n    y\nend\n";
        let run = run_cop_with_edits::<CaseIndentation>(src);
        let fixed = apply(src, &run.edits);
        assert!(
            run_cop::<CaseIndentation>(&fixed).is_empty(),
            "not idempotent: {fixed:?}"
        );
    }

    // --- EnforcedStyle: end ---

    fn end_opts() -> CaseIndentationOptions {
        CaseIndentationOptions {
            enforced_style: CaseIndentationStyle::End,
            indent_one_step: false,
            indentation_width: Some(2),
        }
    }

    #[test]
    fn end_style_accepts_when_aligned_with_end() {
        // `case`/`end` aligned at col 0 → `when` at col 0 is correct.
        let offenses = run_cop_with_options::<CaseIndentation>(
            "a = case n\nwhen 0\n  x\nelse\n  y\nend\n",
            &end_opts(),
        );
        assert!(offenses.is_empty(), "got {offenses:?}");
    }

    #[test]
    fn end_style_flags_when_indented_to_case() {
        let src = "a = case n\n    when 0\n      x\n    else\n      y\nend\n";
        let offenses = run_cop_with_options::<CaseIndentation>(src, &end_opts());
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(offenses[0].message, "Indent `when` as deep as `end`.");
    }

    #[test]
    fn end_style_corrects_to_end_column() {
        let src = "a = case n\n    when 0\n      x\nend\n";
        let run = run_cop_with_options_and_edits::<CaseIndentation>(src, &end_opts());
        assert_eq!(apply(src, &run.edits), "a = case n\nwhen 0\n      x\nend\n");
    }

    // --- IndentOneStep: true ---

    fn one_step_opts() -> CaseIndentationOptions {
        CaseIndentationOptions {
            enforced_style: CaseIndentationStyle::Case,
            indent_one_step: true,
            indentation_width: Some(2),
        }
    }

    #[test]
    fn one_step_accepts_when_indented_one_level() {
        let offenses = run_cop_with_options::<CaseIndentation>(
            "case n\n  when 0\n    x\nend\n",
            &one_step_opts(),
        );
        assert!(offenses.is_empty(), "got {offenses:?}");
    }

    #[test]
    fn one_step_flags_when_aligned_with_case() {
        let src = "case n\nwhen 0\n  x\nend\n";
        let offenses = run_cop_with_options::<CaseIndentation>(src, &one_step_opts());
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(offenses[0].message, "Indent `when` one step more than `case`.");
    }

    /// Regression (Codex #386): the bundled default `IndentationWidth: ~` merges
    /// to JSON `null`. With `IndentationWidth` modelled as `Option<i64>`, a null
    /// decodes to `None` (not an error), so the user's other keys survive. A
    /// plain `i64` would reject null, error the whole struct, and silently fall
    /// back to defaults — discarding `EnforcedStyle`/`IndentOneStep`.
    #[test]
    fn null_indentation_width_preserves_other_keys() {
        let json = br#"{"EnforcedStyle":"end","IndentOneStep":true,"IndentationWidth":null}"#;
        let opts = <CaseIndentationOptions as CopOptions>::from_config_json(json)
            .expect("null IndentationWidth must decode, not error");
        assert!(opts.enforced_style == CaseIndentationStyle::End);
        assert!(opts.indent_one_step);
        assert_eq!(opts.indentation_width, None);
        // `None` resolves to the passed fallback width in `indentation_width`.
        assert_eq!(super::indentation_width(&opts, 2), 2);
    }

    /// Cross-cop fallback (murphy-kke2): with `IndentOneStep: true` and this
    /// cop's own `IndentationWidth` unset, the step width comes from the
    /// run-wide resolved `Layout/IndentationWidth.Width`. At width 4 a `when`
    /// indented 4 from `case` is accepted; under the old hardcoded 2 it was
    /// flagged.
    #[test]
    fn falls_back_to_layout_indentation_width_for_one_step() {
        let opts = CaseIndentationOptions {
            enforced_style: CaseIndentationStyle::Case,
            indent_one_step: true,
            indentation_width: None,
        };
        test::<CaseIndentation>()
            .with_options(&opts)
            .with_indentation_width(4)
            .expect_no_offenses("case n\n    when 0\n      x\nend\n");
    }
}
