//! `Lint/EmptyExpression` — flag empty parenthesized expressions.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/EmptyExpression
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's on_begin check for begin nodes with no children. No
//!   options or autocorrect.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct EmptyExpression;

#[cop(
    name = "Lint/EmptyExpression",
    description = "Flag empty expressions.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl EmptyExpression {
    #[on_node(kind = "begin")]
    fn check_begin(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Begin(children) = *cx.kind(node) else { return; };
        if cx.list(children).is_empty() {
            cx.emit_offense(cx.range(node), "Avoid empty expressions.", None);
        }
    }
}

murphy_plugin_api::submit_cop!(EmptyExpression);

#[cfg(test)]
mod tests {
    use super::EmptyExpression;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_empty_parentheses() {
        test::<EmptyExpression>().expect_offense(indoc! {r#"
            foo = ()
                  ^^ Avoid empty expressions.
        "#});
    }

    #[test]
    fn accepts_non_empty_parentheses() {
        test::<EmptyExpression>().expect_no_offenses("foo = (bar)\n");
    }
}
