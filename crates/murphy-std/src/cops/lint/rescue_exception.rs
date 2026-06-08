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

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind};

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
            if cx.is_global_const(exception, "Exception") {
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
}
