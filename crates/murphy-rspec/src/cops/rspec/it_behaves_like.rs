//! `RSpec/ItBehavesLike` — consistent shared-example inclusion style.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/ItBehavesLike
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `example_inclusion_offense` (`(send _ % ...)`, i.e. any
//!   receiver whose method is the non-preferred spelling) with
//!   `RESTRICT_ON_SEND = [:it_behaves_like, :it_should_behave_like]` and
//!   `EnforcedStyle` (`it_behaves_like` default, `it_should_behave_like`
//!   alternate). Upstream matches any receiver (`_`), so explicit-receiver
//!   forms flag too; argument count is not constrained. The offense range is
//!   the whole node per `add_offense(node)`. Detection is at parity;
//!   autocorrect (replace with the preferred spelling) is not ported in this
//!   batch — same convention as `RSpec/NotToNot` (status: partial, autocorrect
//!   as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with `methods = ["it_behaves_like",
//! "it_should_behave_like"]` (any receiver). Default style `it_behaves_like`:
//!
//! - `it_should_behave_like 'a foo'` — flagged.
//! - `it_behaves_like 'a foo'` — preferred spelling, not flagged.
//!
//! Alternate style `it_should_behave_like`
//! (`EnforcedStyle: it_should_behave_like`) mirrors:
//! `it_behaves_like` flags, `it_should_behave_like` passes.
//!
//! ## No autocorrect
//!
//! Upstream replaces with the preferred spelling. This batch reports only.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ItBehavesLike;

#[derive(CopOptions)]
pub struct ItBehavesLikeOptions {
    #[option(
        name = "EnforcedStyle",
        default = "it_behaves_like",
        description = "Whether to enforce it_behaves_like or it_should_behave_like for shared examples."
    )]
    pub enforced_style: ItBehavesLikeStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum ItBehavesLikeStyle {
    #[option(value = "it_behaves_like")]
    ItBehavesLike,
    #[option(value = "it_should_behave_like")]
    ItShouldBehaveLike,
}

#[cop(
    name = "RSpec/ItBehavesLike",
    description = "Checks that only one it_behaves_like style is used.",
    default_severity = "warning",
    default_enabled = true,
    options = ItBehavesLikeOptions,
)]
impl ItBehavesLike {
    #[on_node(
        kind = "send",
        methods = ["it_behaves_like", "it_should_behave_like"]
    )]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { method, .. } = *cx.kind(node) else {
            return;
        };
        let opts = cx.options_or_default::<ItBehavesLikeOptions>();
        let method_name = cx.symbol_str(method);
        let preferred = match opts.enforced_style {
            ItBehavesLikeStyle::ItBehavesLike => "it_behaves_like",
            ItBehavesLikeStyle::ItShouldBehaveLike => "it_should_behave_like",
        };
        if method_name == preferred {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            &format!(
                "Prefer `{preferred}` over `{method_name}` when including examples in a nested context."
            ),
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{ItBehavesLike, ItBehavesLikeOptions, ItBehavesLikeStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn should_style() -> ItBehavesLikeOptions {
        ItBehavesLikeOptions {
            enforced_style: ItBehavesLikeStyle::ItShouldBehaveLike,
        }
    }

    #[test]
    fn flags_should_in_default_style() {
        test::<ItBehavesLike>().expect_offense(indoc! {r#"
                it_should_behave_like 'a foo'
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `it_behaves_like` over `it_should_behave_like` when including examples in a nested context.
            "#});
    }

    #[test]
    fn does_not_flag_behaves_like_in_default_style() {
        test::<ItBehavesLike>().expect_no_offenses(indoc! {r#"
                it_behaves_like 'a foo'
            "#});
    }

    #[test]
    fn flags_behaves_like_in_should_style() {
        test::<ItBehavesLike>()
            .with_options(&should_style())
            .expect_offense(indoc! {r#"
                it_behaves_like 'a foo'
                ^^^^^^^^^^^^^^^^^^^^^^^ Prefer `it_should_behave_like` over `it_behaves_like` when including examples in a nested context.
            "#});
    }

    #[test]
    fn does_not_flag_should_in_should_style() {
        test::<ItBehavesLike>()
            .with_options(&should_style())
            .expect_no_offenses(indoc! {r#"
                it_should_behave_like 'a foo'
            "#});
    }
}

murphy_plugin_api::submit_cop!(ItBehavesLike);
