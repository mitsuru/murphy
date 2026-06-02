//! `Style/MultilineBlockChain` — flags multi-line chains of blocks.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/MultilineBlockChain
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detects block, numblock, and itblock nodes whose `call` receiver is
//!   itself a block node (block/numblock/itblock) that spans multiple lines.
//!   The offense range runs from the start of the receiver block's closing
//!   `end` keyword (or closing `}`) to the end of the outer block's `call`
//!   (send) node, highlighting `end.method` or `end&.method` or `}.method`.
//!   This matches RuboCop's
//!   range_between(receiver.loc.end.begin_pos, node.send_node.source_range.end_pos).
//!   No autocorrect is provided — RuboCop does not autocorrect this cop.
//!   Gap: when the outer block's send_node has intermediate chained calls
//!   between the receiver block and the outer call (e.g. `a do end.c1.c2 do end`),
//!   RuboCop walks each_node(:call) in the outer send to find the multiline block
//!   receiver; Murphy only checks the direct receiver of the outer call's send node.
//! ```
//!
//! ## Matched shapes
//!
//! A `block`, `numblock`, or `itblock` node whose `call` is a `send` or
//! `csend` with a receiver that is a block-type node spanning multiple lines.
//!
//! ## Examples
//!
//! ```ruby
//! # bad
//! a do
//!   b
//! end.c do
//!   d
//! end
//!
//! # good (single-line receiver block)
//! a { b }.c { d }
//!
//! # good (no chain)
//! a do
//!   b
//! end
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

const MSG: &str = "Avoid multi-line chains of blocks.";

#[derive(Default)]
pub struct MultilineBlockChain;

#[cop(
    name = "Style/MultilineBlockChain",
    description = "Avoid multi-line chains of blocks.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl MultilineBlockChain {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// Returns the `call` (send) node for any block type.
fn call_of(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    match *cx.kind(node) {
        NodeKind::Block { call, .. } => Some(call),
        NodeKind::Numblock { send, .. } | NodeKind::Itblock { send, .. } => Some(send),
        _ => None,
    }
}

/// Returns true if the node is a block-type node (block, numblock, or itblock).
fn is_block_type(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        *cx.kind(node),
        NodeKind::Block { .. } | NodeKind::Numblock { .. } | NodeKind::Itblock { .. }
    )
}

/// Find the start offset of the closing delimiter token (`end` or `}`) for a block.
///
/// RuboCop uses `receiver.loc.end.begin_pos` to obtain the start of the closing
/// delimiter. Murphy's `NodeLoc` does not have an `end` field (it has `expression`
/// and `name`), so we instead scan the token stream for the last token in the block's
/// range — which must be `end` (do/end blocks) or `}` (brace blocks).
///
/// Returns `None` if the closing delimiter token cannot be found (e.g. parse error
/// or unexpected AST shape). The caller should skip emitting an offense in that case.
fn closing_delimiter_start(node: NodeId, cx: &Cx<'_>) -> Option<u32> {
    let node_end = cx.range(node).end;
    let toks = cx.sorted_tokens();
    let src = cx.source().as_bytes();
    // Look for the token whose range.end == node_end.
    let idx = toks.partition_point(|t| t.range.end < node_end);
    if let Some(tok) = toks.get(idx) {
        if tok.range.end == node_end {
            // Verify it's `end` or `}`.
            let is_end = tok.kind == SourceTokenKind::Other
                && &src[tok.range.start as usize..tok.range.end as usize] == b"end";
            let is_rbrace = tok.kind == SourceTokenKind::RightBrace;
            if is_end || is_rbrace {
                return Some(tok.range.start);
            }
        }
    }
    // No closing delimiter found — malformed or unexpected AST shape.
    None
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Get the call node (send/csend) of the outer block.
    let Some(call) = call_of(node, cx) else {
        return;
    };

    // The call must be a Send or Csend with a receiver.
    let receiver_id = match *cx.kind(call) {
        NodeKind::Send { receiver, .. } => receiver.get(),
        NodeKind::Csend { receiver, .. } => Some(receiver),
        _ => return,
    };

    // The receiver must be a block-type node.
    let Some(recv_id) = receiver_id else {
        return;
    };
    if !is_block_type(recv_id, cx) {
        return;
    }

    // The receiver block must be multiline.
    if !cx.is_multiline(recv_id) {
        return;
    }

    // Offense range: from start of receiver's closing delimiter to end of the
    // method name (loc.name.end). This highlights `end.method` (or `end&.method`
    // or `}.method`), matching RuboCop's
    // range_between(receiver.loc.end.begin_pos, node.send_node.source_range.end_pos).
    let Some(offense_start) = closing_delimiter_start(recv_id, cx) else {
        // No closing delimiter found. In a well-formed AST this path should be
        // unreachable — every block node ends with `end` or `}`. Skip the
        // offense rather than reporting with an incorrect range.
        debug_assert!(false, "MultilineBlockChain: no closing delimiter for block node");
        return;
    };
    // Use loc.name.end (end of the method name token itself) to match RuboCop's
    // `node.send_node.source_range.end_pos`. In RuboCop, `send_node.source_range`
    // covers only the method name for a chained call (e.g. `c` in `end.c`), which
    // aligns with `loc.name.end` in Murphy's arena AST.
    let method_name_end = cx.node(call).loc.name.end;

    cx.emit_offense(
        Range {
            start: offense_start,
            end: method_name_end,
        },
        MSG,
        None,
    );
}

#[cfg(test)]
mod tests {
    use super::MultilineBlockChain;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_simple_multiline_do_end_chain() {
        test::<MultilineBlockChain>().expect_offense(indoc! {"
            a do
              b
            end.c do
            ^^^^^  Avoid multi-line chains of blocks.
              d
            end
        "});
    }

    #[test]
    fn flags_multiline_brace_receiver_chained() {
        test::<MultilineBlockChain>().expect_offense(indoc! {"
            Thread.list.find_all { |t|
              t.alive?
            }.map { |thread| thread.object_id }
            ^^^^^  Avoid multi-line chains of blocks.
        "});
    }

    #[test]
    fn flags_safe_navigation_chain() {
        test::<MultilineBlockChain>().expect_offense(indoc! {"
            a do
              b
            end&.c do
            ^^^^^^  Avoid multi-line chains of blocks.
              d
            end
        "});
    }

    #[test]
    fn accepts_single_line_chain() {
        test::<MultilineBlockChain>().expect_no_offenses("a { b }.c { d }\n");
    }

    #[test]
    fn accepts_single_line_do_end_chain() {
        // w do x end.y do z end — single-line, no offense
        test::<MultilineBlockChain>().expect_no_offenses("w do x end.y do z end\n");
    }

    #[test]
    fn accepts_multiline_block_no_chain() {
        test::<MultilineBlockChain>().expect_no_offenses(indoc! {"
            a do
              b
            end
        "});
    }

    #[test]
    fn accepts_multiline_block_non_block_chain() {
        // Chaining non-block methods on multiline block is fine.
        test::<MultilineBlockChain>().expect_no_offenses(indoc! {"
            a do
              b
            end.c.d
        "});
    }

    #[test]
    fn accepts_single_line_receiver_multiline_chained() {
        // Single-line receiver block, multiline chained block — no offense.
        test::<MultilineBlockChain>().expect_no_offenses(indoc! {"
            Thread.list.find_all { |t| t.alive? }.map { |t|
              t.object_id
            }
        "});
    }

    #[test]
    fn flags_numblock_chain() {
        test::<MultilineBlockChain>().expect_offense(indoc! {"
            foo do
              bar
            end.baz { _1 }
            ^^^^^^^  Avoid multi-line chains of blocks.
        "});
    }
}

murphy_plugin_api::submit_cop!(MultilineBlockChain);
