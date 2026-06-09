//! `Lint/EnsureReturn` — flag `return` statements inside `ensure` bodies.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/EnsureReturn
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's on_ensure traversal by checking every return node under
//!   the ensure branch. No options or autocorrect.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct EnsureReturn;

#[cop(
    name = "Lint/EnsureReturn",
    description = "Flag return statements inside ensure blocks.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl EnsureReturn {
    #[on_node(kind = "ensure")]
    fn check_ensure(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Ensure { ensure_, .. } = *cx.kind(node) else { return; };
        let Some(ensure_body) = ensure_.get() else { return; };
        check_returns_in_ensure_scope(ensure_body, cx);
    }
}

fn check_returns_in_ensure_scope(root: NodeId, cx: &Cx<'_>) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if is_return_scope_boundary(node, cx) {
            continue;
        }
        if matches!(cx.kind(node), NodeKind::Return(_)) {
            cx.emit_offense(cx.range(node), "Do not return from an `ensure` block.", None);
        }
        stack.extend(cx.children(node));
    }
}

fn is_return_scope_boundary(node: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Def { .. } | NodeKind::Defs { .. } | NodeKind::Lambda => true,
        NodeKind::Block { call, .. } => is_lambda_or_proc_call(call, cx),
        _ => false,
    }
}

fn is_lambda_or_proc_call(node: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Send { receiver, method, .. } => {
            let name = cx.symbol_str(method);
            matches!(name, "lambda" | "proc")
                || (name == "new"
                    && receiver.get().is_some_and(|recv| matches!(*cx.kind(recv), NodeKind::Const { name, .. } if cx.symbol_str(name) == "Proc")))
        }
        _ => false,
    }
}

murphy_plugin_api::submit_cop!(EnsureReturn);

#[cfg(test)]
mod tests {
    use super::EnsureReturn;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_return_inside_ensure() {
        test::<EnsureReturn>().expect_offense(indoc! {r#"
            def foo
              work
            ensure
              return self
              ^^^^^^^^^^^ Do not return from an `ensure` block.
            end
        "#});
    }

    #[test]
    fn accepts_return_before_ensure() {
        test::<EnsureReturn>().expect_no_offenses(indoc! {r#"
            def foo
              return self
            ensure
              cleanup
            end
        "#});
    }

    #[test]
    fn ignores_returns_inside_nested_definitions_and_lambdas() {
        test::<EnsureReturn>()
            .expect_no_offenses(indoc! {r#"
                def foo
                  work
                ensure
                  def helper
                    return 1
                  end
                end
            "#})
            .expect_no_offenses(indoc! {r#"
                def foo
                  work
                ensure
                  lambda { return 1 }
                  Proc.new { return 2 }
                end
            "#});
    }
}
