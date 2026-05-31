//! `Layout/TrailingWhitespace` -- flags space / tab characters between
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/TrailingWhitespace
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues:
//!   - murphy-wapy
//! notes: >
//!   AllowInHeredoc option and message punctuation implemented.
//!   Heredoc autocorrect gap: when AllowInHeredoc is false, RuboCop
//!   wraps trailing whitespace inside dynamic heredoc bodies with
//!   `#{'  '}` interpolation (preserving string value) and silently
//!   skips autocorrect for static (single-quoted) heredocs. Murphy
//!   emits the offense but suppresses emit_edit for heredoc content
//!   lines to avoid corrupting string values -- the interpolation-wrap
//!   form is deferred pending a static-vs-dynamic heredoc predicate in
//!   the plugin API.
//! ```
//!
//! the last non-whitespace character on a line and the line's terminator.
//! Mirrors RuboCop's same-named cop.
//!
//! This is the raw-source vector of §12d: the cop scans `cx.source()`
//! directly rather than walking the arena. The dispatch surface is
//! `NodeCop::KINDS = &[]`, the file-visit form documented on
//! [`NodeCop`](murphy_plugin_api::NodeCop) -- invoked exactly once per
//! file with `node == cx.root()`.
//!
//! ## Edge cases
//!
//! - **CRLF / Mac-style endings**: `\r\n` is the de-facto Ruby line
//!   terminator on Windows-written files; `\r` alone is essentially
//!   dead history. We treat `\r` as ordinary whitespace before a `\n` --
//!   trailing `\r` before EOL is a `Layout/TrailingWhitespace` offense
//!   too, so editors that auto-strip get pointed at it.
//! - **No final newline**: the last line still counts; trailing
//!   whitespace at EOF is reported on its own range.
//! - **Whitespace-only lines**: the whole line is trailing whitespace
//!   and reported as such.
//! - **AllowInHeredoc: true**: trailing whitespace inside heredoc bodies
//!   is not flagged. Heredoc bodies are detected via `HeredocStart` /
//!   `HeredocEnd` token pairs from `cx.sorted_tokens()`.
//! - **Heredoc autocorrect safety**: even when AllowInHeredoc is false,
//!   Murphy only emits the offense for heredoc body lines -- it does
//!   NOT emit an autocorrect edit, because removing trailing whitespace
//!   from inside a heredoc body changes the string's runtime value.

use murphy_plugin_api::{CopOptions, Cx, Range, SourceTokenKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct TrailingWhitespace;

/// Options for [`TrailingWhitespace`].
#[derive(CopOptions)]
pub struct TrailingWhitespaceOptions {
    #[option(
        name = "AllowInHeredoc",
        default = false,
        description = "When true, trailing whitespace inside heredoc bodies is not flagged."
    )]
    pub allow_in_heredoc: bool,
}

#[cop(
    name = "Layout/TrailingWhitespace",
    description = "Flag space or tab characters between the last non-whitespace character on a line and the line terminator.",
    default_severity = "warning",
    default_enabled = true,
    options = TrailingWhitespaceOptions
)]
impl TrailingWhitespace {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<TrailingWhitespaceOptions>();
        let src = cx.source();
        // Walk byte-by-byte so range offsets stay in the file's byte
        // index space (ADR 0001: offense ranges are byte offsets).
        let bytes = src.as_bytes();

        // Always collect heredoc body byte ranges so we can suppress
        // autocorrect edits inside heredoc bodies (which would corrupt
        // the string's runtime value). When AllowInHeredoc is true, we
        // also use these ranges to suppress offenses entirely.
        //
        // Each HeredocStart / HeredocEnd pair brackets the body:
        //   body_start = heredoc_start.end + 1 (skip the newline after the opener)
        //   body_end   = heredoc_end.start
        let heredoc_body_ranges = collect_heredoc_body_ranges(cx);

        let mut line_start = 0usize;
        let mut i = 0usize;
        while i < bytes.len() {
            if bytes[i] == b'\n' {
                let in_heredoc = byte_in_heredoc_body(line_start as u32, &heredoc_body_ranges);
                if !opts.allow_in_heredoc || !in_heredoc {
                    emit_if_trailing(cx, bytes, line_start, i, in_heredoc);
                }
                line_start = i + 1;
            }
            i += 1;
        }
        // Last line -- only flag if it has trailing whitespace. (A line
        // with zero whitespace at the end is clean; an unterminated
        // final line with no whitespace at all just means "no final
        // newline" which is a different cop's concern.)
        if line_start < bytes.len() {
            let in_heredoc = byte_in_heredoc_body(line_start as u32, &heredoc_body_ranges);
            if !opts.allow_in_heredoc || !in_heredoc {
                emit_if_trailing(cx, bytes, line_start, bytes.len(), in_heredoc);
            }
        }
    }
}

/// Collect (body_start, body_end) byte-offset pairs for all heredoc bodies
/// in the file. Body bytes run from `heredoc_start.end + 1` (skipping the
/// newline after the opener) to `heredoc_end.start`.
///
/// Tokens in `sorted_tokens` are ordered by start position. We match the
/// k-th `HeredocStart` with the k-th `HeredocEnd` (document order).
fn collect_heredoc_body_ranges(cx: &Cx<'_>) -> Vec<(u32, u32)> {
    let tokens = cx.sorted_tokens();
    let mut starts: Vec<u32> = Vec::new();
    let mut ends: Vec<u32> = Vec::new();

    for tok in tokens {
        match tok.kind {
            SourceTokenKind::HeredocStart => starts.push(tok.range.end),
            SourceTokenKind::HeredocEnd => ends.push(tok.range.start),
            _ => {}
        }
    }

    starts
        .into_iter()
        .zip(ends)
        .map(|(start_after_opener, end_before_closer)| {
            // Skip the newline that immediately follows the heredoc opener.
            // The opener line looks like: `x = <<~RUBY\n` -- the HeredocStart
            // token covers up to and including the `Y`; the `\n` at the end
            // of the opener line is NOT part of the body.
            let body_start = start_after_opener + 1; // skip `\n`
            (body_start, end_before_closer)
        })
        .collect()
}

/// Returns true when `byte_offset` falls within any heredoc body range.
fn byte_in_heredoc_body(byte_offset: u32, ranges: &[(u32, u32)]) -> bool {
    ranges
        .iter()
        .any(|&(start, end)| byte_offset >= start && byte_offset < end)
}

/// Inspect bytes `[line_start, line_end)` (exclusive of the `\n` itself)
/// and emit an offense if there is trailing whitespace.
///
/// When `in_heredoc` is true the offense is emitted but the autocorrect
/// edit is suppressed: removing trailing whitespace from inside a heredoc
/// body changes the string's runtime value, which is incorrect. The
/// full RuboCop behaviour (wrap in `#{'…'}` interpolation) is deferred.
fn emit_if_trailing(
    cx: &Cx<'_>,
    bytes: &[u8],
    line_start: usize,
    line_end: usize,
    in_heredoc: bool,
) {
    let mut trim = line_end;
    while trim > line_start && is_trailing_ws(bytes[trim - 1]) {
        trim -= 1;
    }
    if trim == line_end {
        return;
    }
    let range = Range {
        start: trim as u32,
        end: line_end as u32,
    };
    cx.emit_offense(range, "Trailing whitespace detected.", None);
    // Inside a heredoc body, suppress the autocorrect edit to avoid
    // silently changing the string's runtime value.
    if !in_heredoc {
        cx.emit_edit(range, "");
    }
}

/// Bytes that count as trailing whitespace for this cop. `\r` is in the
/// set so CRLF files get the leftover `\r` flagged before the `\n`.
fn is_trailing_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r')
}

#[cfg(test)]
mod tests {
    use super::{TrailingWhitespace, TrailingWhitespaceOptions};
    use murphy_plugin_api::test_support::test;

    fn allow_in_heredoc() -> TrailingWhitespaceOptions {
        TrailingWhitespaceOptions {
            allow_in_heredoc: true,
        }
    }

    // ----- Basic behavior -------------------------------------------

    #[test]
    fn flags_trailing_space() {
        // "x = 0   \n": trailing whitespace starts at byte 5 (after 'x = 0'),
        // 3 trailing spaces. Annotation: 5 spaces + 3 carets.
        test::<TrailingWhitespace>()
            .expect_offense("x = 0   \n     ^^^ Trailing whitespace detected.\n");
    }

    #[test]
    fn corrects_trailing_space() {
        test::<TrailingWhitespace>().expect_correction(
            "x = 0   \n     ^^^ Trailing whitespace detected.\n",
            "x = 0\n",
        );
    }

    #[test]
    fn accepts_clean_line() {
        test::<TrailingWhitespace>().expect_no_offenses("x = 0\n");
    }

    // ----- Message punctuation --------------------------------------

    #[test]
    fn message_ends_with_period() {
        // "x   \n": trailing whitespace starts at byte 1 (after 'x'), 3 spaces.
        // Annotation: 1 space + 3 carets.
        test::<TrailingWhitespace>().expect_offense("x   \n ^^^ Trailing whitespace detected.\n");
    }

    // ----- AllowInHeredoc: false (default) -------------------------

    #[test]
    fn flags_trailing_space_inside_heredoc_by_default() {
        // With default options (AllowInHeredoc: false), trailing whitespace
        // inside a heredoc body is still flagged.
        // "x = <<~RUBY\n  hello   \nRUBY\n"
        // Line 2: "  hello   " -- trailing WS starts at byte (12 + 7) = 19
        // (12 bytes for "x = <<~RUBY\n", then "  hello" = 7 bytes, then 3 spaces)
        // Range: start=19, end=22; on line 2, col 7: annotation has 7 spaces + 3 carets
        test::<TrailingWhitespace>().expect_offense(
            "x = <<~RUBY\n  hello   \n       ^^^ Trailing whitespace detected.\nRUBY\n",
        );
    }

    #[test]
    fn heredoc_body_offense_has_no_autocorrect() {
        // Trailing whitespace inside a heredoc body must NOT be auto-removed --
        // doing so would silently change the string's runtime value.
        // Verify the offense fires (expect_offense) but no edit is emitted
        // (expect_no_corrections on the same clean source).
        let src = "x = <<~RUBY\n  hello   \nRUBY\n";
        test::<TrailingWhitespace>().expect_no_corrections(src);
    }

    // ----- AllowInHeredoc: true ------------------------------------

    #[test]
    fn allows_trailing_space_in_heredoc_body_when_allow_in_heredoc_true() {
        // When AllowInHeredoc: true, trailing whitespace in the body is exempt.
        test::<TrailingWhitespace>()
            .with_options(&allow_in_heredoc())
            .expect_no_offenses("x = <<~RUBY\n  hello   \nRUBY\n");
    }

    #[test]
    fn still_flags_outside_heredoc_when_allow_in_heredoc_true() {
        // AllowInHeredoc only exempts the body -- non-heredoc trailing
        // whitespace is still flagged.
        // "x = 0   \n": trailing WS at byte 5, 3 spaces.
        test::<TrailingWhitespace>()
            .with_options(&allow_in_heredoc())
            .expect_offense(
                "x = 0   \n     ^^^ Trailing whitespace detected.\ny = <<~RUBY\n  hello   \nRUBY\n",
            );
    }

    #[test]
    fn allows_trailing_space_in_multiple_heredoc_bodies() {
        test::<TrailingWhitespace>()
            .with_options(&allow_in_heredoc())
            .expect_no_offenses("x = <<~A\n  hello   \nA\ny = <<~B\n  world   \nB\n");
    }
}
