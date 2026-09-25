//! `RSpec/Eq` — use `eq` instead of `be ==`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/Eq
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `be_equals` (`(send _ #Runners.all $(send (send nil?
//!   :be) :== _))`) with `RESTRICT_ON_SEND = Runners.all` (`to` /
//!   `to_not` / `not_to`). The inner matcher must be `be == value`: a
//!   single-arg `==` whose receiver is a bare zero-arg `be`. The offense
//!   range covers `be ==` per upstream `range_between(matcher.begin,
//!   selector.end)` (the compared value is excluded). Detection is at
//!   parity; autocorrect (replace `be ==` with `eq`) is not ported in this
//!   batch — same convention as `RSpec/BeEql` (status: partial,
//!   autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with `methods = ["to", "to_not", "not_to"]`.
//! Flags when any positional arg is `be == value`:
//!
//! - `expect(foo).to be == true` — flagged (range is `be ==`).
//! - `expect(bar).not_to be == 1` — flagged.
//! - `expect(foo).to eq true` — already `eq`, not flagged.
//! - `expect(foo).to be` — bare `be`, not flagged.
//! - `expect(foo).to be_truthy == 1` — non-`be` receiver, not flagged.
//!
//! ## No autocorrect
//!
//! Upstream replaces `be ==` with `eq`. This batch reports only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct Eq;

#[cop(
    name = "RSpec/Eq",
    description = "Use eq instead of be == to compare objects.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl Eq {
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
            if cx.symbol_str(method) != "==" {
                continue;
            }
            if cx.list(inner_args).len() != 1 {
                continue;
            }
            let Some(be_id) = receiver.get() else {
                continue;
            };
            let NodeKind::Send {
                receiver: be_recv,
                method: be_method,
                args: be_args,
            } = *cx.kind(be_id)
            else {
                continue;
            };
            if be_recv != OptNodeId::NONE {
                continue;
            }
            if cx.symbol_str(be_method) != "be" {
                continue;
            }
            if !cx.list(be_args).is_empty() {
                continue;
            }
            let range = Range {
                start: cx.range(be_id).start,
                end: cx.node(arg).loc.name.end,
            };
            cx.emit_offense(
                range,
                "Use `eq` instead of `be ==` to compare objects.",
                None,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Eq;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_to_be_eq() {
        test::<Eq>().expect_offense(indoc! {r#"
                expect(foo).to be == true
                               ^^^^^ Use `eq` instead of `be ==` to compare objects.
            "#});
    }

    #[test]
    fn flags_not_to_be_eq() {
        test::<Eq>().expect_offense(indoc! {r#"
                expect(bar).not_to be == 1
                                   ^^^^^ Use `eq` instead of `be ==` to compare objects.
            "#});
    }

    #[test]
    fn does_not_flag_eq() {
        test::<Eq>().expect_no_offenses(indoc! {r#"
                expect(foo).to eq true
            "#});
    }

    #[test]
    fn does_not_flag_bare_be() {
        test::<Eq>().expect_no_offenses(indoc! {r#"
                expect(foo).to be
            "#});
    }

    #[test]
    fn does_not_flag_truthy_eq() {
        test::<Eq>().expect_no_offenses(indoc! {r#"
                expect(foo).to be_truthy == 1
            "#});
    }

    #[test]
    fn does_not_flag_be_with_arg_eq() {
        // `be(1) == 2` is not the bare-`be` shape upstream matches.
        test::<Eq>().expect_no_offenses(indoc! {r#"
                expect(foo).to be(1) == 2
            "#});
    }
}

murphy_plugin_api::submit_cop!(Eq);
