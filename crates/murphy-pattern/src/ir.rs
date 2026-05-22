//! `PatternIr` — a flat, pointer-free node array derived from `PatternAst`.
//! Consumed by the C backend interpreter (murphy-9cr.19). Mirrors the
//! `murphy-ast` arena design: nodes in a `Vec`, variable-length children in
//! a side table.

use crate::CaptureKind;
use murphy_ast::NodeKindTag;

/// Index into [`PatternIr::nodes`].
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IrNodeId(pub u32);

/// A reference to a contiguous slice of a side table (`children` or `tags`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IrSlice {
    pub start: u32,
    pub len: u32,
}

/// A reference to a `[start, start+len)` byte range of `str_pool`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StrRef {
    pub start: u32,
    pub len: u32,
}

/// A compiled pattern: a flat node array plus side tables. No pointers.
#[derive(Debug, Clone, PartialEq)]
pub struct PatternIr {
    pub nodes: Vec<IrNode>,
    /// Side table for `IrNode::Node` children and `IrNode::Union` arms.
    pub children: Vec<IrNodeId>,
    /// Side table for `IrHead::OneOf` alternatives.
    pub tags: Vec<NodeKindTag>,
    /// Predicate names, string/symbol literals, capture names.
    pub str_pool: String,
    /// One entry per `$` capture, in slot order.
    pub captures: Vec<CaptureMeta>,
    /// The root node.
    pub root: IrNodeId,
}

/// Per-capture metadata, indexed by slot.
#[derive(Debug, Clone, PartialEq)]
pub struct CaptureMeta {
    pub kind: CaptureKind,
    /// `Some` for `$ident` named captures.
    pub name: Option<StrRef>,
}

/// A flat IR node. `PatKind` resolved and flattened; children by index.
#[derive(Debug, Clone, PartialEq)]
pub enum IrNode {
    Wildcard,
    Rest,
    NilTest,
    LitInt(i64),
    LitFloat(f64),
    LitStr(StrRef),
    LitSym(StrRef),
    LitTrue,
    LitFalse,
    LitNil,
    Predicate(StrRef),
    Kind(NodeKindTag),
    Node { head: IrHead, children: IrSlice },
    Union(IrSlice),
    Not(IrNodeId),
    Capture { slot: u16, body: IrNodeId },
    Parent(IrNodeId),
    Descend(IrNodeId),
}

/// The head of an `IrNode::Node`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrHead {
    Exact(NodeKindTag),
    Any,
    OneOf(IrSlice),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ir_construction_smoke() {
        let ir = PatternIr {
            nodes: vec![IrNode::Wildcard],
            children: vec![],
            tags: vec![],
            str_pool: String::new(),
            captures: vec![],
            root: IrNodeId(0),
        };
        assert_eq!(ir.nodes.len(), 1);
        assert_eq!(ir.root, IrNodeId(0));
    }
}
