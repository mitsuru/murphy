//! `Style/ConditionalAssignment` — use return value of conditional for assignment.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ConditionalAssignment
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   EnforcedStyle: assign_to_condition (default) supported.
//!   Handles simple if/else and case/when patterns where each branch
//!   assigns to the same variable.
//!   Autocorrect is a v1 gap (report-only).
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, cop};

const MSG: &str = "Use the return of the conditional for variable assignment and comparison.";

#[derive(Default)]
pub struct ConditionalAssignment;

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    #[default]
    #[option(value = "assign_to_condition")]
    AssignToCondition,
    #[option(value = "assign_inside_condition")]
    AssignInsideCondition,
}

#[derive(CopOptions)]
pub struct ConditionalAssignmentOptions {
    #[option(
        default = "assign_to_condition",
        description = "Enforced style for conditional assignment."
    )]
    pub enforced_style: EnforcedStyle,
}

#[cop(
    name = "Style/ConditionalAssignment",
    description = "Use the return of conditional for variable assignment.",
    default_severity = "warning",
    default_enabled = true,
    options = ConditionalAssignmentOptions
)]
impl ConditionalAssignment {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<ConditionalAssignmentOptions>();
        if opts.enforced_style != EnforcedStyle::AssignToCondition {
            return;
        }
        let NodeKind::If { then_, else_, .. } = *cx.kind(node) else {
            return;
        };
        let Some(then_id) = then_.get() else {
            return;
        };
        let Some(else_id) = else_.get() else {
            return;
        };
        let then_lhs = assignment_lhs(then_id, cx);
        let else_lhs = assignment_lhs(else_id, cx);
        let Some(tl) = then_lhs else { return };
        let Some(el) = else_lhs else { return };
        if tl != el {
            return;
        }
        cx.emit_offense(cx.range(node), MSG, None);
    }

    #[on_node(kind = "case")]
    fn check_case(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<ConditionalAssignmentOptions>();
        if opts.enforced_style != EnforcedStyle::AssignToCondition {
            return;
        }
        let NodeKind::Case { else_, whens, .. } = *cx.kind(node) else {
            return;
        };
        let Some(else_id) = else_.get() else {
            return;
        };
        let Some(else_lhs) = assignment_lhs(else_id, cx) else {
            return;
        };
        let all_same = cx.list(whens).iter().all(|&wc| {
            let NodeKind::When { body, .. } = *cx.kind(wc) else {
                return false;
            };
            let Some(body_id) = body.get() else {
                return false;
            };
            assignment_lhs(body_id, cx)
                .map(|lhs| lhs == else_lhs)
                .unwrap_or(false)
        });
        if all_same {
            cx.emit_offense(cx.range(node), MSG, None);
        }
    }
}

fn assignment_lhs(node: NodeId, cx: &Cx<'_>) -> Option<String> {
    let real = match cx.kind(node) {
        NodeKind::Begin(children) => {
            let list = cx.list(*children);
            if list.len() == 1 { list[0] } else { return None }
        }
        _ => node,
    };
    match cx.kind(real) {
        NodeKind::Lvasgn { name, .. } => Some(cx.symbol_str(*name).to_string()),
        NodeKind::Ivasgn { name, .. } => Some(format!("@{}", cx.symbol_str(*name))),
        NodeKind::Gvasgn { name, .. } => Some(format!("${}", cx.symbol_str(*name))),
        NodeKind::Casgn { name, .. } => Some(cx.symbol_str(*name).to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{ConditionalAssignment, ConditionalAssignmentOptions, EnforcedStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_if_else_same_assignment() {
        test::<ConditionalAssignment>().expect_offense(indoc! {"
            if foo
              bar = 1
            else
              bar = 2
            end
        "});
    }

    #[test]
    fn accepts_if_else_different_vars() {
        test::<ConditionalAssignment>().expect_no_offenses(
            "if foo\n  bar = 1\nelse\n  baz = 2\nend\n",
        );
    }

    #[test]
    fn accepts_direct_assignment() {
        test::<ConditionalAssignment>().expect_no_offenses(
            "bar = if foo\n  1\nelse\n  2\nend\n",
        );
    }

    #[test]
    fn flags_case_when_same_assignment() {
        test::<ConditionalAssignment>().expect_offense(indoc! {"
            case foo
            when 'a'
              bar = 1
            else
              bar = 2
            end
        "});
    }
}
murphy_plugin_api::submit_cop!(ConditionalAssignment);
