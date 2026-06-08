//! `Lint/RedundantCopEnableDirective` — detect unnecessary `# rubocop:enable`
//! comments.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/RedundantCopEnableDirective
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues:
//!   - murphy-k19j
//! notes: >
//!   The `all` keyword and department names (e.g. `Lint`) are treated as
//!   opaque/global rather than expanded to concrete cop names (the host has
//!   no registry access). Redundant enables that depend on such expansion —
//!   for example a specific `enable X` after a `disable all`, or a department
//!   enable that only partially overlaps an active disable — are not detected
//!   (a safe false negative). A valid enable is never falsely flagged. The
//!   `extra_enabled_directives` primitive's `disable_all_depth` proxy keeps
//!   those cases on the conservative side. A follow-up issue will track
//!   registry-backed `all`/department expansion (TODO: murphy-k19j follow-up).
//!   Redundant cop names that are prefixes of other listed cops (and the `all`
//!   keyword / department names) are located by a `comment.text.index`-style
//!   search exactly as RuboCop does — this reproduces RuboCop's behavior rather
//!   than "correcting" it (e.g. `Style/For` matches inside `Style/FormatString`).
//!   A malformed directive that repeats the same cop name (`enable Foo, Foo`) is
//!   not specially handled (deferred).
//! ```

use murphy_plugin_api::{Cx, NoOptions, Range, RangeSide, SpaceRangeOptions, cop};

#[derive(Default)]
pub struct RedundantCopEnableDirective;

#[cop(
    name = "Lint/RedundantCopEnableDirective",
    description = "Detect unnecessary rubocop:enable comments.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl RedundantCopEnableDirective {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        // RuboCop fast path: nothing to do without an `enable` token anywhere.
        if !cx.source().contains("enable") {
            return;
        }

        for extra in cx.extra_enabled_directives() {
            let comment_src = cx.raw_source(extra.comment_range);

            // When every cop named by the directive is redundant, RuboCop
            // removes the whole enable comment plus the space on its right
            // (which eats the trailing newline). Emit this ONCE per comment,
            // outside the per-name loop, so multiple redundant names in the
            // same directive never produce overlapping removal edits.
            if extra.all_in_directive {
                let removal = cx.range_with_surrounding_space(
                    extra.comment_range,
                    SpaceRangeOptions {
                        side: RangeSide::Right,
                        ..SpaceRangeOptions::default()
                    },
                );
                cx.emit_edit(removal, "");
            }

            for name in &extra.cop_names {
                let Some(idx) = comment_src.find(name) else {
                    continue;
                };
                let start = extra.comment_range.start + idx as u32;
                let offense_range = Range {
                    start,
                    end: start + name.len() as u32,
                };
                let shown = if *name == "all" { "all cops" } else { name };
                cx.emit_offense(
                    offense_range,
                    &format!("Unnecessary enabling of {shown}."),
                    None,
                );

                // Partial redundancy: remove just this cop name with its comma.
                if !extra.all_in_directive
                    && let Some(removal) = comma_removal_range(comment_src, idx, name.len())
                {
                    let abs = Range {
                        start: extra.comment_range.start + removal.start,
                        end: extra.comment_range.start + removal.end,
                    };
                    cx.emit_edit(abs, "");
                }
            }
        }
    }
}

/// Compute the comment-relative byte range to remove for a single redundant
/// cop name, mirroring RuboCop's `range_with_comma` / `range_to_remove`.
///
/// `comment_src` is the whole comment text, `name_idx`/`name_len` locate the
/// cop name within it. Returns a comment-relative [`Range`]; the caller adds
/// the comment's absolute start offset.
fn comma_removal_range(comment_src: &str, name_idx: usize, name_len: usize) -> Option<Range> {
    let bytes = comment_src.as_bytes();
    // RuboCop `range_with_comma`: expand begin/end past `[ \t]` (no newlines)
    // before deciding which comma to consume.
    let begin_pos = reposition(bytes, name_idx, -1);
    let end_pos = reposition(bytes, name_idx + name_len, 1);

    // `range_to_remove`.
    if begin_pos > 0 && bytes.get(begin_pos - 1) == Some(&b',') {
        // Comma before: remove `, Name`.
        Some(Range {
            start: (begin_pos - 1) as u32,
            end: end_pos as u32,
        })
    } else if bytes.get(end_pos) == Some(&b',') {
        // Comma after: remove `Name, `. If no space follows the comma, keep
        // begin where it is (do not eat a preceding space); the trailing
        // `+1` past the comma stays.
        let mut begin = begin_pos;
        if bytes.get(end_pos + 1) != Some(&b' ') {
            begin += 1;
        }
        Some(Range {
            start: begin as u32,
            end: (end_pos + 1) as u32,
        })
    } else {
        // No surrounding comma: RuboCop falls back to removing the whole
        // comment. In practice this branch is unreachable — a single redundant
        // name always yields `all_in_directive == true` (handled separately by
        // whole-comment removal), so every comma-removal case has >= 2 names
        // and thus a surrounding comma. Kept as a faithful, defensive port of
        // RuboCop's `range_to_remove` else-branch.
        Some(Range {
            start: 0,
            end: comment_src.len() as u32,
        })
    }
}

/// RuboCop `SurroundingSpace#reposition` with `include_newlines: false`:
/// walk `pos` by `step` while the adjacent byte is a space or tab.
/// For `step == -1` the adjacent byte is `src[pos - 1]`; for `step == 1` it
/// is `src[pos]`.
fn reposition(src: &[u8], mut pos: usize, step: i32) -> usize {
    if step < 0 {
        while pos > 0 && matches!(src[pos - 1], b' ' | b'\t') {
            pos -= 1;
        }
    } else {
        while pos < src.len() && matches!(src[pos], b' ' | b'\t') {
            pos += 1;
        }
    }
    pos
}

murphy_plugin_api::submit_cop!(RedundantCopEnableDirective);

#[cfg(test)]
mod tests {
    use super::RedundantCopEnableDirective;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_enable_without_disable() {
        test::<RedundantCopEnableDirective>().expect_offense(indoc! {r#"
            foo = 1
            # rubocop:enable Layout/LineLength
                             ^^^^^^^^^^^^^^^^^ Unnecessary enabling of Layout/LineLength.
        "#});
    }

    #[test]
    fn accepts_enable_matching_a_disable() {
        test::<RedundantCopEnableDirective>().expect_no_offenses(indoc! {r#"
            # rubocop:disable Style/StringLiterals
            foo = "1"
            # rubocop:enable Style/StringLiterals
        "#});
    }

    #[test]
    fn flags_enable_all_when_nothing_disabled() {
        test::<RedundantCopEnableDirective>().expect_offense(indoc! {r#"
            foo
            # rubocop:enable all
                             ^^^ Unnecessary enabling of all cops.
        "#});
    }

    #[test]
    fn corrects_whole_comment_when_all_redundant() {
        test::<RedundantCopEnableDirective>().expect_correction(
            indoc! {r#"
                foo = 1
                # rubocop:enable Layout/LineLength
                                 ^^^^^^^^^^^^^^^^^ Unnecessary enabling of Layout/LineLength.
            "#},
            "foo = 1\n",
        );
    }

    #[test]
    fn corrects_partial_redundancy_removing_one_cop() {
        // `Layout/LineLength` is never disabled, so it is the redundant name;
        // `Style/StringLiterals` is validly enabled. RuboCop removes the
        // redundant trailing name together with its preceding comma.
        test::<RedundantCopEnableDirective>().expect_correction(
            indoc! {r#"
                # rubocop:disable Style/StringLiterals
                foo
                # rubocop:enable Style/StringLiterals, Layout/LineLength
                                                       ^^^^^^^^^^^^^^^^^ Unnecessary enabling of Layout/LineLength.
            "#},
            "# rubocop:disable Style/StringLiterals\nfoo\n# rubocop:enable Style/StringLiterals\n",
        );
    }

    #[test]
    fn corrects_partial_redundancy_comma_after() {
        // Redundant name first → comma-after removal (`Name, ` consumed).
        test::<RedundantCopEnableDirective>().expect_correction(
            indoc! {r#"
                # rubocop:disable Style/StringLiterals
                foo
                # rubocop:enable Layout/LineLength, Style/StringLiterals
                                 ^^^^^^^^^^^^^^^^^ Unnecessary enabling of Layout/LineLength.
            "#},
            "# rubocop:disable Style/StringLiterals\nfoo\n# rubocop:enable Style/StringLiterals\n",
        );
    }

    #[test]
    fn corrects_partial_redundancy_no_space_after_comma() {
        // No space after the comma → the preceding space is preserved
        // (RuboCop's `begin_pos += 1` nuance).
        test::<RedundantCopEnableDirective>().expect_correction(
            indoc! {r#"
                # rubocop:disable Style/StringLiterals
                foo
                # rubocop:enable Layout/LineLength,Style/StringLiterals
                                 ^^^^^^^^^^^^^^^^^ Unnecessary enabling of Layout/LineLength.
            "#},
            "# rubocop:disable Style/StringLiterals\nfoo\n# rubocop:enable Style/StringLiterals\n",
        );
    }

    // Parity lock: a redundant cop name that is a prefix of another listed cop is
    // located via find/comment.text.index exactly like RuboCop 1.87.0 — identical
    // offense column AND identical (byte-for-byte) autocorrect (whole comment text
    // removed, trailing newline retained). Do NOT "fix" this by carrying exact
    // per-name ranges; that would diverge from RuboCop and break parity.
    #[test]
    fn prefix_cop_name_matches_rubocop_index_semantics() {
        // `Style/For` is the 9-char prefix of `Style/FormatString`, so
        // `find`/`comment.text.index` lands the caret at the start of the
        // `Style/FormatString` token (column 18). Because that match sits
        // mid-token, neither side is bordered by a comma, so RuboCop's
        // `range_to_remove` falls through to removing the *whole* enable
        // comment (the no-surrounding-comma fallback). The comment text is
        // deleted but its newline is retained, leaving a trailing blank line.
        test::<RedundantCopEnableDirective>().expect_offense(indoc! {r#"
            # rubocop:disable Style/FormatString
            foo
            # rubocop:enable Style/FormatString, Style/For
                             ^^^^^^^^^ Unnecessary enabling of Style/For.
        "#});

        test::<RedundantCopEnableDirective>().expect_correction(
            indoc! {r#"
                # rubocop:disable Style/FormatString
                foo
                # rubocop:enable Style/FormatString, Style/For
                                 ^^^^^^^^^ Unnecessary enabling of Style/For.
            "#},
            "# rubocop:disable Style/FormatString\nfoo\n\n",
        );
    }

    #[test]
    fn corrects_whole_comment_when_all_names_redundant() {
        // Both cops redundant → two offenses, single whole-comment removal.
        test::<RedundantCopEnableDirective>().expect_correction(
            indoc! {r#"
                foo
                # rubocop:enable Layout/LineLength, Style/StringLiterals
                                 ^^^^^^^^^^^^^^^^^ Unnecessary enabling of Layout/LineLength.
                                                    ^^^^^^^^^^^^^^^^^^^^ Unnecessary enabling of Style/StringLiterals.
                bar
            "#},
            "foo\nbar\n",
        );
    }
}
