//! `RSpec/MessageSpies` — set message expectations with spies.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/MessageSpies
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_send` (`message_expectation`: `(send (send
//!   nil? :expect $_) #Runners.all ...)` with `Runners.all` = `to` /
//!   `to_not` / `not_to`, plus `receive_message` searching `$(send
//!   nil? {:receive :have_received} ...)` in the runner subtree).
//!   Dispatch is on the three runners; the receiver must be a bare
//!   `expect(...)` send and the runner subtree must contain a bare
//!   `receive` / `have_received` send. When that call already uses
//!   the enforced style it is clean, otherwise its selector flags:
//!   `Prefer `receive` for setting message expectations.` under
//!   `EnforcedStyle: receive`, or `Prefer `have_received` for setting
//!   message expectations. Setup `%<source>s` as a spy using `allow`
//!   or `instance_spy`.` (with the `expect` argument source) under
//!   the default `have_received`. Option and defaults mirror upstream
//!   `config/default.yml` (upstream `Enabled: true`). Detection is at
//!   parity; upstream ships no autocorrect and none is added here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with `methods = ["to", "to_not", "not_to"]`
//! (default style `have_received`):
//!
//! - `expect(foo).to receive(:bar)` — flagged (the `receive`
//!   selector, with the spy-setup message).
//! - `expect(foo).to have_received(:bar)` — preferred style, clean.
//! - `expect(foo).not_to receive(:bar)` — flagged (all runners).
//! - `allow(foo).to receive(:bar)` — receiver is `allow`, clean.
//! - `expect(foo).to eq(1)` — no `receive`, clean.
//!
//! With `EnforcedStyle: receive` the `have_received` shape flags
//! instead.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; converting to spies restructures
//! the example and needs human judgement.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct MessageSpies;

#[derive(CopOptions)]
pub struct MessageSpiesOptions {
    #[option(
        name = "EnforcedStyle",
        default = "have_received",
        description = "Which message expectation style to enforce."
    )]
    pub enforced_style: MessageSpiesStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum MessageSpiesStyle {
    #[option(value = "have_received")]
    HaveReceived,
    #[option(value = "receive")]
    Receive,
}

#[cop(
    name = "RSpec/MessageSpies",
    description = "Checks that message expectations are set using spies.",
    default_severity = "warning",
    default_enabled = true,
    options = MessageSpiesOptions,
)]
impl MessageSpies {
    // `methods` mirrors upstream `RESTRICT_ON_SEND = Runners.all`
    // (`to` / `to_not` / `not_to`) — dispatch only on candidate
    // runners. The body still gates on the bare `expect` receiver
    // and the subtree `receive` / `have_received`.
    #[on_node(kind = "send", methods = ["to", "to_not", "not_to"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, .. } = *cx.kind(node) else {
            return;
        };
        let Some(recv) = receiver.get() else {
            return;
        };
        let NodeKind::Send {
            receiver: inner_receiver,
            method: inner_method,
            args: inner_args,
        } = *cx.kind(recv)
        else {
            return;
        };
        // `(send nil? :expect ...)` — bare `expect`, any args.
        if inner_receiver != OptNodeId::NONE
            || cx.symbol_str(inner_method) != "expect"
        {
            return;
        }
        // `receive_message`: `$(send nil? {:receive :have_received}
        // ...)` searched in the runner node's subtree.
        let Some(matcher) = find_message_matcher(cx, node) else {
            return;
        };
        let opts = cx.options_or_default::<MessageSpiesOptions>();
        let matcher_name = matcher_name(cx, matcher);
        let preferred = matcher_name == style_name(opts.enforced_style);
        if preferred {
            return;
        }
        let message = match opts.enforced_style {
            MessageSpiesStyle::Receive => {
                "Prefer `receive` for setting message expectations.".to_owned()
            }
            MessageSpiesStyle::HaveReceived => {
                let source = cx
                    .list(inner_args)
                    .first()
                    .map_or("".to_owned(), |&arg| {
                        cx.raw_source(cx.range(arg)).to_owned()
                    });
                format!(
                    "Prefer `have_received` for setting message expectations. \
                     Setup `{source}` as a spy using `allow` or `instance_spy`."
                )
            }
        };
        cx.emit_offense(cx.node(matcher).loc.name, &message, None);
    }
}

/// The first bare `receive` / `have_received` send in `root`'s
/// subtree (document order), or `None`.
///
/// Mirrors upstream `receive_message` (`def_node_search` for
/// `$(send nil? {:receive :have_received} ...)`).
fn find_message_matcher(cx: &Cx<'_>, root: NodeId) -> Option<NodeId> {
    core::iter::once(root)
        .chain(cx.descendants(root))
        .filter(|&id| {
            if let NodeKind::Send {
                receiver, method, ..
            } = *cx.kind(id)
                && receiver == OptNodeId::NONE
                && matches!(cx.symbol_str(method), "receive" | "have_received")
            {
                true
            } else {
                false
            }
        })
        // `descendants` order is unspecified; sort by offset so the
        // first textual match wins like upstream's node search.
        .min_by_key(|&id| cx.range(id).start)
}

/// The matcher selector name (`receive` / `have_received`).
fn matcher_name<'a>(cx: &'a Cx<'a>, matcher: NodeId) -> &'a str {
    let NodeKind::Send { method, .. } = *cx.kind(matcher) else {
        return "";
    };
    cx.symbol_str(method)
}

/// The enforced style's selector spelling.
fn style_name(style: MessageSpiesStyle) -> &'static str {
    match style {
        MessageSpiesStyle::HaveReceived => "have_received",
        MessageSpiesStyle::Receive => "receive",
    }
}

#[cfg(test)]
mod tests {
    use super::{MessageSpies, MessageSpiesOptions, MessageSpiesStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn receive_style() -> MessageSpiesOptions {
        MessageSpiesOptions {
            enforced_style: MessageSpiesStyle::Receive,
        }
    }

    #[test]
    fn flags_to_receive_by_default() {
        test::<MessageSpies>().expect_offense(indoc! {r#"
                expect(foo).to receive(:bar)
                               ^^^^^^^ Prefer `have_received` for setting message expectations. Setup `foo` as a spy using `allow` or `instance_spy`.
            "#});
    }
    #[test]
    fn does_not_flag_have_received_by_default() {
        test::<MessageSpies>().expect_no_offenses(indoc! {r#"
                expect(foo).to have_received(:bar)
            "#});
    }
    #[test]
    fn flags_not_to_receive_by_default() {
        test::<MessageSpies>().expect_offense(indoc! {r#"
                expect(foo).not_to receive(:bar)
                                   ^^^^^^^ Prefer `have_received` for setting message expectations. Setup `foo` as a spy using `allow` or `instance_spy`.
            "#});
    }
    #[test]
    fn does_not_flag_allow_receiver() {
        // The runner receiver must be `expect` (verified vs 3.7.0).
        test::<MessageSpies>().expect_no_offenses(indoc! {r#"
                allow(foo).to receive(:bar)
            "#});
    }
    #[test]
    fn does_not_flag_runner_without_matcher() {
        // No bare `receive` / `have_received` in the subtree (verified vs 3.7.0).
        test::<MessageSpies>().expect_no_offenses(indoc! {r#"
                expect(foo).to eq(1)
            "#});
    }
    #[test]
    fn flags_have_received_under_receive_style() {
        test::<MessageSpies>().with_options(&receive_style()).expect_offense(indoc! {r#"
                expect(foo).to have_received(:bar)
                               ^^^^^^^^^^^^^ Prefer `receive` for setting message expectations.
            "#});
    }
    #[test]
    fn does_not_flag_receive_under_receive_style() {
        test::<MessageSpies>().with_options(&receive_style()).expect_no_offenses(indoc! {r#"
                expect(foo).to receive(:bar)
            "#});
    }
}

murphy_plugin_api::submit_cop!(MessageSpies);
