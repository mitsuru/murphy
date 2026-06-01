//! `Lint/EmptyWhen` — flag a `when` branch whose body is empty.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/EmptyWhen
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues:
//!   - murphy-9cr.9
//! notes: >
//!   Message text aligned with RuboCop MSG. AllowComments default (true) matches RuboCop. AllowComments:false option override is ABI-blocked (options not wired through Cx until murphy-9cr.9).
//! ```
//!
//!
//! ## Defaults that mirror RuboCop
//!
//! - **`AllowComments`** (default `true`): a `when` branch whose body
//!   region contains only a comment (`when 1; # noop`) is treated as
//!   intentionally empty and not flagged. The `allow_comments = false`
//!   override is exported in the schema but is not yet wired at
//!   dispatch time — see the "Known v1 limitation" note below.
//!
//! ## Known v1 limitation: option overrides not wired through `Cx`
//!
//! `allow_comments` is exported via `#[derive(CopOptions)]` so the host
//! validates `[cops.rules."Lint/EmptyWhen"]` keys, but runtime reads
//! still come from `Options::default()`. `murphy-9cr.9` will route
//! overrides through `Cx`; until then, setting
//! `allow_comments = false` in `murphy.toml` has no effect at dispatch
//! time. This is the same shape as `RSpec/ExampleLength` and the new
//! `Lint/UnusedMethodArgument` options — see `references/options.md`
//! in the port-rubocop-cop skill.
//!
//! ## Body-region heuristic
//!
//! Murphy's `When` node range ends right after the conditions (`when 1`
//! is the whole node when the body is empty), so to test "is there a
//! comment in the body region" the cop walks up to the parent `Case`,
//! finds this `When`'s index, and uses the start of the next sibling
//! (`When` / `else` / the `end` keyword via `Case.range.end`) as the
//! exclusive upper bound. Any comment whose start falls in that
//! half-open interval counts.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};

#[derive(Default)]
pub struct EmptyWhen;

/// Cop options for [`EmptyWhen`]. v1: read from `Default` at dispatch
/// time (`murphy-9cr.9` will wire live overrides through `Cx`).
#[derive(CopOptions)]
pub struct Options {
    #[option(
        default = true,
        description = "When true, don't flag a when branch whose body region contains only a comment."
    )]
    pub allow_comments: bool,
}

#[cop(
    name = "Lint/EmptyWhen",
    description = "Flag when branches without a body.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl EmptyWhen {
    #[on_node(kind = "when")]
    fn check_when(&self, node: NodeId, cx: &Cx<'_>) {
        if cx.when_body(node).get().is_some() {
            return;
        }
        let opts = Options::default();
        if opts.allow_comments
            && let Some(region) = empty_when_body_region(cx, node)
            && region_has_comment(cx, region)
        {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            "Avoid `when` branches without a body.",
            None,
        );
    }
}

/// The byte range that would hold this empty `when`'s body if it had
/// one — i.e. the source between the `when` selector's end and the next
/// sibling. `None` if the parent shape is unexpected.
fn empty_when_body_region(cx: &Cx<'_>, when_id: NodeId) -> Option<Range> {
    let parent_id = cx.parent(when_id).get()?;
    if !matches!(cx.kind(parent_id), NodeKind::Case { .. }) {
        return None;
    }
    let when_list = cx.case_when_branches(parent_id);
    let idx = when_list.iter().position(|&w| w == when_id)?;
    let next_start = if idx + 1 < when_list.len() {
        cx.range(when_list[idx + 1]).start
    } else if let Some(else_id) = cx.case_else_branch(parent_id).get() {
        cx.range(else_id).start
    } else {
        // No next sibling — the `end` keyword closes the `case`. Murphy
        // doesn't expose the `end` keyword location directly, so use
        // the `Case`'s range end (just past `end`); any trailing whitespace
        // or comment before `end` falls inside this window.
        cx.range(parent_id).end
    };
    Some(Range {
        start: cx.range(when_id).end,
        end: next_start,
    })
}

fn region_has_comment(cx: &Cx<'_>, region: Range) -> bool {
    !cx.comments_in_range(region).is_empty()
}

#[cfg(test)]
mod tests {
    use super::EmptyWhen;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_empty_when() {
        test::<EmptyWhen>().expect_offense(indoc! {r#"
            case value
            when 1
            ^^^^^^ Avoid `when` branches without a body.
            when 2
              :ok
            end
        "#});
    }

    #[test]
    fn ignores_non_empty_when_with_multibyte_body() {
        test::<EmptyWhen>().expect_no_offenses("case x\nwhen 1\n  名前\nend\n");
    }

    // murphy-aj9q: AllowComments (default true).

    #[test]
    fn comment_only_body_is_allowed_by_default() {
        test::<EmptyWhen>().expect_no_offenses(indoc! {r#"
                case value
                when 1
                  # noop
                when 2
                  :ok
                end
            "#});
    }

    #[test]
    fn comment_in_last_when_before_end_is_allowed() {
        test::<EmptyWhen>().expect_no_offenses(indoc! {r#"
                case value
                when 1
                  :ok
                when 2
                  # noop
                end
            "#});
    }

    #[test]
    fn comment_in_when_before_else_is_allowed() {
        test::<EmptyWhen>().expect_no_offenses(indoc! {r#"
                case value
                when 1
                  # noop
                else
                  :ok
                end
            "#});
    }

    #[test]
    fn empty_when_with_no_comment_is_still_flagged() {
        test::<EmptyWhen>().expect_offense(indoc! {r#"
                case value
                when 1
                ^^^^^^ Avoid `when` branches without a body.
                when 2
                  :ok
                end
            "#});
    }

    #[test]
    fn comment_outside_when_body_does_not_save_other_empty_when() {
        // The `# noop` belongs to `when 2`'s body, not `when 1`'s.
        test::<EmptyWhen>().expect_offense(indoc! {r#"
                case value
                when 1
                ^^^^^^ Avoid `when` branches without a body.
                when 2
                  # noop
                  :ok
                end
            "#});
    }

    #[test]
    fn offense_message_matches_rubocop_verbatim() {
        // Pins RuboCop's MSG = 'Avoid `when` branches without a body.'
        test::<EmptyWhen>().expect_offense(indoc! {r#"
            case x
            when 1
            ^^^^^^ Avoid `when` branches without a body.
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(EmptyWhen);
