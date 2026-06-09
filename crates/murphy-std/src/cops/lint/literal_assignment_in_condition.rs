//! `Lint/LiteralAssignmentInCondition` — flag literal assignments in conditions.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/LiteralAssignmentInCondition
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's plain equals-assignment traversal in if/while/until
//!   conditions. Literal assignment RHS detection uses Murphy's recursive
//!   literal helpers with RuboCop's dstr/xstr and splat-array exclusions.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const MSG_PREFIX: &str = "Don't use literal assignment `= ";
const MSG_SUFFIX: &str = "` in conditional, should be `==` or non-literal operand.";

#[derive(Default)]
pub struct LiteralAssignmentInCondition;

#[cop(
    name = "Lint/LiteralAssignmentInCondition",
    description = "Flag literal assignments in if/while/until conditions.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl LiteralAssignmentInCondition {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::If { cond, .. } = *cx.kind(node) else { return; };
        check_condition(cond, cx);
    }

    #[on_node(kind = "while")]
    fn check_while(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::While { cond, .. } = *cx.kind(node) else { return; };
        check_condition(cond, cx);
    }

    #[on_node(kind = "until")]
    fn check_until(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Until { cond, .. } = *cx.kind(node) else { return; };
        check_condition(cond, cx);
    }
}

fn check_condition(cond: NodeId, cx: &Cx<'_>) {
    let mut ids = vec![cond];
    ids.extend(cx.descendants(cond));

    for id in ids {
        if !cx.is_equals_asgn(id) || is_inner_scope_body_assignment(id, cond, cx) {
            continue;
        }
        let Some(rhs) = assignment_rhs(id, cx) else {
            continue;
        };
        if !all_literals(rhs, cx) || parallel_assignment_with_splat_operator(rhs, cx) {
            continue;
        }
        let rhs_src = cx.raw_source(cx.range(rhs));
        let Some(range) = operator_to_rhs_range(id, rhs, cx) else {
            continue;
        };
        cx.emit_offense(range, &format!("{MSG_PREFIX}{rhs_src}{MSG_SUFFIX}"), None);
    }
}

fn assignment_rhs(id: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    match *cx.kind(id) {
        NodeKind::Lvasgn { value, .. }
        | NodeKind::Ivasgn { value, .. }
        | NodeKind::Cvasgn { value, .. }
        | NodeKind::Gvasgn { value, .. }
        | NodeKind::Casgn { value, .. } => value.get(),
        NodeKind::Masgn { rhs, .. } => Some(rhs),
        _ => None,
    }
}

fn all_literals(id: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(id) {
        NodeKind::Dstr(..) | NodeKind::Xstr(..) => false,
        NodeKind::Array(list) => cx.list(list).iter().all(|&child| all_literals(child, cx)),
        NodeKind::Hash(list) => cx.list(list).iter().all(|&pair| all_literals(pair, cx)),
        NodeKind::Pair { key, value } => all_literals(key, cx) && all_literals(value, cx),
        _ => cx.is_recursive_literal(id),
    }
}

fn parallel_assignment_with_splat_operator(id: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Array(list) = *cx.kind(id) else { return false; };
    cx.list(list)
        .first()
        .is_some_and(|&first| matches!(*cx.kind(first), NodeKind::Splat(_)))
}

fn is_inner_scope_body_assignment(id: NodeId, stop: NodeId, cx: &Cx<'_>) -> bool {
    if cx.is_any_block_type(stop) {
        return true;
    }
    let mut current = id;
    while let Some(parent) = cx.parent(current).get() {
        if parent == stop {
            return false;
        }
        if cx.is_any_block_type(parent) {
            return true;
        }
        if cx.is_any_def_type(parent) && cx.def_body(parent).get() == Some(current) {
            return true;
        }
        current = parent;
    }
    false
}

fn operator_to_rhs_range(asgn: NodeId, rhs: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let asgn_range = cx.range(asgn);
    let rhs_range = cx.range(rhs);
    let start = asgn_range.start as usize;
    let rhs_start = rhs_range.start as usize;
    let rel = cx.source().get(start..rhs_start)?.rfind('=')?;
    Some(Range {
        start: (start + rel) as u32,
        end: rhs_range.end,
    })
}

murphy_plugin_api::submit_cop!(LiteralAssignmentInCondition);

#[cfg(test)]
mod tests {
    use super::LiteralAssignmentInCondition;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_literal_assignment_in_if_condition() {
        test::<LiteralAssignmentInCondition>().expect_offense(indoc! {r#"
            if test = 42
                    ^^^^ Don't use literal assignment `= 42` in conditional, should be `==` or non-literal operand.
            end
        "#});
    }

    #[test]
    fn accepts_non_literal_assignment_in_if_condition() {
        test::<LiteralAssignmentInCondition>().expect_no_offenses("if test = do_something\nend\n");
    }

    #[test]
    fn flags_hash_literal_assignment_in_if_condition() {
        test::<LiteralAssignmentInCondition>().expect_offense(indoc! {r#"
            if test = {x: :y}
                    ^^^^^^^^^ Don't use literal assignment `= {x: :y}` in conditional, should be `==` or non-literal operand.
            end
        "#});
    }

    #[test]
    fn accepts_interpolated_string_assignment() {
        test::<LiteralAssignmentInCondition>().expect_no_offenses("if test = \"#{foo}\"\nend\n");
    }

    #[test]
    fn skips_assignment_inside_condition_block_body() {
        test::<LiteralAssignmentInCondition>().expect_no_offenses("if foo { |x| y = 1 }\nend\n");
    }

    #[test]
    fn flags_assignment_after_logical_or() {
        test::<LiteralAssignmentInCondition>().expect_offense(indoc! {r#"
            if test == 10 || foo = 1
                                 ^^^ Don't use literal assignment `= 1` in conditional, should be `==` or non-literal operand.
            end
       "#});
    }
}
