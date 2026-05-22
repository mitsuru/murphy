//! Core node types for the Murphy arena AST. See ADR 0037.

/// Index into [`Ast::nodes`](crate::Ast). 32-bit: an arena holds one file.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

/// Optional [`NodeId`]. Uses the sentinel `u32::MAX` for `None` rather than
/// relying on an enum niche, so the layout is explicit across the ABI
/// (ADR 0037).
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OptNodeId(pub u32);

impl OptNodeId {
    /// The `None` sentinel.
    pub const NONE: OptNodeId = OptNodeId(u32::MAX);

    /// Wrap a present [`NodeId`].
    pub fn some(id: NodeId) -> OptNodeId {
        debug_assert!(
            id.0 != u32::MAX,
            "NodeId u32::MAX collides with the OptNodeId sentinel"
        );
        OptNodeId(id.0)
    }

    /// Resolve to an `Option`.
    pub fn get(self) -> Option<NodeId> {
        if self.0 == u32::MAX {
            None
        } else {
            Some(NodeId(self.0))
        }
    }

    /// `true` iff this is the sentinel.
    pub fn is_none(self) -> bool {
        self.0 == u32::MAX
    }
}

impl From<Option<NodeId>> for OptNodeId {
    fn from(o: Option<NodeId>) -> Self {
        match o {
            Some(id) => OptNodeId::some(id),
            None => OptNodeId::NONE,
        }
    }
}

/// Interned identifier (method name, variable name, …). Index into the
/// [`Interner`](crate::Interner).
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Symbol(pub u32);

/// Interned string-literal contents. Index into the
/// [`Interner`](crate::Interner).
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StringId(pub u32);

/// A half-open byte range into the source buffer.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Range {
    pub start: u32,
    pub end: u32,
}

/// A reference to a contiguous slice of `node_lists` — the side table for
/// variable-length children (call args, array elements, …).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeList {
    pub start: u32,
    pub len: u32,
}

impl NodeList {
    /// The empty list.
    pub const EMPTY: NodeList = NodeList { start: 0, len: 0 };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opt_node_id_round_trips() {
        assert_eq!(OptNodeId::NONE.get(), None);
        assert!(OptNodeId::NONE.is_none());
        let some = OptNodeId::some(NodeId(7));
        assert_eq!(some.get(), Some(NodeId(7)));
        assert!(!some.is_none());
        assert_eq!(OptNodeId::from(Some(NodeId(3))).get(), Some(NodeId(3)));
        assert_eq!(OptNodeId::from(None).get(), None);
        // NodeId(0) is the typical first-pushed arena node — it must not be
        // confused with the `None` sentinel. Also exercise the value just
        // below the sentinel.
        assert_eq!(OptNodeId::some(NodeId(0)).get(), Some(NodeId(0)));
        assert!(!OptNodeId::some(NodeId(0)).is_none());
        assert_eq!(
            OptNodeId::some(NodeId(u32::MAX - 1)).get(),
            Some(NodeId(u32::MAX - 1))
        );
    }

    #[test]
    fn node_list_empty_is_zero_len() {
        assert_eq!(NodeList::EMPTY.len, 0);
    }
}
