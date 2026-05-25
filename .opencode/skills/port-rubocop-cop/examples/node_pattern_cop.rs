//! Starter template — cop using `node_pattern!` for shape matching.
//!
//! Use this when the RuboCop original used `def_node_matcher` /
//! `def_node_search`, or whenever the shape spans more than one node
//! kind and would be awkward to destructure by hand.
//!
//! Authoritative grammar: `references/node-pattern.md` plus infra
//! guide §3 ("Reusable matchers: `node_pattern!`").

//! `Pack/MyPatternCop` — flags `expect(...).to eq(true)` (use a
//! boolean matcher instead). Demonstrates `node_pattern!` for a shape
//! that spans `(send (send nil :expect _) :to (send nil :eq true))`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, cop};
use murphy_plugin_macros::node_pattern;

// Zero-capture matcher → bool. Tests shape only.
node_pattern!(
    is_expect_to_eq_true,
    "(send (send nil :expect _) :to (send nil :eq true))"
);

#[derive(Default)]
pub struct MyPatternCop;

#[cop(
    name = "Pack/MyPatternCop",
    description = "Use `be true` / `be_truthy`, not `eq(true)`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl MyPatternCop {
    // No `methods = […]` here — the shape predicate covers method-name
    // discrimination. Bare `kind = "send"` dispatches once per call;
    // the `node_pattern!` guard rejects everything that isn't the
    // target shape.
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        if !is_expect_to_eq_true(node, cx) {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            "Prefer `be true` over `eq(true)`.",
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::MyPatternCop;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn pattern_dispatch_contract() {
        test::<MyPatternCop>()
            .expect_offense(indoc! {r#"
                expect(a).to eq(true)
                ^^^^^^^^^^^^^^^^^^^^^ Prefer `be true` over `eq(true)`.
            "#})
            // `eq(1)` doesn't match the `node_pattern!` because the
            // argument is not the `true` literal.
            .expect_no_offenses(indoc! {r#"
                expect(a).to eq(1)
            "#})
            // The already-preferred form is silent.
            .expect_no_offenses(indoc! {r#"
                expect(a).to be true
            "#});
    }
}
