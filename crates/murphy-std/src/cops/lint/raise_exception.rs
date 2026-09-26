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

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop, def_node_matcher};

// RuboCop parity: `Lint/RaiseException` `exception?` is
// `(send nil? {:raise :fail} $(const ${cbase nil?} :Exception) ...)` and
// `exception_new_with_message?` is
// `(send nil? {:raise :fail} (send $(const ${cbase nil?} :Exception) :new ...))`.
// Murphy splits the inner const check (`Exception` top-level) from the outer
// `raise`/`fail` dispatch (bare + `Kernel`, `AllowedImplicitNamespaces` +
// autocorrect stay hand-rolled), so the verbatim port covers the inner part
// only: `(const nil? :Exception)` + `(send (const nil? :Exception) :new ...)`
// (send-only for the `new` form, matching the `Send`-only `new` check; no
// `Csend` handling).
// In Murphy `::Exception` collapses to `Const{scope:None}`: `nil?` covers
// bare + `::` (both flag, pinned by `flags_raise_cbase_exception` +
// `flags_raise_cbase_exception_new`). Namespaced `Foo::Exception` is not
// top-level, so silent (pinned by `accepts_raise_with_explicit_namespace` +
// `boundary_ignores_namespaced_exception_new`). `send` covers `Send` only,
// matching the `Send`-only `new` check (csend silent, pinned by
// `boundary_ignores_csend_exception_new`). The outer `Kernel` + namespace +
// replacement (`StandardError` vs `::StandardError`) stays hand-rolled below.
def_node_matcher!(is_exception_const, "(const nil? :Exception)");
def_node_matcher!(exception_new, "(send (const nil? :Exception) :new ...)");

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
        // `(const nil? :Exception)` — top-level `Exception` argument.
        if is_exception_const(first, cx) {
            if let Some(replacement) = exception_replacement(first, cx) {
                if replacement == "StandardError" && implicit_namespace_allowed(node, cx, opts) {
                    return;
                }
                emit_exception_offense(first, cx, replacement);
                return;
            }
            return;
        }
        // `(send (const nil? :Exception) :new ...)` — top-level `Exception.new`.
        // The `Send`-only `new` check stays via `send` (no `Csend` handling).
        if exception_new(first, cx) {
            let NodeKind::Send { receiver, .. } = *cx.kind(first) else {
                return;
            };
            let Some(recv) = receiver.get() else {
                return;
            };
            if let Some(replacement) = exception_replacement(recv, cx) {
                if replacement == "StandardError" && implicit_namespace_allowed(node, cx, opts) {
                    return;
                }
                emit_exception_offense(recv, cx, replacement);
            }
        }
    }
}

fn exception_replacement(node: NodeId, cx: &Cx<'_>) -> Option<&'static str> {
    // Predicate via verbatim `(const nil? :Exception)`; replacement
    // (`StandardError` vs `::StandardError`) stays hand-rolled below.
    if !is_exception_const(node, cx) {
        return None;
    }
    let NodeKind::Const { scope, .. } = *cx.kind(node) else {
        return None;
    };
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

    // --- Boundary characterization (murphy-ft88.17): pin the exact node set
    // the hand-rolled `exception_replacement` (nil/cbase scope, reject
    // namespaced) matches, so the verbatim `(const nil? :Exception)` +
    // `(send (const nil? :Exception) :new ...)` refactor can be proven
    // equivalent. `::Exception` collapses to `Const{scope:None}`: `nil?`
    // covers bare + `::` (both flag, pre-existing `flags_raise_cbase_exception`
    // + `flags_raise_cbase_exception_new`). Namespaced `Foo::Exception` is
    // not top-level, so silent (pre-existing `accepts_raise_with_explicit_namespace`
    // pins bare; this pins `new`). `send` covers `Send` only, matching the
    // `Send`-only `new` check (no `Csend` handling, so `&.` is silent).

    #[test]
    fn boundary_ignores_namespaced_exception_new() {
        test::<RaiseException>().expect_no_offenses("raise Foo::Exception.new('msg')\n");
    }

    #[test]
    fn boundary_ignores_csend_exception_new() {
        test::<RaiseException>().expect_no_offenses("raise Exception&.new('msg')\n");
    }
}
