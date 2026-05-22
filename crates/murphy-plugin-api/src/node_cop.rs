//! `NodeKindTag` and the `NodeCop` dispatch trait.

use murphy_ast::{NodeId, NodeKind};

use crate::cop::Cop;
use crate::cx::Cx;

/// The `u8` discriminant of a [`NodeKind`] variant — its payload-free
/// projection, used to declare a [`NodeCop`]'s dispatch targets.
///
/// The discriminant is `NodeKind`'s `#[repr(C, u8)]` declaration order,
/// frozen by ADR 0037. (murphy-ast grows an identical `NodeKindTag` in
/// murphy-9cr.17; this crate keeps its own copy to stay mergeable in
/// parallel — see the plan's Task 8 note.)
///
/// No validation: an out-of-range tag (e.g. `NodeKindTag(200)`) is
/// permitted and simply never matches during dispatch.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeKindTag(pub u8);

impl NodeKindTag {
    /// The tag of a node kind.
    pub fn of(kind: &NodeKind) -> NodeKindTag {
        // Safety: the pointer has valid provenance from the `&NodeKind`
        // reference; `u8` has alignment 1 so the read cannot be misaligned.
        // `NodeKind` is `#[repr(C, u8)]` (ADR 0037 — frozen layout), so its
        // first byte is the discriminant.
        NodeKindTag(unsafe { *(kind as *const NodeKind as *const u8) })
    }
}

/// The dispatch trait: a cop subscribes to node kinds and is called once
/// per matching node.
///
/// Merges the spike's `NodeCop` and `CallCop` (a call cop is just a
/// `NodeCop` on the `NodeKind::Send` variant); `FileCop` / `run_file`
/// are deleted (ADR 0038).
pub trait NodeCop: Cop {
    /// Node kinds this cop is dispatched on. `#[on_node]` (murphy-9cr.8)
    /// generates this; until then it is written by hand.
    const KINDS: &'static [NodeKindTag];

    /// Inspect one matched node. Stateless: everything the callback needs
    /// is `node` and `cx`.
    fn check(&self, node: NodeId, cx: &Cx<'_>);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_kind_tag_reads_the_discriminant() {
        use murphy_ast::{NodeKind, Symbol};
        // `#[repr(C, u8)]` discriminants are NodeKind declaration order.
        assert_eq!(NodeKindTag::of(&NodeKind::Error).0, 0);
        assert_eq!(NodeKindTag::of(&NodeKind::Nil).0, 1);
        assert_eq!(NodeKindTag::of(&NodeKind::Lvar(Symbol(0))).0, 9);
    }

    #[test]
    fn node_cop_declares_kinds_and_a_check_fn() {
        use murphy_ast::NodeId;
        struct Stub;
        impl crate::Cop for Stub {
            type Options = crate::NoOptions;
            const NAME: &'static str = "Plugin/Stub";
        }
        impl NodeCop for Stub {
            // NodeKindTag(1) == NodeKind::Nil.
            const KINDS: &'static [NodeKindTag] = &[NodeKindTag(1)];
            fn check(&self, _node: NodeId, _cx: &crate::Cx<'_>) {}
        }
        assert_eq!(<Stub as NodeCop>::KINDS, &[NodeKindTag(1)]);
    }
}
