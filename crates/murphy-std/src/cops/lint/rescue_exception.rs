//! `Lint/RescueException` — avoid rescuing the `Exception` class.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/RescueException
//! upstream_version_checked: master
//! status: partial
//! gap_issues: []
//! notes: >
//!   Initial port covers bare `Exception`, `::Exception`, and mixed rescue
//!   lists while excluding namespaced constants such as `Test::Exception`.
//! ```

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind, def_node_matcher};

// RuboCop parity: `Lint/RescueException` `targets_exception?` is
// `const_name == 'Exception'` (bare + `::Exception`, namespaced excluded).
// Verbatim as `(const nil? :Exception)`.
// In Murphy `::Exception` collapses to `Const{scope:None}`: `nil?` covers
// bare + `::` (pinned by `boundary_flags_cbase_exception`). Namespaced
// `Foo::Exception` / `Test::Exception` still accept (pinned by
// `accepts_namespaced_exception` +
// `boundary_accepts_namespaced_exception_foo`). The `resbody` exception-list
// loop stays hand-rolled below.
def_node_matcher!(is_exception_const, "(const nil? :Exception)");

#[derive(Default)]
pub struct RescueException;

#[cop(
    name = "Lint/RescueException",
    description = "Avoid rescuing the Exception class.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RescueException {
    #[on_node(kind = "resbody")]
    fn check_resbody(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Resbody { exceptions, .. } = *cx.kind(node) else {
            return;
        };
        for &exception in cx.list(exceptions) {
            // `(const nil? :Exception)` (`Exception` / `::Exception`,
            // top-level only).
            if is_exception_const(exception, cx) {
                cx.emit_offense(cx.range(exception), "Avoid rescuing the `Exception` class. Perhaps you meant to rescue `StandardError`?", None);
            }
        }
    }
}

murphy_plugin_api::submit_cop!(RescueException);

#[cfg(test)]
mod tests {
    use super::RescueException;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_exception() {
        test::<RescueException>().expect_offense(indoc! {r#"
            begin
              work
            rescue Exception
                   ^^^^^^^^^ Avoid rescuing the `Exception` class. Perhaps you meant to rescue `StandardError`?
            end
        "#});
    }

    #[test]
    fn accepts_namespaced_exception() {
        test::<RescueException>()
            .expect_no_offenses("begin\n  work\nrescue Test::Exception\nend\n");
    }

    // --- Boundary characterization (murphy-ft88.10): pin the exact node set
    // the hand-rolled `is_global_const` guard matches, so the verbatim
    // `(const nil? :Exception)` refactor can be proven equivalent.
    // `::Exception` collapses to `Const{scope:None}` in Murphy: `nil?`
    // covers bare + `::`. Namespaced `Foo::Exception` still accepts.

    #[test]
    fn boundary_flags_cbase_exception() {
        test::<RescueException>().expect_offense(indoc! {r#"
            begin
              work
            rescue ::Exception
                   ^^^^^^^^^^^ Avoid rescuing the `Exception` class. Perhaps you meant to rescue `StandardError`?
            end
        "#});
    }

    #[test]
    fn boundary_accepts_namespaced_exception_foo() {
        test::<RescueException>()
            .expect_no_offenses("begin\n  work\nrescue Foo::Exception\nend\n");
    }
}
