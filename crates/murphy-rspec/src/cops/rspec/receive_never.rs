//! `RSpec/ReceiveNever` — prefer `not_to receive` over `receive(...).never`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/ReceiveNever
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_send` (`node.method?(:never)` plus
//!   `method_on_stub?` search for `(send nil? :receive ...)` within the
//!   `never` node's subtree) with `RESTRICT_ON_SEND = [:never]`. Any
//!   `never` whose subtree contains a bare `receive` flags; the receiver of
//!   `never` itself is not constrained. The offense range is the selector per
//!   `add_offense(node.loc.selector)`. Detection is at parity; autocorrect
//!   (replace parent runner with `not_to`, drop `.never`) is not ported in
//!   this batch — same convention as `RSpec/ItBehavesLike` (status: partial,
//!   autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with `methods = ["never"]`:
//!
//! - `expect(foo).to receive(:bar).never` — flagged (range is `never`).
//! - `allow(foo).to receive(:bar).never` — flagged.
//! - `expect(foo).not_to receive(:bar)` — no `never`, not flagged.
//! - `foo.never` — no `receive` in subtree, not flagged.
//!
//! ## No autocorrect
//!
//! Upstream replaces the parent runner with `not_to` and drops `.never`.
//! This batch reports only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ReceiveNever;

#[cop(
    name = "RSpec/ReceiveNever",
    description = "Prefer not_to receive over receive.never.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl ReceiveNever {
    #[on_node(kind = "send", methods = ["never"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        if !subtree_has_bare_receive(cx, node) {
            return;
        }
        cx.emit_offense(
            cx.node(node).loc.name,
            "Use `not_to receive` instead of `never`.",
            None,
        );
    }
}

/// `true` when `root` or any descendant is a bare `receive` send.
///
/// Mirrors upstream `method_on_stub?` (`def_node_search` for
/// `(send nil? :receive ...)`), which searches the `never` node and its
/// descendants (the receiver chain lives inside the subtree, e.g.
/// `expect(foo).to(receive(:bar).never)` parses with `receive` under `never`).
fn subtree_has_bare_receive(cx: &Cx<'_>, root: NodeId) -> bool {
    for id in core::iter::once(root).chain(cx.descendants(root)) {
        if let NodeKind::Send {
            receiver, method, ..
        } = *cx.kind(id)
            && receiver == OptNodeId::NONE
            && cx.symbol_str(method) == "receive"
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::ReceiveNever;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_expect_receive_never() {
        test::<ReceiveNever>().expect_offense(indoc! {r#"
                expect(foo).to receive(:bar).never
                                             ^^^^^ Use `not_to receive` instead of `never`.
            "#});
    }

    #[test]
    fn flags_allow_receive_never() {
        test::<ReceiveNever>().expect_offense(indoc! {r#"
                allow(foo).to receive(:bar).never
                                            ^^^^^ Use `not_to receive` instead of `never`.
            "#});
    }

    #[test]
    fn does_not_flag_not_to_receive() {
        test::<ReceiveNever>().expect_no_offenses(indoc! {r#"
                expect(foo).not_to receive(:bar)
            "#});
    }

    #[test]
    fn does_not_flag_bare_never() {
        // No `receive` in the subtree.
        test::<ReceiveNever>().expect_no_offenses(indoc! {r#"
                foo.never
            "#});
    }
}

murphy_plugin_api::submit_cop!(ReceiveNever);
