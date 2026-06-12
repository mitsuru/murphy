//! `Layout/TrailingEmptyLines` — enforce the trailing blank-line / final
//! newline convention at end-of-file.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/TrailingEmptyLines
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's Layout/TrailingEmptyLines. `EnforcedStyle:
//!   final_newline` (default) wants exactly one newline and no trailing blank
//!   lines; `final_blank_line` wants one trailing blank line. The trailing
//!   whitespace window is the `/\s*\Z/` match (all trailing whitespace), and
//!   `blank_lines = whitespace_at_end.count("\n") - 1`. Files ending in
//!   `__END__` (`/\s*__END__/` or the last token followed by `__END__`) and
//!   files ending with `%\n\n` are skipped exactly as upstream does.
//!   Autocorrect replaces the trailing-whitespace range with `"\n"`
//!   (final_newline) or `"\n\n"` (final_blank_line).
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, Range, cop};

#[derive(Default)]
pub struct TrailingEmptyLines;

#[derive(CopOptions)]
pub struct TrailingEmptyLinesOptions {
    #[option(
        name = "EnforcedStyle",
        default = "final_newline",
        description = "Whether the file should end with a final newline or a final blank line."
    )]
    pub enforced_style: TrailingEmptyLinesStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum TrailingEmptyLinesStyle {
    /// Exactly one newline and no trailing blank lines at EOF.
    #[option(value = "final_newline")]
    FinalNewline,
    /// One trailing blank line followed by a newline at EOF.
    #[option(value = "final_blank_line")]
    FinalBlankLine,
}

#[cop(
    name = "Layout/TrailingEmptyLines",
    description = "Enforce the trailing blank-line / final newline convention.",
    default_severity = "warning",
    default_enabled = true,
    options = TrailingEmptyLinesOptions,
)]
impl TrailingEmptyLines {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let source = cx.source();
        // `return if buffer.source.empty?`
        if source.is_empty() {
            return;
        }

        // `return if ends_in_end?(processed_source)`
        if ends_in_end(cx) {
            return;
        }
        // `return if end_with_percent_blank_string?(processed_source)`
        if source.ends_with("%\n\n") {
            return;
        }

        let opts = cx.options_or_default::<TrailingEmptyLinesOptions>();

        // `whitespace_at_end = buffer.source[/\s*\Z/]` — trailing whitespace.
        let whitespace_at_end = trailing_whitespace(source);
        // `blank_lines = whitespace_at_end.count("\n") - 1`
        let blank_lines = whitespace_at_end.matches('\n').count() as i64 - 1;
        let wanted_blank_lines = match opts.enforced_style {
            TrailingEmptyLinesStyle::FinalNewline => 0,
            TrailingEmptyLinesStyle::FinalBlankLine => 1,
        };

        // `return unless blank_lines != wanted_blank_lines`
        if blank_lines == wanted_blank_lines {
            return;
        }

        self.offense_detected(cx, wanted_blank_lines, blank_lines, whitespace_at_end);
    }
}

impl TrailingEmptyLines {
    fn offense_detected(
        &self,
        cx: &Cx<'_>,
        wanted_blank_lines: i64,
        blank_lines: i64,
        whitespace_at_end: &str,
    ) {
        let source_len = cx.source().len();
        let begin_pos = source_len - whitespace_at_end.len();
        // `autocorrect_range = range_between(begin_pos, buffer.source.length)`
        let autocorrect_range = Range {
            start: begin_pos as u32,
            end: source_len as u32,
        };
        // `begin_pos += 1 unless whitespace_at_end.empty?`
        let report_begin = if whitespace_at_end.is_empty() {
            begin_pos
        } else {
            begin_pos + 1
        };
        let report_range = Range {
            start: report_begin as u32,
            end: source_len as u32,
        };

        let msg = message(wanted_blank_lines, blank_lines);
        cx.emit_offense(report_range, &msg, None);

        // `corrector.replace(autocorrect_range, style == :final_newline ? "\n" : "\n\n")`
        let replacement = if wanted_blank_lines == 0 { "\n" } else { "\n\n" };
        cx.emit_edit(autocorrect_range, replacement);
    }
}

/// `whitespace_at_end = buffer.source[/\s*\Z/]` — the maximal run of trailing
/// ASCII/Unicode whitespace at end-of-file. Returns the matched suffix slice.
fn trailing_whitespace(source: &str) -> &str {
    let trimmed_len = source.trim_end().len();
    &source[trimmed_len..]
}

/// `message(wanted_blank_lines, blank_lines)` from RuboCop.
fn message(wanted_blank_lines: i64, blank_lines: i64) -> String {
    match blank_lines {
        -1 => "Final newline missing.".to_string(),
        0 => "Trailing blank line missing.".to_string(),
        _ => {
            let instead_of = if wanted_blank_lines == 0 {
                String::new()
            } else {
                format!("instead of {wanted_blank_lines} ")
            };
            format!("{blank_lines} trailing blank lines {instead_of}detected.")
        }
    }
}

/// `ends_in_end?(processed_source)` — true when the file ends in an `__END__`
/// data section, which RuboCop never touches.
fn ends_in_end(cx: &Cx<'_>) -> bool {
    let source = cx.source();
    // `return true if buffer.source.match?(/\s*__END__/)` — note RuboCop's
    // regex is unanchored, so it matches an `__END__` *anywhere* in the file.
    if source.contains("__END__") {
        return true;
    }
    // `return false if processed_source.tokens.empty?`
    let tokens = cx.sorted_tokens();
    let Some(last) = tokens.last() else {
        return false;
    };
    // `extra = buffer.source[last.end_pos..]; extra&.strip&.start_with?('__END__')`
    let extra = &source[last.range.end as usize..];
    extra.trim_start().starts_with("__END__")
}

murphy_plugin_api::submit_cop!(TrailingEmptyLines);

#[cfg(test)]
mod tests {
    use super::{TrailingEmptyLines, TrailingEmptyLinesOptions, TrailingEmptyLinesStyle};
    use murphy_plugin_api::test_support::{
        run_cop, run_cop_with_edits, run_cop_with_options, run_cop_with_options_and_edits,
        CapturedEdit,
    };

    fn final_blank_line() -> TrailingEmptyLinesOptions {
        TrailingEmptyLinesOptions {
            enforced_style: TrailingEmptyLinesStyle::FinalBlankLine,
        }
    }

    /// Apply a single non-overlapping EOF edit (the only shape this cop emits).
    fn apply(source: &str, edits: &[CapturedEdit]) -> String {
        assert_eq!(edits.len(), 1, "TrailingEmptyLines emits exactly one edit");
        let edit = &edits[0];
        let mut out = String::with_capacity(source.len());
        out.push_str(&source[..edit.range.start as usize]);
        out.push_str(&edit.replacement);
        out.push_str(&source[edit.range.end as usize..]);
        out
    }

    #[test]
    fn accepts_single_trailing_newline() {
        assert!(run_cop::<TrailingEmptyLines>("x = 0\n").is_empty());
    }

    #[test]
    fn flags_missing_final_newline() {
        let offenses = run_cop::<TrailingEmptyLines>("x = 0");
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].message, "Final newline missing.");
    }

    #[test]
    fn corrects_missing_final_newline() {
        let run = run_cop_with_edits::<TrailingEmptyLines>("x = 0");
        assert_eq!(apply("x = 0", &run.edits), "x = 0\n");
    }

    #[test]
    fn flags_one_trailing_blank_line() {
        let offenses = run_cop::<TrailingEmptyLines>("x = 0\n\n");
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].message, "1 trailing blank lines detected.");
    }

    #[test]
    fn corrects_one_trailing_blank_line() {
        let run = run_cop_with_edits::<TrailingEmptyLines>("x = 0\n\n");
        assert_eq!(apply("x = 0\n\n", &run.edits), "x = 0\n");
    }

    #[test]
    fn flags_multiple_trailing_blank_lines() {
        let offenses = run_cop::<TrailingEmptyLines>("x = 0\n\n\n\n");
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].message, "3 trailing blank lines detected.");
    }

    #[test]
    fn corrects_multiple_trailing_blank_lines() {
        let run = run_cop_with_edits::<TrailingEmptyLines>("x = 0\n\n\n\n");
        assert_eq!(apply("x = 0\n\n\n\n", &run.edits), "x = 0\n");
    }

    #[test]
    fn accepts_empty_file() {
        assert!(run_cop::<TrailingEmptyLines>("").is_empty());
    }

    #[test]
    fn skips_end_data_section() {
        assert!(run_cop::<TrailingEmptyLines>("x = 0\n__END__\nfoo\n\n\n").is_empty());
    }

    #[test]
    fn final_blank_line_accepts_one_blank_line() {
        assert!(run_cop_with_options::<TrailingEmptyLines>("x = 0\n\n", &final_blank_line()).is_empty());
    }

    #[test]
    fn final_blank_line_flags_missing_blank_line() {
        let offenses = run_cop_with_options::<TrailingEmptyLines>("x = 0\n", &final_blank_line());
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].message, "Trailing blank line missing.");
    }

    #[test]
    fn final_blank_line_corrects_missing_blank_line() {
        let run = run_cop_with_options_and_edits::<TrailingEmptyLines>("x = 0\n", &final_blank_line());
        assert_eq!(apply("x = 0\n", &run.edits), "x = 0\n\n");
    }

    #[test]
    fn final_blank_line_flags_too_many_blank_lines() {
        let offenses =
            run_cop_with_options::<TrailingEmptyLines>("x = 0\n\n\n", &final_blank_line());
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].message, "2 trailing blank lines instead of 1 detected.");
    }
}
