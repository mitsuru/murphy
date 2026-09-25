//! `RSpec/MessageExpectation` — consistent `allow` / `expect` message-expectation style.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/MessageExpectation
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_send` (`message_expectation`: `(send $(send
//!   nil? {:expect :allow} ...) :to #receive_message?)` where
//!   `receive_message?` searches `(send nil? :receive ...)` in the
//!   `to` subtree). Dispatch is on `to`; the receiver must be a bare
//!   `expect` / `allow` send and the `to` subtree must contain a bare
//!   `receive` send. When the inner call already uses the enforced
//!   style it is clean, otherwise the inner selector flags per
//!   `add_offense(match.loc.selector)` with
//!   `Prefer `%<style>s` for setting message expectations.`
//!   `EnforcedStyle: allow` (default) / `expect` mirrors upstream
//!   `config/default.yml` (upstream `Enabled: false`). Detection is at
//!   parity; upstream ships no autocorrect and none is added here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with `methods = ["to"]` (default style
//! `allow`):
//!
//! - `expect(foo).to receive(:bar)` — flagged (the `expect`
//!   selector).
//! - `allow(foo).to receive(:bar)` — preferred style, clean.
//! - `expect(foo).to eq(1)` — no `receive`, clean.
//! - `expect(foo).not_to receive(:bar)` — different runner, clean.
//! - `foo.expect(x).to receive(y)` — explicit receiver, clean.
//!
//! With `EnforcedStyle: expect` the `allow` shape flags instead.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; swapping the expectation helper
//! changes semantics and needs human judgement.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct MessageExpectation;

#[derive(CopOptions)]
pub struct MessageExpectationOptions {
    #[option(
        name = "EnforcedStyle",
        default = "allow",
        description = "Which message expectation style to enforce."
    )]
    pub enforced_style: MessageExpectationStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum MessageExpectationStyle {
    #[option(value = "allow")]
    Allow,
    #[option(value = "expect")]
    Expect,
}

#[cop(
    name = "RSpec/MessageExpectation",
    description = "Checks for consistent message expectation style.",
    default_severity = "warning",
    default_enabled = false,
    options = MessageExpectationOptions,
)]
impl MessageExpectation {
    // `methods = ["to"]` mirrors upstream `RESTRICT_ON_SEND = [:to]` —
    // dispatch only on candidate runners. The body still gates on the
    // bare `expect` / `allow` receiver and the subtree `receive`.
    #[on_node(kind = "send", methods = ["to"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, .. } = *cx.kind(node)
        else {
            return;
        };
        let Some(recv) = receiver.get() else {
            return;
        };
        let NodeKind::Send {
            receiver: inner_receiver,
            method: inner_method,
            ..
        } = *cx.kind(recv)
        else {
            return;
        };
        // `(send nil? {:expect :allow} ...)` — bare inner call, any
        // args.
        if inner_receiver != OptNodeId::NONE {
            return;
        }
        let inner_name = cx.symbol_str(inner_method);
        if !matches!(inner_name, "expect" | "allow") {
            return;
        }
        // `#receive_message?`: `(send nil? :receive ...)` searched in
        // the `to` node's subtree (the receiver chain lives inside,
        // e.g. `expect(foo).to(receive(:bar))` parses with `receive`
        // under `to`).
        if !subtree_has_bare_receive(cx, node) {
            return;
        }
        let opts = cx.options_or_default::<MessageExpectationOptions>();
        let style = match opts.enforced_style {
            MessageExpectationStyle::Allow => "allow",
            MessageExpectationStyle::Expect => "expect",
        };
        if inner_name == style {
            return;
        }
        cx.emit_offense(
            cx.node(recv).loc.name,
            &format!("Prefer `{style}` for setting message expectations."),
            None,
        );
    }
}

/// `true` when `root` or any descendant is a bare `receive` send.
///
/// Mirrors upstream `receive_message?` (`def_node_search` for
/// `(send nil? :receive ...)`), which searches the `to` node and its
/// descendants.
fn subtree_has_bare_receive(cx: &Cx<'_>, root: NodeId) -> bool {
    for id in core::iter::once(root).chain(cx.descendants(root)) {
        if let NodeKind::Send {
            receiver, method, ..
        } = *cx.kind(id)
            && receiver == OptNodeId::NONE
            && cx.symbol_str(method) == "receive"
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{MessageExpectation, MessageExpectationOptions, MessageExpectationStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn expect_style() -> MessageExpectationOptions {
        MessageExpectationOptions {
            enforced_style: MessageExpectationStyle::Expect,
        }
    }

    #[test]
    fn flags_expect_to_receive_by_default() {
        test::<MessageExpectation>().expect_offense(indoc! {r#"
                expect(foo).to receive(:bar)
                ^^^^^^ Prefer `allow` for setting message expectations.
            "#});
    }
    #[test]
    fn does_not_flag_allow_to_receive_by_default() {
        test::<MessageExpectation>().expect_no_offenses(indoc! {r#"
                allow(foo).to receive(:bar)
            "#});
    }
    #[test]
    fn does_not_flag_to_without_receive() {
        // No bare `receive` in the `to` subtree (verified vs 3.7.0).
        test::<MessageExpectation>().expect_no_offenses(indoc! {r#"
                expect(foo).to eq(1)
            "#});
    }
    #[test]
    fn does_not_flag_not_to_runner() {
        // Upstream only restricts `to` (verified vs 3.7.0: `RESTRICT_ON_SEND = [:to]`).
        test::<MessageExpectation>().expect_no_offenses(indoc! {r#"
                expect(foo).not_to receive(:bar)
            "#});
    }
    #[test]
    fn does_not_flag_explicit_inner_receiver() {
        // The inner call must be bare (verified vs 3.7.0: `(send nil? ...)`).
        test::<MessageExpectation>().expect_no_offenses(indoc! {r#"
                foo.expect(x).to receive(y)
            "#});
    }
    #[test]
    fn does_not_flag_explicit_receive_receiver() {
        // `receive_message?` requires a bare `receive` (verified vs 3.7.0).
        test::<MessageExpectation>().expect_no_offenses(indoc! {r#"
                expect(foo).to obj.receive(:bar)
            "#});
    }
    #[test]
    fn flags_allow_to_receive_under_expect_style() {
        test::<MessageExpectation>().with_options(&expect_style()).expect_offense(indoc! {r#"
                allow(foo).to receive(:bar)
                ^^^^^ Prefer `expect` for setting message expectations.
            "#});
    }
    #[test]
    fn does_not_flag_expect_to_receive_under_expect_style() {
        test::<MessageExpectation>().with_options(&expect_style()).expect_no_offenses(indoc! {r#"
                expect(foo).to receive(:bar)
            "#});
    }
}

murphy_plugin_api::submit_cop!(MessageExpectation);
