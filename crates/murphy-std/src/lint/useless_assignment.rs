use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

#[derive(Default)]
pub struct UselessAssignment;

#[cop(
    name = "Lint/UselessAssignment",
    description = "Flag local variable assignments that are never read in the same scope.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl UselessAssignment {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        visit_scope(cx, cx.root());
    }
}

fn visit_scope(cx: &Cx<'_>, root: NodeId) {
    analyze_scope(cx, root);
    for node in scope_nodes(cx, root) {
        if node != root && is_scope(cx, node) {
            visit_scope(cx, node);
        }
    }
}

fn analyze_scope(cx: &Cx<'_>, root: NodeId) {
    let nodes = scope_nodes(cx, root);
    for id in &nodes {
        if let NodeKind::Lvasgn { name, .. } = *cx.kind(*id) {
            let name = cx.symbol_str(name);
            if name.starts_with('_') {
                continue;
            }
            let assign_range = assignment_name_range(cx, *id, name);
            let read_after = nodes.iter().any(|candidate| {
                cx.range(*candidate).start > cx.range(*id).end
                    && matches!(*cx.kind(*candidate), NodeKind::Lvar(s) if cx.symbol_str(s) == name)
            });
            if !read_after {
                cx.emit_offense(assign_range, "Useless assignment to local variable", None);
            }
        }
    }
}

fn scope_nodes(cx: &Cx<'_>, root: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        out.push(node);
        if node != root && is_scope(cx, node) {
            continue;
        }
        let mut children = cx.children(node);
        children.reverse();
        stack.extend(children);
    }
    out
}

fn is_scope(cx: &Cx<'_>, node: NodeId) -> bool {
    matches!(
        *cx.kind(node),
        NodeKind::Def { .. }
            | NodeKind::Block { .. }
            | NodeKind::Class { .. }
            | NodeKind::Module { .. }
            | NodeKind::Sclass { .. }
    )
}

fn assignment_name_range(cx: &Cx<'_>, node: NodeId, name: &str) -> Range {
    let range = cx.range(node);
    let raw = cx.raw_source(range);
    let pos = raw.find(name).unwrap_or(0) as u32;
    Range {
        start: range.start + pos,
        end: range.start + pos + name.len() as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::UselessAssignment;
    use murphy_plugin_api::test_support::{
        expect_no_offenses, expect_offense, indoc, run_cop_with_edits,
    };

    #[test]
    fn flags_assignments_that_are_never_read() {
        expect_offense!(
            UselessAssignment,
            indoc! {r#"
            used = 1
            unused = 2
            ^^^^^^ Useless assignment to local variable
            used
        "#}
        );
    }

    #[test]
    fn ignores_underscore_assignments_and_has_no_autocorrect() {
        expect_no_offenses!(UselessAssignment, "名前 = 1\n名前\n_unused = 2\n");
        let run = run_cop_with_edits::<UselessAssignment>("unused = 1\n");
        assert_eq!(run.edits.len(), 0);
    }

    #[test]
    fn nested_method_read_does_not_satisfy_outer_assignment() {
        expect_offense!(
            UselessAssignment,
            indoc! {r#"
            outer = 1
            ^^^^^ Useless assignment to local variable
            def inner
              outer
            end
        "#}
        );
    }

    #[test]
    fn earlier_read_does_not_satisfy_later_assignment() {
        expect_offense!(
            UselessAssignment,
            indoc! {r#"
            x = 0
            x
            x = 1
            ^ Useless assignment to local variable
        "#}
        );
    }
}
