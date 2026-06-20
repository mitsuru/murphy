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
//!   Autocorrect (murphy-h8t9): inserts `normalized_autocorrect_notice + "\n"`
//!   before the first code token, after a leading shebang (`\A#!`) and/or an
//!   encoding magic comment (`\A#.*coding\s?[:=]\s?(?:UTF|utf)-8`), mirroring
//!   `insert_notice_before`. `normalized_autocorrect_notice` prefixes each
//!   `AutocorrectNotice` line that lacks a leading `#` with `# ` (blank lines
//!   become `#`). The fix is withheld (offense still reported) when
//!   `AutocorrectNotice` is empty or `normalized.gsub(/^# */, '')` does not
//!   match `Notice` — this is RuboCop's `verify_autocorrect_notice!` guard,
//!   and the guard is also load-bearing for idempotency (a non-matching notice
//!   would re-insert on every pass).
//!
//!   Gaps vs upstream:
//!   - RuboCop *raises* a config `Warning` (aborting the run) when
//!     `AutocorrectNotice` is empty or does not match `Notice`. Murphy has no
//!     cop-facing config-warning surface (`cx.warn()` does not exist — see
//!     `crates/murphy-plugin-api/src/cx.rs`), so the warning is not surfaced;
//!     Murphy instead withholds the autocorrect (the offense is still
//!     reported). Closing this requires an ABI addition and must not bypass the
//!     single-surface boundary — tracked as the remaining murphy-h8t9 gap.
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
    #[option(
        name = "AutocorrectNotice",
        default = "",
        description = "Literal notice text inserted before the first code token by autocorrect (e.g. '# Copyright (c) 2024 Acme Inc')."
    )]
    pub autocorrect_notice: String,
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
        emit_autocorrect(&opts, &re, cx);
    }
}

/// RuboCop's `autocorrect` + `verify_autocorrect_notice!`. Inserts
/// `normalized_autocorrect_notice + "\n"` before the first code token (after a
/// shebang / encoding magic comment). RuboCop *raises a Warning* — instead of
/// correcting — when `AutocorrectNotice` is empty or its normalized form does
/// not itself match `Notice`. Murphy has no config-warning surface, so it
/// withholds the fix in those cases (re-running would otherwise insert the
/// notice forever). The offense is still reported by the caller.
fn emit_autocorrect(
    opts: &CopyrightOptions,
    notice_re: &murphy_plugin_api::regex::Regex,
    cx: &Cx<'_>,
) {
    if opts.autocorrect_notice.is_empty() {
        return;
    }
    let normalized = normalized_autocorrect_notice(&opts.autocorrect_notice);
    // `normalized_autocorrect_notice.gsub(/^# */, '').match?(notice_regexp)` —
    // the idempotency guard. Withhold the fix if it would not satisfy `Notice`.
    if !notice_re.is_match(&strip_comment_prefixes(&normalized)) {
        return;
    }
    let insert_at = insert_offset(cx);
    cx.emit_edit(
        Range {
            start: insert_at,
            end: insert_at,
        },
        &format!("{normalized}\n"),
    );
}

/// RuboCop's `normalized_autocorrect_notice`: for each line of the configured
/// notice — keep it if it already starts with `#`, become `#` if blank,
/// otherwise prefix `# `. Lines are rejoined preserving their `\n` separators.
fn normalized_autocorrect_notice(notice: &str) -> String {
    let mut out = String::with_capacity(notice.len() + 8);
    // `String#lines` keeps trailing newlines and yields no final empty element,
    // matching Ruby's `String#lines`.
    let mut rest = notice;
    while !rest.is_empty() {
        let (line, tail) = match rest.find('\n') {
            Some(idx) => (&rest[..=idx], &rest[idx + 1..]),
            None => (rest, ""),
        };
        let (content, nl) = match line.strip_suffix('\n') {
            Some(c) => (c, "\n"),
            None => (line, ""),
        };
        if content.starts_with('#') {
            out.push_str(content);
        } else if content.is_empty() {
            out.push('#');
        } else {
            out.push_str("# ");
            out.push_str(content);
        }
        out.push_str(nl);
        rest = tail;
    }
    out
}

/// RuboCop's `gsub(/^# */, '')` over the normalized notice — strip a leading
/// `#` plus following spaces from every line so the bare text is matched
/// against `Notice`.
fn strip_comment_prefixes(normalized: &str) -> String {
    normalized
        .lines()
        .map(strip_comment_marker)
        .collect::<Vec<_>>()
        .join("\n")
}

/// RuboCop's `insert_notice_before`: starting at token index 0, skip a leading
/// shebang token, then an encoding magic-comment token, and return the byte
/// offset of the next token's start (the notice is inserted before it).
///
/// RuboCop indexes into the full token stream. Murphy's stream carries
/// `Newline`/`IgnoredNewline` tokens that the parser-gem stream does not, so
/// those are skipped to align the index with RuboCop's.
fn insert_offset(cx: &Cx<'_>) -> u32 {
    use murphy_plugin_api::SourceTokenKind;
    let significant: Vec<_> = cx
        .sorted_tokens()
        .iter()
        .filter(|t| {
            !matches!(
                t.kind,
                SourceTokenKind::Newline | SourceTokenKind::IgnoredNewline
            )
        })
        .copied()
        .collect();
    let is_comment = |t: &murphy_plugin_api::SourceToken| t.kind == SourceTokenKind::Comment;
    let mut idx = 0usize;
    if significant
        .first()
        .is_some_and(|t| is_comment(t) && is_shebang(cx.raw_source(t.range)))
    {
        idx += 1;
    }
    if significant
        .get(idx)
        .is_some_and(|t| is_comment(t) && is_encoding_comment(cx.raw_source(t.range)))
    {
        idx += 1;
    }
    match significant.get(idx) {
        Some(t) => t.range.start,
        None => 0,
    }
}

/// `/\A#!.*\z/` — a shebang comment.
fn is_shebang(text: &str) -> bool {
    text.starts_with("#!")
}

/// `/\A#.*coding\s?[:=]\s?(?:UTF|utf)-8/` — an encoding magic comment.
fn is_encoding_comment(text: &str) -> bool {
    let Some(after_hash) = text.strip_prefix('#') else {
        return false;
    };
    let Some(coding_pos) = after_hash.find("coding") else {
        return false;
    };
    let after_coding = &after_hash[coding_pos + "coding".len()..];
    // `coding\s?[:=]\s?(?:UTF|utf)-8` — at most one space, a `:`/`=`, at most
    // one space, then `UTF-8`/`utf-8`.
    let after_coding = after_coding.strip_prefix(' ').unwrap_or(after_coding);
    let Some(after_sep) = after_coding
        .strip_prefix(':')
        .or_else(|| after_coding.strip_prefix('='))
    else {
        return false;
    };
    let after_sep = after_sep.strip_prefix(' ').unwrap_or(after_sep);
    after_sep.starts_with("UTF-8") || after_sep.starts_with("utf-8")
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
            autocorrect_notice: String::new(),
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

    fn opts2(notice: &str, autocorrect: &str) -> CopyrightOptions {
        CopyrightOptions {
            notice: notice.to_string(),
            autocorrect_notice: autocorrect.to_string(),
        }
    }

    // RuboCop inserts `AutocorrectNotice + "\n"` (normalized to `# `-prefixed
    // lines) before the first code token when the notice is missing.
    #[test]
    fn autocorrects_missing_notice() {
        test::<Copyright>()
            .with_options(&opts2("Copyright", "# Copyright Acme"))
            .expect_correction(
                indoc! {"
                    x = 1
                    ^^^^^ Include a copyright notice matching /Copyright/ before any code.
                "},
                "# Copyright Acme\nx = 1\n",
            );
    }

    // `normalized_autocorrect_notice`: a line without a leading `#` is
    // prefixed with `# `.
    #[test]
    fn autocorrect_normalizes_unprefixed_notice() {
        test::<Copyright>()
            .with_options(&opts2("Copyright", "Copyright Acme"))
            .expect_correction(
                indoc! {"
                    x = 1
                    ^^^^^ Include a copyright notice matching /Copyright/ before any code.
                "},
                "# Copyright Acme\nx = 1\n",
            );
    }

    // The notice is inserted after a shebang line.
    #[test]
    fn autocorrect_after_shebang() {
        test::<Copyright>()
            .with_options(&opts2("Copyright", "# Copyright Acme"))
            .expect_correction(
                indoc! {"
                    #!/usr/bin/env ruby
                    ^^^^^^^^^^^^^^^^^^^ Include a copyright notice matching /Copyright/ before any code.
                    x = 1
                "},
                "#!/usr/bin/env ruby\n# Copyright Acme\nx = 1\n",
            );
    }

    // The notice is inserted after a shebang AND an encoding magic comment.
    #[test]
    fn autocorrect_after_shebang_and_encoding() {
        test::<Copyright>()
            .with_options(&opts2("Copyright", "# Copyright Acme"))
            .expect_correction(
                indoc! {"
                    #!/usr/bin/env ruby
                    ^^^^^^^^^^^^^^^^^^^ Include a copyright notice matching /Copyright/ before any code.
                    # encoding: utf-8
                    x = 1
                "},
                "#!/usr/bin/env ruby\n# encoding: utf-8\n# Copyright Acme\nx = 1\n",
            );
    }

    // Idempotency guard: when `AutocorrectNotice` does not itself match the
    // `Notice` regex, RuboCop raises a config Warning rather than correcting.
    // Murphy has no warning surface, so it reports the offense but withholds
    // the fix (re-running would otherwise insert forever).
    #[test]
    fn no_autocorrect_when_notice_does_not_match() {
        test::<Copyright>()
            .with_options(&opts2("Copyright", "# Wrong Header"))
            .expect_offense(indoc! {"
                x = 1
                ^^^^^ Include a copyright notice matching /Copyright/ before any code.
            "})
            .expect_no_corrections("x = 1\n");
    }

    // Idempotency guard: an empty `AutocorrectNotice` withholds the fix
    // (RuboCop raises the empty-notice Warning).
    #[test]
    fn no_autocorrect_when_autocorrect_notice_empty() {
        test::<Copyright>()
            .with_options(&opts2("Copyright", ""))
            .expect_offense(indoc! {"
                x = 1
                ^^^^^ Include a copyright notice matching /Copyright/ before any code.
            "})
            .expect_no_corrections("x = 1\n");
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
