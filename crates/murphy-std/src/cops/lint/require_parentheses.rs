//! `Lint/RequireParentheses` — require parentheses where precedence is ambiguous.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/RequireParentheses
//! upstream_version_checked: master
//! status: partial
//! gap_issues: []
//! notes: >
//!   Initial port covers predicate calls with trailing `&&`/`||` arguments and
//!   call arguments whose ternary condition uses symbolic logical operators.
//! ```

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind};

#[derive(Default)]
pub struct RequireParentheses;

#[cop(
    name = "Lint/RequireParentheses",
    description = "Use parentheses in the method call to avoid confusion about precedence.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RequireParentheses {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if cx.is_parenthesized(node) || cx.is_assignment_method(node) || cx.is_operator_method(node) {
        return;
    }
    let args = cx.call_arguments(node);
    if args.is_empty() {
        return;
    }
    let ambiguous_predicate_arg = cx.is_predicate_method(node)
        && args
            .last()
            .is_some_and(|&arg| matches!(cx.kind(arg), NodeKind::And { .. } | NodeKind::Or { .. }));
    let ambiguous_ternary_arg = args.first().is_some_and(|&arg| {
        matches!(cx.kind(arg), NodeKind::If { .. })
            && cx
                .descendants(arg)
                .into_iter()
                .any(|child| matches!(cx.kind(child), NodeKind::And { .. } | NodeKind::Or { .. }))
    });
    if ambiguous_predicate_arg || ambiguous_ternary_arg {
        cx.emit_offense(
            cx.range(node),
            "Use parentheses in the method call to avoid confusion about precedence.",
            None,
        );
    }
}

murphy_plugin_api::submit_cop!(RequireParentheses);

#[cfg(test)]
mod tests {
    use super::RequireParentheses;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_predicate_call_with_logical_argument() {
        test::<RequireParentheses>().expect_offense(indoc! {r#"
            foo? a && b
            ^^^^^^^^^^^ Use parentheses in the method call to avoid confusion about precedence.
        "#});
    }

    #[test]
    fn accepts_parenthesized_call() {
        test::<RequireParentheses>().expect_no_offenses("foo?(a && b)\n");
    }
}
