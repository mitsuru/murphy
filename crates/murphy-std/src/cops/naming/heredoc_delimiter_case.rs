//! `Naming/HeredocDelimiterCase` — enforce a consistent case for heredoc
//! delimiters (`<<~SQL` / `<<-SQL` … `SQL`).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Naming/HeredocDelimiterCase
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Detection-only port. RuboCop's `on_heredoc` extracts the delimiter via
//!   `delimiter_string` (capture 2 of `/(<<[~-]?)['"`]?([^'"`]+)['"`]?/` on the
//!   opener) and offends when `delimiter != correct_delimiters(delimiter)`,
//!   i.e. `delimiter != delimiter.upcase` for the default `uppercase` style
//!   (or `!= delimiter.downcase` for `lowercase`). Ruby requires the closing
//!   terminator to match the opener's delimiter exactly, so Murphy reads the
//!   bare label straight off each `HeredocEnd` token (`cx.raw_source(end_tok)`)
//!   — the opener regex is unnecessary and every heredoc kind (`str`/`dstr`/
//!   `xstr`, plain/dash/squiggly) is covered by the same token scan. The case
//!   check (`label != correct_case(label)`) reproduces RuboCop's `_FOO_`/digit
//!   no-op behaviour exactly (verified against rubocop 1.87.0).
//!
//!   Offense range mirrors RuboCop's `add_offense(node.loc.heredoc_end)`, which
//!   spans the terminator line's leading whitespace plus the label (verified:
//!   `  Sql` reports col 1..5, not 3..5). Murphy's `HeredocEnd` token starts at
//!   the label (and includes its trailing newline), so the range is widened
//!   back to the terminator line start and clamped to the trimmed label end.
//!
//!   Detection-only by issue scope (`Safe: safe / no-autocorrect`). RuboCop
//!   additionally `extend AutoCorrector`s, rewriting both the opener and
//!   `node.loc.heredoc_end` to the corrected case; because the `heredoc_end`
//!   replacement covers the indent, it also *strips* the terminator's leading
//!   whitespace (`  Sql` -> `SQL` at col 1, verified with `rubocop -a`). That
//!   correction is intentionally not ported, so Murphy emits the offense only
//!   and applies no edit — detection parity is complete (`status: verified`).
//! ```
//!
//! ## Offense range
//!
//! The closing terminator, including any squiggly-heredoc leading indentation:
//! `[line_start(terminator) .. end_of_label]`. Mirrors RuboCop's
//! `node.loc.heredoc_end`.

use murphy_plugin_api::{
    CopOptionEnum, CopOptions, Cx, Range, SourceTokenKind, cop,
};

#[derive(Default)]
pub struct HeredocDelimiterCase;

/// Enforced case for heredoc delimiters.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DelimiterCaseStyle {
    /// Delimiters must be all uppercase (`<<~SQL`). RuboCop default.
    #[default]
    #[option(value = "uppercase")]
    Uppercase,
    /// Delimiters must be all lowercase (`<<~sql`).
    #[option(value = "lowercase")]
    Lowercase,
}

impl DelimiterCaseStyle {
    /// Apply RuboCop's `correct_delimiters`: `upcase` / `downcase`.
    fn correct_case(self, label: &str) -> String {
        match self {
            DelimiterCaseStyle::Uppercase => label.to_uppercase(),
            DelimiterCaseStyle::Lowercase => label.to_lowercase(),
        }
    }

    fn message_word(self) -> &'static str {
        match self {
            DelimiterCaseStyle::Uppercase => "uppercase",
            DelimiterCaseStyle::Lowercase => "lowercase",
        }
    }
}

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "EnforcedStyle",
        default = "uppercase",
        description = "Required case for heredoc delimiters: `uppercase` (default) or `lowercase`."
    )]
    pub enforced_style: DelimiterCaseStyle,
}

#[cop(
    name = "Naming/HeredocDelimiterCase",
    description = "Use configured case for heredoc delimiters.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl HeredocDelimiterCase {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<Options>();
        let source = cx.source().as_bytes();

        // Each `HeredocEnd` token is exactly the bare terminator label. Ruby
        // requires the terminator to match the opener's delimiter, so this is
        // RuboCop's `delimiter_string(node)` without needing the opener regex.
        for tok in cx.sorted_tokens() {
            if tok.kind != SourceTokenKind::HeredocEnd {
                continue;
            }

            // The `HeredocEnd` token spans the bare label plus its trailing
            // newline; trim the trailing whitespace to recover the label
            // (matching `redundant_heredoc_delimiter_quotes`'s `strip_whitespace`).
            let raw = cx.raw_source(tok.range);
            let label = raw.trim_end();
            if label.is_empty() {
                continue;
            }

            // No offense when the delimiter is already in the configured case
            // (mirrors `delimiter == correct_delimiters(delimiter)`). This also
            // no-ops on case-neutral labels like `_FOO_` or all-digit labels.
            if label == opts.enforced_style.correct_case(label) {
                continue;
            }

            // RuboCop offends on `node.loc.heredoc_end`, which spans the
            // terminator line's leading indentation plus the label (but not the
            // newline). Murphy's token starts at the label, so widen the start
            // back to the line and clamp the end to the trimmed label length.
            let offense_range = Range {
                start: line_start(source, tok.range.start),
                end: tok.range.start + label.len() as u32,
            };
            let message =
                format!("Use {} heredoc delimiters.", opts.enforced_style.message_word());
            cx.emit_offense(offense_range, &message, None);
        }
    }
}

/// Byte offset of the first byte on the line containing `pos` (the byte after
/// the previous `\n`, or 0 at the start of file).
fn line_start(bytes: &[u8], pos: u32) -> u32 {
    bytes[..pos as usize]
        .iter()
        .rposition(|&b| b == b'\n')
        .map(|i| i as u32 + 1)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{DelimiterCaseStyle, HeredocDelimiterCase, Options};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn option_defaults_match_rubocop() {
        let opts = Options::default();
        assert_eq!(opts.enforced_style, DelimiterCaseStyle::Uppercase);
    }

    // --- uppercase (default) ---

    #[test]
    fn flags_lowercase_dash_delimiter() {
        // rubocop: offense on terminator line, col 1..3 (`sql`).
        test::<HeredocDelimiterCase>().expect_offense(indoc! {r#"
            x = <<-sql
              SELECT * FROM foo
            sql
            ^^^ Use uppercase heredoc delimiters.
        "#});
    }

    #[test]
    fn flags_mixed_case_squiggly_delimiter() {
        // `Foo`.upcase == `FOO` != `Foo` → offense.
        test::<HeredocDelimiterCase>().expect_offense(indoc! {r#"
            z = <<~Foo
              mixed
            Foo
            ^^^ Use uppercase heredoc delimiters.
        "#});
    }

    #[test]
    fn flags_indented_terminator_includes_leading_whitespace() {
        // Squiggly terminator may be indented; rubocop's heredoc_end span
        // covers the leading whitespace: `  Sql` reports col 1..5.
        test::<HeredocDelimiterCase>().expect_offense(indoc! {r#"
            def m
              x = <<~Sql
                SELECT
              Sql
            ^^^^^ Use uppercase heredoc delimiters.
            end
        "#});
    }

    #[test]
    fn flags_interpolated_heredoc_delimiter() {
        // dstr (interpolated) heredocs are caught by the same token scan.
        test::<HeredocDelimiterCase>().expect_offense(indoc! {r#"
            x = 1
            y = <<~sql
              SELECT #{x}
            sql
            ^^^ Use uppercase heredoc delimiters.
        "#});
    }

    #[test]
    fn no_offense_for_uppercase_delimiter() {
        test::<HeredocDelimiterCase>().expect_no_offenses(indoc! {r#"
            y = <<~RUBY
              hello
            RUBY
        "#});
    }

    #[test]
    fn no_offense_for_case_neutral_delimiter() {
        // `_FOO_`.upcase == `_FOO_` → no offense (verified against rubocop).
        test::<HeredocDelimiterCase>().expect_no_offenses(indoc! {r#"
            y = <<~_FOO_
              hi
            _FOO_
        "#});
    }

    // --- lowercase EnforcedStyle ---

    #[test]
    fn lowercase_flags_uppercase_delimiter() {
        test::<HeredocDelimiterCase>()
            .with_options(&Options { enforced_style: DelimiterCaseStyle::Lowercase })
            .expect_offense(indoc! {r#"
                y = <<~RUBY
                  hello
                RUBY
                ^^^^ Use lowercase heredoc delimiters.
            "#});
    }

    #[test]
    fn lowercase_no_offense_for_lowercase_delimiter() {
        test::<HeredocDelimiterCase>()
            .with_options(&Options { enforced_style: DelimiterCaseStyle::Lowercase })
            .expect_no_offenses(indoc! {r#"
                x = <<-sql
                  SELECT * FROM foo
                sql
            "#});
    }

    #[test]
    fn no_offense_for_ascii_only_code() {
        test::<HeredocDelimiterCase>().expect_no_offenses(indoc! {r#"
            def say_hello
              puts "hi"
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(HeredocDelimiterCase);
