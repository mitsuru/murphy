//! `Style/Send` — flags `.send(...)` calls and suggests using `.__send__` or
//! `.public_send` instead.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/Send
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Disabled by default (matches upstream Enabled: false).
//!   The cop fires on `send` method calls with at least one argument, matching
//!   RuboCop's `node.arguments?` guard. Bare `obj.send` (no args) is not flagged.
//!   Both `send` and `csend` (safe-navigation) calls are handled, mirroring
//!   RuboCop's `alias on_csend on_send`.
//!   No autocorrect is provided, matching the upstream implementation.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! Foo.send(bar)
//! quuz.send(fred)
//!
//! # good
//! Foo.__send__(bar)
//! quuz.public_send(fred)
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct Send;

const MSG: &str = "Prefer `Object#__send__` or `Object#public_send` to `send`.";

#[cop(
    name = "Style/Send",
    description = "Prefer `Object#__send__` or `Object#public_send` to `send`, as `send` may overlap with existing methods.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl Send {
    #[on_node(kind = "send", methods = ["send"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        if cx.method_name(node) == Some("send") {
            check(node, cx);
        }
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Only flag calls that have at least one argument.
    if !cx.has_call_arguments(node) {
        return;
    }

    // Offense is on the selector only (matching RuboCop's `node.loc.selector`).
    let selector = cx.selector(node);
    cx.emit_offense(selector, MSG, None);
    // No autocorrect -- upstream provides none.
}

#[cfg(test)]
mod tests {
    use super::Send;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Offenses -----

    #[test]
    fn flags_send_with_arg() {
        test::<Send>().expect_offense(indoc! {"
            Foo.send(bar)
                ^^^^ Prefer `Object#__send__` or `Object#public_send` to `send`.
        "});
    }

    #[test]
    fn flags_send_with_multiple_args() {
        test::<Send>().expect_offense(indoc! {"
            quuz.send(fred, baz)
                 ^^^^ Prefer `Object#__send__` or `Object#public_send` to `send`.
        "});
    }

    #[test]
    fn flags_csend_with_arg() {
        test::<Send>().expect_offense(indoc! {"
            obj&.send(bar)
                 ^^^^ Prefer `Object#__send__` or `Object#public_send` to `send`.
        "});
    }

    // ----- No offense -----

    #[test]
    fn accepts_public_send() {
        test::<Send>().expect_no_offenses("Foo.public_send(bar)\n");
    }

    #[test]
    fn accepts_dunder_send() {
        test::<Send>().expect_no_offenses("Foo.__send__(bar)\n");
    }

    #[test]
    fn accepts_send_without_args() {
        // Bare `obj.send` (no arguments) is not flagged.
        test::<Send>().expect_no_offenses("obj.send\n");
    }
}
murphy_plugin_api::submit_cop!(Send);
