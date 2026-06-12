//! `Layout/SpaceAroundBlockParameters` — checks the spacing inside and after
//! block-parameter pipes (`{ |x, y| ... }`).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/SpaceAroundBlockParameters
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues:
//!   - murphy-uynf
//! notes: >
//!   Dispatches on `Block` and checks pipe spacing for `|...|`-delimited block
//!   parameters. Pipes are located by their adjacency to the first/last arg
//!   node (`token_before(first.start)` / `token_after(last.end)`), so the cop
//!   works regardless of what the `Args`-list range covers. Both
//!   `EnforcedStyleInsidePipes` styles are supported: `no_space` (default) and
//!   `space`. The between-arg "Extra space before block parameter detected."
//!   check runs in both styles, matching RuboCop's unconditional
//!   `check_each_arg`. Multiline gaps are skipped (deferred to
//!   `Layout/MultilineBlockLayout`). Gaps (murphy-uynf): lambda paren-delimited
//!   params (`->(x, y)`), trailing comma (`|x,|`), mlhs destructuring
//!   (`|(a, b)|`), and block-local variables (`|x; y|`) are not checked — the
//!   pipe-adjacency lookup bails gracefully on those shapes.
//! ```
//!
//! ## Options
//!
//! - `EnforcedStyleInsidePipes` (`no_space` | `space`, default `no_space`) —
//!   `no_space`: `|x, y|`; `space`: `| x, y |`.
//!
//! ## Autocorrect
//!
//! Inserts a missing space (`check_space`) or removes an extra space
//! (`check_no_space`) at the offending position.

use murphy_plugin_api::{
    CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, SourceTokenKind, cop,
};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SpaceAroundBlockParameters;

#[derive(CopOptions)]
pub struct SpaceAroundBlockParametersOptions {
    #[option(
        name = "EnforcedStyleInsidePipes",
        default = "no_space",
        description = "Spacing style immediately inside the block-parameter pipes."
    )]
    pub enforced_style_inside_pipes: InsidePipesStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum InsidePipesStyle {
    #[option(value = "no_space")]
    NoSpace,
    #[option(value = "space")]
    Space,
}

#[cop(
    name = "Layout/SpaceAroundBlockParameters",
    description = "Check spacing inside and after block-parameter pipes.",
    default_severity = "warning",
    default_enabled = true,
    options = SpaceAroundBlockParametersOptions,
)]
impl SpaceAroundBlockParameters {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { args, body, .. } = *cx.kind(node) else {
            return;
        };
        let NodeKind::Args(list) = *cx.kind(args) else {
            return;
        };
        let arg_ids = cx.list(list);
        // `node.arguments?` — bail on empty (`{ }` / `{ || }`).
        if arg_ids.is_empty() {
            return;
        }

        let first = arg_ids[0];
        let last = *arg_ids.last().unwrap();

        // Locate the opening/closing pipes by adjacency to the first/last arg.
        // Bails (no offense) on lambda paren params, trailing commas, etc.
        let Some(open) = cx.token_before(cx.range(first).start) else {
            return;
        };
        if !is_pipe(cx, open) {
            return;
        }
        let Some(close) = cx.token_after(cx.range(last).end) else {
            return;
        };
        if !is_pipe(cx, close) {
            return;
        }

        let style = cx
            .options_or_default::<SpaceAroundBlockParametersOptions>()
            .enforced_style_inside_pipes;

        // --- check_inside_pipes ---
        match style {
            InsidePipesStyle::NoSpace => {
                check_no_space(
                    cx,
                    open.range.end,
                    cx.range(first).start,
                    "Space before first",
                );
                check_no_space(
                    cx,
                    cx.range(last).end,
                    close.range.start,
                    "Space after last",
                );
            }
            InsidePipesStyle::Space => {
                // before-first: space required after opening pipe. RuboCop
                // highlights the first arg node and inserts before it.
                check_space(
                    cx,
                    open.range.end,
                    cx.range(first).start,
                    cx.range(first),
                    cx.range(first).start,
                    "before first block parameter",
                );
                // extra space before first (more than one).
                if cx.range(first).start > open.range.end {
                    check_no_space(
                        cx,
                        open.range.end,
                        cx.range(first).start - 1,
                        "Extra space before first",
                    );
                }
                // after-last: space required before closing pipe. RuboCop
                // highlights the last arg node and inserts after it.
                check_space(
                    cx,
                    cx.range(last).end,
                    close.range.start,
                    cx.range(last),
                    cx.range(last).end,
                    "after last block parameter",
                );
                // extra space after last (more than one).
                check_no_space(
                    cx,
                    cx.range(last).end + 1,
                    close.range.start,
                    "Extra space after last",
                );
            }
        }

        // --- check_after_closing_pipe (only when block has a body) ---
        // RuboCop highlights the closing pipe and inserts after it.
        if let Some(body_id) = body.get() {
            check_space(
                cx,
                close.range.end,
                cx.range(body_id).start,
                close.range,
                close.range.end,
                "after closing `|`",
            );
        }

        // --- check_each_arg (both styles): extra space before each arg ---
        for &arg in arg_ids {
            check_each_arg_extra_space(cx, arg);
        }
    }
}

/// `true` when `token` is a pipe (`Other` kind, source text `|`).
fn is_pipe(cx: &Cx<'_>, token: murphy_plugin_api::SourceToken) -> bool {
    token.kind == SourceTokenKind::Other && cx.raw_source(token.range) == "|"
}

/// RuboCop's `check_no_space`: offense when `[begin_pos, end_pos)` is a
/// non-empty single-line run of whitespace; the autocorrect removes it.
fn check_no_space(cx: &Cx<'_>, begin_pos: u32, end_pos: u32, msg: &str) {
    if begin_pos >= end_pos {
        return;
    }
    let range = Range {
        start: begin_pos,
        end: end_pos,
    };
    let source = cx.raw_source(range);
    if source.contains('\n') {
        return;
    }
    cx.emit_offense(range, &format!("{msg} block parameter detected."), None);
    cx.emit_edit(range, "");
}

/// RuboCop's `check_space`: offense when `[begin_pos, end_pos)` is empty (no
/// space where one is required). `target` is the highlighted neighbor (the
/// adjacent pipe or argument node, matching RuboCop's `add_offense(target)`),
/// and `insert_at` is the byte position where the corrective space is
/// inserted (`target.start` for before-first, `target.end` otherwise).
fn check_space(cx: &Cx<'_>, begin_pos: u32, end_pos: u32, target: Range, insert_at: u32, msg: &str) {
    if begin_pos != end_pos {
        return;
    }
    cx.emit_offense(target, &format!("Space {msg} missing."), None);
    cx.emit_edit(
        Range {
            start: insert_at,
            end: insert_at,
        },
        " ",
    );
}

/// RuboCop's `check_arg` — extra space immediately before an argument.
/// `range_with_surrounding_space(side: :left)` expands left over whitespace;
/// the offense is `[expanded_start, arg.start - 1)`.
fn check_each_arg_extra_space(cx: &Cx<'_>, arg: NodeId) {
    // mlhs destructuring (`|(a, b)|`) recurses in RuboCop; documented as a gap.
    let arg_start = cx.range(arg).start;
    let src = cx.source().as_bytes();
    let mut expanded = arg_start as usize;
    while expanded > 0 && matches!(src[expanded - 1], b' ' | b'\t') {
        expanded -= 1;
    }
    // `expr.begin_pos - 1` — the run excludes the single space directly before
    // the arg (that one is the allowed separator space).
    check_no_space(cx, expanded as u32, arg_start.saturating_sub(1), "Extra space before");
}

#[cfg(test)]
mod tests {
    use super::{
        InsidePipesStyle, SpaceAroundBlockParameters, SpaceAroundBlockParametersOptions,
    };
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn options_default_is_no_space() {
        let d = SpaceAroundBlockParametersOptions::default();
        assert_eq!(d.enforced_style_inside_pipes, InsidePipesStyle::NoSpace);
    }

    // ---------- no_space style (default) ----------

    #[test]
    fn accepts_canonical_no_space() {
        test::<SpaceAroundBlockParameters>().expect_no_offenses("{}.each { |x, y| puts x }\n");
    }

    #[test]
    fn flags_space_before_first_param() {
        test::<SpaceAroundBlockParameters>().expect_offense(indoc! {r#"
            {}.each { | x, y| puts x }
                       ^ Space before first block parameter detected.
        "#});
    }

    #[test]
    fn flags_space_after_last_param() {
        test::<SpaceAroundBlockParameters>().expect_offense(indoc! {r#"
            {}.each { |x, y | puts x }
                           ^ Space after last block parameter detected.
        "#});
    }

    #[test]
    fn flags_extra_space_between_params() {
        test::<SpaceAroundBlockParameters>().expect_offense(indoc! {r#"
            {}.each { |x,  y| puts x }
                         ^ Extra space before block parameter detected.
        "#});
    }

    #[test]
    fn flags_missing_space_after_closing_pipe() {
        test::<SpaceAroundBlockParameters>().expect_offense(indoc! {r#"
            {}.each { |x, y|puts x }
                           ^ Space after closing `|` missing.
        "#});
    }

    #[test]
    fn corrects_space_before_first_no_space_style() {
        test::<SpaceAroundBlockParameters>().expect_correction(
            indoc! {r#"
                {}.each { | x, y| puts x }
                           ^ Space before first block parameter detected.
            "#},
            "{}.each { |x, y| puts x }\n",
        );
    }

    #[test]
    fn corrects_missing_space_after_closing_pipe() {
        test::<SpaceAroundBlockParameters>().expect_correction(
            indoc! {r#"
                {}.each { |x, y|puts x }
                               ^ Space after closing `|` missing.
            "#},
            "{}.each { |x, y| puts x }\n",
        );
    }

    #[test]
    fn no_offense_for_block_without_params() {
        test::<SpaceAroundBlockParameters>().expect_no_offenses("{}.each { puts 1 }\n");
    }

    #[test]
    fn ignores_lambda_paren_params() {
        // `->(x,  y)` uses parens, not pipes — documented gap, no offense.
        test::<SpaceAroundBlockParameters>().expect_no_offenses("->(x,  y) { puts x }\n");
    }

    // ---------- space style ----------

    #[test]
    fn space_style_accepts_canonical() {
        test::<SpaceAroundBlockParameters>()
            .with_options(&SpaceAroundBlockParametersOptions {
                enforced_style_inside_pipes: InsidePipesStyle::Space,
            })
            .expect_no_offenses("{}.each { | x, y | puts x }\n");
    }

    #[test]
    fn space_style_flags_missing_space_before_first() {
        test::<SpaceAroundBlockParameters>()
            .with_options(&SpaceAroundBlockParametersOptions {
                enforced_style_inside_pipes: InsidePipesStyle::Space,
            })
            .expect_offense(indoc! {r#"
                {}.each { |x, y | puts x }
                           ^ Space before first block parameter missing.
            "#});
    }

    #[test]
    fn space_style_flags_missing_space_after_last() {
        test::<SpaceAroundBlockParameters>()
            .with_options(&SpaceAroundBlockParametersOptions {
                enforced_style_inside_pipes: InsidePipesStyle::Space,
            })
            .expect_offense(indoc! {r#"
                {}.each { | x, y| puts x }
                               ^ Space after last block parameter missing.
            "#});
    }

    #[test]
    fn space_style_corrects_missing_both_sides() {
        test::<SpaceAroundBlockParameters>()
            .with_options(&SpaceAroundBlockParametersOptions {
                enforced_style_inside_pipes: InsidePipesStyle::Space,
            })
            .expect_correction(
                indoc! {r#"
                    {}.each { |x, y| puts x }
                               ^ Space before first block parameter missing.
                                  ^ Space after last block parameter missing.
                "#},
                "{}.each { | x, y | puts x }\n",
            );
    }

    #[test]
    fn between_arg_extra_space_flagged_in_space_style_too() {
        test::<SpaceAroundBlockParameters>()
            .with_options(&SpaceAroundBlockParametersOptions {
                enforced_style_inside_pipes: InsidePipesStyle::Space,
            })
            .expect_offense(indoc! {r#"
                {}.each { | x,  y | puts x }
                              ^ Extra space before block parameter detected.
            "#});
    }

    #[test]
    fn leaves_clean_program_without_corrections() {
        test::<SpaceAroundBlockParameters>().expect_no_corrections("{}.each { |x, y| puts x }\n");
    }
}

murphy_plugin_api::submit_cop!(SpaceAroundBlockParameters);
