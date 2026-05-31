//! `Layout/EmptyLines` — flags two or more consecutive blank lines.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/EmptyLines
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Consecutive blank lines (lines with only whitespace count as blank)
//!   are flagged. Heredoc body lines are excluded via HeredocStart/End
//!   token scanning, matching RuboCop's token-based approach.
//!   Message: "Extra blank line detected."
//!   Autocorrect: removes the extra blank line range.
//! ```
//!
//! ## Algorithm
//!
//! 1. Collect heredoc body byte ranges from `cx.sorted_tokens()` (same
//!    approach as `Layout/TrailingWhitespace`).
//! 2. Walk `cx.source()` line by line, tracking whether each line is
//!    blank (contains only whitespace) and whether it falls inside a
//!    heredoc body.
//! 3. Whenever two consecutive blank non-heredoc lines are found, the
//!    second (and any further) blank lines emit an offense covering the
//!    full line (including the terminating `\n`).
//! 4. Autocorrect: emit_edit removes the offending line range.

use murphy_plugin_api::{Cx, NoOptions, Range, SourceTokenKind, cop};

#[derive(Default)]
pub struct EmptyLines;

#[cop(
    name = "Layout/EmptyLines",
    description = "Do not use several empty lines in a row.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl EmptyLines {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let src = cx.source();
        let bytes = src.as_bytes();

        let heredoc_body_ranges = collect_heredoc_body_ranges(cx);

        // Walk lines, tracking consecutive blank lines.
        let mut prev_blank = false;
        let mut line_start = 0usize;

        while line_start < bytes.len() {
            // Find the end of this line (exclusive of \n, or EOF).
            let line_end = bytes[line_start..]
                .iter()
                .position(|&b| b == b'\n')
                .map(|i| line_start + i)
                .unwrap_or(bytes.len());

            let line_bytes = &bytes[line_start..line_end];
            let is_blank = line_bytes.iter().all(|b| b.is_ascii_whitespace());
            let in_heredoc = byte_in_heredoc_body(line_start as u32, &heredoc_body_ranges);

            if is_blank && !in_heredoc {
                if prev_blank {
                    // This is the second (or further) consecutive blank line.
                    // Offense covers the whole line including its terminating \n.
                    let offense_end = if line_end < bytes.len() {
                        line_end + 1
                    } else {
                        line_end
                    };
                    let range = Range {
                        start: line_start as u32,
                        end: offense_end as u32,
                    };
                    cx.emit_offense(range, "Extra blank line detected.", None);
                    cx.emit_edit(range, "");
                    // prev_blank stays true — further consecutive blank lines
                    // are also flagged independently.
                } else {
                    prev_blank = true;
                }
            } else {
                prev_blank = false;
            }

            line_start = line_end + 1; // skip the \n
        }
    }
}

fn collect_heredoc_body_ranges(cx: &Cx<'_>) -> Vec<(u32, u32)> {
    let source = cx.source().as_bytes();
    let tokens = cx.sorted_tokens();
    let mut starts: Vec<u32> = Vec::new();
    let mut ranges: Vec<(u32, u32)> = Vec::new();

    for tok in tokens {
        match tok.kind {
            SourceTokenKind::HeredocStart => {
                starts.push(tok.range.end + 1);
            }
            SourceTokenKind::HeredocEnd => {
                if let Some(body_start) = starts.pop() {
                    let terminator_line_start = terminator_line_start(source, tok.range.start);
                    ranges.push((body_start, terminator_line_start));
                }
            }
            _ => {}
        }
    }

    ranges
}

fn terminator_line_start(source: &[u8], pos: u32) -> u32 {
    let pos = pos as usize;
    source[..pos]
        .iter()
        .rposition(|&b| b == b'\n')
        .map(|i| i + 1)
        .unwrap_or(0) as u32
}

fn byte_in_heredoc_body(byte_offset: u32, ranges: &[(u32, u32)]) -> bool {
    ranges
        .iter()
        .any(|&(start, end)| byte_offset >= start && byte_offset < end)
}

#[cfg(test)]
mod tests {
    use super::EmptyLines;
    use murphy_plugin_api::test_support::{run_cop, test};

    #[test]
    fn accepts_single_blank_line() {
        test::<EmptyLines>().expect_no_offenses("foo\n\nbar\n");
    }

    #[test]
    fn accepts_no_blank_lines() {
        test::<EmptyLines>().expect_no_offenses("foo\nbar\n");
    }

    /// "foo\n\n\nbar\n" — line 3 is the extra blank line.
    /// Byte layout: foo=0..3, \n=3, \n=4, \n=5, bar=6..9, \n=9
    /// Extra blank = bytes 5..6 (the second \n).
    #[test]
    fn flags_two_consecutive_blank_lines() {
        let src = "foo\n\n\nbar\n";
        let offenses = run_cop::<EmptyLines>(src);
        assert_eq!(offenses.len(), 1, "expected 1 offense, got {offenses:?}");
        assert_eq!(offenses[0].message, "Extra blank line detected.");
        // Offense range: [5, 6) — the second \n (extra blank line).
        assert_eq!(offenses[0].range.start, 5, "wrong start");
        assert_eq!(offenses[0].range.end, 6, "wrong end");
    }

    #[test]
    fn corrects_two_blank_lines_to_one() {
        let src = "foo\n\n\nbar\n";
        let result = murphy_plugin_api::test_support::run_cop_with_edits::<EmptyLines>(src);
        assert_eq!(result.offenses.len(), 1);
        // Apply the edit.
        let edit = &result.edits[0];
        assert_eq!(edit.replacement, "");
        assert_eq!(edit.range.start, 5);
        assert_eq!(edit.range.end, 6);
    }

    /// Three consecutive blank lines → two extra offenses.
    #[test]
    fn flags_three_consecutive_blank_lines() {
        // "foo\n\n\n\nbar\n"
        let src = "foo\n\n\n\nbar\n";
        let offenses = run_cop::<EmptyLines>(src);
        assert_eq!(offenses.len(), 2, "expected 2 offenses, got {offenses:?}");
        assert!(
            offenses
                .iter()
                .all(|o| o.message == "Extra blank line detected.")
        );
    }

    #[test]
    fn accepts_blank_line_inside_heredoc() {
        // Blank lines inside a heredoc body must not be flagged.
        test::<EmptyLines>().expect_no_offenses("x = <<~RUBY\n  hello\n\n  world\nRUBY\n");
    }

    #[test]
    fn accepts_consecutive_blank_lines_inside_heredoc() {
        // Two consecutive blank lines inside a heredoc body must not be flagged.
        test::<EmptyLines>().expect_no_offenses("x = <<~RUBY\n  hello\n\n\n  world\nRUBY\n");
    }

    #[test]
    fn accepts_single_trailing_blank_line() {
        // A file ending with exactly one trailing blank line must not be flagged.
        // Regression test: the old `loop` implementation ran a final "virtual"
        // iteration past the last \n, causing a false-positive offense.
        test::<EmptyLines>().expect_no_offenses("foo\nbar\n\n");
    }

    #[test]
    fn flags_consecutive_blank_lines_outside_heredoc() {
        // Two consecutive blank lines after (outside) the heredoc are flagged.
        // Source: "x = <<~RUBY\n  hello\nRUBY\n\n\nbar\n"
        let src = "x = <<~RUBY\n  hello\nRUBY\n\n\nbar\n";
        let offenses = run_cop::<EmptyLines>(src);
        assert_eq!(offenses.len(), 1, "expected 1 offense, got {offenses:?}");
        assert_eq!(offenses[0].message, "Extra blank line detected.");
    }
}
