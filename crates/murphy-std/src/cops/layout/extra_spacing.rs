//! `Layout/ExtraSpacing` — flag unnecessary (more-than-one) spacing between
//! tokens on the same line.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/ExtraSpacing
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Implements the core `MSG_UNNECESSARY` behaviour plus `AllowForAlignment`
//!   (default true) and `AllowBeforeTrailingComments` (default false). For
//!   each consecutive token pair on the same line, when the gap between
//!   `token1.end` and `token2.start` is >= 2 characters, the all-but-one-space
//!   range `[token1.end, token2.start - 1)` is reported and the autocorrect
//!   removes it (leaving exactly one space), mirroring upstream's
//!   `range_between(token1.end_pos, token2.begin_pos - 1)` +
//!   `corrector.remove(range)`. The "same line" guard is RuboCop's
//!   `token1.line != token2.line`: a pair whose gap spans a newline (leading
//!   indentation, the `(newline, first-token)` pair) is skipped, so indented
//!   code is never flagged. `AllowForAlignment` reuses the shared vertical-
//!   alignment heuristic `is_alignment_at_column` (RuboCop's
//!   `aligned_with_something?`); aligned trailing comments are exempted via a
//!   precomputed same-column comment set (`aligned_locations`). Multiline-hash
//!   key->value gaps are excluded (`ignored_ranges`) since `Layout/HashAlignment`
//!   owns them.
//!   Gaps (documented, not bypassed):
//!     - `ForceEqualSignAlignment` (default false) is NOT implemented. It
//!       forces `=` on consecutive assignment lines to align vertically, which
//!       requires the multi-line `AlignmentCorrector`-style edit machinery
//!       (insert/remove leading spaces across a window of assignment tokens)
//!       that is not yet available across the single-surface ABI. With the
//!       default config (false) the cop's behaviour is unaffected; only users
//!       who opt in lose the `=`-alignment offense + correction.
//! ```

use murphy_plugin_api::{
    CopOptions, Cx, NodeId, NodeKind, Range, SourceToken, SourceTokenKind, cop,
};
use std::collections::HashSet;

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ExtraSpacing;

const MSG_UNNECESSARY: &str = "Unnecessary spacing detected.";

/// Options for [`ExtraSpacing`].
#[derive(CopOptions)]
pub struct ExtraSpacingOptions {
    #[option(
        name = "AllowForAlignment",
        default = true,
        description = "Allow extra spacing when it aligns a token with one on an adjacent line."
    )]
    pub allow_for_alignment: bool,

    #[option(
        name = "AllowBeforeTrailingComments",
        default = false,
        description = "Allow extra spacing before an end-of-line comment."
    )]
    pub allow_before_trailing_comments: bool,

    #[option(
        name = "ForceEqualSignAlignment",
        default = false,
        description = "Force `=` on consecutive assignment lines to align vertically (not yet implemented)."
    )]
    pub force_equal_sign_alignment: bool,
}

#[cop(
    name = "Layout/ExtraSpacing",
    description = "Flag unnecessary (more-than-one) spacing between tokens on the same line.",
    default_severity = "warning",
    default_enabled = true,
    options = ExtraSpacingOptions,
)]
impl ExtraSpacing {
    #[on_new_investigation]
    fn investigate(&self, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<ExtraSpacingOptions>();
        let src = cx.source().as_bytes();

        // RuboCop: `return if processed_source.blank?`.
        if src.is_empty() {
            return;
        }

        // Lines (1-based) on which a comment shares a column with an adjacent
        // comment — RuboCop's `@aligned_comments`. Used to exempt aligned
        // trailing comments under `AllowForAlignment`.
        let aligned_comment_lines = aligned_comment_lines(cx);

        // Byte ranges of multiline-hash key->value gaps, owned by
        // `Layout/HashAlignment` (RuboCop's `ignored_ranges`).
        let ignored = ignored_ranges(cx);

        for pair in cx.sorted_tokens().windows(2) {
            let token1 = pair[0];
            let token2 = pair[1];

            // RuboCop `check_tokens`: `return if token2.type == :tNL`.
            if matches!(
                token2.kind,
                SourceTokenKind::Newline | SourceTokenKind::IgnoredNewline
            ) {
                continue;
            }

            // RuboCop `check_other`:
            // `return false if allow_for_trailing_comments? && token2.text.start_with?('#')`.
            if opts.allow_before_trailing_comments && token2.kind == SourceTokenKind::Comment {
                continue;
            }

            // RuboCop `extra_space_range`: `return if token1.line != token2.line`.
            // Two tokens are on the same line iff no `\n` falls in the gap
            // between them (Murphy folds trailing newlines into the preceding
            // token, so the `(newline, first-token-of-next-line)` pair has a
            // newline inside `token1` whose end abuts the gap). Checking the
            // span `[token1.start, token2.start)` for any newline subsumes both
            // a newline inside token1 and a newline in the inter-token gap.
            if span_has_newline(src, token1.range.start, token2.range.start) {
                continue;
            }

            // RuboCop `extra_space_range`:
            //   start_pos = token1.end_pos
            //   end_pos   = token2.begin_pos - 1
            //   return if end_pos <= start_pos
            // The offense covers all-but-one space; the correction removes it,
            // leaving exactly one separating space. So fire only when the gap
            // is >= 2 characters.
            let start_pos = token1.range.end;
            // token2.range.start >= start_pos always (sorted, non-overlapping);
            // guard the subtraction defensively.
            if token2.range.start == 0 {
                continue;
            }
            let end_pos = token2.range.start - 1;
            if end_pos <= start_pos {
                continue;
            }

            let range = Range {
                start: start_pos,
                end: end_pos,
            };

            // RuboCop `extra_space_range`:
            // `return if allow_for_alignment? && aligned_tok?(token2)`.
            if opts.allow_for_alignment && aligned_tok(src, token2, &aligned_comment_lines) {
                continue;
            }

            // RuboCop `check_other`: `next if ignored_range?(ast, range.begin_pos)`.
            if ignored
                .iter()
                .any(|r| start_pos >= r.start && start_pos < r.end)
            {
                continue;
            }

            cx.emit_offense(range, MSG_UNNECESSARY, None);
            // RuboCop: `corrector.remove(range)`.
            cx.emit_edit(range, "");
        }
    }
}

/// RuboCop's `aligned_tok?`: a comment token is aligned when its line is in
/// the precomputed same-column comment set; any other token is aligned when a
/// non-whitespace character sits at the same column on an adjacent line.
fn aligned_tok(
    src: &[u8],
    token: SourceToken,
    aligned_comment_lines: &HashSet<usize>,
) -> bool {
    if token.kind == SourceTokenKind::Comment {
        aligned_comment_lines.contains(&line_of(src, token.range.start as usize))
    } else {
        crate::cops::util::is_alignment_at_column(src, token.range.start as usize)
    }
}

/// RuboCop's `aligned_locations(comments)`: the set of source lines (1-based)
/// holding a comment that shares a column with an immediately adjacent comment.
fn aligned_comment_lines(cx: &Cx<'_>) -> HashSet<usize> {
    let src = cx.source().as_bytes();
    let comments = cx.comments();
    let mut aligned = HashSet::new();
    for win in comments.windows(2) {
        let c1 = win[0];
        let c2 = win[1];
        let col1 = column_of(src, c1.range.start as usize);
        let col2 = column_of(src, c2.range.start as usize);
        if col1 == col2 {
            aligned.insert(line_of(src, c1.range.start as usize));
            aligned.insert(line_of(src, c2.range.start as usize));
        }
    }
    aligned
}

/// RuboCop's `ignored_ranges`: the key-end .. value-start gaps of every pair
/// inside a multiline hash. `Layout/HashAlignment` owns those, so this cop
/// stays out of them.
fn ignored_ranges(cx: &Cx<'_>) -> Vec<Range> {
    let mut ranges = Vec::new();
    // Fast-path: a multiline hash needs a `\n`. When the source is a single
    // line there is no multiline hash, so skip the full-tree descendants walk
    // (a per-file Vec allocation) entirely. NB: do NOT also gate on `{` — a
    // braceless multiline hash (`foo(\n  a: 1,\n  bb: 2\n)`) is still a
    // `NodeKind::Hash` whose pair gaps `Layout/HashAlignment` owns.
    let src = cx.source().as_bytes();
    if !src.contains(&b'\n') {
        return ranges;
    }
    for &node in cx.descendants(cx.root()).iter() {
        if !matches!(*cx.kind(node), NodeKind::Hash(_)) {
            continue;
        }
        if is_single_line_hash(node, cx) {
            continue;
        }
        for &pair in cx.hash_pairs(node).iter() {
            let children = cx.children(pair);
            let (Some(&key), Some(&value)) = (children.first(), children.get(1)) else {
                continue;
            };
            let key_end = cx.range(key).end;
            let value_start = cx.range(value).start;
            if value_start > key_end {
                ranges.push(Range {
                    start: key_end,
                    end: value_start,
                });
            }
        }
    }
    ranges
}

/// Whether the hash literal occupies a single source line (RuboCop's
/// `node.single_line?`): the first and last pair share a line.
fn is_single_line_hash(node: NodeId, cx: &Cx<'_>) -> bool {
    let pairs = cx.hash_pairs(node);
    let (Some(&first), Some(&last)) = (pairs.first(), pairs.last()) else {
        return true;
    };
    let src = cx.source().as_bytes();
    line_of(src, cx.range(first).start as usize)
        == line_of(src, cx.range(last).end.saturating_sub(1) as usize)
}

/// True if any `\n` falls in `src[start..end)`.
fn span_has_newline(src: &[u8], start: u32, end: u32) -> bool {
    let (start, end) = (start as usize, end as usize);
    if start >= end || end > src.len() {
        // A degenerate or out-of-bounds span carries no newline of concern.
        return false;
    }
    src[start..end].contains(&b'\n')
}

/// 1-based source line number containing byte `offset`.
fn line_of(src: &[u8], offset: usize) -> usize {
    src[..offset.min(src.len())]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

/// 0-based column (char count) of `offset` within its source line.
fn column_of(src: &[u8], offset: usize) -> usize {
    let offset = offset.min(src.len());
    let line_start = src[..offset]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |p| p + 1);
    std::str::from_utf8(&src[line_start..offset])
        .map(|s| s.chars().count())
        .unwrap_or(offset - line_start)
}

#[cfg(test)]
mod tests {
    use super::{ExtraSpacing, ExtraSpacingOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    fn no_align() -> ExtraSpacingOptions {
        ExtraSpacingOptions {
            allow_for_alignment: false,
            allow_before_trailing_comments: false,
            force_equal_sign_alignment: false,
        }
    }

    fn allow_trailing_comments() -> ExtraSpacingOptions {
        ExtraSpacingOptions {
            allow_for_alignment: false,
            allow_before_trailing_comments: true,
            force_equal_sign_alignment: false,
        }
    }

    // ----- Core offense + correction --------------------------------

    #[test]
    fn flags_and_corrects_double_space_between_tokens() {
        // "x =  1": two spaces after `=`. Single line, no adjacent line to
        // align to, so even the default AllowForAlignment doesn't exempt it.
        // Offense covers all-but-one space; the correction leaves "x = 1".
        test::<ExtraSpacing>().expect_correction(
            indoc! {r#"
                x =  1
                   ^ Unnecessary spacing detected.
            "#},
            "x = 1\n",
        );
    }

    #[test]
    fn accepts_single_space() {
        test::<ExtraSpacing>().expect_no_offenses("x = 1\n");
    }

    #[test]
    fn accepts_no_space() {
        test::<ExtraSpacing>().expect_no_offenses("set_app(\"RuboCop\")\n");
    }

    // ----- False-positive guards (the safe-port bar) ----------------

    #[test]
    fn accepts_leading_indentation() {
        // The most catastrophic false positive: 2-space method-body indent.
        test::<ExtraSpacing>().expect_no_offenses(indoc! {r#"
            def foo
              bar
            end
        "#});
    }

    #[test]
    fn accepts_deeply_indented_code() {
        test::<ExtraSpacing>().expect_no_offenses(indoc! {r#"
            class C
              def foo
                if x
                  bar
                end
              end
            end
        "#});
    }

    #[test]
    fn accepts_multiple_internal_spaces_in_string() {
        // A string literal with multiple internal spaces is one token — no
        // inter-token gap, so no offense.
        test::<ExtraSpacing>().expect_no_offenses("x = \"a    b\"\n");
    }

    // ----- AllowForAlignment (default true) -------------------------

    #[test]
    fn allows_aligned_assignments_by_default() {
        // Stacked assignments aligned on `=` — the canonical AllowForAlignment
        // case. With the default (true) this is clean.
        test::<ExtraSpacing>().expect_no_offenses(indoc! {r#"
            name     = "RuboCop"
            website += "/rubocop"
        "#});
    }

    #[test]
    fn flags_alignment_when_allow_for_alignment_false() {
        // With AllowForAlignment off, the extra spacing before `=` on the
        // first line (aligned to the longer second line) is flagged.
        let opts = no_align();
        let result = murphy_plugin_api::test_support::run_cop_with_options::<ExtraSpacing>(
            "name     = 1\nwebsite += 2\n",
            &opts,
        );
        assert!(
            !result.is_empty(),
            "expected an offense with AllowForAlignment off, got none"
        );
        assert!(result.iter().all(|o| o.message == super::MSG_UNNECESSARY));
    }

    // ----- AllowBeforeTrailingComments (default false) --------------

    #[test]
    fn flags_extra_space_before_comment_by_default() {
        // "object.method(arg)  # comment": default config flags the double
        // space before the comment.
        let result = murphy_plugin_api::test_support::run_cop::<ExtraSpacing>(
            "object.method(arg)  # comment\n",
        );
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].message, super::MSG_UNNECESSARY);
    }

    #[test]
    fn allows_extra_space_before_comment_when_opted_in() {
        test::<ExtraSpacing>()
            .with_options(&allow_trailing_comments())
            .expect_no_offenses("object.method(arg)  # comment\n");
    }

    #[test]
    fn allows_aligned_trailing_comments_by_default() {
        // Trailing comments aligned to the same column are exempt under the
        // default AllowForAlignment via the aligned-comment set.
        test::<ExtraSpacing>().expect_no_offenses(indoc! {r#"
            object.method(arg)         # this is a comment
            another_object.method(arg) # this is another comment
            some_object.method(arg)    # this is some comment
        "#});
    }

    // ----- Multiline hash key->value gaps (ignored_ranges) ----------

    #[test]
    fn ignores_multiline_hash_key_value_gap() {
        // Extra spacing between key and value in a multiline hash is owned by
        // Layout/HashAlignment — ExtraSpacing must not double-report it.
        test::<ExtraSpacing>().expect_no_offenses(indoc! {r#"
            h = {
              a   => 1,
              bb  => 2,
            }
        "#});
    }

    #[test]
    fn ignores_braceless_multiline_hash_key_value_gap() {
        // A braceless multiline kwargs hash has no `{`, but its pair gaps are
        // still owned by Layout/HashAlignment. The fast-path must not skip it.
        test::<ExtraSpacing>().expect_no_offenses(indoc! {r#"
            foo(
              a:   1,
              bb:  2,
            )
        "#});
    }
}

murphy_plugin_api::submit_cop!(ExtraSpacing);
