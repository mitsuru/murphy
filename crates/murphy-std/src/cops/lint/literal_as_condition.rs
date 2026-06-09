//! `Lint/LiteralAsCondition` — checks literal conditions.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/LiteralAsCondition
//! upstream_version_checked: master
//! status: partial
//! gap_issues: []
//! notes: >
//!   Covers literal `if`/`unless`/ternary, `while`/`until`, `case` subjects,
//!   `case` without subject where every `when` condition is literal, literal
//!   LHS of condition-level `&&`/`||`, and `!literal`. Known v1 limitations:
//!   RuboCop's broad autocorrection is not implemented; post-loop rewrite and
//!   `case in`/pattern-matching match-var exclusions are partial; nested
//!   condition-level `and`/`or` handling is conservative.
//! ```

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind};

const MSG_TEMPLATE: &str = "Literal `%s` appeared as a condition.";

#[derive(Default)]
pub struct LiteralAsCondition;

#[cop(
    name = "Lint/LiteralAsCondition",
    description = "Checks for literals used as conditions.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl LiteralAsCondition {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::If { cond, .. } = *cx.kind(node) else {
            return;
        };
        check_literal(cond, cx);
    }

    #[on_node(kind = "while")]
    fn check_while(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::While { cond, .. } = *cx.kind(node) else {
            return;
        };
        if !matches!(cx.kind(cond), NodeKind::True_) {
            check_literal(cond, cx);
        }
    }

    #[on_node(kind = "until")]
    fn check_until(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Until { cond, .. } = *cx.kind(node) else {
            return;
        };
        if !matches!(cx.kind(cond), NodeKind::False_) {
            check_literal(cond, cx);
        }
    }

    #[on_node(kind = "case")]
    fn check_case(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Case { subject, whens, .. } = *cx.kind(node) else {
            return;
        };
        if let Some(subject) = subject.get() {
            if !matches!(cx.kind(subject), NodeKind::Dstr(_)) {
                check_literal(subject, cx);
            }
            return;
        }
        for &when_node in cx.list(whens) {
            let NodeKind::When { conds, .. } = *cx.kind(when_node) else {
                continue;
            };
            let conds = cx.list(conds);
            if !conds.is_empty() && conds.iter().all(|&cond| cx.is_literal(cond)) {
                for &cond in conds {
                    check_literal(cond, cx);
                }
            }
        }
    }

    #[on_node(kind = "and")]
    fn check_and(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::And { lhs, .. } = *cx.kind(node) else {
            return;
        };
        if condition_operand(node, cx) && cx.is_truthy_literal(lhs) {
            check_literal(lhs, cx);
        }
    }

    #[on_node(kind = "or")]
    fn check_or(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Or { lhs, .. } = *cx.kind(node) else {
            return;
        };
        if condition_operand(node, cx) && cx.is_falsey_literal(lhs) {
            check_literal(lhs, cx);
        }
    }

    #[on_node(kind = "send", methods = ["!"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        if let Some(receiver) = cx.call_receiver(node).get() {
            check_literal(receiver, cx);
        }
    }
}

fn check_literal(node: NodeId, cx: &Cx<'_>) {
    if !cx.is_literal(node) {
        return;
    }
    let literal = cx.raw_source(cx.range(node));
    let message = MSG_TEMPLATE.replace("%s", literal);
    cx.emit_offense(cx.range(node), &message, None);
}

fn condition_operand(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return true;
    };
    match *cx.kind(parent) {
        NodeKind::If { cond, .. } | NodeKind::While { cond, .. } | NodeKind::Until { cond, .. } => {
            cond == node
        }
        NodeKind::Case { subject, .. } => subject.get() == Some(node),
        NodeKind::Send { method, .. } => cx.symbol_str(method) == "!" && condition_operand(parent, cx),
        NodeKind::And { .. } | NodeKind::Or { .. } => condition_operand(parent, cx),
        _ => false,
    }
}

murphy_plugin_api::submit_cop!(LiteralAsCondition);

#[cfg(test)]
mod tests {
    use super::LiteralAsCondition;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_truthy_and_falsey_if_conditions() {
        test::<LiteralAsCondition>()
            .expect_offense(indoc! {r#"
                if 20
                   ^^ Literal `20` appeared as a condition.
                  top
                end
            "#})
            .expect_offense(indoc! {r#"
                if nil
                   ^^^ Literal `nil` appeared as a condition.
                  top
                end
            "#});
    }

    #[test]
    fn flags_loop_and_case_conditions() {
        test::<LiteralAsCondition>()
            .expect_offense(indoc! {r#"
                while 1
                      ^ Literal `1` appeared as a condition.
                  top
                end
            "#})
            .expect_offense(indoc! {r#"
                case :sym
                     ^^^^ Literal `:sym` appeared as a condition.
                when x then top
                end
            "#});
    }

    #[test]
    fn accepts_allowed_and_non_condition_literals() {
        test::<LiteralAsCondition>()
            .expect_no_offenses("while true\n  break\nend\n")
            .expect_no_offenses("until false\n  break\nend\n")
            .expect_no_offenses("if test(20)\n  top\nend\n")
            .expect_no_offenses("case x\nwhen 20 then top\nend\n");
    }
}
