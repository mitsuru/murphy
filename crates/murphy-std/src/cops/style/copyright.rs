//! `Style/Copyright` — requires a copyright notice in each source file.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/Copyright
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: [murphy-h8t9]
//! notes: >
//!   `Notice` is an (unanchored) regex, matching RuboCop. The leading comment
//!   block — every comment before the first code token (RuboCop's
//!   `notice_found?` iterates `processed_source.tokens` and `break`s at the
//!   first non-comment) — is checked: each comment's `# ` prefix is stripped
//!   and the texts are joined with `\n` into a `multiline_notice`, which is
//!   matched against the notice regexp (the raw comment text is also matched,
//!   mirroring upstream's per-token check). `notice_regexp` strips a leading
//!   `\A#`/`^#` from the configured pattern (RuboCop's
//!   `notice.sub(/\A(?:\\A|\^)?#(?:\\s[*+?]?|\s)*/, '')`). The regex is
//!   compiled with `multi_line(true)` so Ruby-style `^`/`$` line anchors match
//!   each line of the joined notice.
//!
//!   Gaps vs upstream:
//!   - Autocorrect (inserting `AutocorrectNotice` before the first code token,
//!     after any shebang / encoding comment) is not implemented — murphy-h8t9.
//!     RuboCop also raises a config Warning when `AutocorrectNotice` is missing
//!     or does not itself match `Notice`; Murphy does not model that warning.
//!   - The pattern is matched by Rust's `regex` engine, which has no lookaround
//!     or backreferences. A `Notice` that relies on Ruby-only regex features is
//!     treated as non-matching (the file is reported as missing a notice)
//!     rather than raising — an invalid/unsupported pattern never silently
//!     passes a file.
//! ```

use murphy_plugin_api::{CopOptions, Cx, Range, cop};

#[derive(Default)]
pub struct Copyright;

#[derive(CopOptions)]
pub struct CopyrightOptions {
    #[option(
        name = "Notice",
        default = "",
        description = "Unanchored regexp; a comment in the leading block must match it (e.g. '^Copyright (\\(c\\) )?2\\d{3} Acme Inc')."
    )]
    pub notice: String,
}

#[cop(
    name = "Style/Copyright",
    description = "Require a copyright notice in each source file.",
    default_severity = "warning",
    default_enabled = false,
    options = CopyrightOptions
)]
impl Copyright {
    #[on_new_investigation]
    fn check_investigation(&self, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<CopyrightOptions>();
        if opts.notice.is_empty() {
            return;
        }
        let source = cx.source();
        if source.trim().is_empty() {
            return;
        }

        // `notice_regexp`: strip a leading `\A#`/`^#` (plus following `\s`) from
        // the configured pattern, then compile multi-line so Ruby's `^`/`$`
        // line anchors match per line of the joined notice. A pattern Rust's
        // engine cannot compile (e.g. Ruby-only lookaround) is treated as
        // non-matching, so the file is reported rather than silently accepted.
        let pattern = strip_notice_anchor(&opts.notice);
        let Ok(re) = murphy_plugin_api::regex::RegexBuilder::new(pattern)
            .multi_line(true)
            .build()
        else {
            cx.emit_offense(first_line_range(source), &message(&opts.notice), None);
            return;
        };

        if notice_found(&re, cx) {
            return;
        }
        cx.emit_offense(first_line_range(source), &message(&opts.notice), None);
    }
}

/// RuboCop's `notice_found?`: scan the leading comment block (every comment
/// before the first code token), match each raw comment text and the joined
/// `# `-stripped `multiline_notice` against the notice regexp.
fn notice_found(re: &murphy_plugin_api::regex::Regex, cx: &Cx<'_>) -> bool {
    let first_code = first_code_offset(cx);
    let mut multiline_notice = String::new();
    for comment in cx.comments() {
        // Only the leading comment block (comments before any code) counts —
        // RuboCop `break`s at the first non-comment token.
        if first_code.is_some_and(|code| comment.range.start >= code) {
            break;
        }
        let text = cx.raw_source(comment.range);
        multiline_notice.push_str(strip_comment_marker(text));
        multiline_notice.push('\n');
        if re.is_match(text) {
            return true;
        }
    }
    re.is_match(&multiline_notice)
}

/// Byte offset of the first non-comment, non-newline token (the first "code").
/// Comments and newlines before it form the leading comment block.
fn first_code_offset(cx: &Cx<'_>) -> Option<u32> {
    use murphy_plugin_api::SourceTokenKind;
    cx.sorted_tokens()
        .iter()
        .find(|t| {
            !matches!(
                t.kind,
                SourceTokenKind::Comment
                    | SourceTokenKind::Newline
                    | SourceTokenKind::IgnoredNewline
            )
        })
        .map(|t| t.range.start)
}

/// Strip a leading `# ` (RuboCop's `token.text.sub(/\A# */, '')`).
fn strip_comment_marker(text: &str) -> &str {
    text.strip_prefix('#')
        .map_or(text, |rest| rest.trim_start_matches(' '))
}

/// RuboCop's `notice.sub(/\A(?:\\A|\^)?#(?:\\s[*+?]?|\s)*/, '')`: drop an
/// optional `\A`/`^` anchor, then a literal `#`, then any run of literal or
/// `\s` whitespace markers at the very start of the pattern. Only applied when
/// the pattern actually begins with that `[anchor]#` shape.
fn strip_notice_anchor(notice: &str) -> &str {
    let after_anchor = notice
        .strip_prefix("\\A")
        .or_else(|| notice.strip_prefix('^'))
        .unwrap_or(notice);
    let Some(mut rest) = after_anchor.strip_prefix('#') else {
        // No `#` after the (optional) anchor → the sub does not match; the
        // pattern is used verbatim.
        return notice;
    };
    // Consume `(?:\\s[*+?]?|\s)*` — literal whitespace or `\s` (optionally
    // quantified) whitespace markers.
    loop {
        if let Some(next) = rest.strip_prefix("\\s") {
            rest = next.strip_prefix(['*', '+', '?']).unwrap_or(next);
        } else if let Some(next) = rest.strip_prefix([' ', '\t']) {
            rest = next;
        } else {
            break;
        }
    }
    rest
}

fn message(notice: &str) -> String {
    format!("Include a copyright notice matching /{notice}/ before any code.")
}

fn first_line_range(source: &str) -> Range {
    let end = source.find('\n').unwrap_or(source.len());
    Range { start: 0, end: end as u32 }
}

#[cfg(test)]
mod tests {
    use super::{Copyright, CopyrightOptions, strip_notice_anchor};
    use murphy_plugin_api::test_support::{indoc, test};

    fn opts(notice: &str) -> CopyrightOptions {
        CopyrightOptions {
            notice: notice.to_string(),
        }
    }

    #[test]
    fn flags_missing_copyright() {
        test::<Copyright>()
            .with_options(&opts("Copyright"))
            .expect_offense(indoc! {"
                x = 1
                ^^^^^ Include a copyright notice matching /Copyright/ before any code.
            "});
    }

    #[test]
    fn accepts_copyright_present() {
        test::<Copyright>()
            .with_options(&opts("Copyright"))
            .expect_no_offenses("# Copyright (c) 2024 Acme Inc\nx = 1\n");
    }

    #[test]
    fn empty_notice_does_nothing() {
        test::<Copyright>()
            .with_options(&opts(""))
            .expect_no_offenses("x = 1\n");
    }

    // The default RuboCop Notice is a line-anchored regex. With `# `-stripping
    // and `multi_line(true)`, `^Copyright` matches `# Copyright …`; a naive
    // substring/raw-text match would never satisfy the `^` anchor and would
    // falsely flag every correctly-licensed file.
    #[test]
    fn accepts_anchored_regex_notice() {
        test::<Copyright>()
            .with_options(&opts(r"^Copyright (\(c\) )?2\d{3} Acme Inc"))
            .expect_no_offenses("# Copyright (c) 2024 Acme Inc\nx = 1\n");
    }

    #[test]
    fn flags_when_anchored_regex_does_not_match() {
        // RuboCop reports the offense on the file's first line (`source_range
        // (buffer, 1, 0)`), regardless of whether that line is a comment.
        test::<Copyright>()
            .with_options(&opts(r"^Copyright \d{4} Acme"))
            .expect_offense(indoc! {r"
                # Some unrelated header
                ^^^^^^^^^^^^^^^^^^^^^^^ Include a copyright notice matching /^Copyright \d{4} Acme/ before any code.
                x = 1
            "});
    }

    // RuboCop only inspects the *leading* comment block — it breaks at the
    // first non-comment token. A notice after code does not count.
    #[test]
    fn flags_notice_after_code() {
        test::<Copyright>()
            .with_options(&opts("Copyright"))
            .expect_offense(indoc! {"
                x = 1
                ^^^^^ Include a copyright notice matching /Copyright/ before any code.
                # Copyright 2024 Acme
            "});
    }

    // The leading block spans blank lines and intervening comments (parser's
    // token stream has no blank-line break), so a notice on a later comment
    // line is still found.
    #[test]
    fn accepts_notice_in_later_leading_comment() {
        test::<Copyright>()
            .with_options(&opts(r"^Copyright \d{4}"))
            .expect_no_offenses("# Some header\n\n# Copyright 2024 Acme\nx = 1\n");
    }

    // A multi-line copyright spread across consecutive comments matches the
    // joined `multiline_notice` even when no single line matches alone.
    #[test]
    fn accepts_multiline_notice() {
        test::<Copyright>()
            .with_options(&opts(r"^Copyright\n.*Acme"))
            .expect_no_offenses("# Copyright\n# 2024 Acme Inc\nx = 1\n");
    }

    // A pattern Rust's regex engine cannot compile (Ruby-only lookbehind) must
    // not silently pass the file — it is reported as missing a notice.
    #[test]
    fn unsupported_regex_reports_offense() {
        test::<Copyright>()
            .with_options(&opts(r"(?<=Copyright )Acme"))
            .expect_offense(indoc! {r"
                # Copyright Acme
                ^^^^^^^^^^^^^^^^ Include a copyright notice matching /(?<=Copyright )Acme/ before any code.
                x = 1
            "});
    }

    #[test]
    fn notice_anchor_strip_matches_rubocop() {
        // `\A#` / `^#` with trailing whitespace markers are stripped.
        assert_eq!(strip_notice_anchor(r"\A# Copyright"), "Copyright");
        assert_eq!(strip_notice_anchor("^# Copyright"), "Copyright");
        assert_eq!(strip_notice_anchor(r"^#\s*Copyright"), "Copyright");
        // No `#` after the anchor → pattern is used verbatim.
        assert_eq!(strip_notice_anchor(r"^Copyright"), r"^Copyright");
        assert_eq!(strip_notice_anchor("Copyright"), "Copyright");
    }
}
murphy_plugin_api::submit_cop!(Copyright);
