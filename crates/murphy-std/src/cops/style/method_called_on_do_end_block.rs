//! `Style/MethodCalledOnDoEndBlock` — avoid chaining a method call on a `do...end` block.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/MethodCalledOnDoEndBlock
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detects method calls chained on `do...end` blocks. The receiver of the
//!   outer `send` (or `csend`) node must be a `block`, `numblock`, or
//!   `itblock` node that uses `do...end` delimiters (detected via token scan).
//!
//!   When the outer call itself carries a block (i.e., the outer `send` is
//!   the call inside a `block`/`numblock`/`itblock` node), the offense is
//!   suppressed. This avoids double-reporting with the MultilineBlockChain
//!   cop (which handles `a do ... end.b do ... end` chains) and makes the
//!   rule match RuboCop's `ignore_node(node.send_node)` behaviour.
//!
//!   The check uses `cx.ancestors` to determine at visit time whether the
//!   outer send is the direct call node of a block, replicating RuboCop's
//!   `ignore_node` without a separate state-tracking pass.
//!
//!   The offense range spans from the start of the `end` keyword (i.e., the
//!   beginning of `receiver.loc.end`) to the end of the outer call,
//!   matching RuboCop's `range_between(receiver.loc.end.begin_pos, ...)`.
//!
//!   No autocorrect (RuboCop also does not provide one).
//!
//!   Gap: `Enabled: false` in RuboCop's default config — matches.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! a do
//!   b
//! end.c
//!
//! # good
//! a { b }.c
//!
//! # good (assignment, not chained call)
//! foo = a do
//!   b
//! end
//! foo.c
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct MethodCalledOnDoEndBlock;

const MSG: &str = "Avoid chaining a method call on a do...end block.";

#[cop(
    name = "Style/MethodCalledOnDoEndBlock",
    description = "Avoid chaining a method call on a do...end block.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl MethodCalledOnDoEndBlock {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns true if `node` is a `block`, `numblock`, or `itblock`.
fn is_any_block(id: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        cx.kind(id),
        NodeKind::Block { .. } | NodeKind::Numblock { .. } | NodeKind::Itblock { .. }
    )
}

/// Returns true if `send_node` is the `call` (send/csend) inside a
/// block/numblock/itblock node. That means the outer call _itself_ has a
/// block, so we suppress the offense (matches RuboCop's `ignore_node`).
fn send_has_block_parent(send_node: NodeId, cx: &Cx<'_>) -> bool {
    for ancestor in cx.ancestors(send_node) {
        match *cx.kind(ancestor) {
            NodeKind::Block { call, .. } if call == send_node => return true,
            NodeKind::Numblock { send, .. } if send == send_node => return true,
            NodeKind::Itblock { send, .. } if send == send_node => return true,
            // Stop after the first non-whitespace container.
            _ => break,
        }
    }
    false
}

/// Returns true if `block_node` uses `do...end` delimiters (not braces).
///
/// Scans tokens between the block node start and the body start for the first
/// `do` keyword or `{` token.
fn is_do_end_block(block_node: NodeId, cx: &Cx<'_>) -> bool {
    let from = cx.range(block_node).start;
    let to = body_start(block_node, cx);

    let toks = cx.sorted_tokens();
    let src = cx.source().as_bytes();
    let idx = toks.partition_point(|t| t.range.start < from);
    for tok in &toks[idx..] {
        if tok.range.start >= to {
            break;
        }
        match tok.kind {
            SourceTokenKind::LeftBrace => return false,
            SourceTokenKind::Other
                if &src[tok.range.start as usize..tok.range.end as usize] == b"do" =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}

/// Returns the start of the block body, or the block node end for empty bodies.
fn body_start(node: NodeId, cx: &Cx<'_>) -> u32 {
    match *cx.kind(node) {
        NodeKind::Block { body, .. }
        | NodeKind::Numblock { body, .. }
        | NodeKind::Itblock { body, .. } => {
            body.get().map_or(cx.range(node).end, |b| cx.range(b).start)
        }
        _ => cx.range(node).end,
    }
}

/// Find the `end` keyword token that closes a block node.
/// Returns `None` if the block does not end with an `end` token.
fn find_end_token(block_node: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let node_end = cx.range(block_node).end;
    let toks = cx.sorted_tokens();
    let src = cx.source().as_bytes();
    let idx = toks.partition_point(|t| t.range.end < node_end);
    if let Some(tok) = toks.get(idx) {
        if tok.range.end == node_end
            && tok.kind == SourceTokenKind::Other
            && &src[tok.range.start as usize..tok.range.end as usize] == b"end"
        {
            return Some(tok.range);
        }
    }
    None
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // The receiver must be a block node.
    let Some(receiver) = cx.call_receiver(node).get() else {
        return;
    };
    if !is_any_block(receiver, cx) {
        return;
    }

    // The block must use do...end (not braces).
    if !is_do_end_block(receiver, cx) {
        return;
    }

    // If this send node is itself the send inside a block (i.e., the outer
    // call has a block), suppress the offense — matches RuboCop's
    // `ignore_node(node.send_node)` in on_block/on_numblock/on_itblock.
    if send_has_block_parent(node, cx) {
        return;
    }

    // Offense range: from the start of the `end` keyword of the receiver block
    // to the end of the outer call node.
    // This matches RuboCop's `range_between(receiver.loc.end.begin_pos, node.source_range.end_pos)`.
    let Some(end_tok) = find_end_token(receiver, cx) else {
        return;
    };
    let offense_range = Range {
        start: end_tok.start,
        end: cx.range(node).end,
    };

    cx.emit_offense(offense_range, MSG, None);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::MethodCalledOnDoEndBlock;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_method_called_on_do_end_block() {
        test::<MethodCalledOnDoEndBlock>().expect_offense(indoc! {"
            a do
              b
            end.c
            ^^^^^ Avoid chaining a method call on a do...end block.
        "});
    }

    #[test]
    fn flags_method_with_args_called_on_do_end_block() {
        test::<MethodCalledOnDoEndBlock>().expect_offense(indoc! {"
            a do
              b
            end.c(1, 2)
            ^^^^^^^^^^^ Avoid chaining a method call on a do...end block.
        "});
    }

    #[test]
    fn flags_csend_on_do_end_block() {
        test::<MethodCalledOnDoEndBlock>().expect_offense(indoc! {"
            a do
              b
            end&.c
            ^^^^^^ Avoid chaining a method call on a do...end block.
        "});
    }

    #[test]
    fn accepts_brace_block_with_method_call() {
        test::<MethodCalledOnDoEndBlock>().expect_no_offenses("a { b }.c\n");
    }

    #[test]
    fn accepts_do_end_block_without_method_call() {
        test::<MethodCalledOnDoEndBlock>().expect_no_offenses("a do\n  b\nend\n");
    }

    #[test]
    fn accepts_do_end_block_assigned() {
        test::<MethodCalledOnDoEndBlock>()
            .expect_no_offenses("foo = a do\n  b\nend\nfoo.c\n");
    }

    #[test]
    fn accepts_do_end_block_with_outer_block() {
        // `a do b end.c do d end` — the outer `.c` call has a block,
        // so we suppress the offense (handled by MultilineBlockChain).
        test::<MethodCalledOnDoEndBlock>().expect_no_offenses(indoc! {"
            a do
              b
            end.c do
              d
            end
        "});
    }
}
murphy_plugin_api::submit_cop!(MethodCalledOnDoEndBlock);
