//! The `NodeCop` dispatch trait. `NodeKindTag` is re-exported from
//! `murphy-ast` (the canonical home — `NodeKind`'s discriminant is an
//! AST concern) so `node_pattern!`-generated matchers can compare a
//! literal tag against `cx.kind(node).tag()` without a cross-crate type
//! mismatch (murphy-a70).

use murphy_ast::{NodeId, NodeKindTag};

use crate::cop::Cop;
use crate::cx::Cx;

/// The dispatch trait: a cop subscribes to node kinds and is called once
/// per matching node.
///
/// Merges the spike's `NodeCop` and `CallCop` (a call cop is just a
/// `NodeCop` on the `NodeKind::Send` variant); `FileCop` / `run_file`
/// are deleted (ADR 0038).
///
/// ## File-visit (`KINDS = &[]`)
///
/// A cop with **`KINDS = &[]`** is the intentional degenerate form for
/// whole-file scans: the dispatcher invokes `check` exactly once per
/// file with `node == cx.root()`. This keeps the single-surface
/// invariant — every cop is still a `NodeCop` and still receives a
/// `NodeId` — while letting raw-source cops like
/// `Layout/TrailingWhitespace` walk `cx.raw_source(cx.range(root))`
/// without subscribing to every possible root kind (the root of a Ruby
/// file can be any of `Begin`, `Nil`, `Send`, `Def`, `Class`, …, so a
/// fixed kind subscription would not work). The exact dispatch
/// semantics live in `murphy-core::dispatch::run_cops`.
pub trait NodeCop: Cop {
    /// Node kinds this cop is dispatched on. `#[on_node]` (murphy-9cr.8)
    /// generates this; until then it is written by hand. An empty slice
    /// selects the file-visit form documented on the trait.
    const KINDS: &'static [NodeKindTag];

    /// Runtime dispatch kinds. Static cops use [`Self::KINDS`]; dynamic
    /// in-process cops can override this without changing the plugin ABI.
    fn kinds(&self) -> &[NodeKindTag] {
        Self::KINDS
    }

    /// Inspect one matched node. Stateless: everything the callback needs
    /// is `node` and `cx`. For a file-visit cop (`KINDS = &[]`) the
    /// caller passes `cx.root()`; the cop typically derives the file's
    /// range from there.
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

    #[test]
    fn node_cop_can_override_kinds_for_dynamic_dispatch() {
        struct Dynamic {
            kinds: Vec<NodeKindTag>,
        }
        impl crate::Cop for Dynamic {
            type Options = crate::NoOptions;
            const NAME: &'static str = "Plugin/Dynamic";
        }
        impl NodeCop for Dynamic {
            const KINDS: &'static [NodeKindTag] = &[];
            fn kinds(&self) -> &[NodeKindTag] {
                &self.kinds
            }
            fn check(&self, _node: NodeId, _cx: &crate::Cx<'_>) {}
        }

        let cop = Dynamic {
            kinds: vec![NodeKindTag(1), NodeKindTag(2)],
        };

        assert_eq!(cop.kinds(), &[NodeKindTag(1), NodeKindTag(2)]);
    }
}
