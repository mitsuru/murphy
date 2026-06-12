//! `Lint/EmptyInPattern` — flag pattern-match `in` branches without a body.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/EmptyInPattern
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Message and default AllowComments:true behavior mirror RuboCop; the
//!   AllowComments override is read live via cx.options_or_default. The
//!   comment-region heuristic ([in.end, next_sibling.start)) is verified
//!   against RuboCop's `allow_comments?`, including the
//!   `comments_contain_disables?` nuance via
//!   `crate::cops::util::region_has_allowing_comment`: a `rubocop:disable
//!   Lint/EmptyInPattern` (or `disable all`) covering the branch is NOT an
//!   allowing comment, so the empty `in` branch is still flagged; a directive
//!   naming a different cop is an ordinary allowing comment (murphy-6rhg
//!   closed). Scope seam vs RuboCop: only directives *within* the branch body
//!   region are detected; RuboCop's `comments_contain_disables?` uses
//!   line-range coverage, so a block-level disable placed *above* the `case`
//!   would also cover the branch — Murphy does not detect that out-of-region
//!   form (the in-branch form, the common case, matches).

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};

#[derive(Default)]
pub struct EmptyInPattern;

#[derive(CopOptions)]
pub struct Options {
    #[option(name = "AllowComments", default = true, description = "When true, don't flag an empty in branch whose body region contains a comment.")]
    pub allow_comments: bool,
}

#[cop(
    name = "Lint/EmptyInPattern",
    description = "Flag in pattern branches without a body.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl EmptyInPattern {
    #[on_node(kind = "in_pattern")]
    fn check_in_pattern(&self, node: NodeId, cx: &Cx<'_>) {
        if cx.in_pattern_body(node).get().is_some() {
            return;
        }
        let opts = cx.options_or_default::<Options>();
        if opts.allow_comments
            && let Some(region) = empty_in_body_region(cx, node)
            && crate::cops::util::region_has_allowing_comment(cx, region, "Lint/EmptyInPattern")
        {
            return;
        }
        cx.emit_offense(cx.range(node), "Avoid `in` branches without a body.", None);
    }
}

fn empty_in_body_region(cx: &Cx<'_>, in_id: NodeId) -> Option<Range> {
    let parent_id = cx.parent(in_id).get()?;
    if !matches!(cx.kind(parent_id), NodeKind::CaseMatch { .. }) {
        return None;
    }
    let branches = cx.in_pattern_branches(parent_id);
    let idx = branches.iter().position(|&branch| branch == in_id)?;
    let next_start = if idx + 1 < branches.len() {
        cx.range(branches[idx + 1]).start
    } else if let Some(else_id) = cx.case_match_else_branch(parent_id).get() {
        cx.range(else_id).start
    } else {
        cx.range(parent_id).end
    };
    Some(Range { start: cx.range(in_id).end, end: next_start })
}

murphy_plugin_api::submit_cop!(EmptyInPattern);

#[cfg(test)]
mod tests {
    use super::{EmptyInPattern, Options};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_empty_in_pattern() {
        test::<EmptyInPattern>().expect_offense(indoc! {r#"
            case condition
            in [a]
              do_something
            in [a, b]
            ^^^^^^^^^ Avoid `in` branches without a body.
            end
        "#});
    }

    #[test]
    fn allows_comment_only_branch_by_default() {
        test::<EmptyInPattern>().expect_no_offenses(indoc! {r#"
            case condition
            in [a]
              do_something
            in [a, b]
              # noop
            end
        "#});
    }

    #[test]
    fn flags_comment_only_branch_when_comments_are_not_allowed() {
        test::<EmptyInPattern>()
            .with_options(&Options { allow_comments: false })
            .expect_offense(indoc! {r#"
                case condition
                in [a]
                  do_something
                in [a, b]
                ^^^^^^^^^ Avoid `in` branches without a body.
                  # noop
                end
            "#});
    }

    // murphy-6rhg: `comments_contain_disables?` — a `rubocop:disable` directive
    // for this cop must NOT count as an allowing comment.

    #[test]
    fn disable_directive_comment_is_not_an_allowing_comment() {
        test::<EmptyInPattern>().expect_offense(indoc! {r#"
            case condition
            in [a]
              do_something
            in [a, b]
            ^^^^^^^^^ Avoid `in` branches without a body.
              # rubocop:disable Lint/EmptyInPattern
            end
        "#});
    }

    #[test]
    fn disable_all_directive_comment_is_not_an_allowing_comment() {
        test::<EmptyInPattern>().expect_offense(indoc! {r#"
            case condition
            in [a]
              do_something
            in [a, b]
            ^^^^^^^^^ Avoid `in` branches without a body.
              # rubocop:disable all
            end
        "#});
    }

    #[test]
    fn disable_directive_for_other_cop_still_allows_comment() {
        test::<EmptyInPattern>().expect_no_offenses(indoc! {r#"
            case condition
            in [a]
              do_something
            in [a, b]
              # rubocop:disable Lint/SomethingElse
            end
        "#});
    }
}
