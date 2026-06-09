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
        for child in std::iter::once(ensure_body).chain(cx.descendants(ensure_body)) {
            if matches!(cx.kind(child), NodeKind::Return(_)) {
                cx.emit_offense(child_range(child, cx), "Do not return from an `ensure` block.", None);
            }
        }
    }
}

fn child_range(node: NodeId, cx: &Cx<'_>) -> murphy_plugin_api::Range {
    cx.range(node)
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
}
