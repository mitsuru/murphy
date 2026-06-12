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
//! gap_issues:
//!   - murphy-y8tm
//! notes: >
//!   Ported from RuboCop Lint/DuplicateMagicComment. Buckets the leading
//!   magic comments returned by `cx.magic_comments()` into `encoding` and
//!   `frozen_string_literal` categories by their key only (the value is
//!   ignored, so `frozen_string_literal: true` and `: TRUE` collide, matching
//!   RuboCop). Within each bucket every occurrence after the first is flagged
//!   on its whole line, and the autocorrect removes that whole line including
//!   the trailing newline (`cx.range_by_whole_lines`). `cx.magic_comments()`
//!   already restricts to the leading comment block, skips the shebang, and
//!   classifies only `encoding`/`frozen_string_literal` — so
//!   `shareable_constant_value`, `rbs_inline`, and `typed` are correctly out
//!   of scope (that is `Lint/OrderedMagicComments`' concern), matching
//!   RuboCop's `magic_comment_lines`. Known divergence (tracked in
//!   murphy-y8tm): the shared `leading_comment_region_end` ends the leading
//!   region at the first blank line, whereas RuboCop's region runs up to the
//!   first non-comment *token* and treats blank lines as transparent. This
//!   only matters for the pathological layout of two same-kind magic comments
//!   separated by a blank line, which RuboCop flags and Murphy does not.
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

use murphy_plugin_api::{Cx, MagicCommentKind, NoOptions, cop};

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
        // Track, per kind, whether we've already seen a magic comment of that
        // kind. `cx.magic_comments()` yields the file's leading magic comments
        // in source order (shebang first if present), so the first occurrence
        // of each kind is kept and every later one is flagged.
        let mut seen_encoding = false;
        let mut seen_frozen = false;

        for comment in cx.magic_comments() {
            let already_seen = match comment.kind {
                MagicCommentKind::Encoding => std::mem::replace(&mut seen_encoding, true),
                MagicCommentKind::FrozenStringLiteral => std::mem::replace(&mut seen_frozen, true),
                // The shebang is not a duplicable magic comment kind.
                MagicCommentKind::Shebang => continue,
            };
            if !already_seen {
                continue;
            }

            // Duplicate — offense on the whole line, autocorrect removes the
            // whole line including its trailing newline.
            let line_no_nl = cx.range_by_whole_lines(comment.range, false);
            cx.emit_offense(line_no_nl, MSG, None);

            let line_with_nl = cx.range_by_whole_lines(comment.range, true);
            cx.emit_edit(line_with_nl, "");
        }
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

    #[test]
    fn does_not_flag_blank_separated_duplicates_shared_region_boundary() {
        // Documented divergence (murphy-y8tm): the shared
        // `leading_comment_region_end` ends the leading region at the first
        // blank line, so the second frozen comment falls outside the region
        // and is not categorised. RuboCop, whose region runs to the first
        // non-comment token, would flag the second comment.
        test::<DuplicateMagicComment>().expect_no_offenses(indoc! {r#"
            # frozen_string_literal: true

            # frozen_string_literal: true
        "#});
    }
}
