//! `Lint/SelfAssignment` — checks for self-assignments.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/SelfAssignment
//! upstream_version_checked: master
//! status: partial
//! gap_issues: []
//! notes: >
//!   Initial port covers plain local/instance/class/global/constant assignments
//!   where the RHS is the same variable. Method, safe-navigation, multiple, and
//!   shorthand assignment forms are v1 gaps.
//! ```

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind, Symbol};

#[derive(Default)]
pub struct SelfAssignment;

#[cop(
    name = "Lint/SelfAssignment",
    description = "Checks for self-assignments.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl SelfAssignment {
    #[on_node(kind = "lvasgn")]
    fn check_lvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        check_assignment(node, cx);
    }
    #[on_node(kind = "ivasgn")]
    fn check_ivasgn(&self, node: NodeId, cx: &Cx<'_>) {
        check_assignment(node, cx);
    }
    #[on_node(kind = "cvasgn")]
    fn check_cvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        check_assignment(node, cx);
    }
    #[on_node(kind = "gvasgn")]
    fn check_gvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        check_assignment(node, cx);
    }
    #[on_node(kind = "casgn")]
    fn check_casgn(&self, node: NodeId, cx: &Cx<'_>) {
        check_assignment(node, cx);
    }
}

#[derive(Clone, Copy)]
enum VarKind {
    Local,
    Instance,
    Class,
    Global,
    Const,
}

fn check_assignment(node: NodeId, cx: &Cx<'_>) {
    let Some((kind, name, value)) = assignment_parts(node, cx) else {
        return;
    };
    let Some(value) = value.get() else {
        return;
    };
    if matches_var(value, kind, name, cx) {
        cx.emit_offense(cx.range(node), "Self-assignment detected.", None);
    }
}

fn assignment_parts(
    node: NodeId,
    cx: &Cx<'_>,
) -> Option<(VarKind, Symbol, murphy_plugin_api::OptNodeId)> {
    match *cx.kind(node) {
        NodeKind::Lvasgn { name, value } => Some((VarKind::Local, name, value)),
        NodeKind::Ivasgn { name, value } => Some((VarKind::Instance, name, value)),
        NodeKind::Cvasgn { name, value } => Some((VarKind::Class, name, value)),
        NodeKind::Gvasgn { name, value } => Some((VarKind::Global, name, value)),
        NodeKind::Casgn { name, value, .. } => Some((VarKind::Const, name, value)),
        _ => None,
    }
}

fn matches_var(node: NodeId, kind: VarKind, name: Symbol, cx: &Cx<'_>) -> bool {
    match (kind, *cx.kind(node)) {
        (VarKind::Local, NodeKind::Lvar(sym))
        | (VarKind::Instance, NodeKind::Ivar(sym))
        | (VarKind::Class, NodeKind::Cvar(sym))
        | (VarKind::Global, NodeKind::Gvar(sym)) => sym == name,
        (VarKind::Const, NodeKind::Const { name: sym, .. }) => sym == name,
        _ => false,
    }
}

murphy_plugin_api::submit_cop!(SelfAssignment);

#[cfg(test)]
mod tests {
    use super::SelfAssignment;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_local_self_assignment() {
        test::<SelfAssignment>().expect_offense(indoc! {r#"
            foo = foo
            ^^^^^^^^^ Self-assignment detected.
        "#});
    }

    #[test]
    fn accepts_different_assignment() {
        test::<SelfAssignment>().expect_no_offenses("foo = bar\n");
    }
}
