//! `Style/Encoding` — remove unnecessary UTF-8 encoding comments.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/Encoding
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Since Ruby 2.0 UTF-8 is the default encoding, so `# encoding: utf-8`
//!   (and variants like `# coding: UTF-8`, `# -*- coding: utf-8 -*-`) are
//!   redundant. This cop flags any encoding magic comment whose value is a
//!   case-insensitive match for `utf-8` and autocorrects by removing the
//!   comment line (and its trailing newline if present).
//!   Only utf-8 comments are flagged; non-utf-8 encoding declarations are
//!   meaningful and left alone.
//! ```
//!
//! ## What is checked
//!
//! Files whose leading comment block contains an encoding magic comment
//! (`# [coding|encoding]: utf-8` in any case) are flagged.
//!
//! ## Autocorrect
//!
//! Remove the encoding comment line entirely (including its newline).

use murphy_plugin_api::{Cx, NoOptions, Range, cop};

const MSG: &str = "Unnecessary utf-8 encoding comment.";

#[derive(Default)]
pub struct Encoding;

#[cop(
    name = "Style/Encoding",
    description = "Use UTF-8 as the source file encoding.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl Encoding {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let Some(enc) = cx.encoding_comment() else {
            return;
        };
        // Only flag utf-8 encoding declarations; leave meaningful non-utf-8 alone.
        let value = cx.raw_source(enc.value_range);
        if !value.eq_ignore_ascii_case("utf-8") {
            return;
        }
        // Report offense on the full comment line (the comment range).
        cx.emit_offense(enc.range, MSG, None);
        // Autocorrect: remove the encoding comment line including the trailing newline.
        let src = cx.source().as_bytes();
        let remove_start = line_start(src, enc.range.start);
        let remove_end = line_end_including_newline(src, enc.range.end);
        cx.emit_edit(
            Range {
                start: remove_start,
                end: remove_end,
            },
            "",
        );
    }
}

/// Returns the byte offset of the start of the line containing `pos`.
fn line_start(src: &[u8], pos: u32) -> u32 {
    let pos = pos as usize;
    src[..pos]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |p| p + 1) as u32
}

/// Returns the byte offset just past the newline at `end` (or `end` itself
/// if no newline follows).
fn line_end_including_newline(src: &[u8], end: u32) -> u32 {
    let end = end as usize;
    if end < src.len() && src[end] == b'\n' {
        (end + 1) as u32
    } else if end + 1 < src.len() && src[end] == b'\r' && src[end + 1] == b'\n' {
        (end + 2) as u32
    } else {
        end as u32
    }
}

#[cfg(test)]
mod tests {
    use super::Encoding;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_utf8_encoding_comment() {
        test::<Encoding>().expect_offense(indoc! {"
            # encoding: utf-8
            ^^^^^^^^^^^^^^^^^^ Unnecessary utf-8 encoding comment.
            x = 1
        "});
    }

    #[test]
    fn flags_utf8_uppercase() {
        test::<Encoding>().expect_offense(indoc! {"
            # encoding: UTF-8
            ^^^^^^^^^^^^^^^^^^ Unnecessary utf-8 encoding comment.
            x = 1
        "});
    }

    #[test]
    fn flags_coding_alias() {
        test::<Encoding>().expect_offense(indoc! {"
            # coding: utf-8
            ^^^^^^^^^^^^^^^^ Unnecessary utf-8 encoding comment.
            x = 1
        "});
    }

    #[test]
    fn accepts_non_utf8_encoding() {
        test::<Encoding>().expect_no_offenses("# encoding: iso-8859-1\nx = 1\n");
    }

    #[test]
    fn accepts_file_without_encoding_comment() {
        test::<Encoding>().expect_no_offenses("x = 1\n");
    }

    #[test]
    fn accepts_empty_file() {
        test::<Encoding>().expect_no_offenses("");
    }

    #[test]
    fn autocorrects_by_removing_encoding_line() {
        test::<Encoding>().expect_correction(
            indoc! {"
                # encoding: utf-8
                ^^^^^^^^^^^^^^^^^^ Unnecessary utf-8 encoding comment.
                x = 1
            "},
            "x = 1\n",
        );
    }

    #[test]
    fn autocorrects_encoding_after_shebang() {
        test::<Encoding>().expect_correction(
            indoc! {"
                #!/usr/bin/env ruby
                # encoding: utf-8
                ^^^^^^^^^^^^^^^^^^ Unnecessary utf-8 encoding comment.
                x = 1
            "},
            "#!/usr/bin/env ruby\nx = 1\n",
        );
    }
}

murphy_plugin_api::submit_cop!(Encoding);
