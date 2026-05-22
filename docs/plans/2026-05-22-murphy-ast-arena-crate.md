# murphy-ast: arena AST crate — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Create the `murphy-ast` crate — an owned, flat, parser-shaped, typed arena AST that becomes Murphy's core AST representation (ADR 0037).

**Architecture:** A single `Ast` value owns one file: a flat `Vec<AstNode>` (fixed-size POD nodes), a `node_lists` side table for variable-length children, a string interner, comments, and the source buffer. Nodes hold no pointers; children are referenced by `NodeList` index ranges. `AstBuilder` constructs an `Ast` and computes parent links in one pass at `finish()`. The crate is prism-independent — `murphy-translate` (murphy-9cr.15) will be the prism→arena bridge later.

**Tech Stack:** Rust 2024 edition, std only (zero external dependencies). Workspace member auto-discovered via `members = ["crates/*"]`.

**Source of truth:** beads issue `murphy-9cr.14` design field, and ADR 0037 (`docs/decisions/0037-arena-parser-shaped-typed-ast.md`).

**Conventions:** TDD is mandatory (CLAUDE.md) — write the failing test, see it fail, implement, see it pass, commit. One commit per task. Commit message prefix `feat(murphy-ast):`.

---

## Task 1: Crate scaffold

**Files:**
- Create: `crates/murphy-ast/Cargo.toml`
- Create: `crates/murphy-ast/src/lib.rs`

**Step 1: Create `crates/murphy-ast/Cargo.toml`**

```toml
[package]
name = "murphy-ast"
version = "0.1.0"
edition = "2024"
description = "Owned, flat, parser-shaped, typed arena AST — Murphy's core AST representation (ADR 0037)."

[dependencies]
```

**Step 2: Create `crates/murphy-ast/src/lib.rs`**

```rust
//! Murphy's core AST representation: an owned, flat, parser-shaped, typed
//! arena. See ADR 0037 (`docs/decisions/0037-arena-parser-shaped-typed-ast.md`).
//!
//! One [`Ast`] owns one file. Nodes are fixed-size POD values in a flat
//! `Vec`; variable-length children live in a side table referenced by
//! [`NodeList`]. The crate is prism-independent — `murphy-translate` is the
//! prism→arena bridge.

mod ast;
mod builder;
mod interner;
mod node;
mod serialize;

pub use ast::{Ancestors, Ast};
pub use builder::AstBuilder;
pub use interner::Interner;
pub use node::{
    AstNode, Comment, CommentKind, NodeId, NodeKind, NodeList, OptNodeId, Range, SourceBuffer,
    StringId, Symbol,
};
```

> **Settled — `Symbol` / `StringId` live in `node.rs`** (next to `NodeKind`,
> which references them inline). `interner.rs` never redefines them; it
> works with the raw `u32` index. `lib.rs` re-exports both from `node`.
> Do not move them.

> Note: `lib.rs` references modules that do not exist yet. Create empty
> placeholder files so the crate compiles after each task. For this task,
> create each module file with a single `//!` doc line and nothing else,
> and temporarily comment out the `pub use` lines. Re-enable each `pub use`
> in the task that defines those items.

**Step 3: Create placeholder module files**

Create these five files, each containing only a module doc comment:
- `crates/murphy-ast/src/node.rs` — `//! Core node types.`
- `crates/murphy-ast/src/interner.rs` — `//! String interner.`
- `crates/murphy-ast/src/ast.rs` — `//! Arena and traversal.`
- `crates/murphy-ast/src/builder.rs` — `//! Arena builder.`
- `crates/murphy-ast/src/serialize.rs` — `//! Flat serialization.`

In `lib.rs`, comment out the three `pub use` lines for now (they will be
uncommented as items are defined). Keep the `mod` lines.

**Step 4: Verify the crate builds**

Run: `cargo build -p murphy-ast`
Expected: PASS (empty crate compiles).

**Step 5: Commit**

```bash
git add crates/murphy-ast/
git commit -m "feat(murphy-ast): scaffold empty crate"
```

---

## Task 2: Primitive types

**Files:**
- Modify: `crates/murphy-ast/src/node.rs`

**Step 1: Write the failing test**

Add to the bottom of `node.rs`:

```rust
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
    }

    #[test]
    fn node_list_empty_is_zero_len() {
        assert_eq!(NodeList::EMPTY.len, 0);
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p murphy-ast`
Expected: FAIL — `NodeId`, `OptNodeId`, `NodeList` not defined.

**Step 3: Write the implementation**

At the top of `node.rs` (above the `tests` module):

```rust
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
        debug_assert!(id.0 != u32::MAX, "NodeId u32::MAX collides with the OptNodeId sentinel");
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
```

> `Symbol` and `StringId` are defined here in `node.rs` and stay here
> (settled in Task 1). `interner.rs` does not redefine them.

In `lib.rs`, uncomment the `pub use node::{...}` line and ensure it lists
`NodeId, OptNodeId, Symbol, StringId, Range, NodeList`.

**Step 4: Run test to verify it passes**

Run: `cargo test -p murphy-ast`
Expected: PASS (2 tests).

**Step 5: Commit**

```bash
git add crates/murphy-ast/
git commit -m "feat(murphy-ast): add NodeId, OptNodeId, Symbol, StringId, Range, NodeList"
```

---

## Task 3: NodeKind, AstNode, Comment, SourceBuffer

**Files:**
- Modify: `crates/murphy-ast/src/node.rs`

**Step 1: Write the failing test**

Add to the `tests` module in `node.rs`:

```rust
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
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p murphy-ast`
Expected: FAIL — `AstNode`, `NodeKind` not defined.

**Step 3: Write the implementation**

Add to `node.rs` (above the `tests` module):

```rust
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
}

/// A source comment, stored outside the node tree.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Comment {
    pub range: Range,
    pub kind: CommentKind,
}

/// Whether a comment is a `#` line comment or a `=begin`/`=end` block.
// `#[repr(u8)]`, not `#[repr(C, u8)]`: the compiler rejects the combined
// hint (E0566) on a fieldless C-like enum. `#[repr(u8)]` alone pins the
// stable `u8` discriminant.
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
```

In `lib.rs`, ensure the `pub use node::{...}` line lists
`AstNode, Comment, CommentKind, NodeKind, SourceBuffer` in addition to the
Task 2 types.

**Step 4: Run test to verify it passes**

Run: `cargo test -p murphy-ast`
Expected: PASS (4 tests). If `size_of::<AstNode>()` exceeds 48, stop and
report the measured size — do not loosen the assertion silently.

**Step 5: Commit**

```bash
git add crates/murphy-ast/
git commit -m "feat(murphy-ast): add NodeKind (37 variants), AstNode, Comment, SourceBuffer"
```

---

## Task 4: Interner

**Files:**
- Modify: `crates/murphy-ast/src/interner.rs`

**Step 1: Write the failing test**

Add to `interner.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intern_deduplicates() {
        let mut b = InternBuilder::default();
        let a1 = b.intern("call");
        let a2 = b.intern("call");
        let other = b.intern("new");
        assert_eq!(a1, a2, "same string interns to the same index");
        assert_ne!(a1, other);
        let interner = b.finish();
        assert_eq!(interner.len(), 2);
        assert_eq!(interner.resolve(a1), "call");
        assert_eq!(interner.resolve(other), "new");
    }

    #[test]
    fn empty_interner() {
        let interner = InternBuilder::default().finish();
        assert!(interner.is_empty());
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p murphy-ast`
Expected: FAIL — `Interner`, `InternBuilder` not defined.

**Step 3: Write the implementation**

At the top of `interner.rs`:

```rust
//! Flat string interner shared by [`Symbol`] (identifiers) and
//! [`StringId`] (string-literal contents). See design §4.

use std::collections::HashMap;

use crate::node::Range;

/// A finished, serializable interner: a flat byte blob plus per-entry
/// offsets. Index it with the `u32` inside a [`Symbol`] or [`StringId`].
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Interner {
    pub(crate) blob: Vec<u8>,
    pub(crate) offsets: Vec<Range>,
}

impl Interner {
    /// Resolve an entry index to its string.
    pub fn resolve(&self, index: u32) -> &str {
        let r = self.offsets[index as usize];
        // Only valid UTF-8 is ever interned (see `InternBuilder::intern`);
        // `from_bytes` validates on the deserialization path.
        std::str::from_utf8(&self.blob[r.start as usize..r.end as usize])
            .expect("interner blob holds valid UTF-8")
    }

    /// Number of interned entries.
    pub fn len(&self) -> usize {
        self.offsets.len()
    }

    /// `true` iff nothing has been interned.
    pub fn is_empty(&self) -> bool {
        self.offsets.is_empty()
    }
}

/// Build-time interner with deduplication. The `dedup` map is dropped by
/// [`InternBuilder::finish`]; only the flat [`Interner`] survives.
#[derive(Debug, Default)]
pub struct InternBuilder {
    interner: Interner,
    dedup: HashMap<String, u32>,
}

impl InternBuilder {
    /// Intern a string, returning its entry index. Repeated strings return
    /// the same index.
    pub fn intern(&mut self, s: &str) -> u32 {
        if let Some(&idx) = self.dedup.get(s) {
            return idx;
        }
        let start = self.interner.blob.len() as u32;
        self.interner.blob.extend_from_slice(s.as_bytes());
        let end = self.interner.blob.len() as u32;
        let idx = self.interner.offsets.len() as u32;
        self.interner.offsets.push(Range { start, end });
        self.dedup.insert(s.to_owned(), idx);
        idx
    }

    /// Consume the builder, returning the flat interner.
    pub fn finish(self) -> Interner {
        self.interner
    }
}
```

> `Symbol` / `StringId` are defined in `node.rs` (Task 2). This module
> does not define them and does not need them — `intern` works with the
> raw `u32` index.

In `lib.rs`, uncomment `pub use interner::Interner;`.

**Step 4: Run test to verify it passes**

Run: `cargo test -p murphy-ast`
Expected: PASS (6 tests).

**Step 5: Commit**

```bash
git add crates/murphy-ast/
git commit -m "feat(murphy-ast): add Interner and InternBuilder with dedup"
```

---

## Task 5: `collect_children` — the single source of truth

**Files:**
- Modify: `crates/murphy-ast/src/ast.rs`

`collect_children` enumerates a node's child `NodeId`s in source order. It
drives **both** parent computation (`AstBuilder::finish`) and the
`children` traversal iterator. An exhaustive `match` means every future
`NodeKind` variant must be handled here.

**Step 1: Write the failing test**

Add to `ast.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{NodeId, NodeKind, NodeList, OptNodeId, Symbol};

    #[test]
    fn collect_children_handles_opt_list_and_direct() {
        // Send { receiver: Some(1), args: [2, 3] } → [1, 2, 3]
        let lists = vec![NodeId(2), NodeId(3)];
        let kind = NodeKind::Send {
            receiver: OptNodeId::some(NodeId(1)),
            method: Symbol(0),
            args: NodeList { start: 0, len: 2 },
        };
        let mut out = Vec::new();
        collect_children(&kind, &lists, &mut out);
        assert_eq!(out, vec![NodeId(1), NodeId(2), NodeId(3)]);
    }

    #[test]
    fn collect_children_skips_none() {
        // Send { receiver: None, args: [] } → []
        let kind = NodeKind::Send {
            receiver: OptNodeId::NONE,
            method: Symbol(0),
            args: NodeList::EMPTY,
        };
        let mut out = Vec::new();
        collect_children(&kind, &[], &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn collect_children_leaf_has_no_children() {
        let mut out = Vec::new();
        collect_children(&NodeKind::Int(5), &[], &mut out);
        assert!(out.is_empty());
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p murphy-ast`
Expected: FAIL — `collect_children` not defined.

**Step 3: Write the implementation**

At the top of `ast.rs`:

```rust
//! The [`Ast`] arena and its traversal API.

use crate::interner::Interner;
use crate::node::{AstNode, Comment, NodeId, NodeKind, NodeList, OptNodeId, Range, SourceBuffer};

#[inline]
fn push_opt(out: &mut Vec<NodeId>, o: OptNodeId) {
    if let Some(id) = o.get() {
        out.push(id);
    }
}

#[inline]
fn push_list(out: &mut Vec<NodeId>, lists: &[NodeId], l: NodeList) {
    let start = l.start as usize;
    out.extend_from_slice(&lists[start..start + l.len as usize]);
}

/// Append every child `NodeId` of `kind`, in source order, to `out`.
///
/// Single source of truth for parent computation
/// ([`AstBuilder::finish`](crate::AstBuilder::finish)) and the
/// [`Ast::children`] iterator. The `match` is exhaustive on purpose: a new
/// `NodeKind` variant will not compile until it is handled here.
pub(crate) fn collect_children(kind: &NodeKind, lists: &[NodeId], out: &mut Vec<NodeId>) {
    match *kind {
        NodeKind::Error
        | NodeKind::Nil
        | NodeKind::True_
        | NodeKind::False_
        | NodeKind::SelfExpr
        | NodeKind::Int(_)
        | NodeKind::Float(_)
        | NodeKind::Str(_)
        | NodeKind::Sym(_)
        | NodeKind::Lvar(_)
        | NodeKind::Ivar(_)
        | NodeKind::Cvar(_)
        | NodeKind::Gvar(_)
        | NodeKind::Arg(_) => {}

        NodeKind::Const { scope, .. } => push_opt(out, scope),

        NodeKind::Lvasgn { value, .. } | NodeKind::Ivasgn { value, .. } => push_opt(out, value),

        NodeKind::Casgn { scope, value, .. } => {
            push_opt(out, scope);
            push_opt(out, value);
        }

        NodeKind::Send { receiver, args, .. } => {
            push_opt(out, receiver);
            push_list(out, lists, args);
        }

        NodeKind::Csend { receiver, args, .. } => {
            out.push(receiver);
            push_list(out, lists, args);
        }

        NodeKind::Block { call, args, body } => {
            out.push(call);
            out.push(args);
            push_opt(out, body);
        }

        NodeKind::BlockPass(o) | NodeKind::Splat(o) | NodeKind::Return(o) => push_opt(out, o),

        NodeKind::Array(l) | NodeKind::Hash(l) | NodeKind::Begin(l) | NodeKind::Args(l) => {
            push_list(out, lists, l)
        }

        NodeKind::Pair { key, value } => {
            out.push(key);
            out.push(value);
        }

        NodeKind::If { cond, then_, else_ } => {
            out.push(cond);
            push_opt(out, then_);
            push_opt(out, else_);
        }

        NodeKind::Case {
            subject,
            whens,
            else_,
        } => {
            push_opt(out, subject);
            push_list(out, lists, whens);
            push_opt(out, else_);
        }

        NodeKind::When { conds, body } => {
            push_list(out, lists, conds);
            push_opt(out, body);
        }

        NodeKind::And { lhs, rhs } | NodeKind::Or { lhs, rhs } => {
            out.push(lhs);
            out.push(rhs);
        }

        NodeKind::Def { args, body, .. } => {
            out.push(args);
            push_opt(out, body);
        }

        NodeKind::Class {
            name,
            superclass,
            body,
        } => {
            out.push(name);
            push_opt(out, superclass);
            push_opt(out, body);
        }

        NodeKind::Module { name, body } => {
            out.push(name);
            push_opt(out, body);
        }
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p murphy-ast`
Expected: PASS (9 tests).

**Step 5: Commit**

```bash
git add crates/murphy-ast/
git commit -m "feat(murphy-ast): add collect_children, the child-enumeration source of truth"
```

---

## Task 6: `Ast` struct and `AstBuilder`

**Files:**
- Modify: `crates/murphy-ast/src/ast.rs` (add the `Ast` struct)
- Modify: `crates/murphy-ast/src/builder.rs`

**Step 1: Write the failing test**

Add to `builder.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{NodeKind, OptNodeId, Range};

    fn r() -> Range {
        Range { start: 0, end: 1 }
    }

    #[test]
    fn finish_computes_parents_from_structure() {
        // Tree:  Begin [ lvasgn x = int(1) ]
        let mut b = AstBuilder::new("x = 1", "test.rb");
        let int = b.push(NodeKind::Int(1), r());
        let x = b.intern_symbol("x");
        let asgn = b.push(
            NodeKind::Lvasgn {
                name: x,
                value: OptNodeId::some(int),
            },
            r(),
        );
        let list = b.push_list(&[asgn]);
        let root = b.push(NodeKind::Begin(list), r());
        let ast = b.finish(root);

        assert_eq!(ast.parent(root), OptNodeId::NONE, "root has no parent");
        assert_eq!(ast.parent(asgn).get(), Some(root));
        assert_eq!(ast.parent(int).get(), Some(asgn));
        assert_eq!(ast.root(), root);
    }

    #[test]
    fn builder_interns_and_stores_source() {
        let mut b = AstBuilder::new("source", "f.rb");
        let s1 = b.intern_symbol("dup");
        let s2 = b.intern_symbol("dup");
        assert_eq!(s1, s2);
        let root = b.push(NodeKind::Nil, r());
        let ast = b.finish(root);
        assert_eq!(ast.source(), "source");
        assert_eq!(ast.path().to_str(), Some("f.rb"));
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p murphy-ast`
Expected: FAIL — `AstBuilder`, `Ast` not defined.

**Step 3a: Add the `Ast` struct to `ast.rs`**

Add to `ast.rs` (after `collect_children`):

```rust
/// An owned, flat, parser-shaped, typed AST for one file. See ADR 0037.
#[derive(Debug, Clone, PartialEq)]
pub struct Ast {
    pub(crate) nodes: Vec<AstNode>,
    pub(crate) node_lists: Vec<NodeId>,
    pub(crate) interner: Interner,
    pub(crate) comments: Vec<Comment>,
    pub(crate) source: SourceBuffer,
    pub(crate) root: NodeId,
}
```

**Step 3b: Write `AstBuilder` in `builder.rs`**

```rust
//! [`AstBuilder`] — the construction API consumed by `murphy-translate`.

use std::path::PathBuf;

use crate::ast::{collect_children, Ast};
use crate::interner::InternBuilder;
use crate::node::{
    AstNode, Comment, CommentKind, NodeId, NodeKind, NodeList, OptNodeId, Range, SourceBuffer,
    Symbol, StringId,
};

/// Builds an [`Ast`]. Push nodes and lists; `finish` computes parent links
/// from the node structure in one pass.
pub struct AstBuilder {
    nodes: Vec<AstNode>,
    node_lists: Vec<NodeId>,
    interner: InternBuilder,
    comments: Vec<Comment>,
    source: SourceBuffer,
}

impl AstBuilder {
    /// Start building an AST for one file.
    pub fn new(source_text: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        AstBuilder {
            nodes: Vec::new(),
            node_lists: Vec::new(),
            interner: InternBuilder::default(),
            comments: Vec::new(),
            source: SourceBuffer {
                text: source_text.into(),
                path: path.into(),
            },
        }
    }

    /// Intern an identifier.
    pub fn intern_symbol(&mut self, s: &str) -> Symbol {
        Symbol(self.interner.intern(s))
    }

    /// Intern string-literal contents.
    pub fn intern_string(&mut self, s: &str) -> StringId {
        StringId(self.interner.intern(s))
    }

    /// Append a node. `parent` is left as `NONE` until [`AstBuilder::finish`].
    pub fn push(&mut self, kind: NodeKind, range: Range) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        debug_assert!(id.0 != u32::MAX, "arena exceeded u32 node capacity");
        self.nodes.push(AstNode {
            kind,
            parent: OptNodeId::NONE,
            range,
        });
        id
    }

    /// Append a child list, returning a [`NodeList`] handle.
    pub fn push_list(&mut self, ids: &[NodeId]) -> NodeList {
        let start = self.node_lists.len() as u32;
        self.node_lists.extend_from_slice(ids);
        NodeList {
            start,
            len: ids.len() as u32,
        }
    }

    /// Record a source comment.
    pub fn add_comment(&mut self, range: Range, kind: CommentKind) {
        self.comments.push(Comment { range, kind });
    }

    /// Finish building. Computes every node's `parent` from the structure
    /// in one pass, then returns the immutable [`Ast`]. `root` keeps
    /// `parent == NONE`.
    pub fn finish(mut self, root: NodeId) -> Ast {
        let mut buf: Vec<NodeId> = Vec::new();
        for i in 0..self.nodes.len() {
            buf.clear();
            collect_children(&self.nodes[i].kind, &self.node_lists, &mut buf);
            let parent = OptNodeId::some(NodeId(i as u32));
            for &child in &buf {
                self.nodes[child.0 as usize].parent = parent;
            }
        }
        Ast {
            nodes: self.nodes,
            node_lists: self.node_lists,
            interner: self.interner.finish(),
            comments: self.comments,
            source: self.source,
            root,
        }
    }
}
```

In `lib.rs`, uncomment `pub use builder::AstBuilder;` and `pub use ast::{Ancestors, Ast};` (the `Ancestors` type is added in Task 7 — if the crate fails to compile, temporarily re-export only `Ast` and add `Ancestors` in Task 7).

**Step 4: Run test to verify it passes**

Run: `cargo test -p murphy-ast`
Expected: PASS (11 tests). The `finish` test needs `Ast::parent`,
`Ast::root`, `Ast::source`, `Ast::path` — if those are not yet defined,
add the minimal accessors now (full traversal API is Task 7):

```rust
// Minimal accessors on `Ast` in ast.rs — Task 7 adds the rest.
impl Ast {
    pub fn root(&self) -> NodeId { self.root }
    pub fn parent(&self, id: NodeId) -> OptNodeId { self.nodes[id.0 as usize].parent }
    pub fn source(&self) -> &str { &self.source.text }
    pub fn path(&self) -> &std::path::Path { &self.source.path }
}
```

**Step 5: Commit**

```bash
git add crates/murphy-ast/
git commit -m "feat(murphy-ast): add Ast struct and AstBuilder with parent computation"
```

---

## Task 7: Traversal API

**Files:**
- Modify: `crates/murphy-ast/src/ast.rs`

**Step 1: Write the failing test**

Add to the `tests` module in `ast.rs`:

```rust
#[test]
fn traversal_children_ancestors_descendants() {
    use crate::builder::AstBuilder;

    // Begin [ if(cond=int, then=int) ]
    let mut b = AstBuilder::new("src", "t.rb");
    let r = Range { start: 0, end: 1 };
    let cond = b.push(NodeKind::Int(1), r);
    let then_ = b.push(NodeKind::Int(2), r);
    let iff = b.push(
        NodeKind::If {
            cond,
            then_: OptNodeId::some(then_),
            else_: OptNodeId::NONE,
        },
        r,
    );
    let list = b.push_list(&[iff]);
    let root = b.push(NodeKind::Begin(list), r);
    let ast = b.finish(root);

    // children
    assert_eq!(ast.children(root).collect::<Vec<_>>(), vec![iff]);
    assert_eq!(ast.children(iff).collect::<Vec<_>>(), vec![cond, then_]);

    // ancestors (nearest first)
    assert_eq!(
        ast.ancestors(cond).collect::<Vec<_>>(),
        vec![iff, root]
    );
    assert_eq!(ast.ancestors(root).collect::<Vec<_>>(), Vec::<NodeId>::new());

    // descendants (DFS pre-order, excludes self)
    assert_eq!(
        ast.descendants(root).collect::<Vec<_>>(),
        vec![iff, cond, then_]
    );
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p murphy-ast`
Expected: FAIL — `children`, `ancestors`, `descendants` not defined.

**Step 3: Write the implementation**

Add to `ast.rs` — extend the `impl Ast` block (replacing the minimal one
from Task 6) with the full API:

```rust
impl Ast {
    /// The root node.
    pub fn root(&self) -> NodeId {
        self.root
    }

    /// Number of nodes.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// `true` iff the arena has no nodes.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The node at `id`.
    pub fn node(&self, id: NodeId) -> &AstNode {
        &self.nodes[id.0 as usize]
    }

    /// The kind of the node at `id`.
    pub fn kind(&self, id: NodeId) -> &NodeKind {
        &self.nodes[id.0 as usize].kind
    }

    /// The source range of the node at `id`.
    pub fn range(&self, id: NodeId) -> Range {
        self.nodes[id.0 as usize].range
    }

    /// The parent of `id`. `OptNodeId::NONE` for the root.
    pub fn parent(&self, id: NodeId) -> OptNodeId {
        self.nodes[id.0 as usize].parent
    }

    /// The direct children of `id`, in source order.
    pub fn children(&self, id: NodeId) -> std::vec::IntoIter<NodeId> {
        let mut out = Vec::new();
        collect_children(self.kind(id), &self.node_lists, &mut out);
        out.into_iter()
    }

    /// The ancestors of `id`, nearest first, up to (and including) the root.
    pub fn ancestors(&self, id: NodeId) -> Ancestors<'_> {
        Ancestors {
            ast: self,
            current: self.parent(id),
        }
    }

    /// All descendants of `id` in DFS pre-order, excluding `id` itself.
    pub fn descendants(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        let mut stack: Vec<NodeId> = self.children(id).collect();
        stack.reverse();
        std::iter::from_fn(move || {
            let next = stack.pop()?;
            let mut kids: Vec<NodeId> = self.children(next).collect();
            kids.reverse();
            stack.extend(kids);
            Some(next)
        })
    }

    /// The full source text.
    pub fn source(&self) -> &str {
        &self.source.text
    }

    /// The file path.
    pub fn path(&self) -> &std::path::Path {
        &self.source.path
    }

    /// The source text covered by `range`.
    pub fn raw_source(&self, range: Range) -> &str {
        &self.source.text[range.start as usize..range.end as usize]
    }

    /// The comments, in source order.
    pub fn comments(&self) -> &[Comment] {
        &self.comments
    }

    /// The string interner.
    pub fn interner(&self) -> &Interner {
        &self.interner
    }
}

/// Iterator over a node's ancestors, nearest first. See [`Ast::ancestors`].
pub struct Ancestors<'a> {
    ast: &'a Ast,
    current: OptNodeId,
}

impl Iterator for Ancestors<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        let id = self.current.get()?;
        self.current = self.ast.parent(id);
        Some(id)
    }
}
```

Ensure `lib.rs` re-exports `Ancestors` and `Ast` from `ast`.

**Step 4: Run test to verify it passes**

Run: `cargo test -p murphy-ast`
Expected: PASS (12 tests).

**Step 5: Commit**

```bash
git add crates/murphy-ast/
git commit -m "feat(murphy-ast): add traversal API (children, ancestors, descendants)"
```

---

## Task 8: Serialization round-trip

**Files:**
- Modify: `crates/murphy-ast/src/serialize.rs`

`Ast` must be serializable (ADR 0037 §3.5 — serializability is a v1 design
goal; the cache *feature* is murphy-9cr.26). Because `NodeKind` is a
`#[repr(C, u8)]` enum with uninitialized padding, a raw `memcpy` of the
node `Vec` is unsound. Serialize **field by field, little-endian** — sound,
deterministic, and the basis the cache builds on.

**Step 1: Write the failing test**

Add to `serialize.rs`:

```rust
#[cfg(test)]
mod tests {
    use crate::builder::AstBuilder;
    use crate::node::{CommentKind, NodeKind, OptNodeId, Range};

    fn r(a: u32, b: u32) -> Range {
        Range { start: a, end: b }
    }

    #[test]
    fn round_trip_is_bit_equal() {
        let mut b = AstBuilder::new("x = 1 # c", "t.rb");
        let int = b.push(NodeKind::Int(1), r(4, 5));
        let name = b.intern_symbol("x");
        let asgn = b.push(
            NodeKind::Lvasgn {
                name,
                value: OptNodeId::some(int),
            },
            r(0, 5),
        );
        let list = b.push_list(&[asgn]);
        let root = b.push(NodeKind::Begin(list), r(0, 5));
        b.add_comment(r(6, 9), CommentKind::Inline);
        let ast = b.finish(root);

        let bytes = ast.to_bytes();
        let restored = crate::Ast::from_bytes(&bytes).expect("round-trip");
        assert_eq!(ast, restored, "round-trip must be bit-equal");
    }

    #[test]
    fn round_trip_empty_ast() {
        let mut b = AstBuilder::new("", "e.rb");
        let root = b.push(NodeKind::Nil, r(0, 0));
        let ast = b.finish(root);
        let restored = crate::Ast::from_bytes(&ast.to_bytes()).unwrap();
        assert_eq!(ast, restored);
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p murphy-ast`
Expected: FAIL — `to_bytes`, `from_bytes` not defined.

**Step 3: Write the implementation**

Implement in `serialize.rs`. Design constraints:

- A small cursor-based reader/writer over `Vec<u8>` / `&[u8]`, all
  little-endian.
- Primitive writers: `u8`, `u32`, `u64`, `i64`, `f64` (`to_le_bytes` /
  `from_le_bytes`), and a length-prefixed `&str` / `&[u8]`.
- Each typed value (`NodeId`, `OptNodeId`, `Symbol`, `StringId`, `Range`,
  `NodeList`, `Comment`, `CommentKind`, `AstNode`, `NodeKind`) gets a
  `write(&self, out: &mut Vec<u8>)` and a `read(cur: &mut &[u8]) -> Result<Self, SerError>`.
- **`NodeKind`**: write the discriminant as a `u8` (declaration order,
  `Error == 0`), then each field in declaration order. `read` matches on
  the discriminant. All 37 variants must be handled — the `write` side can
  use a `match`, the `read` side a `match` on the `u8`. An unknown
  discriminant is a `SerError`.
- **`Ast::to_bytes`** concatenates, each length-prefixed with a `u64`
  count: `nodes`, `node_lists`, `interner.blob`, `interner.offsets`,
  `comments`, `source.text`, `source.path` (as UTF-8 bytes — use
  `Path::to_str` / on the read side `PathBuf::from`), then the `root`
  `NodeId`. No magic/version header — that is murphy-9cr.26.
- **`Ast::from_bytes`** reverses it, returning `Result<Ast, SerError>`.
  After reading the interner blob, validate it is UTF-8
  (`std::str::from_utf8`) so `Interner::resolve` stays sound.
- Define `pub enum SerError { UnexpectedEof, BadDiscriminant, InvalidUtf8 }`
  (derive `Debug`). Re-export it from `lib.rs`.
- `to_bytes` / `from_bytes` are inherent methods on `Ast`. Put them in
  `serialize.rs` as `impl Ast { ... }` (the impl block can live in any
  module of the crate). They need access to the `pub(crate)` fields of
  `Ast` and `Interner` — both are in this crate, so that works.

**Templates — copy these, then complete the remaining variants.**

Cursor helpers (little-endian):

```rust
/// Serialization failure.
#[derive(Debug)]
pub enum SerError {
    UnexpectedEof,
    BadDiscriminant,
    InvalidUtf8,
}

fn put_u8(out: &mut Vec<u8>, v: u8) { out.push(v); }
fn put_u32(out: &mut Vec<u8>, v: u32) { out.extend_from_slice(&v.to_le_bytes()); }
fn put_u64(out: &mut Vec<u8>, v: u64) { out.extend_from_slice(&v.to_le_bytes()); }
fn put_i64(out: &mut Vec<u8>, v: i64) { out.extend_from_slice(&v.to_le_bytes()); }
fn put_f64(out: &mut Vec<u8>, v: f64) { out.extend_from_slice(&v.to_le_bytes()); }

fn take<'a>(cur: &mut &'a [u8], n: usize) -> Result<&'a [u8], SerError> {
    if cur.len() < n {
        return Err(SerError::UnexpectedEof);
    }
    let (head, rest) = cur.split_at(n);
    *cur = rest;
    Ok(head)
}
fn get_u8(cur: &mut &[u8]) -> Result<u8, SerError> { Ok(take(cur, 1)?[0]) }
fn get_u32(cur: &mut &[u8]) -> Result<u32, SerError> {
    Ok(u32::from_le_bytes(take(cur, 4)?.try_into().unwrap()))
}
// get_u64 / get_i64 / get_f64 follow the same shape.
```

`NodeKind` write/read — discriminant = declaration order (`Error == 0` …
`Arg == 36`). This serialization discriminant is chosen to match
declaration order; it does not have to equal the in-memory `#[repr]`
discriminant, but matching keeps it simple. **Field order = declaration
order.** Three representative pairs — a unit, a scalar, a struct variant:

```rust
fn write_node_kind(k: &NodeKind, out: &mut Vec<u8>) {
    match *k {
        NodeKind::Error => put_u8(out, 0),
        // unit variant: discriminant only, no fields
        NodeKind::Nil => put_u8(out, 1),

        // scalar payload: discriminant, then the field
        NodeKind::Int(v) => { put_u8(out, 5); put_i64(out, v); }

        // struct payload: discriminant, then each field in declaration order.
        // OptNodeId/NodeId/Symbol/StringId are repr(transparent) over u32.
        NodeKind::Send { receiver, method, args } => {
            put_u8(out, 17);
            put_u32(out, receiver.0);
            put_u32(out, method.0);
            put_u32(out, args.start);
            put_u32(out, args.len);
        }
        // ... all remaining variants follow the same pattern ...
    }
}

fn read_node_kind(cur: &mut &[u8]) -> Result<NodeKind, SerError> {
    Ok(match get_u8(cur)? {
        0 => NodeKind::Error,
        1 => NodeKind::Nil,
        5 => NodeKind::Int(get_i64(cur)?),
        17 => NodeKind::Send {
            receiver: OptNodeId(get_u32(cur)?),
            method: Symbol(get_u32(cur)?),
            args: NodeList { start: get_u32(cur)?, len: get_u32(cur)? },
        },
        // ... all remaining discriminants ...
        _ => return Err(SerError::BadDiscriminant),
    })
}
```

Complete all 37 variants on **both** sides with identical field order. A
field-order mismatch between `write` and `read` is the one bug the
round-trip test will not always catch (a symmetric mistake round-trips
clean) — so transcribe field order directly from the `NodeKind`
declaration in `node.rs`, variant by variant.

Write the remaining helper functions, keep each focused, and reference
design §8.

In `lib.rs`, add `pub use serialize::SerError;`.

**Step 4: Run test to verify it passes**

Run: `cargo test -p murphy-ast`
Expected: PASS (14 tests).

**Step 5: Commit**

```bash
git add crates/murphy-ast/
git commit -m "feat(murphy-ast): add to_bytes/from_bytes serialization round-trip"
```

---

## Task 9: Final quality gate

**Files:**
- Modify: `crates/murphy-ast/src/lib.rs` (tidy re-exports, crate docs)

**Step 1: Verify the full re-export surface**

Confirm `lib.rs` re-exports exactly the public API and that every `pub use`
resolves. The intended surface:

```rust
pub use ast::{Ancestors, Ast};
pub use builder::AstBuilder;
pub use interner::Interner;
pub use node::{
    AstNode, Comment, CommentKind, NodeId, NodeKind, NodeList, OptNodeId, Range, SourceBuffer,
    StringId, Symbol,
};
pub use serialize::SerError;
```

(`Symbol`/`StringId` re-exported from whichever module defines them.)

**Step 2: Run the formatting gate**

Run: `cargo fmt --check`
Expected: PASS. If it fails, run `cargo fmt` and re-check.

**Step 3: Run the clippy gate**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS with zero warnings. Fix any lint in `murphy-ast`. Do not
suppress with `#[allow]` unless there is a documented reason.

**Step 4: Run the full workspace test suite**

Run: `cargo test --workspace`
Expected: PASS — all pre-existing tests plus the 14 new `murphy-ast`
tests, 0 failures.

**Step 5: Commit**

```bash
git add crates/murphy-ast/
git commit -m "feat(murphy-ast): finalize public API surface and pass quality gates"
```

---

## Acceptance criteria (from murphy-9cr.14)

- [ ] `crates/murphy-ast` is created and `cargo build` recognizes it in the workspace
- [ ] Zero external crate dependencies (std only), prism-independent
- [ ] `NodeKind` is `#[repr(C, u8)]` with 37 variants (design §3; declaration order = discriminant)
- [ ] Core types `AstNode/NodeId/OptNodeId/Symbol/StringId/Range/NodeList/Comment/CommentKind` defined; POD types are `#[repr(C)]`
- [ ] `AstBuilder` builds an `Ast`; `finish(root)` derives parents structurally in one pass via `collect_children`
- [ ] Traversal API `parent/children/ancestors/descendants/root` works
- [ ] `to_bytes`/`from_bytes` round-trip is bit-equal
- [ ] TDD unit tests per module (interner/layout/collect_children/builder/traversal/serialize)
- [ ] `cargo build` / `cargo test --workspace` / `cargo fmt --check` / `cargo clippy --workspace --all-targets -- -D warnings` all pass
