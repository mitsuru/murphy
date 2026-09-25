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
//!   Verbatim port of the `send` call head `(call _ :send ...)` (murphy-s1yc.3):
//!   `call` = `{send csend}` covers safe-navigation (`obj&.send`), mirroring
//!   RuboCop's `alias on_csend on_send`; the `_` receiver binds an absent
//!   (receiverless `send`) or present receiver per murphy-if9y; trailing `...`
//!   absorbs any argument list, with the upstream `node.arguments?` guard
//!   applied separately (bare `obj.send` is not flagged).
//!   Disabled by default (matches upstream Enabled: false).
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

use murphy_plugin_api::{Cx, NoOptions, NodeId, cop, def_node_matcher};

// Verbatim port of the `send` call head (murphy-s1yc.3):
// `(call _ :send ...)` — `call` = `{send csend}` covers safe-navigation
// (`obj&.send(bar)`); the `_` receiver binds an absent (receiverless `send`)
// or present receiver; trailing `...` absorbs any argument list (zero or
// more), so the upstream `return unless node.arguments?` guard is applied
// separately in `check` (`obj.send` with no args does not match the cop).
def_node_matcher!(send_call, "(call _ :send ...)");

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
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Verbatim `(call _ :send ...)` head: filters to `send` calls on either
    // `send` or `csend` (safe-navigation), with any receiver (absent or
    // present). Trailing `...` matches zero args too, so apply RuboCop's
    // `return unless node.arguments?` guard separately.
    if !send_call(node, cx) {
        return;
    }
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

    // --- Characterization (murphy-s1yc.3): pin the exact node set the
    // hand-rolled send/csend dispatch matches, so the verbatim
    // `(call _ :send ...)` port can be proven byte-identical (plus the
    // upstream `arguments?` guard). `call` = `{send csend}` covers
    // safe-navigation; `_` binds the absent (receiverless) slot per if9y.

    #[test]
    fn s1yc3_flags_bare_send_with_arg() {
        // Bare `send(bar)` matches: `_` binds the nil-filled receiver.
        test::<Send>().expect_offense(indoc! {"
            send(bar)
            ^^^^ Prefer `Object#__send__` or `Object#public_send` to `send`.
        "});
    }

    #[test]
    fn s1yc3_accepts_csend_send_without_args() {
        // Matcher `(call _ :send ...)` matches `obj&.send`, but the upstream
        // `arguments?` guard rejects zero-arg calls — no offense.
        test::<Send>().expect_no_offenses("obj&.send\n");
    }

    #[test]
    fn s1yc3_accepts_bare_send_without_args() {
        // Bare `send` with no args: matcher matches, guard rejects.
        test::<Send>().expect_no_offenses("send\n");
    }
}
murphy_plugin_api::submit_cop!(Send);
