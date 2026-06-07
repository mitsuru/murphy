//! `Lint/InheritException` — flag classes inheriting from `Exception`
//! instead of `StandardError` or `RuntimeError`.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/InheritException
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's Lint/InheritException cop with default style
//!   `standard_error`. The `EnforcedStyle` option is exported in the
//!   schema but runtime reads come from `Default` (v1 limitation).

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct InheritException;

#[derive(CopOptions)]
pub struct Options {
    #[option(
        default = "standard_error",
        description = "Preferred base class: `standard_error` or `runtime_error`."
    )]
    pub enforced_style: String,
}

#[cop(
    name = "Lint/InheritException",
    description = "Inherit from StandardError or RuntimeError instead of Exception.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl InheritException {
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Class { superclass, .. } = *cx.kind(node) else { return; };
        let Some(super_id) = superclass.get() else { return; };
        if !is_exception_const(cx, super_id) {
            return;
        }
        if has_preceding_exception_sibling(cx, node, super_id) {
            return;
        }
        let preferred = preferred_base_class();
        cx.emit_offense(
            cx.range(super_id),
            &format!("Inherit from `{preferred}` instead of `Exception`."),
            None,
        );
        cx.emit_edit(cx.range(super_id), &format!("{preferred}"));
    }

    #[on_node(kind = "send", methods = ["new"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let receiver = cx.call_receiver(node);
        let Some(recv_id) = receiver.get() else { return; };
        let is_class_new = match *cx.kind(recv_id) {
            NodeKind::Const { scope: _, name } => cx.symbol_str(name) == "Class",
            _ => false,
        };
        if !is_class_new {
            return;
        }
        let args = cx.call_arguments(node);
        if args.len() != 1 { return; }
        let first = args[0];
        if !is_exception_const(cx, first) {
            return;
        }
        let preferred = preferred_base_class();
        cx.emit_offense(
            cx.range(first),
            &format!("Inherit from `{preferred}` instead of `Exception`."),
            None,
        );
        cx.emit_edit(cx.range(first), &format!("{preferred}"));
    }
}

fn is_exception_const(cx: &Cx<'_>, id: NodeId) -> bool {
    match *cx.kind(id) {
        NodeKind::Const { scope: _, name } => cx.symbol_str(name) == "Exception",
        _ => false,
    }
}

fn has_preceding_exception_sibling(cx: &Cx<'_>, class_node: NodeId, superclass_id: NodeId) -> bool {
    let NodeKind::Const { scope, .. } = *cx.kind(superclass_id) else { return false; };
    if matches!(scope.get(), Some(s) if matches!(*cx.kind(s), NodeKind::Cbase)) {
        return false;
    }
    for sibling in cx.children(class_node) {
        if sibling == superclass_id { break; }
        if is_exception_const(cx, sibling) {
            return true;
        }
    }
    false
}

fn preferred_base_class() -> &'static str {
    "StandardError"
}

#[cfg(test)]
mod tests {
    use super::InheritException;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_class_inheriting_exception() {
        test::<InheritException>().expect_offense(indoc! {r#"
            class C < Exception; end
                      ^^^^^^^^^ Inherit from `StandardError` instead of `Exception`.
        "#});
    }

    #[test]
    fn ignores_class_inheriting_standard_error() {
        test::<InheritException>().expect_no_offenses("class C < StandardError; end\n");
    }

    #[test]
    fn ignores_class_with_no_superclass() {
        test::<InheritException>().expect_no_offenses("class C; end\n");
    }

    #[test]
    fn autocorrects_to_standard_error() {
        test::<InheritException>().expect_correction(
            indoc! {r#"
                class C < Exception; end
                          ^^^^^^^^^ Inherit from `StandardError` instead of `Exception`.
            "#},
            "class C < StandardError; end\n",
        );
    }

    #[test]
    fn flags_class_new_exception() {
        test::<InheritException>().expect_offense(indoc! {r#"
            C = Class.new(Exception)
                          ^^^^^^^^^ Inherit from `StandardError` instead of `Exception`.
        "#});
    }

    #[test]
    fn autocorrects_class_new() {
        test::<InheritException>().expect_correction(
            indoc! {r#"
                C = Class.new(Exception)
                              ^^^^^^^^^ Inherit from `StandardError` instead of `Exception`.
            "#},
            "C = Class.new(StandardError)\n",
        );
    }
}
murphy_plugin_api::submit_cop!(InheritException);
