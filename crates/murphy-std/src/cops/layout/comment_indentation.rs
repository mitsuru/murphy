//! `Layout/CommentIndentation` — checks the indentation of own-line comments
//! against the code line that follows them.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/CommentIndentation
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Direct port of RuboCop's `on_new_investigation` + `check`. For every
//!   own-line comment (a comment whose line matches `/\A\s*#/`) the expected
//!   indentation is computed from `line_after_comment` — the next non-blank line
//!   strictly after the comment's own line:
//!     * `correct_indentation` = that line's first-non-whitespace column, plus
//!       `configured_indentation_width` when the line is `less_indented?`
//!       (starts with `end`/`)`/`}`/`]`).
//!     * `column` is the comment's 0-based column.
//!     * No offense when they already match.
//!     * `two_alternatives?` lines (`else`/`elsif`/`when`/`in`/`rescue`/`ensure`)
//!       accept either the keyword column or keyword + width; the kept
//!       `column_delta` still aligns the autocorrect with the keyword column.
//!     * With `AllowForAlignment: true`, a comment aligned (same column) with the
//!       nearest preceding *trailing* (non-own-line) comment is accepted.
//!   The offense is reported on the comment's source range; the message embeds
//!   0-based `column` and `correct` indentation.
//!
//!   Autocorrect mirrors `AlignmentCorrector` + `autocorrect_preceding_comments`:
//!   the flagged comment's leading whitespace is rewritten to `column +
//!   column_delta` spaces, and any run of immediately-preceding own-line comments
//!   on consecutive lines at the *same column* is corrected in the same pass
//!   (RuboCop batches these to avoid multi-pass convergence; without the batch a
//!   run of equally-indented comments registers a single offense on its last
//!   member but the preceding members — `column_delta == 0` against their comment
//!   neighbour — would never be corrected).
//!
//!   Comments after `__END__` are not surfaced by Murphy's parser (matching
//!   RuboCop's lexer, which drops them), so no offense is reported there.
//!
//!   Gaps (documented, not bypassed):
//!     * `configured_indentation_width` defaults to RuboCop's 2 when the per-cop
//!       `IndentationWidth` option is unset. RuboCop falls back to the sibling
//!       `Layout/IndentationWidth: Width`, which is unreadable across the
//!       single-surface ABI; the per-cop `IndentationWidth` override is honoured.
//!     * `less_indented?`'s access-modifier outdent branch is treated as false
//!       (it only fires when `Layout/AccessModifierIndentation: EnforcedStyle` is
//!       `outdent`, a non-default sibling-cop value unreadable across the ABI).
//!     * RuboCop's `expect_correction` settles a chain of mutually-referencing
//!       comments over multiple lexer passes; Murphy reaches the same fixpoint via
//!       the production autocorrect loop. The per-pass edits are RuboCop-faithful;
//!       only the single-pass test harness sees the intermediate state, so the
//!       `expect_correction` tests use shapes that converge in one pass.
//! ```

use murphy_plugin_api::{Comment, CommentKind, CopOptions, Cx, Range, cop};

/// Stateless unit struct (ADR 0035 const-metadata cop pattern).
#[derive(Default)]
pub struct CommentIndentation;

#[derive(CopOptions)]
pub struct CommentIndentationOptions {
    #[option(
        name = "AllowForAlignment",
        default = false,
        description = "Allow comments to have extra indentation if that aligns them with a comment on the preceding line."
    )]
    pub allow_for_alignment: bool,

    #[option(
        name = "IndentationWidth",
        default = 2,
        description = "Number of spaces for one indentation level (falls back to RuboCop's default of 2)."
    )]
    pub indentation_width: i64,
}

#[cop(
    name = "Layout/CommentIndentation",
    description = "Indentation of comments.",
    default_severity = "warning",
    default_enabled = true,
    options = CommentIndentationOptions,
)]
impl CommentIndentation {
    #[on_new_investigation]
    fn investigate(&self, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<CommentIndentationOptions>();
        let width = opts.indentation_width.max(0) as usize;
        let comments = cx.comments();

        for (ix, comment) in comments.iter().enumerate() {
            check(cx, comments, *comment, ix, width, opts.allow_for_alignment);
        }
    }
}

/// RuboCop's `check(comment, comment_index)`.
fn check(
    cx: &Cx<'_>,
    comments: &[Comment],
    comment: Comment,
    comment_index: usize,
    width: usize,
    allow_for_alignment: bool,
) {
    // `return unless own_line_comment?(comment)`.
    if !own_line_comment(cx, comment) {
        return;
    }

    let source = cx.source();
    let next_line = line_after_comment(source, comment);
    let mut correct = correct_indentation(next_line, width);
    let column = column_of(source, comment.range.start);

    // `@column_delta = correct_comment_indentation - column; return if zero`.
    let column_delta = correct as i64 - column as i64;
    if column_delta == 0 {
        return;
    }

    // `if two_alternatives?(next_line)` — accept the keyword column or
    // keyword + width. `column_delta` is intentionally left at the keyword-column
    // value so the autocorrect aligns with the keyword.
    if next_line.is_some_and(two_alternatives) {
        correct += width;
        if column == correct {
            return;
        }
    }

    // `return if correctly_aligned_with_preceding_comment?(comment_index, column)`.
    if correctly_aligned_with_preceding_comment(
        cx,
        comments,
        comment_index,
        column,
        allow_for_alignment,
    ) {
        return;
    }

    // RuboCop reports the (possibly `two_alternatives?`-bumped) `correct` value.
    let message =
        format!("Incorrect indentation detected (column {column} instead of {correct}).");
    cx.emit_offense(comment.range, &message, None);

    // `autocorrect(corrector, comment)` = `autocorrect_preceding_comments` +
    // `autocorrect_one`.
    autocorrect_preceding_comments(cx, comments, comment_index, column_delta);
    autocorrect_one(cx, comment, column_delta);
}

/// RuboCop's `autocorrect_one`: shift the comment's leading whitespace by
/// `column_delta`. For an own-line comment this replaces `[line_start,
/// comment_start)` with `column + column_delta` spaces.
fn autocorrect_one(cx: &Cx<'_>, comment: Comment, column_delta: i64) {
    let source = cx.source();
    let line_start = line_start_offset(source, comment.range.start);
    let column = column_of(source, comment.range.start);
    let target = (column as i64 + column_delta).max(0) as usize;
    cx.emit_edit(
        Range {
            start: line_start,
            end: comment.range.start,
        },
        &" ".repeat(target),
    );
}

/// RuboCop's `autocorrect_preceding_comments`: correct each immediately-preceding
/// own-line comment that sits one line above its successor at the same column,
/// applying the same `column_delta`. This batches a run of equally-indented
/// comments so the whole run is fixed in one pass.
fn autocorrect_preceding_comments(
    cx: &Cx<'_>,
    comments: &[Comment],
    comment_index: usize,
    column_delta: i64,
) {
    let mut below = comments[comment_index];
    // Walk upward over `comments[0..comment_index]`.
    for above in comments[..comment_index].iter().rev() {
        if !should_correct(cx, *above, below) {
            break;
        }
        autocorrect_one(cx, *above, column_delta);
        below = *above;
    }
}

/// RuboCop's `should_correct?(preceding, reference)`: the preceding comment is
/// exactly one line above the reference and at the same column.
fn should_correct(cx: &Cx<'_>, above: Comment, below: Comment) -> bool {
    if !own_line_comment(cx, above) {
        return false;
    }
    let source = cx.source();
    let above_line = line_of(source, above.range.start);
    let below_line = line_of(source, below.range.start);
    let above_col = column_of(source, above.range.start);
    let below_col = column_of(source, below.range.start);
    above_line + 1 == below_line && above_col == below_col
}

/// RuboCop's `correctly_aligned_with_preceding_comment?`.
///
/// Only meaningful when `AllowForAlignment` is true: scan preceding comments in
/// reverse, and the first one that is **not** own-line (i.e. a trailing/EOL
/// comment) determines the result — accepted iff its column equals `column`.
fn correctly_aligned_with_preceding_comment(
    cx: &Cx<'_>,
    comments: &[Comment],
    comment_index: usize,
    column: usize,
    allow_for_alignment: bool,
) -> bool {
    if !allow_for_alignment {
        return false;
    }
    let source = cx.source();
    for other in comments[..comment_index].iter().rev() {
        if !own_line_comment(cx, *other) {
            return column_of(source, other.range.start) == column;
        }
    }
    false
}

/// RuboCop's `own_line_comment?`: the comment's line matches `/\A\s*#/` — i.e.
/// only whitespace precedes the comment on its line. Block (`=begin`/`=end`)
/// comments are not `#` comments and are excluded.
fn own_line_comment(cx: &Cx<'_>, comment: Comment) -> bool {
    if comment.kind != CommentKind::Inline {
        return false;
    }
    let source = cx.source();
    let line_start = line_start_offset(source, comment.range.start) as usize;
    source[line_start..comment.range.start as usize]
        .bytes()
        .all(|b| b.is_ascii_whitespace())
}

/// RuboCop's `line_after_comment`: the next non-blank source line strictly after
/// the comment's own line. `None` when there is no following non-blank line.
fn line_after_comment(source: &str, comment: Comment) -> Option<&str> {
    let comment_line_start = line_start_offset(source, comment.range.start) as usize;
    // First byte after the comment's line (`lines[comment.loc.line..]`).
    let after = source[comment_line_start..]
        .find('\n')
        .map(|i| comment_line_start + i + 1)?;
    source[after..]
        .lines()
        .find(|line| !line.trim().is_empty())
}

/// RuboCop's `correct_indentation(next_line)`.
fn correct_indentation(next_line: Option<&str>, width: usize) -> usize {
    let Some(line) = next_line else {
        return 0;
    };
    // `indentation_of_next_line = next_line =~ /\S/` — char column of the first
    // non-whitespace char (each whitespace char counts as one column, tabs
    // included). Char-counted to match `column_of`'s char-based columns.
    let indentation = line.chars().take_while(|c| c.is_whitespace()).count();
    indentation + if less_indented(line) { width } else { 0 }
}

/// RuboCop's `less_indented?`: line begins with `end` (word-boundary), or a
/// closing `)`, `}`, `]`. The access-modifier outdent branch is treated as
/// false (non-default sibling-cop config, unreadable across the ABI).
fn less_indented(line: &str) -> bool {
    let trimmed = line.trim_start();
    if let Some(rest) = trimmed.strip_prefix("end") {
        // `end\b` — a word boundary after `end`.
        return rest
            .chars()
            .next()
            .is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
    }
    matches!(trimmed.as_bytes().first(), Some(b')' | b'}' | b']'))
}

/// RuboCop's `two_alternatives?`: line begins with one of the alternation
/// keywords, at a word boundary.
fn two_alternatives(line: &str) -> bool {
    let trimmed = line.trim_start();
    for kw in ["else", "elsif", "when", "in", "rescue", "ensure"] {
        if let Some(rest) = trimmed.strip_prefix(kw)
            && rest
                .chars()
                .next()
                .is_none_or(|c| !(c.is_alphanumeric() || c == '_'))
        {
            return true;
        }
    }
    false
}

/// 0-based column (char count from line start) of byte `offset`.
fn column_of(source: &str, offset: u32) -> usize {
    let line_start = line_start_offset(source, offset) as usize;
    source[line_start..offset as usize].chars().count()
}

/// 1-based source line number of byte `offset`.
fn line_of(source: &str, offset: u32) -> usize {
    source.as_bytes()[..offset as usize]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

/// Byte offset of the first byte on the line containing `offset`.
fn line_start_offset(source: &str, offset: u32) -> u32 {
    source.as_bytes()[..offset as usize]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |pos| pos as u32 + 1)
}

murphy_plugin_api::submit_cop!(CommentIndentation);

#[cfg(test)]
mod tests {
    use super::{CommentIndentation, CommentIndentationOptions};
    use murphy_plugin_api::test_support::{indoc, run_cop_with_options, test};

    fn allow_alignment() -> CommentIndentationOptions {
        CommentIndentationOptions {
            allow_for_alignment: true,
            indentation_width: 2,
        }
    }

    // ── good cases ────────────────────────────────────────────────────────────

    #[test]
    fn accepts_correctly_indented_outer_comment() {
        test::<CommentIndentation>().expect_no_offenses("# comment\n");
    }

    #[test]
    fn accepts_trailing_comment() {
        test::<CommentIndentation>().expect_no_offenses("hello # comment\n");
    }

    #[test]
    fn accepts_comments_around_program_structure_keywords() {
        test::<CommentIndentation>().expect_no_offenses(indoc! {r#"
            #
            def m
              #
              if a
                #
                b
              # this is accepted
              elsif aa
                # this is accepted
              else
                #
              end
              #
            end
            #
        "#});
    }

    #[test]
    fn accepts_comments_near_brackets() {
        test::<CommentIndentation>().expect_no_offenses(indoc! {r#"
            #
            a = {
              #
              x: [
                1
                #
              ],
              #
            }
            #
        "#});
    }

    #[test]
    fn accepts_blank_line_following_comment() {
        test::<CommentIndentation>().expect_no_offenses(indoc! {r#"
            def m
              # comment

            end
        "#});
    }

    // ── offenses ──────────────────────────────────────────────────────────────

    #[test]
    fn flags_indented_outer_comment() {
        // Raw (no `indoc!`) to preserve the comment's column-2 indentation —
        // `indoc!` would strip the common indent shared with the caret line.
        test::<CommentIndentation>().expect_offense(concat!(
            "  # comment\n",
            "  ^^^^^^^^^ Incorrect indentation detected (column 2 instead of 0).\n",
        ));
    }

    #[test]
    fn corrects_indented_outer_comment() {
        test::<CommentIndentation>().expect_correction(
            concat!(
                "  # comment\n",
                "  ^^^^^^^^^ Incorrect indentation detected (column 2 instead of 0).\n",
            ),
            "# comment\n",
        );
    }

    #[test]
    fn flags_each_incorrectly_indented_comment() {
        test::<CommentIndentation>().expect_offense(indoc! {r#"
            # a
            ^^^ Incorrect indentation detected (column 0 instead of 2).
              # b
              ^^^ Incorrect indentation detected (column 2 instead of 4).
                # c
                ^^^ Incorrect indentation detected (column 4 instead of 0).
            # d
            def test; end
        "#});
    }

    #[test]
    fn corrects_independent_comments_in_one_pass() {
        // Each comment's reference line is code (not another comment), so the
        // corrections are independent and converge in a single autocorrect pass.
        // (A chain where each comment references the next comment converges only
        // over multiple production fixpoint passes; the single-pass test harness
        // would not reach RuboCop's fully-settled output for that shape.)
        test::<CommentIndentation>().expect_correction(
            concat!(
                "  # a\n",
                "  ^^^ Incorrect indentation detected (column 2 instead of 0).\n",
                "def foo; end\n",
                "   # b\n",
                "   ^^^ Incorrect indentation detected (column 3 instead of 0).\n",
                "def bar; end\n",
            ),
            "# a\ndef foo; end\n# b\ndef bar; end\n",
        );
    }

    #[test]
    fn flags_comment_before_end_keyword() {
        // The next line `end` is `less_indented?`, so the comment must be at the
        // `end` column + width.
        test::<CommentIndentation>().expect_offense(indoc! {r#"
            if a
              b
            #
            ^ Incorrect indentation detected (column 0 instead of 2).
            end
        "#});
    }

    #[test]
    fn accepts_comment_aligned_with_keyword_below() {
        // `two_alternatives?`: comment may align with the `else` keyword column.
        test::<CommentIndentation>().expect_no_offenses(indoc! {r#"
            if foo
              bar
            # aligned with else
            else
              baz
            end
        "#});
    }

    #[test]
    fn accepts_comment_aligned_with_else_body() {
        // `two_alternatives?`: comment may also align with keyword + width.
        test::<CommentIndentation>().expect_no_offenses(indoc! {r#"
            if foo
              bar
              # aligned with body
            else
              baz
            end
        "#});
    }

    // ── batch correction of equally-indented comment runs ─────────────────────

    #[test]
    fn corrects_run_of_equally_indented_comments_in_one_pass() {
        // Only the last comment of the run (next line is code at column 0) gets a
        // direct offense; the two preceding comments share its column and are
        // batch-corrected.
        test::<CommentIndentation>().expect_correction(
            indoc! {r#"
                  # comment 1
                  # comment 2
                  # comment 3
                  ^^^^^^^^^^^ Incorrect indentation detected (column 2 instead of 0).
                hash1 = { a: 0 }
            "#},
            indoc! {r#"
                # comment 1
                # comment 2
                # comment 3
                hash1 = { a: 0 }
            "#},
        );
    }

    // ── AllowForAlignment ─────────────────────────────────────────────────────

    #[test]
    fn flags_extra_indentation_when_alignment_disabled() {
        // Default `AllowForAlignment: false` — a comment aligned with a preceding
        // trailing comment is still flagged.
        let run = test::<CommentIndentation>();
        run.expect_offense(indoc! {r#"
            x = 1            # trailing
                             # continuation
                             ^^^^^^^^^^^^^^ Incorrect indentation detected (column 17 instead of 0).
        "#});
    }

    #[test]
    fn accepts_extra_indentation_when_alignment_enabled() {
        let source = indoc! {r#"
            x = 1            # trailing
                             # continuation
        "#};
        assert!(
            run_cop_with_options::<CommentIndentation>(source, &allow_alignment()).is_empty(),
            "comment aligned with preceding trailing comment is accepted",
        );
    }
}
