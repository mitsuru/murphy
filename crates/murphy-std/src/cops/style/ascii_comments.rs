//! `Style/AsciiComments` — flags non-ASCII characters in comments.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/AsciiComments
//! upstream_version_checked: 1.86.2
//! version_added: "0.9"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Flags the first contiguous run of non-ASCII characters in each comment,
//!   excluding any chars listed in AllowedChars (default: ["©"]).
//!   No autocorrect (RuboCop does not implement one either).
//!   Both inline (#) and block (=begin/=end) comments are checked,
//!   matching RuboCop's behavior of iterating processed_source.comments.
//! ```
//!
//! ## Matched shapes
//!
//! Any comment (`# …` or `=begin … =end`) containing a non-ASCII character
//! that is not in the `AllowedChars` list.
//!
//! Only the **first contiguous run** of non-ASCII characters is flagged,
//! mirroring RuboCop's `/[^[:ascii:]]+/.match` behavior.
//!
//! ## Examples
//!
//! ```ruby
//! # bad
//! # Translates from English to 日本語。
//!
//! # good
//! # Translates from English to Japanese
//! ```
//!
//! ## No autocorrect
//!
//! There is no safe automated replacement for non-ASCII comment text —
//! the correct ASCII equivalent is context-dependent and requires human
//! judgment. RuboCop also does not autocorrect this cop.

use murphy_plugin_api::{CopOptions, Cx, Range, cop};

const MSG: &str = "Use only ascii symbols in comments.";

/// Stateless unit struct.
#[derive(Default)]
pub struct AsciiComments;

/// Options for [`AsciiComments`].
#[derive(CopOptions)]
pub struct AsciiCommentsOptions {
    #[option(
        name = "AllowedChars",
        default = ["©"],
        description = "Non-ASCII characters that are explicitly permitted in comments."
    )]
    pub allowed_chars: Vec<String>,
}

#[cop(
    name = "Style/AsciiComments",
    description = "Use only ascii symbols in comments.",
    default_severity = "warning",
    default_enabled = false,
    options = AsciiCommentsOptions
)]
impl AsciiComments {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<AsciiCommentsOptions>();
        let source = cx.source();

        for &comment in cx.comments() {
            let comment_start = comment.range.start as usize;
            let comment_end = comment.range.end as usize;
            let comment_text = &source[comment_start..comment_end];

            // Build allowed char set.
            let allowed: Vec<char> = opts
                .allowed_chars
                .iter()
                .filter_map(|s| s.chars().next())
                .collect();

            // Check whether ALL non-ASCII chars in the comment are allowed.
            // If yes, skip entirely (mirrors RuboCop's only_allowed_non_ascii_chars?).
            let all_allowed = comment_text
                .chars()
                .filter(|c| !c.is_ascii())
                .all(|c| allowed.contains(&c));

            if all_allowed {
                continue;
            }

            // Find the first contiguous run of non-ASCII chars and flag it.
            // Mirrors RuboCop's first_offense_range / first_non_ascii_chars.
            if let Some(range) =
                first_non_ascii_run_range(comment_text, comment_start, &allowed)
            {
                cx.emit_offense(range, MSG, None);
            }
        }
    }
}

/// Find the byte range of the first contiguous run of non-ASCII characters
/// in `comment_text`, returning a `Range` in file byte coordinates.
///
/// Mirrors RuboCop's `first_non_ascii_chars` (`/[^[:ascii:]]+/.match`)
/// which returns the first non-ASCII run without filtering by AllowedChars.
/// The caller has already established that not all non-ASCII chars are
/// allowed, so the run is guaranteed to contain at least one disallowed
/// char — but we still return the whole first non-ASCII run, not just the
/// disallowed portion.
fn first_non_ascii_run_range(
    comment_text: &str,
    comment_byte_start: usize,
    _allowed: &[char],
) -> Option<Range> {
    let mut iter = comment_text.char_indices();

    // Scan to the first non-ASCII char.
    let (run_start_offset, first_ch) = iter.by_ref().find(|(_, c)| !c.is_ascii())?;

    let run_start_byte = comment_byte_start + run_start_offset;
    let mut run_end_byte = run_start_byte + first_ch.len_utf8();

    // Extend the run while consecutive chars are non-ASCII.
    for (offset, ch) in iter {
        if ch.is_ascii() {
            break;
        }
        run_end_byte = comment_byte_start + offset + ch.len_utf8();
    }

    Some(Range {
        start: run_start_byte as u32,
        end: run_end_byte as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::{AsciiComments, AsciiCommentsOptions};
    use murphy_plugin_api::test_support::{indoc, run_cop_with_options, test};

    fn with_allowed(chars: Vec<String>) -> AsciiCommentsOptions {
        AsciiCommentsOptions {
            allowed_chars: chars,
        }
    }

    // ── Positive cases ──────────────────────────────────────────────────

    #[test]
    fn flags_comment_with_cjk_chars() {
        // "# 这是什么？" — chars: #(0), space(1), 这(2), 是(3), 什(4), 么(5), ？(6)
        // First non-ASCII run starts at char index 2 (0-indexed), length 5 chars.
        test::<AsciiComments>().expect_offense(indoc! {"
            # 这是什么？
              ^^^^^ Use only ascii symbols in comments.
        "});
    }

    #[test]
    fn flags_comment_with_isolated_non_ascii() {
        // "# foo ∂ bar" — chars: #(0), space(1), f(2), o(3), o(4), space(5), ∂(6)
        // First non-ASCII run at char index 6, length 1.
        test::<AsciiComments>().expect_offense(indoc! {"
            # foo ∂ bar
                  ^ Use only ascii symbols in comments.
        "});
    }

    // ── Negative cases ───────────────────────────────────────────────────

    #[test]
    fn accepts_ascii_only_comment() {
        test::<AsciiComments>().expect_no_offenses("# AZaz1@$%~,;*_`|\n");
    }

    #[test]
    fn accepts_comment_with_allowed_copyright_symbol() {
        // Default AllowedChars includes "©", so this should be clean.
        test::<AsciiComments>().expect_no_offenses("# Copyright © 2024\n");
    }

    // ── AllowedChars option ──────────────────────────────────────────────

    #[test]
    fn accepts_comment_with_custom_allowed_char() {
        test::<AsciiComments>()
            .with_options(&with_allowed(vec!["∂".to_string()]))
            .expect_no_offenses("# foo ∂ bar\n");
    }

    #[test]
    fn flags_comment_with_non_allowed_char_when_custom_allowed() {
        // AllowedChars = ["∂"] — CJK chars are still not allowed.
        // "# 这是什么？" — first run at char index 2, length 5.
        test::<AsciiComments>()
            .with_options(&with_allowed(vec!["∂".to_string()]))
            .expect_offense(indoc! {"
                # 这是什么？
                  ^^^^^ Use only ascii symbols in comments.
            "});
    }

    #[test]
    fn flags_first_run_even_when_run_starts_with_allowed_char() {
        // AllowedChars = ["©"] (default). Comment "# © 日本語":
        // chars: #(0), space(1), ©(2), space(3), 日(4), 本(5), 語(6)
        // Not all non-ASCII are allowed (日本語 are not).
        // First non-ASCII run is "©" at char index 2, length 1.
        // Per RuboCop semantics, the first run is flagged regardless of
        // whether it contains only-allowed chars — the overall comment
        // has disallowed chars so it is not exempt.
        let offenses = run_cop_with_options::<AsciiComments>(
            "# © 日本語\n",
            &AsciiCommentsOptions::default(),
        );
        assert_eq!(offenses.len(), 1);
        // © is U+00A9, 2 bytes; "# " = 2 bytes; so start=2, end=4
        assert_eq!(offenses[0].range.start, 2);
        assert_eq!(offenses[0].range.end, 4);
        assert_eq!(offenses[0].message, "Use only ascii symbols in comments.");
    }

    #[test]
    fn accepts_comment_with_only_allowed_chars() {
        // When ALL non-ASCII chars in the comment are in AllowedChars, skip.
        test::<AsciiComments>()
            .with_options(&with_allowed(vec!["©".to_string()]))
            .expect_no_offenses("# Copyright © 2024\n");
    }

    #[test]
    fn flags_non_ascii_in_block_comment() {
        // =begin/=end block comments should also be checked.
        let offenses = run_cop_with_options::<AsciiComments>(
            "=begin\n这是什么\n=end\n",
            &AsciiCommentsOptions::default(),
        );
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].message, "Use only ascii symbols in comments.");
    }
}

murphy_plugin_api::submit_cop!(AsciiComments);
