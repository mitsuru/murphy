//! `Layout/LeadingEmptyLines` — flag unnecessary blank lines at the very
//! beginning of a source file.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/LeadingEmptyLines
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Direct port of RuboCop's `on_new_investigation`: take the first token of
//!   the file; if it does not start on line 1 (`token.line > 1`), flag it and
//!   remove the byte range `[0, token.begin_pos)`. RuboCop's
//!   `processed_source.tokens[0]` includes comments but excludes the leading
//!   newline trivia that Murphy emits as `Newline`/`IgnoredNewline` tokens, so
//!   the port selects the first NON-newline token (keeping `Comment`, matching
//!   RuboCop where a leading comment is `tokens[0]`). `token.line > 1` is
//!   equivalent to "there is a `\n` before the token's start byte", since any
//!   newline before the first real token can only be a leading blank line.
//! ```

use murphy_plugin_api::{Cx, NoOptions, Range, SourceTokenKind, cop};

/// Stateless unit struct (ADR 0035 const-metadata cop pattern).
#[derive(Default)]
pub struct LeadingEmptyLines;

#[cop(
    name = "Layout/LeadingEmptyLines",
    description = "Check for unnecessary blank lines at the beginning of a file.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl LeadingEmptyLines {
    #[on_new_investigation]
    fn investigate(&self, cx: &Cx<'_>) {
        // RuboCop: `token = processed_source.tokens[0]`. Murphy emits leading
        // blank lines as `Newline`/`IgnoredNewline` trivia tokens — and a final
        // zero-width / newline-only `Other` EOF sentinel — that RuboCop does not
        // surface in `tokens`. Skip both so we land on the first "real" token (a
        // comment still counts, as it does upstream).
        let source = cx.source();
        let Some(token) = cx.sorted_tokens().iter().find(|t| {
            match t.kind {
                SourceTokenKind::Newline | SourceTokenKind::IgnoredNewline => false,
                // The EOF sentinel is an `Other` whose source is empty or pure
                // whitespace; a real code token never is.
                SourceTokenKind::Other => !cx.raw_source(t.range).trim().is_empty(),
                _ => true,
            }
        }) else {
            return;
        };

        // RuboCop: `return unless token && token.line > 1`. `token.line > 1`
        // iff a newline precedes the token's start byte.
        if !source[..token.range.start as usize].contains('\n') {
            return;
        }

        // RuboCop: `add_offense(token.pos)` then
        // `corrector.remove(range_between(0, token.begin_pos))`.
        cx.emit_offense(
            token.range,
            "Unnecessary blank line at the beginning of the source.",
            None,
        );
        cx.emit_edit(
            Range {
                start: 0,
                end: token.range.start,
            },
            "",
        );
    }
}

murphy_plugin_api::submit_cop!(LeadingEmptyLines);

#[cfg(test)]
mod tests {
    use super::LeadingEmptyLines;
    use murphy_plugin_api::test_support::{run_cop_with_edits, test};

    /// Apply the single non-overlapping leading-trim edit this cop emits.
    fn apply(source: &str, edits: &[murphy_plugin_api::test_support::CapturedEdit]) -> String {
        assert_eq!(edits.len(), 1, "LeadingEmptyLines emits exactly one edit");
        let edit = &edits[0];
        let mut out = String::with_capacity(source.len());
        out.push_str(&source[..edit.range.start as usize]);
        out.push_str(&edit.replacement);
        out.push_str(&source[edit.range.end as usize..]);
        out
    }

    #[test]
    fn accepts_no_leading_blank_line() {
        test::<LeadingEmptyLines>().expect_no_offenses("x = 0\n");
    }

    #[test]
    fn flags_single_leading_blank_line() {
        let offenses = murphy_plugin_api::test_support::run_cop::<LeadingEmptyLines>("\nx = 0\n");
        assert_eq!(offenses.len(), 1);
        assert_eq!(
            offenses[0].message,
            "Unnecessary blank line at the beginning of the source."
        );
    }

    #[test]
    fn corrects_single_leading_blank_line() {
        let run = run_cop_with_edits::<LeadingEmptyLines>("\nx = 0\n");
        assert_eq!(apply("\nx = 0\n", &run.edits), "x = 0\n");
    }

    #[test]
    fn corrects_multiple_leading_blank_lines() {
        let run = run_cop_with_edits::<LeadingEmptyLines>("\n\n\nx = 0\n");
        assert_eq!(apply("\n\n\nx = 0\n", &run.edits), "x = 0\n");
    }

    #[test]
    fn corrects_leading_blank_line_with_indentation() {
        // The removal range spans from byte 0 up to the first token's start,
        // so any leading whitespace on the first real line is trimmed too.
        let run = run_cop_with_edits::<LeadingEmptyLines>("\n  x = 0\n");
        assert_eq!(apply("\n  x = 0\n", &run.edits), "x = 0\n");
    }

    #[test]
    fn accepts_empty_file() {
        test::<LeadingEmptyLines>().expect_no_offenses("");
    }

    #[test]
    fn accepts_whitespace_only_first_line() {
        // No real token at all → no offense (matches RuboCop's nil-token guard).
        test::<LeadingEmptyLines>().expect_no_offenses("\n\n");
    }

    #[test]
    fn accepts_leading_comment_on_line_one() {
        test::<LeadingEmptyLines>().expect_no_offenses("# frozen_string_literal: true\nx = 0\n");
    }

    #[test]
    fn flags_leading_blank_line_before_comment() {
        // A leading comment is RuboCop's `tokens[0]`; a blank line before it
        // still fires.
        let run = run_cop_with_edits::<LeadingEmptyLines>("\n# a comment\nx = 0\n");
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(
            apply("\n# a comment\nx = 0\n", &run.edits),
            "# a comment\nx = 0\n"
        );
    }
}
