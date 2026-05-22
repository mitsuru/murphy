//! `Cx<'a>` — the single surface through which a cop reads the AST.

use std::marker::PhantomData;

use murphy_ast::{AstNode, Comment, NodeId, NodeKind, OptNodeId, Range, collect_children};

use crate::abi::CxRaw;

/// Borrowed, direct-read view of the arena for one dispatch call.
///
/// Traversal and `NodeKind` matching are pure memory reads — zero FFI
/// (ADR 0038). The lifetime `'a` forbids retaining any part past the
/// call; the arena is immutable and host-owned for the call's duration.
#[derive(Clone, Copy)]
pub struct Cx<'a> {
    raw: &'a CxRaw,
    _marker: PhantomData<&'a murphy_ast::Ast>,
}

/// Reconstruct a slice from a `#[repr(C)]` pointer+length pair.
///
/// # Safety
/// `len == 0` → empty; otherwise `ptr..ptr+len` must be valid for `'a`.
unsafe fn slice<'a, T>(ptr: *const T, len: usize) -> &'a [T] {
    if len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(ptr, len) }
    }
}

impl<'a> Cx<'a> {
    /// Wrap a raw context.
    ///
    /// # Safety
    /// Every pointer/length pair in `raw` must describe live, immutable
    /// data valid for `'a`, and `raw.fns` must be non-null. The host
    /// upholds this for one dispatch call (ADR 0038 safety contract).
    pub unsafe fn from_raw(raw: &'a CxRaw) -> Cx<'a> {
        Cx {
            raw,
            _marker: PhantomData,
        }
    }

    fn nodes(&self) -> &'a [AstNode] {
        unsafe { slice(self.raw.nodes, self.raw.nodes_len) }
    }

    fn lists(&self) -> &'a [NodeId] {
        unsafe { slice(self.raw.lists, self.raw.lists_len) }
    }

    /// The arena root node.
    pub fn root(&self) -> NodeId {
        self.raw.root
    }

    /// The node at `id`.
    pub fn node(&self, id: NodeId) -> &'a AstNode {
        &self.nodes()[id.0 as usize]
    }

    /// The kind of the node at `id`.
    pub fn kind(&self, id: NodeId) -> &'a NodeKind {
        &self.nodes()[id.0 as usize].kind
    }

    /// The source range of the node at `id`.
    pub fn range(&self, id: NodeId) -> Range {
        self.nodes()[id.0 as usize].range
    }

    /// The parent of `id`; `OptNodeId::NONE` for the root.
    pub fn parent(&self, id: NodeId) -> OptNodeId {
        self.nodes()[id.0 as usize].parent
    }

    /// Direct children of `id`, in source order. Allocates one `Vec` per
    /// call because `collect_children` writes into a `Vec`; an
    /// allocation-free iterator variant could be added later if profiling
    /// shows it matters.
    pub fn children(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        collect_children(self.kind(id), self.lists(), &mut out);
        out
    }

    /// Ancestors of `id`, nearest first, up to and including the root.
    pub fn ancestors(&self, id: NodeId) -> impl Iterator<Item = NodeId> + 'a {
        let nodes = self.nodes();
        let mut current = nodes[id.0 as usize].parent;
        std::iter::from_fn(move || {
            let next = current.get()?;
            current = nodes[next.0 as usize].parent;
            Some(next)
        })
    }

    /// All descendants of `id` in DFS pre-order, excluding `id`. Allocates
    /// one `Vec` per call (plus per-node `Vec`s via [`Self::children`]); an
    /// allocation-free iterator variant could be added later if profiling
    /// shows it matters.
    pub fn descendants(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = self.children(id);
        stack.reverse();
        while let Some(n) = stack.pop() {
            out.push(n);
            let mut kids = self.children(n);
            kids.reverse();
            stack.extend(kids);
        }
        out
    }

    /// Resolve an interner index (`Symbol` / `StringId`) to its string.
    fn resolve(&self, index: u32) -> &'a str {
        let offsets: &[Range] =
            unsafe { slice(self.raw.interner_offsets, self.raw.interner_offsets_len) };
        let blob: &[u8] = unsafe { slice(self.raw.interner_blob, self.raw.interner_blob_len) };
        let r = offsets[index as usize];
        std::str::from_utf8(&blob[r.start as usize..r.end as usize])
            .expect("interner blob holds valid UTF-8")
    }

    /// The string behind an interned `Symbol`.
    pub fn symbol_str(&self, sym: murphy_ast::Symbol) -> &'a str {
        self.resolve(sym.0)
    }

    /// The contents behind an interned string-literal `StringId`.
    pub fn string_str(&self, id: murphy_ast::StringId) -> &'a str {
        self.resolve(id.0)
    }

    /// The file's comments, in source order.
    pub fn comments(&self) -> &'a [Comment] {
        unsafe { slice(self.raw.comments, self.raw.comments_len) }
    }

    /// The source text covered by `range`.
    pub fn raw_source(&self, range: Range) -> &'a str {
        let src: &[u8] = unsafe { slice(self.raw.source, self.raw.source_len) };
        std::str::from_utf8(&src[range.start as usize..range.end as usize])
            .expect("source is valid UTF-8")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::{CxRaw, FnTable, RawEdit, RawOffense, RawSlice};
    use murphy_ast::{Ast, AstBuilder, NodeKind, OptNodeId, Range};

    /// Build `return nil` and return the owned `Ast` (kept alive by the
    /// caller) plus the root id.
    fn fixture() -> (Ast, murphy_ast::NodeId) {
        let mut b = AstBuilder::new("return nil", "t.rb".to_string());
        let nil = b.push(NodeKind::Nil, Range { start: 7, end: 10 });
        let root = b.push(
            NodeKind::Return(OptNodeId::some(nil)),
            Range { start: 0, end: 10 },
        );
        (b.finish(root), root)
    }

    // A FnTable is required to construct CxRaw; reads never call it.
    unsafe extern "C" fn noop_offense(_: *mut std::ffi::c_void, _: *const RawOffense) {}
    unsafe extern "C" fn noop_edit(_: *mut std::ffi::c_void, _: *const RawEdit) {}

    /// Build a `CxRaw` pointing into `ast`'s backing storage. The returned
    /// `CxRaw` borrows both `ast` and `fns` for `'a` (raw-pointer fields,
    /// not lifetime-tracked — the caller keeps both alive).
    fn cx_raw_for<'a>(ast: &'a Ast, fns: &'a FnTable) -> CxRaw {
        let p = ast.raw_parts();
        CxRaw {
            nodes: p.nodes.as_ptr(),
            nodes_len: p.nodes.len(),
            lists: p.node_lists.as_ptr(),
            lists_len: p.node_lists.len(),
            interner_blob: p.interner_blob.as_ptr(),
            interner_blob_len: p.interner_blob.len(),
            interner_offsets: p.interner_offsets.as_ptr(),
            interner_offsets_len: p.interner_offsets.len(),
            comments: p.comments.as_ptr(),
            comments_len: p.comments.len(),
            source: p.source.as_ptr(),
            source_len: p.source.len(),
            root: p.root,
            cop_name: RawSlice::EMPTY,
            fns: fns as *const FnTable,
            sink: std::ptr::null_mut(),
        }
    }

    #[test]
    fn accessors_match_the_underlying_ast() {
        let (ast, root) = fixture();
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };

        assert_eq!(cx.root(), root);
        assert_eq!(*cx.node(root), *ast.node(root));
        assert_eq!(*cx.kind(root), *ast.kind(root));
        assert_eq!(cx.range(root), ast.range(root));
        assert_eq!(cx.parent(root), ast.parent(root));
        let children = cx.children(root);
        assert_eq!(children, ast.children(root).collect::<Vec<_>>());
        // `root` has no ancestors; walk from the `nil` child so the
        // parent-walking loop is actually exercised.
        let nil = children[0];
        assert_eq!(
            cx.ancestors(nil).collect::<Vec<_>>(),
            ast.ancestors(nil).collect::<Vec<_>>()
        );
        assert_eq!(cx.ancestors(nil).collect::<Vec<_>>(), vec![root]);
        assert_eq!(
            cx.ancestors(root).collect::<Vec<_>>(),
            ast.ancestors(root).collect::<Vec<_>>()
        );
        let desc: Vec<_> = cx.descendants(root);
        assert_eq!(desc, ast.descendants(root).collect::<Vec<_>>());
        assert_eq!(cx.comments(), ast.comments());
        assert_eq!(
            cx.raw_source(cx.range(root)),
            ast.raw_source(ast.range(root))
        );
    }

    #[test]
    fn symbol_and_string_resolve_through_the_interner() {
        let mut b = AstBuilder::new("x = \"hi\"", "t.rb".to_string());
        let sym = b.intern_symbol("x");
        let str_id = b.intern_string("hi");
        let root = b.push(NodeKind::Nil, Range { start: 0, end: 0 });
        let ast = b.finish(root);

        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };

        assert_eq!(cx.symbol_str(sym), "x");
        assert_eq!(cx.string_str(str_id), "hi");
    }
}
