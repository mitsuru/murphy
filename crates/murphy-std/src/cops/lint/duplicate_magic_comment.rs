//! `Lint/DuplicateMagicComment` — flags a repeated magic comment of the same
//! kind (`encoding` or `frozen_string_literal`) in a file's leading comment
//! block.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/DuplicateMagicComment
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Ported from RuboCop Lint/DuplicateMagicComment. Buckets leading magic
//!   comments into `encoding` (encoding/coding) and `frozen_string_literal`
//!   categories by their key only (the value is ignored, so
//!   `frozen_string_literal: true` and `: TRUE` collide, matching RuboCop).
//!   Within each bucket every occurrence after the first is flagged on its
//!   whole line, and the autocorrect removes that whole line including the
//!   trailing newline. The leading comment block follows RuboCop's
//!   `leading_comment_lines`: every line before the first non-comment token,
//!   so a shebang is skipped and blank lines are transparent (they do not end
//!   the block). Only `encoding` and `frozen_string_literal` are categorised —
//!   `shareable_constant_value`, `rbs_inline`, and `typed` are intentionally
//!   out of scope (that is `Lint/OrderedMagicComments`' concern), matching
//!   RuboCop's `magic_comment_lines`. Known divergence: an Emacs-style comment
//!   declaring multiple directives is bucketed by the first directive in text
//!   order, whereas RuboCop prefers an encoding directive if present; a
//!   `# -*- frozen_string_literal: …; encoding: … -*-` therefore buckets
//!   differently. This edge is not exercised by RuboCop's specs.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # frozen_string_literal: true
//! # frozen_string_literal: true   # offense on the second line
//! ```
//!
//! ## Autocorrect
//!
//! Removes each duplicate comment line (the whole line including its trailing
//! newline), keeping the first occurrence of each kind.

use murphy_plugin_api::{Cx, NoOptions, Range, cop};

#[derive(Default)]
pub struct DuplicateMagicComment;

const MSG: &str = "Duplicate magic comment detected.";

#[cop(
    name = "Lint/DuplicateMagicComment",
    description = "Checks for duplicated magic comments.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl DuplicateMagicComment {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let src = cx.source().as_bytes();
        if src.is_empty() {
            return;
        }

        let region_end = leading_comment_region_end(src);

        // Track, per kind, whether we've already seen a magic comment of that
        // kind. The first occurrence is kept; every later one is flagged.
        let mut seen_encoding = false;
        let mut seen_frozen = false;

        // `cx.comments()` is sorted by source position, so iterating it gives
        // first-to-last order within each bucket.
        for comment in cx.comments() {
            if comment.range.start as usize >= region_end {
                break;
            }
            // Only own-line comments participate (a trailing comment on a code
            // line is not a leading magic comment).
            if !is_own_line_comment(src, comment.range.start as usize) {
                continue;
            }

            let bytes = cx.raw_source(comment.range).as_bytes();
            let already_seen = match classify_comment(bytes) {
                MagicKind::Encoding => std::mem::replace(&mut seen_encoding, true),
                MagicKind::Frozen => std::mem::replace(&mut seen_frozen, true),
                MagicKind::NotMagic => continue,
            };
            if !already_seen {
                continue;
            }

            // Duplicate — offense on the whole line, autocorrect removes the
            // whole line including its trailing newline.
            let line_no_nl = source_line_range_without_newline(src, comment.range.start as usize);
            cx.emit_offense(line_no_nl, MSG, None);

            let line_with_nl = source_line_range_with_newline(src, comment.range.start as usize);
            cx.emit_edit(line_with_nl, "");
        }
    }
}

/// The two magic-comment kinds this cop tracks, plus the "not magic" sentinel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MagicKind {
    Encoding,
    Frozen,
    NotMagic,
}

/// Classify a comment byte-slice (starting with `#`) as an encoding magic
/// comment, a frozen-string-literal magic comment, or not magic. Handles both
/// simple (`# key: value`) and Emacs-style (`# -*- key: value -*-`) forms.
fn classify_comment(bytes: &[u8]) -> MagicKind {
    if bytes.first() != Some(&b'#') {
        return MagicKind::NotMagic;
    }

    // Emacs-style `# -*- key: value -*-`, possibly several `;`-separated
    // directives. RuboCop prefers an encoding directive when present; we take
    // the first directive in text order (documented divergence).
    if let Some(inner) = strip_emacs_markers(bytes) {
        for part in inner.split(|&b| b == b';') {
            match classify_simple_directive(part) {
                MagicKind::NotMagic => continue,
                kind => return kind,
            }
        }
        return MagicKind::NotMagic;
    }

    classify_simple_directive(bytes)
}

/// Classify a single `# key: value` (or a `key: value` part from an Emacs
/// comment). The leading `#`, if present, is skipped.
fn classify_simple_directive(bytes: &[u8]) -> MagicKind {
    // Skip a leading `#` (simple form) — Emacs parts have none.
    let mut key_start = if bytes.first() == Some(&b'#') { 1 } else { 0 };
    while key_start < bytes.len() && bytes[key_start].is_ascii_whitespace() {
        key_start += 1;
    }
    if key_start >= bytes.len() {
        return MagicKind::NotMagic;
    }

    let mut key_end = key_start;
    while key_end < bytes.len()
        && (bytes[key_end].is_ascii_alphanumeric()
            || bytes[key_end] == b'_'
            || bytes[key_end] == b'-')
    {
        key_end += 1;
    }
    if key_start == key_end {
        return MagicKind::NotMagic;
    }

    // A separator (`:` or `=`) must follow the key (after optional spaces).
    let mut sep = key_end;
    while sep < bytes.len() && bytes[sep].is_ascii_whitespace() {
        sep += 1;
    }
    if !matches!(bytes.get(sep), Some(b':' | b'=')) {
        return MagicKind::NotMagic;
    }

    magic_kind(&bytes[key_start..key_end])
}

/// Recognise the two magic-comment keywords this cop tracks. Mirrors
/// RuboCop's `MagicComment::KEYWORDS` for `encoding` and
/// `frozen_string_literal` (dashes and underscores are interchangeable, the
/// match is case-insensitive).
fn magic_kind(key: &[u8]) -> MagicKind {
    fn eq_normalized(key: &[u8], expected: &[u8]) -> bool {
        key.len() == expected.len()
            && key.iter().zip(expected.iter()).all(|(&k, &e)| {
                let k = if k == b'-' { b'_' } else { k.to_ascii_lowercase() };
                k == e
            })
    }

    if eq_normalized(key, b"encoding") || eq_normalized(key, b"coding") {
        MagicKind::Encoding
    } else if eq_normalized(key, b"frozen_string_literal") {
        MagicKind::Frozen
    } else {
        MagicKind::NotMagic
    }
}

/// Strip `-*- ... -*-` markers from an Emacs-style comment, returning the
/// inner content, or `None` if the comment is not Emacs-style.
fn strip_emacs_markers(bytes: &[u8]) -> Option<&[u8]> {
    let start = bytes.windows(3).position(|w| w == b"-*-")?;
    let after_start = start + 3;
    let end = bytes[after_start..].windows(3).position(|w| w == b"-*-")?;
    Some(&bytes[after_start..after_start + end])
}

/// End of the leading comment region: the byte offset of the first line that
/// is neither a shebang (line 0), a comment line, nor a blank line. Mirrors
/// RuboCop's `leading_comment_lines` (everything before the first non-comment
/// token) — blank lines are transparent and do not end the region.
fn leading_comment_region_end(source: &[u8]) -> usize {
    let mut line_start = 0;
    while line_start < source.len() {
        let line_end = source[line_start..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(source.len(), |pos| line_start + pos);
        let mut content_end = line_end;
        if content_end > line_start && source[content_end - 1] == b'\r' {
            content_end -= 1;
        }

        // Skip a shebang on line 0.
        if line_start == 0 && source.starts_with(b"#!") {
            line_start = line_end.saturating_add(1);
            continue;
        }

        // First non-whitespace byte on the line.
        let mut first = line_start;
        while first < content_end && source[first].is_ascii_whitespace() {
            first += 1;
        }
        // Comment line or blank line → still inside the leading region.
        if first >= content_end || source[first] == b'#' {
            line_start = line_end.saturating_add(1);
            continue;
        }
        // Code line → region ends here.
        return line_start;
    }
    source.len()
}

/// Whether the comment at `comment_start` is an own-line comment (only
/// whitespace precedes the `#` on its line).
fn is_own_line_comment(source: &[u8], comment_start: usize) -> bool {
    let line_start = source[..comment_start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |pos| pos + 1);
    source[line_start..comment_start]
        .iter()
        .all(|byte| byte.is_ascii_whitespace())
}

/// The source range of the line containing `offset`, excluding the trailing
/// newline.
fn source_line_range_without_newline(source: &[u8], offset: usize) -> Range {
    let start = source[..offset]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |pos| pos + 1);
    let mut end = source[offset..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(source.len(), |pos| offset + pos);
    if end > start && source[end - 1] == b'\r' {
        end -= 1;
    }
    Range {
        start: start as u32,
        end: end as u32,
    }
}

/// The source range of the line containing `offset`, including the trailing
/// newline (so removing it deletes the whole physical line).
fn source_line_range_with_newline(source: &[u8], offset: usize) -> Range {
    let start = source[..offset]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |pos| pos + 1);
    let end = source[offset..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(source.len(), |pos| offset + pos + 1);
    Range {
        start: start as u32,
        end: end as u32,
    }
}

murphy_plugin_api::submit_cop!(DuplicateMagicComment);

#[cfg(test)]
mod tests {
    use super::DuplicateMagicComment;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- offenses ---

    #[test]
    fn flags_duplicate_frozen_string_literal() {
        test::<DuplicateMagicComment>().expect_offense(indoc! {r#"
            # frozen_string_literal: true
            # frozen_string_literal: true
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Duplicate magic comment detected.
        "#});
    }

    #[test]
    fn flags_duplicate_frozen_case_insensitive_value() {
        test::<DuplicateMagicComment>().expect_offense(indoc! {r#"
            # frozen_string_literal: true
            # frozen_string_literal: TRUE
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Duplicate magic comment detected.
        "#});
    }

    #[test]
    fn flags_duplicate_encoding() {
        test::<DuplicateMagicComment>().expect_offense(indoc! {r#"
            # encoding: ascii
            # encoding: ascii
            ^^^^^^^^^^^^^^^^^ Duplicate magic comment detected.
        "#});
    }

    #[test]
    fn flags_duplicate_encoding_different_value() {
        test::<DuplicateMagicComment>().expect_offense(indoc! {r#"
            # encoding: ascii
            # encoding: utf-8
            ^^^^^^^^^^^^^^^^^ Duplicate magic comment detected.
        "#});
    }

    #[test]
    fn flags_both_kinds_duplicated() {
        test::<DuplicateMagicComment>().expect_offense(indoc! {r#"
            # encoding: ascii
            # frozen_string_literal: true
            # encoding: ascii
            ^^^^^^^^^^^^^^^^^ Duplicate magic comment detected.
            # frozen_string_literal: true
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Duplicate magic comment detected.
        "#});
    }

    #[test]
    fn flags_duplicate_separated_by_blank_line() {
        // Blank lines are transparent in the leading region, so both frozen
        // comments are categorised and the second is flagged.
        test::<DuplicateMagicComment>().expect_offense(indoc! {r#"
            # frozen_string_literal: true

            # frozen_string_literal: true
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Duplicate magic comment detected.
        "#});
    }

    // --- autocorrect ---

    #[test]
    fn autocorrects_duplicate_frozen() {
        test::<DuplicateMagicComment>().expect_correction(
            indoc! {r#"
                # frozen_string_literal: true
                # frozen_string_literal: true
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Duplicate magic comment detected.
            "#},
            "# frozen_string_literal: true\n",
        );
    }

    #[test]
    fn autocorrects_three_duplicates_to_one() {
        // Two non-overlapping line removals reach fixpoint, leaving one.
        test::<DuplicateMagicComment>().expect_correction(
            indoc! {r#"
                # frozen_string_literal: true
                # frozen_string_literal: true
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Duplicate magic comment detected.
                # frozen_string_literal: true
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Duplicate magic comment detected.
            "#},
            "# frozen_string_literal: true\n",
        );
    }

    #[test]
    fn autocorrects_both_kinds() {
        test::<DuplicateMagicComment>().expect_correction(
            indoc! {r#"
                # encoding: ascii
                # frozen_string_literal: true
                # encoding: ascii
                ^^^^^^^^^^^^^^^^^ Duplicate magic comment detected.
                # frozen_string_literal: true
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Duplicate magic comment detected.
            "#},
            "# encoding: ascii\n# frozen_string_literal: true\n",
        );
    }

    // --- no offenses ---

    #[test]
    fn accepts_single_frozen() {
        test::<DuplicateMagicComment>().expect_no_offenses("# frozen_string_literal: true\n");
    }

    #[test]
    fn accepts_single_encoding() {
        test::<DuplicateMagicComment>().expect_no_offenses("# encoding: ascii\n");
    }

    #[test]
    fn accepts_distinct_kinds() {
        test::<DuplicateMagicComment>()
            .expect_no_offenses("# encoding: ascii\n# frozen_string_literal: true\n");
    }

    #[test]
    fn accepts_empty_file() {
        test::<DuplicateMagicComment>().expect_no_offenses("");
    }

    #[test]
    fn ignores_duplicate_after_code() {
        // The second frozen comment is below code, so it is outside the
        // leading region and not categorised.
        test::<DuplicateMagicComment>().expect_no_offenses(indoc! {r#"
            # frozen_string_literal: true
            x = 1
            # frozen_string_literal: true
        "#});
    }

    #[test]
    fn ignores_non_magic_duplicate_comments() {
        test::<DuplicateMagicComment>()
            .expect_no_offenses("# just a comment\n# just a comment\n");
    }

    #[test]
    fn flags_duplicate_frozen_after_shebang() {
        test::<DuplicateMagicComment>().expect_offense(indoc! {r#"
            #!/usr/bin/env ruby
            # frozen_string_literal: true
            # frozen_string_literal: true
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Duplicate magic comment detected.
        "#});
    }
}
