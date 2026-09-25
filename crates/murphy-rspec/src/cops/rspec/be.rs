//! `RSpec/Be` — flag bare `be` without an argument.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/Be
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `be_without_args` (`(send _ #Runners.all
//!   $(send nil? :be))`) with `RESTRICT_ON_SEND = Runners.all`
//!   (`to` / `to_not` / `not_to`). The inner matcher must be a bare `be`
//!   with zero args; `be(1)` / `be_truthy` do not match. The offense range
//!   is the inner `be` selector per `add_offense(matcher.loc.selector)`.
//!   No autocorrect upstream, none here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with `methods = ["to", "to_not", "not_to"]`.
//! Flags when any positional arg is a bare zero-arg `be`:
//!
//! - `expect(foo).to be` — flagged (range is `be`).
//! - `expect(foo).not_to be` — flagged.
//! - `expect(foo).to be(1)` — has args, not flagged.
//! - `expect(foo).to be_truthy` — different matcher, not flagged.
//! - `expect(foo).to eq(1)` — different matcher, not flagged.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; the intended matcher (`be_truthy`,
//! `be(1)`, …) needs human judgement.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct Be;

#[cop(
    name = "RSpec/Be",
    description = "Check for expectations where be is used without argument.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl Be {
    #[on_node(kind = "send", methods = ["to", "to_not", "not_to"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { args, .. } = *cx.kind(node) else {
            return;
        };
        for arg in cx.list(args).iter().copied() {
            let NodeKind::Send {
                receiver,
                method,
                args: inner_args,
            } = *cx.kind(arg)
            else {
                continue;
            };
            if receiver != OptNodeId::NONE {
                continue;
            }
            if cx.symbol_str(method) != "be" {
                continue;
            }
            if !cx.list(inner_args).is_empty() {
                continue;
            }
            cx.emit_offense(
                cx.node(arg).loc.name,
                "Don't use `be` without an argument.",
                None,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Be;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_to_be() {
        test::<Be>().expect_offense(indoc! {r#"
                expect(foo).to be
                               ^^ Don't use `be` without an argument.
            "#});
    }

    #[test]
    fn flags_not_to_be() {
        test::<Be>().expect_offense(indoc! {r#"
                expect(foo).not_to be
                                   ^^ Don't use `be` without an argument.
            "#});
    }

    #[test]
    fn flags_to_not_be() {
        test::<Be>().expect_offense(indoc! {r#"
                expect(foo).to_not be
                                   ^^ Don't use `be` without an argument.
            "#});
    }

    #[test]
    fn does_not_flag_be_with_arg() {
        test::<Be>().expect_no_offenses(indoc! {r#"
                expect(foo).to be(1)
            "#});
    }

    #[test]
    fn does_not_flag_be_truthy() {
        test::<Be>().expect_no_offenses(indoc! {r#"
                expect(foo).to be_truthy
            "#});
    }

    #[test]
    fn does_not_flag_eq() {
        test::<Be>().expect_no_offenses(indoc! {r#"
                expect(foo).to eq(1)
            "#});
    }
}

murphy_plugin_api::submit_cop!(Be);
