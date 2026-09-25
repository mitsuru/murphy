//! `RSpec/NotToNot` — consistent negated-expectation spelling.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/NotToNot
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `not_to_not_offense` (`(send _ % ...)`, i.e. any
//!   receiver whose method is the non-preferred spelling) with
//!   `RESTRICT_ON_SEND = [:not_to, :to_not]` and `EnforcedStyle`
//!   (`not_to` default, `to_not` alternate). Upstream matches any receiver
//!   (`_`), so explicit-receiver forms flag too; argument count is not
//!   constrained. The offense range is the selector per
//!   `add_offense(node.loc.selector)`. Detection is at parity; autocorrect
//!   (replace with the preferred spelling) is not ported in this batch —
//!   same convention as `RSpec/ClassCheck` (status: partial, autocorrect
//!   as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with `methods = ["not_to", "to_not"]` (any
//! receiver). Default style `not_to`:
//!
//! - `expect(false).to_not be_true` — flagged.
//! - `expect(false).not_to be_true` — preferred spelling, not flagged.
//!
//! Alternate style `to_not` (`EnforcedStyle: to_not`) mirrors:
//! `not_to` flags, `to_not` passes.
//!
//! ## No autocorrect
//!
//! Upstream replaces with the preferred spelling. This batch reports only.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct NotToNot;

#[derive(CopOptions)]
pub struct NotToNotOptions {
    #[option(
        name = "EnforcedStyle",
        default = "not_to",
        description = "Whether to enforce not_to or to_not for negated expectations."
    )]
    pub enforced_style: NotToNotStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum NotToNotStyle {
    #[option(value = "not_to")]
    NotTo,
    #[option(value = "to_not")]
    ToNot,
}

#[cop(
    name = "RSpec/NotToNot",
    description = "Checks for consistent method usage for negating expectations.",
    default_severity = "warning",
    default_enabled = true,
    options = NotToNotOptions,
)]
impl NotToNot {
    #[on_node(kind = "send", methods = ["not_to", "to_not"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { method, .. } = *cx.kind(node) else {
            return;
        };
        let opts = cx.options_or_default::<NotToNotOptions>();
        let method_name = cx.symbol_str(method);
        let preferred = match opts.enforced_style {
            NotToNotStyle::NotTo => "not_to",
            NotToNotStyle::ToNot => "to_not",
        };
        if method_name == preferred {
            return;
        }
        cx.emit_offense(
            cx.node(node).loc.name,
            &format!("Prefer `{preferred}` over `{method_name}`."),
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{NotToNot, NotToNotOptions, NotToNotStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn to_not_style() -> NotToNotOptions {
        NotToNotOptions {
            enforced_style: NotToNotStyle::ToNot,
        }
    }

    #[test]
    fn flags_to_not_in_default_style() {
        test::<NotToNot>().expect_offense(indoc! {r#"
                expect(false).to_not be_true
                              ^^^^^^ Prefer `not_to` over `to_not`.
            "#});
    }

    #[test]
    fn does_not_flag_not_to_in_default_style() {
        test::<NotToNot>().expect_no_offenses(indoc! {r#"
                expect(false).not_to be_true
            "#});
    }

    #[test]
    fn flags_not_to_in_to_not_style() {
        test::<NotToNot>()
            .with_options(&to_not_style())
            .expect_offense(indoc! {r#"
                expect(false).not_to be_true
                              ^^^^^^ Prefer `to_not` over `not_to`.
            "#});
    }

    #[test]
    fn does_not_flag_to_not_in_to_not_style() {
        test::<NotToNot>()
            .with_options(&to_not_style())
            .expect_no_offenses(indoc! {r#"
                expect(false).to_not be_true
            "#});
    }
}

murphy_plugin_api::submit_cop!(NotToNot);
