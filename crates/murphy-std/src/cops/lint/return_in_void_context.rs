//! `Lint/ReturnInVoidContext` — checks for returning values from void methods.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/ReturnInVoidContext
//! upstream_version_checked: master
//! status: partial
//! gap_issues: []
//! notes: >
//!   Initial port covers value returns in instance `initialize` methods and
//!   setter methods, including returns nested in normal blocks inside those
//!   method bodies. Lambda/define_method exclusions are documented v1 gaps.
//! ```

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind};

#[derive(Default)]
pub struct ReturnInVoidContext;

#[cop(
    name = "Lint/ReturnInVoidContext",
    description = "Checks for return in void context.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl ReturnInVoidContext {
    #[on_node(kind = "return")]
    fn check_return(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Return(value) = *cx.kind(node) else {
            return;
        };
        if value.get().is_none() {
            return;
        }

        let Some(method) = cx.ancestors(node).find(|&ancestor| {
            matches!(
                cx.kind(ancestor),
                NodeKind::Def { .. } | NodeKind::Defs { .. }
            )
        }) else {
            return;
        };
        if !cx.is_void_context(method) {
            return;
        }
        let Some(name) = cx.method_name(method) else {
            return;
        };
        let msg = format!("Do not return a value in `{name}`.");
        cx.emit_offense(cx.range(node), &msg, None);
    }
}

murphy_plugin_api::submit_cop!(ReturnInVoidContext);

#[cfg(test)]
mod tests {
    use super::ReturnInVoidContext;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_value_return_in_initialize() {
        test::<ReturnInVoidContext>().expect_offense(indoc! {r#"
            def initialize
              return :value
              ^^^^^^^^^^^^^ Do not return a value in `initialize`.
            end
        "#});
    }

    #[test]
    fn accepts_bare_return_in_initialize() {
        test::<ReturnInVoidContext>().expect_no_offenses("def initialize\n  return\nend\n");
    }
}
