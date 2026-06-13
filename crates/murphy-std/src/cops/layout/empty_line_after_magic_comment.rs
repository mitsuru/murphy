//! `Layout/EmptyLineAfterMagicComment` — require a blank line separating the
//! file's magic comments from the first line of code.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/EmptyLineAfterMagicComment
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: [murphy-dlko]
//! notes: >
//!   Direct port of RuboCop's `on_new_investigation`: find the LAST magic
//!   comment that appears before any code, and if the immediately following
//!   physical line is non-blank, flag the start of that line and insert a `\n`
//!   before it.
//!
//!   Murphy's structured `magic_comments()` only recognizes
//!   `frozen_string_literal` and `encoding`/`coding` (plus the file shebang,
//!   which we deliberately exclude here — RuboCop's `MagicComment.parse` does
//!   not treat `#!` as a magic comment). RuboCop additionally recognizes
//!   `shareable_constant_value` and `warn_indent`; those keys are not modelled
//!   by Murphy's magic-comment table yet, so a file whose only magic comment is
//!   one of those is not flagged. Gap filed as murphy-dlko.
//!
//!   Message: "Add an empty line after magic comments."
//!   Autocorrect: insert "\n" before the offending line.
//! ```

use murphy_plugin_api::{Cx, MagicCommentKind, NoOptions, Range, cop};

/// Stateless unit struct (ADR 0035 const-metadata cop pattern).
#[derive(Default)]
pub struct EmptyLineAfterMagicComment;

const MSG: &str = "Add an empty line after magic comments.";

#[cop(
    name = "Layout/EmptyLineAfterMagicComment",
    description = "Add an empty line after magic comments.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl EmptyLineAfterMagicComment {
    #[on_new_investigation]
    fn investigate(&self, cx: &Cx<'_>) {
        // RuboCop: `last_magic_comment = comments_before_code.reverse.find { ... }`.
        // `magic_comments()` already filters to the leading own-line comment
        // region (before code), so the last non-shebang entry is the one we
        // want. Shebang is excluded — it is not a magic comment in RuboCop.
        let Some(last_magic) = cx
            .magic_comments()
            .into_iter()
            .rfind(|c| c.kind != MagicCommentKind::Shebang)
        else {
            return;
        };

        let src = cx.source().as_bytes();

        // The next physical line begins just after the magic comment's line
        // terminator. `magic_comment.range` excludes the trailing `\n`.
        let comment_end = last_magic.range.end as usize;
        // Find the `\n` that terminates the magic comment line.
        let Some(newline_off) = src[comment_end..].iter().position(|&b| b == b'\n') else {
            // Magic comment is the last line of the file — no next line, so
            // RuboCop's `processed_source[line]` is nil and returns early.
            return;
        };
        let next_line_start = comment_end + newline_off + 1;
        if next_line_start >= src.len() {
            // No content after the terminating newline.
            return;
        }

        // RuboCop: `return if next_line.strip.empty?`. Determine the extent of
        // the next physical line and check whether it is blank.
        let next_line_end = src[next_line_start..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(src.len(), |i| next_line_start + i);
        let next_line = &src[next_line_start..next_line_end];
        if next_line
            .iter()
            .all(|&b| crate::cops::util::is_ruby_blank_byte(b))
        {
            return;
        }

        // RuboCop: `offending_range = source_range(buffer, line + 1, 0)` — the
        // zero-width position at the start of the next line — then
        // `corrector.insert_before(offending_range, "\n")`.
        let offending_range = Range {
            start: next_line_start as u32,
            end: next_line_start as u32,
        };
        cx.emit_offense(offending_range, MSG, None);
        cx.emit_edit(offending_range, "\n");
    }
}

murphy_plugin_api::submit_cop!(EmptyLineAfterMagicComment);

#[cfg(test)]
mod tests {
    use super::EmptyLineAfterMagicComment;
    use murphy_plugin_api::test_support::{run_cop_with_edits, test};

    fn apply(source: &str, edits: &[murphy_plugin_api::test_support::CapturedEdit]) -> String {
        assert_eq!(edits.len(), 1, "expected exactly one insert edit");
        let edit = &edits[0];
        let mut out = String::with_capacity(source.len() + edit.replacement.len());
        out.push_str(&source[..edit.range.start as usize]);
        out.push_str(&edit.replacement);
        out.push_str(&source[edit.range.end as usize..]);
        out
    }

    #[test]
    fn accepts_blank_line_after_magic_comment() {
        test::<EmptyLineAfterMagicComment>()
            .expect_no_offenses("# frozen_string_literal: true\n\nx = 0\n");
    }

    #[test]
    fn accepts_file_without_magic_comment() {
        test::<EmptyLineAfterMagicComment>().expect_no_offenses("x = 0\ny = 1\n");
    }

    #[test]
    fn accepts_magic_comment_as_only_line() {
        // No code line after the magic comment → nothing to separate.
        test::<EmptyLineAfterMagicComment>().expect_no_offenses("# frozen_string_literal: true\n");
    }

    #[test]
    fn flags_code_immediately_after_magic_comment() {
        let run =
            run_cop_with_edits::<EmptyLineAfterMagicComment>("# frozen_string_literal: true\nx = 0\n");
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(run.offenses[0].message, super::MSG);
        assert_eq!(
            apply("# frozen_string_literal: true\nx = 0\n", &run.edits),
            "# frozen_string_literal: true\n\nx = 0\n"
        );
    }

    #[test]
    fn flags_using_last_of_multiple_magic_comments() {
        // The empty line must follow the LAST magic comment, not the first.
        let src = "# encoding: utf-8\n# frozen_string_literal: true\nx = 0\n";
        let run = run_cop_with_edits::<EmptyLineAfterMagicComment>(src);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(
            apply(src, &run.edits),
            "# encoding: utf-8\n# frozen_string_literal: true\n\nx = 0\n"
        );
    }

    #[test]
    fn accepts_shebang_only_followed_by_code() {
        // A shebang is not a magic comment — no offense even with adjacent code.
        test::<EmptyLineAfterMagicComment>().expect_no_offenses("#!/usr/bin/env ruby\nx = 0\n");
    }

    #[test]
    fn flags_after_magic_comment_following_shebang() {
        // Shebang + magic comment + code: the magic comment is what needs the
        // trailing blank line, and the shebang is ignored.
        let src = "#!/usr/bin/env ruby\n# frozen_string_literal: true\nx = 0\n";
        let run = run_cop_with_edits::<EmptyLineAfterMagicComment>(src);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(
            apply(src, &run.edits),
            "#!/usr/bin/env ruby\n# frozen_string_literal: true\n\nx = 0\n"
        );
    }

    #[test]
    fn accepts_comment_after_code_is_not_magic() {
        // A `frozen_string_literal` comment that appears after code is not a
        // leading magic comment and is ignored.
        test::<EmptyLineAfterMagicComment>()
            .expect_no_offenses("x = 0\n# frozen_string_literal: true\ny = 1\n");
    }
}
