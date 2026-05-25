//! Quantifier IR-lowering snapshots (murphy-ycx, PR #2).
//!
//! Companion to `tests/quantifier_snapshots.rs` (PR #1, parse-shape) — this
//! target pins the `PatternIr` shape that `lower(parse(src))` produces for
//! quantifier-bearing patterns and capture-slot kinds. Matching against the
//! Ruby AST is PR #3, so these tests stop at `IrNode::Quantifier` reachable
//! via the IR — they do not exercise the runtime matcher.
//!
//! Structural assertions are favored over `{:#?}` Debug snapshots: the IR
//! contains side-table indices whose absolute values are sensitive to
//! traversal order, and pinning them as literals would be brittle without
//! adding signal beyond what `matches!` already gives us.
//!
//! All AC items pinned here are PR #2-scope; PR #3..5 items (matcher,
//! mruby bridge, B backend, perf gate) are explicitly NOT verified here.

use murphy_pattern::{CaptureKind, IrHead, IrNode, IrNodeId, compile};

/// Resolve a child `IrNodeId` chain to the `IrNode` it points at.
fn ir_node_at(ir: &murphy_pattern::PatternIr, id: IrNodeId) -> &IrNode {
    &ir.nodes[id.0 as usize]
}

/// Return the children `IrNodeId` slice of the root `(...)` Node, panicking
/// on the way in if the root is not an `IrNode::Node`.
fn root_children(ir: &murphy_pattern::PatternIr) -> Vec<IrNodeId> {
    match &ir.nodes[ir.root.0 as usize] {
        IrNode::Node { children, .. } => {
            let start = children.start as usize;
            let len = children.len as usize;
            ir.children[start..start + len].to_vec()
        }
        other => panic!("root should be Node, was {other:?}"),
    }
}

// =====================================================================
// (A) IrNode::Quantifier lowering
// =====================================================================

#[test]
fn lowers_plus_quantifier_to_irnode_quantifier_one_plus() {
    // `(array int+)` — exactly one child, an `IrNode::Quantifier` with
    // `min=1, max=u8::MAX`. Its body is an `IrNode::Kind` for `int`.
    let ir = compile("(array int+)").expect("compile ok");
    let ids = root_children(&ir);
    assert_eq!(ids.len(), 1, "(array int+) should have one child");
    match ir_node_at(&ir, ids[0]) {
        IrNode::Quantifier { body, min, max } => {
            assert_eq!(*min, 1, "+ has min=1");
            assert_eq!(*max, u8::MAX, "+ has max=u8::MAX as 'unbounded'");
            assert!(matches!(ir_node_at(&ir, *body), IrNode::Kind(_)));
        }
        other => panic!("expected Quantifier, got {other:?}"),
    }
}

#[test]
fn lowers_star_quantifier_to_irnode_quantifier_zero_plus() {
    // `(array int*)` — `min=0, max=u8::MAX`.
    let ir = compile("(array int*)").expect("compile ok");
    let ids = root_children(&ir);
    match ir_node_at(&ir, ids[0]) {
        IrNode::Quantifier { min, max, .. } => {
            assert_eq!(*min, 0);
            assert_eq!(*max, u8::MAX);
        }
        other => panic!("expected Quantifier(*), got {other:?}"),
    }
}

#[test]
fn lowers_question_quantifier_to_irnode_quantifier_zero_or_one() {
    // `(send _ :update_columns hash?)` — `min=0, max=1`, body is `Kind(hash)`.
    let ir = compile("(send _ :update_columns hash?)").expect("compile ok");
    let ids = root_children(&ir);
    assert_eq!(ids.len(), 3, "send children: recv, method, hash?");
    match ir_node_at(&ir, ids[2]) {
        IrNode::Quantifier { body, min, max } => {
            assert_eq!(*min, 0);
            assert_eq!(*max, 1);
            assert!(matches!(ir_node_at(&ir, *body), IrNode::Kind(_)));
        }
        other => panic!("expected Quantifier(?), got {other:?}"),
    }
}

#[test]
fn lowers_quantifier_alongside_rest_and_fixed() {
    // `(send _ :foo ... int+)` — mixing rule: rest + quantifier coexist in
    // the same child list and survive lowering.
    let ir = compile("(send _ :foo ... int+)").expect("compile ok");
    let ids = root_children(&ir);
    assert!(matches!(ir_node_at(&ir, ids[2]), IrNode::Rest));
    assert!(matches!(ir_node_at(&ir, ids[3]), IrNode::Quantifier { .. }));
}

#[test]
fn lowers_pluck_sym_plus() {
    // `(send _ :pluck sym+)` — DESIGN's headline example: `min=1` to require
    // at least one symbol argument.
    let ir = compile("(send _ :pluck sym+)").expect("compile ok");
    let ids = root_children(&ir);
    match ir_node_at(&ir, ids[2]) {
        IrNode::Quantifier { min, .. } => assert_eq!(*min, 1),
        other => panic!("expected Quantifier(+), got {other:?}"),
    }
}

// =====================================================================
// (B) Capture-slot meta upgrades
// =====================================================================

#[test]
fn capture_slot_for_dollar_plus_lowers_to_seq() {
    // `(array $int+)` — slot 0 should be `Seq` after lowering, mirroring the
    // PR #1 parse-time AST capture_kinds upgrade.
    let ir = compile("(array $int+)").expect("compile ok");
    assert_eq!(ir.captures.len(), 1);
    assert_eq!(ir.captures[0].kind, CaptureKind::Seq);
    assert!(ir.captures[0].name.is_none(), "anonymous capture");
}

#[test]
fn capture_slot_for_dollar_star_lowers_to_seq() {
    let ir = compile("(array $int*)").expect("compile ok");
    assert_eq!(ir.captures[0].kind, CaptureKind::Seq);
}

#[test]
fn capture_slot_for_dollar_question_lowers_to_optnode() {
    // `(send _ :update_columns $hash?)` — slot 0 should be `OptNode`.
    let ir = compile("(send _ :update_columns $hash?)").expect("compile ok");
    assert_eq!(ir.captures.len(), 1);
    assert_eq!(ir.captures[0].kind, CaptureKind::OptNode);
}

#[test]
fn capture_slot_for_dollar_ellipsis_still_lowers_to_seq() {
    // Regression: the existing `$...` -> Seq path is unaffected.
    let ir = compile("(send nil :puts $...)").expect("compile ok");
    assert_eq!(ir.captures[0].kind, CaptureKind::Seq);
}

#[test]
fn capture_slot_for_named_dollar_ident_still_lowers_to_node() {
    // Regression: `$ident` (no postfix) is still a named Node-kind capture.
    let ir = compile("(send $receiver _)").expect("compile ok");
    assert_eq!(ir.captures[0].kind, CaptureKind::Node);
    assert!(ir.captures[0].name.is_some(), "named capture");
}

// =====================================================================
// (C) Quantifier body is itself lowered (post-order)
// =====================================================================

#[test]
fn quantifier_body_is_pushed_before_parent_quantifier_node() {
    // Post-order invariant: the IR node id of a Quantifier's `body` must be
    // less than the Quantifier's own id (its children were pushed first).
    let ir = compile("(array int+)").expect("compile ok");
    let ids = root_children(&ir);
    let q_id = ids[0];
    match ir_node_at(&ir, q_id) {
        IrNode::Quantifier { body, .. } => {
            assert!(
                body.0 < q_id.0,
                "Quantifier body id={} should be < Quantifier id={} (post-order)",
                body.0,
                q_id.0,
            );
        }
        other => panic!("expected Quantifier, got {other:?}"),
    }
}

#[test]
fn nested_quantifier_lowers_kind_head_correctly() {
    // `(array int+)` — the array head must remain `IrHead::Exact(array)`.
    let ir = compile("(array int+)").expect("compile ok");
    match &ir.nodes[ir.root.0 as usize] {
        IrNode::Node { head, .. } => assert!(matches!(head, IrHead::Exact(_))),
        other => panic!("expected Node, got {other:?}"),
    }
}
