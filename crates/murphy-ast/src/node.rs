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

/// A single AST node: a fixed-size POD value. The discriminated payload
/// lives in `kind`; `parent` is filled in by [`AstBuilder::finish`].
#[repr(C)]
// No `Eq`: `NodeKind` carries `Float(f64)`, and `f64` is not `Eq`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AstNode {
    pub kind: NodeKind,
    /// Parent node. `OptNodeId::NONE` for the root.
    pub parent: OptNodeId,
    pub range: Range,
}

/// The kind of an AST node, with its inline payload.
///
/// `#[repr(C, u8)]` gives a stable layout with a `u8` discriminant. The
/// **declaration order is the discriminant** and is **frozen** — new
/// variants append at the end only (ADR 0037). v1 follows the Ruby
/// `parser` gem's node shapes.
#[repr(C, u8)]
// No `Eq`: the `Float(f64)` variant means `f64` participates, and it is
// not `Eq`. `PartialEq` is enough for the round-trip equality test.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NodeKind {
    /// A prism parse error. Dispatch skips it so syntax errors never crash
    /// a cop.
    Error,

    // --- atoms / literals ---
    Nil,
    True_,
    False_,
    SelfExpr,
    Int(i64),
    Float(f64),
    Str(StringId),
    Sym(Symbol),

    // --- variable reads ---
    Lvar(Symbol),
    Ivar(Symbol),
    Cvar(Symbol),
    Gvar(Symbol),
    Const {
        scope: OptNodeId,
        name: Symbol,
    },

    // --- assignments ---
    Lvasgn {
        name: Symbol,
        value: OptNodeId,
    },
    Ivasgn {
        name: Symbol,
        value: OptNodeId,
    },
    Casgn {
        scope: OptNodeId,
        name: Symbol,
        value: OptNodeId,
    },

    // --- calls / blocks ---
    Send {
        receiver: OptNodeId,
        method: Symbol,
        args: NodeList,
    },
    /// Safe-navigation call (`&.`). The receiver is always present.
    Csend {
        receiver: NodeId,
        method: Symbol,
        args: NodeList,
    },
    Block {
        call: NodeId,
        /// The `args` node (always present, may be an empty `Args`).
        args: NodeId,
        body: OptNodeId,
    },
    BlockPass(OptNodeId),
    Splat(OptNodeId),

    // --- collections ---
    Array(NodeList),
    Hash(NodeList),
    Pair {
        key: NodeId,
        value: NodeId,
    },

    // --- control flow ---
    If {
        cond: NodeId,
        then_: OptNodeId,
        else_: OptNodeId,
    },
    Case {
        subject: OptNodeId,
        whens: NodeList,
        else_: OptNodeId,
    },
    When {
        conds: NodeList,
        body: OptNodeId,
    },
    Begin(NodeList),
    Return(OptNodeId),
    And {
        lhs: NodeId,
        rhs: NodeId,
    },
    Or {
        lhs: NodeId,
        rhs: NodeId,
    },

    // --- definitions ---
    Def {
        /// singleton method（`def self.foo`）なら `receiver` が `Some`。
        receiver: OptNodeId,
        name: Symbol,
        args: NodeId,
        body: OptNodeId,
    },
    Class {
        name: NodeId,
        superclass: OptNodeId,
        body: OptNodeId,
    },
    Module {
        name: NodeId,
        body: OptNodeId,
    },

    // --- arguments ---
    Args(NodeList),
    Arg(Symbol),

    // --- fallback ---
    /// A valid prism node with no `NodeKind` mapping yet. Dispatch may
    /// treat it as opaque; `murphy-translate` never panics on unknown
    /// input. Distinct from `Error` (a prism *parse* error).
    Unknown,

    // --- assignments (appended post-`Unknown` per ADR 0037: variants are
    // append-only; declaration order is the frozen discriminant) ---
    /// `$g = expr` — global-variable assignment.
    Gvasgn {
        name: Symbol,
        value: OptNodeId,
    },
    /// `@@c = expr` — class-variable assignment.
    Cvasgn {
        name: Symbol,
        value: OptNodeId,
    },

    // --- arguments (appended post-`Cvasgn` per ADR 0037: variants are
    // append-only; declaration order is the frozen discriminant) ---
    /// `def f(a = 1)` の `a = 1` — optional positional parameter.
    Optarg {
        name: Symbol,
        default: NodeId,
    },
    /// `*rest` — splat parameter. 匿名 `*` は `name` が空文字 interned。
    Restarg(Symbol),
    /// `def f(k:)` — required keyword parameter.
    Kwarg(Symbol),
    /// `def f(k: 1)` — optional keyword parameter.
    Kwoptarg {
        name: Symbol,
        default: NodeId,
    },
    /// `**opts` — keyword splat parameter. 匿名 `**` は `name` が空文字 interned。
    Kwrestarg(Symbol),
    /// `&blk` — block parameter. 匿名 `&` は `name` が空文字 interned。
    Blockarg(Symbol),

    // --- collections (appended post-`Blockarg` per ADR 0037: variants are
    // append-only; declaration order is the frozen discriminant) ---
    /// `**h` — ハッシュ内のキーワード splat（`AssocSplatNode`）。匿名 `**` は
    /// 内側が `None`。
    Kwsplat(OptNodeId),

    // --- control flow (appended post-`Kwsplat` per ADR 0037: variants are
    // append-only; declaration order is the frozen discriminant) ---
    /// `while cond ... end`。`is_begin_modifier` を `post` に畳む
    /// （`while`/`while_post` の collapse）。
    While {
        cond: NodeId,
        body: OptNodeId,
        /// `true` なら do-while（`begin..end while c`）。
        post: bool,
    },
    /// `until cond ... end`。`post` は [`NodeKind::While`] と同じ意味。
    Until {
        cond: NodeId,
        body: OptNodeId,
        post: bool,
    },
    /// `a..b` / `a...b`（`RangeNode`）。beginless/endless は端が `None`。
    /// 型名 `RangeExpr` は既存のソース範囲 struct [`Range`] との衝突回避。
    RangeExpr {
        begin_: OptNodeId,
        end_: OptNodeId,
        /// `true` なら `...`（終端排他）。
        exclusive: bool,
    },

    // --- definitions (appended post-`RangeExpr` per ADR 0037: variants are
    // append-only; declaration order is the frozen discriminant) ---
    /// `class << expr ... end`（`SingletonClassNode`）— singleton class body。
    Sclass {
        expr: NodeId,
        body: OptNodeId,
    },
}

/// A source comment, stored outside the node tree.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Comment {
    pub range: Range,
    pub kind: CommentKind,
}

/// Whether a comment is a `#` line comment or a `=begin`/`=end` block.
// A fieldless enum: `#[repr(u8)]` alone (not `#[repr(C, u8)]`, which the
// compiler rejects as a conflicting hint for a C-like enum) pins a stable
// `u8` discriminant.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommentKind {
    Inline,
    Block,
}

/// The owned source text and path for one file. All [`Range`] values index
/// into `text` as byte offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceBuffer {
    pub text: String,
    pub path: std::path::PathBuf,
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

    #[test]
    fn layout_invariants() {
        use std::mem::{align_of, size_of};

        // 4-byte handles.
        assert_eq!(size_of::<NodeId>(), 4);
        assert_eq!(size_of::<OptNodeId>(), 4);
        assert_eq!(size_of::<Symbol>(), 4);
        assert_eq!(size_of::<StringId>(), 4);
        // 8-byte side-table refs.
        assert_eq!(size_of::<Range>(), 8);
        assert_eq!(size_of::<NodeList>(), 8);

        // AstNode is a fixed-size POD node, small enough for a flat arena.
        assert!(size_of::<AstNode>() <= 48, "AstNode unexpectedly large");
        assert_eq!(align_of::<AstNode>(), 8, "i64 payload forces 8-byte align");

        // NodeKind carries the largest payload but stays compact.
        assert!(size_of::<NodeKind>() <= 32);
    }

    #[test]
    fn node_kind_is_copy() {
        // A POD enum: cheap to copy, no heap, no pointers.
        let k = NodeKind::Int(42);
        let copy = k;
        assert_eq!(k, copy);
    }
}
