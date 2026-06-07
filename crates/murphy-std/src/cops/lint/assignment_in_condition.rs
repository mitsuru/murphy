//! `Lint/AssignmentInCondition` — flag assignments in conditions of
//! `if`/`while`/`until`.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/AssignmentInCondition
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's Lint/AssignmentInCondition cop: assignments in
//!   if/while/until conditions are flagged. The `AllowSafeAssignment`
//!   option is exported in the schema but runtime reads come from
//!   `Default` (v1 limitation shared with all option-bearing cops).
//! ```
//!
//! ## Known v1 limitation: option overrides not wired through `Cx`
//!
//! `allow_safe_assignment` is exported via `#[derive(CopOptions)]` so the
//! host validates `murphy.toml` entries, but runtime reads still come from
//! `Options::default()`. `murphy-9cr.9` will route overrides through `Cx`.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};

#[derive(Default)]
pub struct AssignmentInCondition;

#[derive(CopOptions)]
pub struct Options {
    #[option(
        default = true,
        description = "When true, allow assignments wrapped in parentheses in conditions."
    )]
    pub allow_safe_assignment: bool,
}

#[cop(
    name = "Lint/AssignmentInCondition",
    description = "Flag assignments in if/while/until conditions.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl AssignmentInCondition {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::If { cond, .. } = *cx.kind(node) else { return; };
        self.check_condition(cond, cx);
    }

    #[on_node(kind = "while")]
    fn check_while(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::While { cond, .. } = *cx.kind(node) else { return; };
        self.check_condition(cond, cx);
    }

    #[on_node(kind = "until")]
    fn check_until(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Until { cond, .. } = *cx.kind(node) else { return; };
        self.check_condition(cond, cx);
    }
}

impl AssignmentInCondition {
    fn check_condition(&self, cond: NodeId, cx: &Cx<'_>) {
        let opts = Options::default();
        let descendants = cx.descendants(cond);
        let mut all_ids = vec![cond];
        all_ids.extend(descendants);

        for &child in &all_ids {
            if !is_assignment_kind(cx, child) {
                continue;
            }
            if is_in_any_block(cx, child, cond) {
                continue;
            }
            if opts.allow_safe_assignment && is_safe_assignment(cx, child) {
                continue;
            }
            let msg = if opts.allow_safe_assignment {
                "Use `==` if you meant to do a comparison or wrap the expression in parentheses to indicate you meant to assign in a condition."
            } else {
                "Use `==` if you meant to do a comparison or move the assignment up out of the condition."
            };
            cx.emit_offense(cx.range(child), msg, None);
            if opts.allow_safe_assignment {
                cx.emit_edit(
                    Range { start: cx.range(child).start, end: cx.range(child).start },
                    "(",
                );
                cx.emit_edit(
                    Range { start: cx.range(child).end, end: cx.range(child).end },
                    ")",
                );
            }
        }
    }
}

fn is_assignment_kind(cx: &Cx<'_>, id: NodeId) -> bool {
    match *cx.kind(id) {
        NodeKind::Lvasgn { .. }
        | NodeKind::Ivasgn { .. }
        | NodeKind::Gvasgn { .. }
        | NodeKind::Cvasgn { .. }
        | NodeKind::Casgn { .. } => true,
        NodeKind::Send { .. } => cx.is_assignment_method(id),
        _ => false,
    }
}

fn is_safe_assignment(cx: &Cx<'_>, id: NodeId) -> bool {
    let Some(parent_id) = cx.parent(id).get() else { return false; };
    matches!(*cx.kind(parent_id), NodeKind::Begin(_))
}

fn is_in_any_block(cx: &Cx<'_>, id: NodeId, stop: NodeId) -> bool {
    let mut current = id;
    loop {
        let Some(parent_id) = cx.parent(current).get() else { return false; };
        if parent_id == stop { return false; }
        match *cx.kind(parent_id) {
            NodeKind::Block { .. }
            | NodeKind::Numblock { .. }
            | NodeKind::Itblock { .. }
            | NodeKind::Lambda => return true,
            _ => current = parent_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AssignmentInCondition;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_assignment_in_if_condition() {
        test::<AssignmentInCondition>().expect_offense(indoc! {r#"
            if test = 10
               ^^^^^^^^^ Use `==` if you meant to do a comparison or wrap the expression in parentheses to indicate you meant to assign in a condition.
              do_something
            end
        "#});
    }

    #[test]
    fn safe_assignment_allows_parentheses() {
        test::<AssignmentInCondition>().expect_no_offenses("if (test = 10)\n  do_something\nend\n");
    }

    #[test]
    fn ignores_comparison_in_condition() {
        test::<AssignmentInCondition>().expect_no_offenses("if test == 10\n  do_something\nend\n");
    }

    #[test]
    fn flags_assignment_in_while() {
        test::<AssignmentInCondition>().expect_offense(indoc! {r#"
            while test = 10
                  ^^^^^^^^^ Use `==` if you meant to do a comparison or wrap the expression in parentheses to indicate you meant to assign in a condition.
              do_something
            end
        "#});
    }

    #[test]
    fn flags_assignment_in_until() {
        test::<AssignmentInCondition>().expect_offense(indoc! {r#"
            until test = 10
                  ^^^^^^^^^ Use `==` if you meant to do a comparison or wrap the expression in parentheses to indicate you meant to assign in a condition.
              do_something
            end
        "#});
    }

    #[test]
    fn flags_instance_variable_assignment() {
        test::<AssignmentInCondition>().expect_offense(indoc! {r#"
            if @test = 10
               ^^^^^^^^^^ Use `==` if you meant to do a comparison or wrap the expression in parentheses to indicate you meant to assign in a condition.
              do_something
            end
        "#});
    }

    #[test]
    fn autocorrects_with_parentheses() {
        test::<AssignmentInCondition>().expect_correction(
            indoc! {r#"
                if test = 10
                   ^^^^^^^^^ Use `==` if you meant to do a comparison or wrap the expression in parentheses to indicate you meant to assign in a condition.
                  do_something
                end
            "#},
            "if (test = 10)\n  do_something\nend\n",
        );
    }
}
murphy_plugin_api::submit_cop!(AssignmentInCondition);
