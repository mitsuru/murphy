//! `Layout/InitialIndentation` — flag indentation on the first non-blank,
//! non-comment line of a file.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/InitialIndentation
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Direct port of RuboCop's `on_new_investigation` + `first_token` +
//!   `space_before`. `first_token` is the first token whose text does not begin
//!   with `#` (the first non-comment token). If that token's column is non-zero
//!   (i.e. there is leading whitespace on its line), the cop removes only the
//!   same-line leading whitespace — `range_with_surrounding_space(side: :left,
//!   newlines: false)` — never the preceding newlines. The offense range is the
//!   token's own range. Murphy's tokenizer emits leading-newline trivia
//!   (`Newline`/`IgnoredNewline`) and a final zero-width / newline-only `Other`
//!   EOF sentinel that RuboCop's `tokens` does not surface; both are skipped.
//! ```

use murphy_plugin_api::{Cx, NoOptions, Range, SourceTokenKind, cop};

/// Stateless unit struct (ADR 0035 const-metadata cop pattern).
#[derive(Default)]
pub struct InitialIndentation;

#[cop(
    name = "Layout/InitialIndentation",
    description = "Check the indentation of the first non-blank non-comment line in a file.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl InitialIndentation {
    #[on_new_investigation]
    fn investigate(&self, cx: &Cx<'_>) {
        let source = cx.source();

        // RuboCop: `first_token = tokens.find { |t| !t.text.start_with?('#') }`.
        // Murphy additionally surfaces newline trivia and an EOF sentinel that
        // RuboCop's `tokens` omits; skip those so we never report on them.
        let Some(token) = cx.sorted_tokens().iter().find(|t| match t.kind {
            // Comments: `t.text.start_with?('#')` — skipped upstream.
            SourceTokenKind::Comment => false,
            SourceTokenKind::Newline | SourceTokenKind::IgnoredNewline => false,
            // The EOF sentinel is an `Other` whose source is empty / whitespace.
            SourceTokenKind::Other => !cx.raw_source(t.range).trim().is_empty(),
            _ => true,
        }) else {
            return;
        };

        // RuboCop `space_before`: `return if token.column.zero?`. The column is
        // zero iff the token starts at the beginning of its line.
        let start = token.range.start as usize;
        let line_start = source[..start].rfind('\n').map_or(0, |pos| pos + 1);
        if start == line_start {
            return;
        }

        // The same-line leading whitespace is `[line_start, start)`. RuboCop's
        // `range_with_surrounding_space(side: :left, newlines: false)` followed
        // by `return if space_range == token.pos` means: only fire when there is
        // actual whitespace to strip. Since `column != 0` here and the slice is
        // the line's indentation, it is always whitespace; guard defensively
        // anyway so a stray non-space (impossible for column logic) is a no-op.
        if source[line_start..start].trim().is_empty() && start > line_start {
            // RuboCop: `add_offense(first_token.pos)` then `corrector.remove(space)`.
            cx.emit_offense(
                token.range,
                "Indentation of first line in file detected.",
                None,
            );
            cx.emit_edit(
                Range {
                    start: line_start as u32,
                    end: start as u32,
                },
                "",
            );
        }
    }
}

murphy_plugin_api::submit_cop!(InitialIndentation);

#[cfg(test)]
mod tests {
    use super::InitialIndentation;
    use murphy_plugin_api::test_support::{run_cop, run_cop_with_edits};

    fn apply(source: &str, edits: &[murphy_plugin_api::test_support::CapturedEdit]) -> String {
        assert_eq!(edits.len(), 1, "InitialIndentation emits exactly one edit");
        let edit = &edits[0];
        let mut out = String::with_capacity(source.len());
        out.push_str(&source[..edit.range.start as usize]);
        out.push_str(&edit.replacement);
        out.push_str(&source[edit.range.end as usize..]);
        out
    }

    #[test]
    fn accepts_unindented_first_line() {
        assert!(run_cop::<InitialIndentation>("x = 0\n").is_empty());
    }

    #[test]
    fn flags_indented_first_line() {
        let offenses = run_cop::<InitialIndentation>("  x = 0\n");
        assert_eq!(offenses.len(), 1);
        assert_eq!(
            offenses[0].message,
            "Indentation of first line in file detected."
        );
    }

    #[test]
    fn corrects_indented_first_line() {
        let run = run_cop_with_edits::<InitialIndentation>("  x = 0\n");
        assert_eq!(apply("  x = 0\n", &run.edits), "x = 0\n");
    }

    #[test]
    fn corrects_tab_indented_first_line() {
        let run = run_cop_with_edits::<InitialIndentation>("\tx = 0\n");
        assert_eq!(apply("\tx = 0\n", &run.edits), "x = 0\n");
    }

    #[test]
    fn accepts_indentation_after_leading_blank_line() {
        // RuboCop only strips same-line whitespace; the first token here is on
        // line 2 at column 0, so there is no leading indentation to remove.
        assert!(run_cop::<InitialIndentation>("\nx = 0\n").is_empty());
    }

    #[test]
    fn flags_indented_first_line_after_blank_line() {
        // First real token is `x` on line 2 with column 2 → strip its
        // same-line indentation only, leaving the blank line intact.
        let run = run_cop_with_edits::<InitialIndentation>("\n  x = 0\n");
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(apply("\n  x = 0\n", &run.edits), "\nx = 0\n");
    }

    #[test]
    fn skips_leading_comment_and_flags_first_code_token() {
        // `first_token` is the first NON-comment token. A leading (column-0)
        // comment is skipped; the indented `x` on the next line is flagged.
        let run = run_cop_with_edits::<InitialIndentation>("# a comment\n  x = 0\n");
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(apply("# a comment\n  x = 0\n", &run.edits), "# a comment\nx = 0\n");
    }

    #[test]
    fn accepts_empty_file() {
        assert!(run_cop::<InitialIndentation>("").is_empty());
    }

    #[test]
    fn accepts_comment_only_file() {
        assert!(run_cop::<InitialIndentation>("# just a comment\n").is_empty());
    }
}
