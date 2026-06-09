//! `Lint/ItWithoutArgumentsInBlock` — checks bare `it` calls in blocks.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/ItWithoutArgumentsInBlock
//! upstream_version_checked: master
//! status: partial
//! gap_issues: []
//! notes: >
//!   Covers bare receiverless `it` calls with no arguments and no parentheses
//!   inside blocks whose argument list is empty without delimiters. Known v1
//!   limitations: target Ruby version gating is not available on the current
//!   cop API, so the cop always behaves like RuboCop for Ruby <= 3.3. Local
//!   variable shadowing is approximated from visible `it = ...` writes in the
//!   block body.
//! ```

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind, OptNodeId};

const MSG: &str = "`it` calls without arguments will refer to the first block param in Ruby 3.4; use `it()` or `self.it`.";

#[derive(Default)]
pub struct ItWithoutArgumentsInBlock;

#[cop(
    name = "Lint/ItWithoutArgumentsInBlock",
    description = "Checks uses of `it` calls without arguments in blocks.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl ItWithoutArgumentsInBlock {
    #[on_node(kind = "lvar")]
    fn check_lvar(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Lvar(name) = *cx.kind(node) else {
            return;
        };
        if cx.symbol_str(name) != "it" {
            return;
        }
        let Some(block) = cx.ancestors(node).find(|&ancestor| {
            matches!(cx.kind(ancestor), NodeKind::Block { .. } | NodeKind::Itblock { .. })
        }) else {
            return;
        };
        if !block_accepts_implicit_it(block, cx) {
            return;
        }
        if block_body(block, cx).is_some_and(|body| local_it_assigned_before(body, node, cx)) {
            return;
        }
        cx.emit_offense(cx.range(node), MSG, None);
    }

    #[on_node(kind = "send", methods = ["it"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, args, .. } = *cx.kind(node) else {
            return;
        };
        if receiver != OptNodeId::NONE || !cx.list(args).is_empty() || cx.is_parenthesized(node) {
            return;
        }
        if cx.block_node(node).get().is_some() {
            return;
        }
        let Some(block) = cx.ancestors(node).find(|&ancestor| {
            matches!(cx.kind(ancestor), NodeKind::Block { .. } | NodeKind::Itblock { .. })
        })
        else {
            return;
        };
        if !block_accepts_implicit_it(block, cx) {
            return;
        }
        if block_body(block, cx).is_some_and(|body| local_it_assigned_before(body, node, cx)) {
            return;
        }
        cx.emit_offense(cx.range(node), MSG, None);
    }
}

fn block_accepts_implicit_it(block: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(block) {
        NodeKind::Block { args, .. } => block_args_empty_without_delimiters(args, cx),
        NodeKind::Itblock { .. } => true,
        _ => false,
    }
}

fn block_body(block: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    match *cx.kind(block) {
        NodeKind::Block { body, .. } | NodeKind::Itblock { body, .. } => body.get(),
        _ => None,
    }
}

fn block_args_empty_without_delimiters(args: NodeId, cx: &Cx<'_>) -> bool {
    matches!(*cx.kind(args), NodeKind::Args(list) if cx.list(list).is_empty())
        && !cx.raw_source(cx.range(args)).contains('|')
}

fn local_it_assigned_before(body: NodeId, node: NodeId, cx: &Cx<'_>) -> bool {
    cx.descendants(body).into_iter().any(|candidate| {
        cx.range(candidate).start < cx.range(node).start
            && matches!(*cx.kind(candidate), NodeKind::Lvasgn { name, .. } if cx.symbol_str(name) == "it")
    })
}

murphy_plugin_api::submit_cop!(ItWithoutArgumentsInBlock);

#[cfg(test)]
mod tests {
    use super::ItWithoutArgumentsInBlock;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_bare_it_without_arguments_in_block() {
        test::<ItWithoutArgumentsInBlock>().expect_offense(indoc! {r#"
            0.times { it }
                      ^^ `it` calls without arguments will refer to the first block param in Ruby 3.4; use `it()` or `self.it`.
        "#});
    }

    #[test]
    fn accepts_non_deprecated_it_shapes() {
        test::<ItWithoutArgumentsInBlock>()
            .expect_no_offenses("0.times { it() }\n")
            .expect_no_offenses("0.times { self.it }\n")
            .expect_no_offenses("0.times { it(42) }\n")
            .expect_no_offenses("if false\n  it\nend\n");
    }
}
