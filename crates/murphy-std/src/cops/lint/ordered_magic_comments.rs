//! `Lint/OrderedMagicComments` — checks that `encoding` magic comments precede
//! all other magic comments.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/OrderedMagicComments
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Ported from RuboCop Lint/OrderedMagicComments.  Checks that encoding
//!   magic comments (encoding/coding) appear before frozen_string_literal,
//!   shareable_constant_value, rbs_inline, and typed magic comments in the
//!   file's leading comment block.  Shebangs are skipped.  Emacs-style
//!   comments (`# -*- key: value -*-`) are also supported.  Autocorrect
//!   swaps the two offending lines.
//! ```
//!
//! ## Matched shapes
//!
//! An encoding magic comment (`# encoding: ...`, `# coding: ...`, or
//! `# -*- encoding: ... -*-`) that appears after a non-encoding magic
//! comment (`# frozen_string_literal:`, `# shareable_constant_value:`,
//! `# rbs_inline:`, or `# typed:`) in the leading comment block.
//!
//! ## Autocorrect
//!
//! Swaps the encoding magic comment line with the first non-encoding
//! magic comment line that precedes it.  This is the same swap
//! autocorrection that RuboCop applies.
//!
//! ## Known v1 limitation: no per-cop `Include`/`Exclude` patterns
//!
//! See `RSpec/DescribeClass` for the canonical wording.  The cop fires on
//! all files, not just `.rb` files (there is no per-cop file-pattern
//! gating yet).

use murphy_plugin_api::{Cx, NoOptions, Range, cop};

const MSG: &str = "The encoding magic comment should precede all other magic comments.";

/// Stateless unit struct.
#[derive(Default)]
pub struct OrderedMagicComments;

#[cop(
    name = "Lint/OrderedMagicComments",
    description = "Checks that encoding magic comments precede all other magic comments.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl OrderedMagicComments {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let src = cx.source().as_bytes();

        if src.is_empty() {
            return;
        }

        let region_end = leading_comment_region_end(src);

        // Collect magic comments in the leading region, in source order.
        let mut magic_comments: Vec<(Range, MagicCommentClassification)> = Vec::new();

        for &comment in cx.comments() {
            if comment.range.start as usize > region_end {
                continue;
            }

            let text = cx.raw_source(comment.range);
            let bytes = text.as_bytes();

            // Only own-line comments.
            if !is_own_line_comment(src, comment.range.start as usize) {
                continue;
            }

            // Classify the comment.
            let classification = classify_comment(bytes);
            match classification {
                CommentClassification::NotMagic => continue,
                _ => {
                    magic_comments.push((comment.range, classification.into()));
                }
            }
        }

        // Find the first encoding and the first non-encoding magic comment.
        let encoding_mc = magic_comments
            .iter()
            .find(|(_, c)| *c == MagicCommentClassification::Encoding);
        let other_mc = magic_comments
            .iter()
            .find(|(_, c)| *c == MagicCommentClassification::Other);

        let (Some(enc), Some(other)) = (encoding_mc, other_mc) else {
            return;
        };

        // No offense if encoding comes before other.
        if enc.0.start < other.0.start {
            return;
        }

        // Report offense on the encoding comment's line.
        let enc_line_range = source_line_range_without_newline(src, enc.0.start as usize);

        cx.emit_offense(enc_line_range, MSG, None);

        // Autocorrect: swap the two lines.
        let enc_full_line_range =
            source_line_range_with_newline(src, enc.0.start as usize);
        let other_full_line_range =
            source_line_range_with_newline(src, other.0.start as usize);

        let enc_line_text = cx.raw_source(enc_full_line_range).to_string();
        let other_line_text = cx.raw_source(other_full_line_range).to_string();

        cx.emit_edit(enc_full_line_range, &other_line_text);
        cx.emit_edit(other_full_line_range, &enc_line_text);
    }
}

/// Classification of a comment as a magic comment type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MagicCommentClassification {
    Encoding,
    Other,
}

/// Raw comment classification before deciding if it's magic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommentClassification {
    NotMagic,
    Encoding,
    Other,
}

impl From<CommentClassification> for MagicCommentClassification {
    fn from(c: CommentClassification) -> Self {
        match c {
            CommentClassification::Encoding => MagicCommentClassification::Encoding,
            CommentClassification::Other => MagicCommentClassification::Other,
            CommentClassification::NotMagic => unreachable!(),
        }
    }
}

/// Recognize magic comment keywords.
///
/// See RuboCop's `MagicComment::KEYWORDS`:
///   encoding: `(?:en)?coding`
///   frozen_string_literal: `frozen[_-]string[_-]literal`
///   rbs_inline: `rbs_inline`
///   shareable_constant_value: `shareable[_-]constant[_-]value`
///   typed: `typed`
fn magic_comment_classification(key: &[u8]) -> CommentClassification {
    fn eq_normalized(key: &[u8], expected: &[u8]) -> bool {
        key.len() == expected.len()
            && key
                .iter()
                .zip(expected.iter())
                .all(|(&k, &e)| {
                    let k = if k == b'-' { b'_' } else { k.to_ascii_lowercase() };
                    k == e
                })
    }

    if eq_normalized(key, b"encoding") || eq_normalized(key, b"coding") {
        CommentClassification::Encoding
    } else if eq_normalized(key, b"frozen_string_literal") {
        CommentClassification::Other
    } else if eq_normalized(key, b"rbs_inline") {
        CommentClassification::Other
    } else if eq_normalized(key, b"shareable_constant_value") {
        CommentClassification::Other
    } else if eq_normalized(key, b"typed") {
        CommentClassification::Other
    } else {
        CommentClassification::NotMagic
    }
}

/// Classify a comment byte-slice (starting with `#`) as encoding magic,
/// other magic, or not magic.
fn classify_comment(bytes: &[u8]) -> CommentClassification {
    if bytes.first() != Some(&b'#') {
        return CommentClassification::NotMagic;
    }

    // Try emacs-style comment: `# -*- key: value -*-`
    if let Some(inner) = strip_emacs_markers(bytes) {
        // Split on `;` for multiple directives.
        for part in inner.split(|&b| b == b';') {
            if let Some(cl) = classify_simple_directive(part) {
                return cl;
            }
        }
        return CommentClassification::NotMagic;
    }

    classify_simple_directive(bytes).unwrap_or(CommentClassification::NotMagic)
}

/// Classify a single `# key: value` (or `key: value` part from emacs).
fn classify_simple_directive(bytes: &[u8]) -> Option<CommentClassification> {
    let mut key_start = 1;
    while key_start < bytes.len() && bytes[key_start].is_ascii_whitespace() {
        key_start += 1;
    }
    if key_start >= bytes.len() {
        return None;
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
        return None;
    }

    let mut sep = key_end;
    while sep < bytes.len() && bytes[sep].is_ascii_whitespace() {
        sep += 1;
    }
    if !matches!(bytes.get(sep), Some(b':' | b'=')) {
        return None;
    }

    let kind = magic_comment_classification(&bytes[key_start..key_end]);
    if matches!(kind, CommentClassification::NotMagic) {
        None
    } else {
        Some(kind)
    }
}

/// Strip `-*- ... -*-` markers from an emacs-style comment.
/// Returns the inner content as a byte slice, or `None` if not emacs-style.
fn strip_emacs_markers(bytes: &[u8]) -> Option<&[u8]> {
    let start = bytes.windows(3).position(|w| w == b"-*-")?;
    let after_start = start + 3;
    let end = bytes[after_start..]
        .windows(3)
        .position(|w| w == b"-*-")?;
    Some(&bytes[after_start..after_start + end])
}

/// Compute the byte offset of the end of the leading comment region.
/// Mirrors `Cx::leading_comment_region_end`.
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

        // Skip shebang on line 0.
        if line_start == 0 && source.starts_with(b"#!") {
            line_start = line_end.saturating_add(1);
            continue;
        }

        let mut first = line_start;
        while first < content_end && source[first].is_ascii_whitespace() {
            first += 1;
        }
        if first < content_end && source[first] == b'#' {
            line_start = line_end.saturating_add(1);
            continue;
        }
        return line_start;
    }
    source.len()
}

/// Check whether a comment at `comment_start` is an own-line comment
/// (only whitespace before `#` on that line).
fn is_own_line_comment(source: &[u8], comment_start: usize) -> bool {
    let line_start = source[..comment_start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |pos| pos + 1);
    source[line_start..comment_start]
        .iter()
        .all(|byte| byte.is_ascii_whitespace())
}

/// Return the source range for the line containing `offset` (without the
/// trailing newline).  Mirrors `Cx::source_line_range_without_newline`.
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

/// Return the source range for the line containing `offset` (including the
/// trailing newline).
fn source_line_range_with_newline(source: &[u8], offset: usize) -> Range {
    let start = source[..offset]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |pos| pos + 1);
    let end = source[offset..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(source.len(), |pos| {
            let candidate = offset + pos;
            // Include the newline if not at EOF.
            if candidate + 1 < source.len() {
                candidate + 1
            } else {
                source.len()
            }
        });
    Range {
        start: start as u32,
        end: end as u32,
    }
}

murphy_plugin_api::submit_cop!(OrderedMagicComments);

#[cfg(test)]
mod tests {
    use super::OrderedMagicComments;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- offenses ---

    #[test]
    fn flags_encoding_after_frozen_string_literal() {
        test::<OrderedMagicComments>()
            .expect_offense(indoc! {r#"
                # frozen_string_literal: true
                # encoding: ascii
                ^^^^^^^^^^^^^^^^^ The encoding magic comment should precede all other magic comments.
            "#});
    }

    #[test]
    fn flags_and_corrects_encoding_after_frozen_string_literal() {
        test::<OrderedMagicComments>()
            .expect_correction(
                indoc! {r#"
                    # frozen_string_literal: true
                    # encoding: ascii
                    ^^^^^^^^^^^^^^^^^ The encoding magic comment should precede all other magic comments.
                "#},
                indoc! {r#"
                    # encoding: ascii
                    # frozen_string_literal: true
                "#},
            );
    }

    #[test]
    fn flags_coding_after_frozen_string_literal() {
        test::<OrderedMagicComments>()
            .expect_offense(indoc! {r#"
                # frozen_string_literal: true
                # coding: ascii
                ^^^^^^^^^^^^^^^ The encoding magic comment should precede all other magic comments.
            "#});
    }

    #[test]
    fn flags_and_corrects_coding_after_frozen_string_literal() {
        test::<OrderedMagicComments>()
            .expect_correction(
                indoc! {r#"
                    # frozen_string_literal: true
                    # coding: ascii
                    ^^^^^^^^^^^^^^^ The encoding magic comment should precede all other magic comments.
                "#},
                indoc! {r#"
                    # coding: ascii
                    # frozen_string_literal: true
                "#},
            );
    }

    #[test]
    fn flags_emacs_encoding_after_frozen_string_literal() {
        test::<OrderedMagicComments>()
            .expect_offense(indoc! {r#"
                # frozen_string_literal: true
                # -*- encoding : ascii-8bit -*-
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ The encoding magic comment should precede all other magic comments.
            "#});
    }

    #[test]
    fn flags_and_corrects_emacs_encoding_after_frozen_string_literal() {
        test::<OrderedMagicComments>()
            .expect_correction(
                indoc! {r#"
                    # frozen_string_literal: true
                    # -*- encoding : ascii-8bit -*-
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ The encoding magic comment should precede all other magic comments.
                "#},
                indoc! {r#"
                    # -*- encoding : ascii-8bit -*-
                    # frozen_string_literal: true
                "#},
            );
    }

    #[test]
    fn flags_encoding_after_frozen_string_literal_with_shebang() {
        test::<OrderedMagicComments>()
            .expect_offense(indoc! {r#"
                #!/usr/bin/env ruby
                # frozen_string_literal: true
                # encoding: ascii
                ^^^^^^^^^^^^^^^^^ The encoding magic comment should precede all other magic comments.
            "#});
    }

    #[test]
    fn flags_and_corrects_encoding_after_frozen_string_literal_with_shebang() {
        test::<OrderedMagicComments>()
            .expect_correction(
                indoc! {r#"
                    #!/usr/bin/env ruby
                    # frozen_string_literal: true
                    # encoding: ascii
                    ^^^^^^^^^^^^^^^^^ The encoding magic comment should precede all other magic comments.
                "#},
                indoc! {r#"
                    #!/usr/bin/env ruby
                    # encoding: ascii
                    # frozen_string_literal: true
                "#},
            );
    }

    #[test]
    fn flags_encoding_after_shareable_constant_value() {
        test::<OrderedMagicComments>()
            .expect_offense(indoc! {r#"
                # shareable_constant_value: literal
                # encoding: ascii
                ^^^^^^^^^^^^^^^^^ The encoding magic comment should precede all other magic comments.
            "#});
    }

    #[test]
    fn flags_and_corrects_encoding_after_shareable_constant_value() {
        test::<OrderedMagicComments>()
            .expect_correction(
                indoc! {r#"
                    # shareable_constant_value: literal
                    # encoding: ascii
                    ^^^^^^^^^^^^^^^^^ The encoding magic comment should precede all other magic comments.
                "#},
                indoc! {r#"
                    # encoding: ascii
                    # shareable_constant_value: literal
                "#},
            );
    }

    // --- no offenses ---

    #[test]
    fn accepts_encoding_before_frozen_string_literal() {
        test::<OrderedMagicComments>()
            .expect_no_offenses("# encoding: ascii\n# frozen_string_literal: true\n");
    }

    #[test]
    fn accepts_encoding_before_shareable_constant_value() {
        test::<OrderedMagicComments>()
            .expect_no_offenses(
                "# encoding: ascii\n# shareable_constant_value: literal\n",
            );
    }

    #[test]
    fn accepts_encoding_on_first_line() {
        test::<OrderedMagicComments>()
            .expect_no_offenses("# encoding: ascii\n");
    }

    #[test]
    fn accepts_encoding_after_shebang() {
        test::<OrderedMagicComments>()
            .expect_no_offenses(
                "#!/usr/bin/env ruby\n# encoding: ascii\n# frozen_string_literal: true\n",
            );
    }

    #[test]
    fn accepts_frozen_string_literal_only() {
        test::<OrderedMagicComments>()
            .expect_no_offenses("# frozen_string_literal: true\n");
    }

    #[test]
    fn accepts_hash_notation_encoding_after_code() {
        // This is not a magic comment — it's a hash literal in code.
        test::<OrderedMagicComments>()
            .expect_no_offenses(
                "# frozen_string_literal: true\n\nx = { encoding: Encoding::SJIS }\nputs x\n",
            );
    }

    #[test]
    fn accepts_encoding_embedded_in_string() {
        // This appears in a string, not as a leading magic comment.
        test::<OrderedMagicComments>()
            .expect_no_offenses(
                "# frozen_string_literal: true\n\n# eval('# encoding: ISO-8859-1')\n",
            );
    }

    #[test]
    fn accepts_empty_file() {
        test::<OrderedMagicComments>().expect_no_offenses("");
    }
}
