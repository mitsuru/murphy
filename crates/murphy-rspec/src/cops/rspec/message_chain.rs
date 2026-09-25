//! `RSpec/MessageChain` — do not stub chains of messages.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/MessageChain
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_send` with `RESTRICT_ON_SEND =
//!   [:receive_message_chain, :stub_chain]`: any send of either
//!   selector flags the selector per
//!   `add_offense(node.loc.selector)` with
//!   `Avoid stubbing using `%<method>s`.`
//!   No receiver gating upstream — explicit receivers
//!   (`foo.stub_chain`) flag too. Upstream ships no autocorrect and
//!   none is added here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with `methods = ["receive_message_chain",
//! "stub_chain"]`:
//!
//! - `allow(foo).to receive_message_chain(:bar, :baz)` — flagged.
//! - `foo.stub_chain(:bar, :baz)` — flagged (no receiver gating).
//! - `allow(foo).to receive(:bar)` — clean.
//! - `allow(foo).to receive_messages(x)` — clean (different selector).
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; unpicking the chain needs human
//! judgement.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct MessageChain;

#[cop(
    name = "RSpec/MessageChain",
    description = "Check that chains of messages are not being stubbed.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl MessageChain {
    #[on_node(kind = "send", methods = ["receive_message_chain", "stub_chain"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { method, .. } = *cx.kind(node) else {
            return;
        };
        let method = cx.symbol_str(method).to_owned();
        cx.emit_offense(
            cx.node(node).loc.name,
            &format!("Avoid stubbing using `{method}`."),
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::MessageChain;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_receive_message_chain() {
        test::<MessageChain>().expect_offense(indoc! {r#"
                allow(foo).to receive_message_chain(:bar, :baz).and_return(42)
                              ^^^^^^^^^^^^^^^^^^^^^ Avoid stubbing using `receive_message_chain`.
            "#});
    }
    #[test]
    fn flags_stub_chain() {
        test::<MessageChain>().expect_offense(indoc! {r#"
                foo.stub_chain(:bar, :baz)
                    ^^^^^^^^^^ Avoid stubbing using `stub_chain`.
            "#});
    }
    #[test]
    fn does_not_flag_receive() {
        test::<MessageChain>().expect_no_offenses(indoc! {r#"
                allow(foo).to receive(:bar)
            "#});
    }
    #[test]
    fn does_not_flag_receive_messages() {
        // `receive_messages` is a different selector (verified vs 3.7.0).
        test::<MessageChain>().expect_no_offenses(indoc! {r#"
                allow(foo).to receive_messages(bar: 1)
            "#});
    }
}

murphy_plugin_api::submit_cop!(MessageChain);
