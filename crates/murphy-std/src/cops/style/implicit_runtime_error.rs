//! `Style/ImplicitRuntimeError` — flags `raise`/`fail` with a bare string
//! message but no explicit exception class.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ImplicitRuntimeError
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Flags bare `raise "msg"` / `fail "msg"` (and dynamic `dstr` variants).
//!   Only bare calls (no receiver) match — `Kernel.raise "msg"` and
//!   `obj.raise "msg"` are not flagged, consistent with RuboCop's
//!   `def_node_matcher :implicit_runtime_error_raise_or_fail` which matches
//!   `(send nil? ...)`.
//!   No autocorrect (RuboCop does not autocorrect this cop).
//!   Disabled by default to match RuboCop upstream (Enabled: false).
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! raise 'Error message here'
//! fail "Something went wrong"
//!
//! # good
//! raise ArgumentError, 'Error message here'
//! raise RuntimeError, 'msg'
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct ImplicitRuntimeError;

#[cop(
    name = "Style/ImplicitRuntimeError",
    description = "Use `raise` or `fail` with an explicit exception class and message, rather than just a message.",
    default_severity = "warning",
    default_enabled = false,
)]
impl ImplicitRuntimeError {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, method, args } = *cx.kind(node) else {
            return;
        };

        // Only bare calls (no receiver).
        if receiver.get().is_some() {
            return;
        }

        let method_str = cx.symbol_str(method);
        if method_str != "raise" && method_str != "fail" {
            return;
        }

        // Must have exactly one argument.
        let arg_list = cx.list(args);
        if arg_list.len() != 1 {
            return;
        }

        // The single argument must be a string literal (Str or Dstr).
        let arg = arg_list[0];
        if !matches!(cx.kind(arg), NodeKind::Str(_) | NodeKind::Dstr(_)) {
            return;
        }

        let message = format!(
            "Use `{method_str}` with an explicit exception class and message, rather than just a message."
        );
        cx.emit_offense(cx.range(node), &message, None);
    }
}

#[cfg(test)]
mod tests {
    use super::ImplicitRuntimeError;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_raise_with_string_literal() {
        test::<ImplicitRuntimeError>().expect_offense(indoc! {r#"
            raise 'Error message here'
            ^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `raise` with an explicit exception class and message, rather than just a message.
        "#});
    }

    #[test]
    fn flags_fail_with_string_literal() {
        test::<ImplicitRuntimeError>().expect_offense(indoc! {r#"
            fail "Something went wrong"
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `fail` with an explicit exception class and message, rather than just a message.
        "#});
    }

    #[test]
    fn flags_raise_with_dstr() {
        test::<ImplicitRuntimeError>().expect_offense(indoc! {r#"
            raise "Error: #{msg}"
            ^^^^^^^^^^^^^^^^^^^^^ Use `raise` with an explicit exception class and message, rather than just a message.
        "#});
    }

    #[test]
    fn no_offense_raise_with_class_and_message() {
        test::<ImplicitRuntimeError>().expect_no_offenses("raise ArgumentError, 'Error message here'\n");
    }

    #[test]
    fn no_offense_raise_with_class_only() {
        test::<ImplicitRuntimeError>().expect_no_offenses("raise ArgumentError\n");
    }

    #[test]
    fn no_offense_bare_raise() {
        test::<ImplicitRuntimeError>().expect_no_offenses("raise\n");
    }

    #[test]
    fn no_offense_raise_with_receiver() {
        // obj.raise "msg" -- has receiver, not flagged.
        test::<ImplicitRuntimeError>().expect_no_offenses("obj.raise 'msg'\n");
    }

    #[test]
    fn no_offense_raise_runtime_error_string() {
        test::<ImplicitRuntimeError>().expect_no_offenses("raise RuntimeError, 'msg'\n");
    }
}

murphy_plugin_api::submit_cop!(ImplicitRuntimeError);
