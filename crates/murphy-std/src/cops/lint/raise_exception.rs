//! `Lint/RaiseException` — flags `raise Exception` or `fail Exception`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/RaiseException
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Port covers bare `Exception`, `::Exception`, `Exception.new(...)`,
//!   RuboCop's default `AllowedImplicitNamespaces: ['Gem']`, and autocorrect
//!   to `StandardError` / `::StandardError`.
//! ```
//!
//! ## Matched shapes
//! - `raise Exception` / `fail Exception` — raising the base Exception class
//! - `raise ::Exception` / `fail ::Exception` — explicit top-level reference
//! - `raise Exception.new(...)` / `fail Exception.new(...)` — raising Exception instance
//!
//! ## Autocorrect
//! Mirrors RuboCop's unsafe autocorrect by replacing `Exception` with
//! `StandardError`, preserving an explicit leading `::` when present.
//!

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

const MSG: &str = "Use `StandardError` over `Exception`.";

#[derive(Default)]
pub struct RaiseException;

#[derive(CopOptions)]
pub struct RaiseExceptionOptions {
    #[option(
        name = "AllowedImplicitNamespaces",
        default = ["Gem"],
        description = "Namespaces where an implicit Exception constant is allowed."
    )]
    pub allowed_implicit_namespaces: Vec<String>,
}

#[cop(
    name = "Lint/RaiseException",
    description = "`raise` or `fail` with the base `Exception` class is discouraged.",
    default_severity = "warning",
    default_enabled = true,
    options = RaiseExceptionOptions
)]
impl RaiseException {
    #[on_node(kind = "send", methods = ["raise", "fail"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>, opts: &RaiseExceptionOptions) {
        if let Some(recv) = cx.call_receiver(node).get() {
            let is_kernel = if let NodeKind::Const { name, scope } = *cx.kind(recv) {
                cx.symbol_str(name) == "Kernel"
                    && scope
                        .get()
                        .is_none_or(|s| matches!(*cx.kind(s), NodeKind::Cbase))
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
        if let Some(replacement) = exception_replacement(first, cx) {
            if replacement == "StandardError" && implicit_namespace_allowed(node, cx, opts) {
                return;
            }
            emit_exception_offense(first, cx, replacement);
            return;
        }
        if let NodeKind::Send { method, receiver, .. } = *cx.kind(first)
            && cx.symbol_str(method) == "new"
            && let Some(recv) = receiver.get()
            && let Some(replacement) = exception_replacement(recv, cx)
        {
            if replacement == "StandardError" && implicit_namespace_allowed(node, cx, opts) {
                return;
            }
            emit_exception_offense(recv, cx, replacement);
        }
    }
}

fn exception_replacement(node: NodeId, cx: &Cx<'_>) -> Option<&'static str> {
    let NodeKind::Const { name, scope } = *cx.kind(node) else {
        return None;
    };
    if cx.symbol_str(name) != "Exception" {
        return None;
    }
    match scope.get() {
        None if cx.raw_source(cx.range(node)).starts_with("::") => Some("::StandardError"),
        None => Some("StandardError"),
        Some(scope) if matches!(*cx.kind(scope), NodeKind::Cbase) => Some("::StandardError"),
        _ => None,
    }
}

fn emit_exception_offense(node: NodeId, cx: &Cx<'_>, replacement: &str) {
    let range = cx.range(node);
    cx.emit_offense(range, MSG, None);
    cx.emit_edit(range, replacement);
}

fn implicit_namespace_allowed(node: NodeId, cx: &Cx<'_>, opts: &RaiseExceptionOptions) -> bool {
    cx.ancestors(node).any(|ancestor| {
        let name = match *cx.kind(ancestor) {
            NodeKind::Module { name, .. } | NodeKind::Class { name, .. } => name,
            _ => return false,
        };
        namespace_allowed(name, cx, opts)
    })
}

fn namespace_allowed(node: NodeId, cx: &Cx<'_>, opts: &RaiseExceptionOptions) -> bool {
    top_level_const_allowed(node, cx, opts)
}

fn top_level_const_allowed(mut node: NodeId, cx: &Cx<'_>, opts: &RaiseExceptionOptions) -> bool {
    loop {
        let NodeKind::Const { name, scope } = *cx.kind(node) else {
            return false;
        };
        match scope.get() {
            Some(parent) if matches!(*cx.kind(parent), NodeKind::Const { .. }) => {
                node = parent;
            }
            Some(parent) if matches!(*cx.kind(parent), NodeKind::Cbase) => {
                let namespace = cx.symbol_str(name);
                return opts
                    .allowed_implicit_namespaces
                    .iter()
                    .any(|allowed| allowed == namespace);
            }
            Some(_) => return false,
            None => {
                let namespace = cx.symbol_str(name);
                return opts
                    .allowed_implicit_namespaces
                    .iter()
                    .any(|allowed| allowed == namespace);
            }
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
        test::<RaiseException>().expect_correction(indoc! {r#"
            raise ::Exception
                  ^^^^^^^^^^^ Use `StandardError` over `Exception`.
        "#}, "raise ::StandardError\n");
    }

    #[test]
    fn flags_raise_cbase_exception_new() {
        test::<RaiseException>().expect_correction(indoc! {r#"
            raise ::Exception.new 'Error with exception'
                  ^^^^^^^^^^^ Use `StandardError` over `Exception`.
        "#}, "raise ::StandardError.new 'Error with exception'\n");
    }

    #[test]
    fn corrects_raise_exception() {
        test::<RaiseException>().expect_correction(indoc! {r#"
            raise Exception
                  ^^^^^^^^^ Use `StandardError` over `Exception`.
        "#}, "raise StandardError\n");
    }

    #[test]
    fn accepts_default_allowed_implicit_namespace() {
        test::<RaiseException>().expect_no_offenses(indoc! {r#"
            module Gem
              def self.foo
                raise Exception
              end
            end
        "#});
    }

    #[test]
    fn accepts_default_allowed_implicit_namespace_in_class() {
        test::<RaiseException>().expect_no_offenses(indoc! {r#"
            class Gem
              def self.foo
                raise Exception
              end
            end
        "#});
    }

    #[test]
    fn accepts_qualified_default_allowed_implicit_namespace() {
        test::<RaiseException>().expect_no_offenses(indoc! {r#"
            module Gem::Foo
              def self.foo
                raise Exception
              end
            end
        "#});
    }

    #[test]
    fn accepts_nested_default_allowed_implicit_namespace() {
        test::<RaiseException>().expect_no_offenses(indoc! {r#"
            module Gem
              module Foo
                def self.foo
                  raise Exception
                end
              end
            end
        "#});
    }

    #[test]
    fn flags_disallowed_implicit_namespace() {
        test::<RaiseException>().expect_offense(indoc! {r#"
            module Foo
              def self.foo
                raise Exception
                      ^^^^^^^^^ Use `StandardError` over `Exception`.
              end
            end
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
