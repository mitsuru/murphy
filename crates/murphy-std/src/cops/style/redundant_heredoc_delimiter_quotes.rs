//! `Style/RedundantHeredocDelimiterQuotes` — flags heredoc openers whose
//! single- or double-quote delimiters are redundant (i.e. the body contains
//! no string interpolation markers or backslash escape sequences).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantHeredocDelimiterQuotes
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Covered:
//!     - Single-quoted heredoc delimiters (<<~'EOS', <<"EOS") where the
//!       body contains no #{...}, #$var, #@var, or backslash sequences.
//!     - Squiggly (<<~) and non-squiggly (<<) forms.
//!     - Double-quoted heredoc delimiters with no meaningful interpolation.
//!     - Terminator labels with non-word characters keep their quotes.
//!     - Autocorrect: strip the surrounding quotes from the opener token.
//!   Not covered:
//!     - Backtick heredocs (<<~`CMD`) -- kept as-is, no offense.
//! ```

use murphy_plugin_api::{Cx, NoOptions, Range, SourceTokenKind, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantHeredocDelimiterQuotes;

const MSG: &str = "Remove the redundant heredoc delimiter quotes, use `%s` instead.";

#[cop(
    name = "Style/RedundantHeredocDelimiterQuotes",
    description = "Checks for redundant heredoc delimiter quotes.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RedundantHeredocDelimiterQuotes {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        check_file(cx);
    }
}

/// Core logic: scan all HeredocStart/HeredocEnd pairs and flag redundant quotes.
fn check_file(cx: &Cx<'_>) {
    use std::collections::VecDeque;

    let src = cx.source();
    let src_bytes = src.as_bytes();
    let tokens = cx.sorted_tokens();

    // Collect (HeredocStart token, body_start) tuples.
    // We pair HeredocStart and HeredocEnd tokens FIFO.
    let mut pending: VecDeque<(murphy_plugin_api::SourceToken, u32)> = VecDeque::new();

    for tok in tokens {
        match tok.kind {
            SourceTokenKind::HeredocStart => {
                // body_start = byte after the newline at end of opener line
                let body_start = tok.range.end + 1;
                pending.push_back((*tok, body_start));
            }
            SourceTokenKind::HeredocEnd => {
                let Some((start_tok, body_start)) = pending.pop_front() else {
                    continue;
                };
                let heredoc_end_tok = *tok;

                // Get the HeredocStart raw bytes: e.g. `<<~'EOS'` or `<<"EOS"`
                let opener_bytes =
                    &src_bytes[start_tok.range.start as usize..start_tok.range.end as usize];

                // Parse the opener to find the delimiter quote and label.
                let Some((quote_char, label_bytes, label_start_offset)) =
                    parse_opener(opener_bytes)
                else {
                    continue; // unquoted or backtick heredoc -- skip
                };

                // Check 1: terminator label must contain only word characters
                // (no spaces, dashes, etc.). If it has \W chars, quotes are needed.
                let terminator_bytes = &src_bytes
                    [heredoc_end_tok.range.start as usize..heredoc_end_tok.range.end as usize];
                let terminator_stripped = strip_whitespace(terminator_bytes);
                if !is_word_only(terminator_stripped) {
                    continue;
                }

                // Check 2: body must have no string interpolation or backslash.
                let body_end = terminator_line_start(src_bytes, heredoc_end_tok.range.start);
                let body_slice = &src_bytes[body_start as usize..body_end as usize];
                if body_needs_quotes(body_slice) {
                    continue;
                }

                // The quotes are redundant. Build the replacement (opener without quotes).
                let replacement = build_replacement(opener_bytes, label_bytes);
                let msg = MSG.replace("%s", &replacement);

                // Offense range = the full HeredocStart token.
                cx.emit_offense(start_tok.range, &msg, None);

                // Autocorrect: delete the opening quote and the closing quote.
                // Opening quote is at position label_start_offset - 1 within the opener.
                let quote_open_offset = start_tok.range.start + label_start_offset as u32 - 1;
                let quote_close_offset = start_tok.range.end - 1;

                // Delete opening quote character.
                cx.emit_edit(
                    Range {
                        start: quote_open_offset,
                        end: quote_open_offset + 1,
                    },
                    "",
                );
                // Delete closing quote character.
                cx.emit_edit(
                    Range {
                        start: quote_close_offset,
                        end: quote_close_offset + 1,
                    },
                    "",
                );

                // suppress unused variable warning
                let _ = quote_char;
            }
            _ => {}
        }
    }
}

/// Parse a HeredocStart token (bytes) to extract:
/// - `quote_char`: `'` or `"`.
/// - `label_bytes`: the identifier bytes between the quotes.
/// - `label_start_offset`: byte offset of the first label byte within `opener_bytes`.
///
/// Returns `None` for unquoted heredocs or backtick heredocs.
///
/// Examples:
///   `<<~'EOS'`  => `('\'', b"EOS", 4)`
///   `<<'EOS'`   => `('\'', b"EOS", 3)`
///   `<<"EOS"`   => `('"', b"EOS", 3)`
///   `<<~EOS`    => `None` (unquoted)
///   `<<~` + backtick + `CMD` + backtick => `None` (backtick)
fn parse_opener(opener: &[u8]) -> Option<(u8, &[u8], usize)> {
    // Openers start with `<<` followed by optional `~` or `-`.
    let mut pos = 2usize;
    while pos < opener.len() && matches!(opener[pos], b'~' | b'-') {
        pos += 1;
    }
    if pos >= opener.len() {
        return None;
    }
    let quote = opener[pos];
    if !matches!(quote, b'\'' | b'"') {
        return None; // unquoted or backtick
    }
    let label_start = pos + 1;
    // The closing quote should be the last byte of the opener.
    if opener.last() != Some(&quote) {
        return None; // malformed -- skip
    }
    let label_end = opener.len() - 1;
    if label_start > label_end {
        return None; // empty label -- skip
    }
    Some((quote, &opener[label_start..label_end], label_start))
}

/// Returns true if `bytes` contains only ASCII word characters (`[A-Za-z0-9_]`).
fn is_word_only(bytes: &[u8]) -> bool {
    !bytes.is_empty()
        && bytes
            .iter()
            .all(|&b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Strip leading and trailing ASCII whitespace from a byte slice.
fn strip_whitespace(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|&b| !b.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|&b| !b.is_ascii_whitespace())
        .map(|i| i + 1)
        .unwrap_or(0);
    if start >= end {
        &[]
    } else {
        &bytes[start..end]
    }
}

/// Returns true if the heredoc body slice requires quotes to preserve meaning:
/// - Contains `#{`, `#$`, or `#@` (interpolation markers).
/// - Contains a backslash `\` (escape sequence).
fn body_needs_quotes(body: &[u8]) -> bool {
    let mut i = 0usize;
    while i < body.len() {
        let b = body[i];
        if b == b'\\' {
            return true;
        }
        if b == b'#' && i + 1 < body.len() {
            match body[i + 1] {
                b'{' | b'$' | b'@' => return true,
                _ => {}
            }
        }
        i += 1;
    }
    false
}

/// Build the replacement string for the HeredocStart token (without quotes).
///
/// For `<<~'EOS'`, returns `<<~EOS`.
/// For `<<"EOS"`, returns `<<EOS`.
fn build_replacement(opener: &[u8], label: &[u8]) -> String {
    // Find the prefix: everything up to (not including) the quote char.
    let prefix_end = opener
        .iter()
        .position(|&b| matches!(b, b'\'' | b'"'))
        .unwrap_or(0);
    let prefix = std::str::from_utf8(&opener[..prefix_end]).unwrap_or("<<");
    let label_str = std::str::from_utf8(label).unwrap_or("");
    format!("{prefix}{label_str}")
}

/// Returns the byte offset of the start of the line containing `pos`.
fn terminator_line_start(source: &[u8], pos: u32) -> u32 {
    let pos = pos as usize;
    source[..pos]
        .iter()
        .rposition(|&b| b == b'\n')
        .map(|i| i + 1)
        .unwrap_or(0) as u32
}

#[cfg(test)]
mod tests {
    use super::RedundantHeredocDelimiterQuotes;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- No-offense cases ---

    #[test]
    fn no_offense_unquoted_heredoc() {
        // Already unquoted -- no offense.
        test::<RedundantHeredocDelimiterQuotes>().expect_no_offenses(indoc! {"
            x = <<~EOS
              no interpolation
            EOS
        "});
    }

    #[test]
    fn no_offense_body_has_interpolation_marker() {
        // Body has #{...} -- quotes are needed.
        test::<RedundantHeredocDelimiterQuotes>().expect_no_offenses(
            "x = <<~'EOS'\n  \x23{string_interpolation_style_text_not_evaluated}\nEOS\n",
        );
    }

    #[test]
    fn no_offense_body_has_backslash() {
        // Body has a backslash -- quotes preserve its literal meaning.
        test::<RedundantHeredocDelimiterQuotes>().expect_no_offenses(
            "x = <<~'EOS'\n  Preserve \\\n  newlines\nEOS\n",
        );
    }

    #[test]
    fn no_offense_body_has_hash_dollar() {
        // Body has #$ interpolation marker.
        test::<RedundantHeredocDelimiterQuotes>().expect_no_offenses(
            "x = <<~'EOS'\n  #$global\nEOS\n",
        );
    }

    #[test]
    fn no_offense_body_has_hash_at() {
        // Body has #@ interpolation marker.
        test::<RedundantHeredocDelimiterQuotes>().expect_no_offenses(
            "x = <<~'EOS'\n  #@ivar\nEOS\n",
        );
    }

    // --- Offense cases ---

    #[test]
    fn flags_single_quoted_squiggly_heredoc_plain_body() {
        test::<RedundantHeredocDelimiterQuotes>().expect_offense(indoc! {"
            do_something(<<~'EOS')
                         ^^^^^^^^ Remove the redundant heredoc delimiter quotes, use `<<~EOS` instead.
              no string interpolation style text
            EOS
        "});
    }

    #[test]
    fn flags_double_quoted_non_squiggly_heredoc_plain_body() {
        test::<RedundantHeredocDelimiterQuotes>().expect_offense(indoc! {r#"
            do_something(<<"EOS")
                         ^^^^^^^ Remove the redundant heredoc delimiter quotes, use `<<EOS` instead.
              no string interpolation style text
            EOS
        "#});
    }

    #[test]
    fn flags_double_quoted_squiggly_heredoc_plain_body() {
        test::<RedundantHeredocDelimiterQuotes>().expect_offense(indoc! {r#"
            do_something(<<~"EOS")
                         ^^^^^^^^ Remove the redundant heredoc delimiter quotes, use `<<~EOS` instead.
              no string interpolation style text
            EOS
        "#});
    }

    // --- Autocorrect ---

    #[test]
    fn corrects_single_quoted_heredoc() {
        test::<RedundantHeredocDelimiterQuotes>().expect_correction(
            indoc! {"
                do_something(<<~'EOS')
                             ^^^^^^^^ Remove the redundant heredoc delimiter quotes, use `<<~EOS` instead.
                  no string interpolation style text
                EOS
            "},
            indoc! {"
                do_something(<<~EOS)
                  no string interpolation style text
                EOS
            "},
        );
    }

    #[test]
    fn corrects_double_quoted_heredoc() {
        test::<RedundantHeredocDelimiterQuotes>().expect_correction(
            indoc! {r#"
                do_something(<<"EOS")
                             ^^^^^^^ Remove the redundant heredoc delimiter quotes, use `<<EOS` instead.
                  no string interpolation style text
                EOS
            "#},
            indoc! {"
                do_something(<<EOS)
                  no string interpolation style text
                EOS
            "},
        );
    }
}

murphy_plugin_api::submit_cop!(RedundantHeredocDelimiterQuotes);
