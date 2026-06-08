//! `Lint/RefinementImportMethods` — checks deprecated mixin imports in `refine` blocks.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/RefinementImportMethods
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Initial Murphy v1 port reports bare `include`/`prepend` calls directly
//!   inside `refine` blocks. Known v1 limitation: Murphy does not expose
//!   TargetRubyVersion gating yet, so this cop behaves as Ruby 3.1+ regardless
//!   of project Ruby version. RuboCop provides no autocorrection.
//! ```

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind};

#[derive(Default)]
pub struct RefinementImportMethods;

#[cop(
    name = "Lint/RefinementImportMethods",
    description = "Checks deprecated `include`/`prepend` calls inside `refine` blocks.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl RefinementImportMethods {
    #[on_node(kind = "send", methods = ["include", "prepend"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send {
            receiver, method, ..
        } = *cx.kind(node)
        else {
            return;
        };
        if receiver.get().is_some() || !inside_refine_block(node, cx) {
            return;
        }

        let current = cx.symbol_str(method);
        let message = format!(
            "Use `import_methods` instead of `{current}` because it is deprecated in Ruby 3.1."
        );
        cx.emit_offense(cx.selector(node), &message, None);
    }
}

fn inside_refine_block(node: NodeId, cx: &Cx<'_>) -> bool {
    for ancestor in cx.ancestors(node) {
        match *cx.kind(ancestor) {
            NodeKind::Block { call, .. } => {
                return cx.method_name(call) == Some("refine")
                    && cx.call_receiver(call).get().is_none();
            }
            NodeKind::Def { .. }
            | NodeKind::Defs { .. }
            | NodeKind::Class { .. }
            | NodeKind::Module { .. } => return false,
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use murphy_plugin_api::test_support::{indoc, test};

    use super::RefinementImportMethods;

    #[test]
    fn flags_include_in_refine_block() {
        test::<RefinementImportMethods>().expect_offense(indoc! {r#"
            refine Foo do
              include Bar
              ^^^^^^^ Use `import_methods` instead of `include` because it is deprecated in Ruby 3.1.
            end
        "#});
    }

    #[test]
    fn flags_prepend_in_refine_block() {
        test::<RefinementImportMethods>().expect_offense(indoc! {r#"
            refine Foo do
              prepend Bar
              ^^^^^^^ Use `import_methods` instead of `prepend` because it is deprecated in Ruby 3.1.
            end
        "#});
    }

    #[test]
    fn accepts_non_deprecated_or_non_refine_calls() {
        test::<RefinementImportMethods>()
            .expect_no_offenses("include Foo\n")
            .expect_no_offenses(indoc! {r#"
                refine Foo do
                  import_methods Bar
                end
            "#})
            .expect_no_offenses(indoc! {r#"
                refine Foo do
                  Bar.include Baz
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(RefinementImportMethods);
