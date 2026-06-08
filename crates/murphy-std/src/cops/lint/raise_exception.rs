//! `Lint/RaiseException` — flags `raise Exception` or `fail Exception`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/RaiseException
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Initial port covers bare `Exception`, `::Exception`, and `Exception.new(...)`.
//!   `AllowedImplicitNamespaces` is not yet implemented.
//!   Autocorrect is not yet implemented.
//! ```
//!
//! ## Matched shapes
//! - `raise Exception` / `fail Exception` — raising the base Exception class
//! - `raise ::Exception` / `fail ::Exception` — explicit top-level reference
//! - `raise Exception.new(...)` / `fail Exception.new(...)` — raising Exception instance
//!
//! ## No autocorrect
//! Choosing the right exception subclass requires human judgment.
//!

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct RaiseException;

#[cop(
    name = "Lint/RaiseException",
    description = "`raise` or `fail` with the base `Exception` class is discouraged.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl RaiseException {
    #[on_node(kind = "send", methods = ["raise", "fail"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        if let Some(recv) = cx.call_receiver(node).get() {
            let is_kernel = if let NodeKind::Const { name, scope } = *cx.kind(recv) {
                cx.symbol_str(name) == "Kernel"
                    && scope.get().map_or(true, |s| matches!(*cx.kind(s), NodeKind::Cbase))
            } else {
                false
            };
            if !is_kernel {
                return;
            }
        }
        let args = cx.call_arguments(node);
        let Some(&first) = args.first() else {
            return;
        };
        if cx.is_global_const(first, "Exception") {
            cx.emit_offense(
                cx.range(first),
                "Use `StandardError` over `Exception`.",
                None,
            );
            return;
        }
        if let NodeKind::Send { method, receiver, .. } = *cx.kind(first)
            && cx.symbol_str(method) == "new"
            && let Some(recv) = receiver.get()
            && cx.is_global_const(recv, "Exception")
        {
            cx.emit_offense(
                cx.range(recv),
                "Use `StandardError` over `Exception`.",
                None,
            );
        }
    }
}

murphy_plugin_api::submit_cop!(RaiseException);

#[cfg(test)]
mod tests {
    use super::RaiseException;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_raise_exception() {
        test::<RaiseException>().expect_offense(indoc! {r#"
            raise Exception
                  ^^^^^^^^^ Use `StandardError` over `Exception`.
        "#});
    }

    #[test]
    fn flags_raise_exception_with_message() {
        test::<RaiseException>().expect_offense(indoc! {r#"
            raise Exception, 'Error with exception'
                  ^^^^^^^^^ Use `StandardError` over `Exception`.
        "#});
    }

    #[test]
    fn flags_raise_exception_new() {
        test::<RaiseException>().expect_offense(indoc! {r#"
            raise Exception.new 'Error with exception'
                  ^^^^^^^^^ Use `StandardError` over `Exception`.
        "#});
    }

    #[test]
    fn flags_raise_exception_new_multi_args() {
        test::<RaiseException>().expect_offense(indoc! {r#"
            raise Exception.new('arg1', 'arg2')
                  ^^^^^^^^^ Use `StandardError` over `Exception`.
        "#});
    }

    #[test]
    fn flags_raise_exception_new_no_args() {
        test::<RaiseException>().expect_offense(indoc! {r#"
            raise Exception.new
                  ^^^^^^^^^ Use `StandardError` over `Exception`.
        "#});
    }

    #[test]
    fn flags_raise_cbase_exception() {
        test::<RaiseException>().expect_offense(indoc! {r#"
            raise ::Exception
                  ^^^^^^^^^^^ Use `StandardError` over `Exception`.
        "#});
    }

    #[test]
    fn flags_raise_cbase_exception_new() {
        test::<RaiseException>().expect_offense(indoc! {r#"
            raise ::Exception.new 'Error with exception'
                  ^^^^^^^^^^^ Use `StandardError` over `Exception`.
        "#});
    }

    #[test]
    fn flags_fail_exception() {
        test::<RaiseException>().expect_offense(indoc! {r#"
            fail Exception
                 ^^^^^^^^^ Use `StandardError` over `Exception`.
        "#});
    }

    #[test]
    fn flags_fail_exception_with_message() {
        test::<RaiseException>().expect_offense(indoc! {r#"
            fail Exception, 'Error with exception'
                 ^^^^^^^^^ Use `StandardError` over `Exception`.
        "#});
    }

    #[test]
    fn flags_fail_exception_new() {
        test::<RaiseException>().expect_offense(indoc! {r#"
            fail Exception.new 'Error with exception'
                 ^^^^^^^^^ Use `StandardError` over `Exception`.
        "#});
    }

    #[test]
    fn accepts_raise_without_args() {
        test::<RaiseException>().expect_no_offenses("raise\n");
    }

    #[test]
    fn accepts_fail_without_args() {
        test::<RaiseException>().expect_no_offenses("fail\n");
    }

    #[test]
    fn accepts_raise_standard_error() {
        test::<RaiseException>().expect_no_offenses("raise StandardError, 'msg'\n");
    }

    #[test]
    fn accepts_raise_with_explicit_namespace() {
        test::<RaiseException>().expect_no_offenses("raise Foo::Exception\n");
    }

    #[test]
    fn accepts_raise_with_receiver() {
        test::<RaiseException>().expect_no_offenses("obj.raise Exception\n");
    }
}
