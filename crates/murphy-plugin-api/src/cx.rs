//! `Cx<'a>` — the single surface through which a cop reads the AST.

use std::marker::PhantomData;

use murphy_ast::{
    AstNode, Comment, NodeId, NodeKind, NodeLoc, OptNodeId, Range, SourceToken, collect_children,
};

use crate::abi::CxRaw;
use crate::{ConfigError, CopOptions};

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

    /// The source range of the node at `id` — shorthand for
    /// `self.loc(id).expression` / `self.node(id).loc.expression`.
    pub fn range(&self, id: NodeId) -> Range {
        self.nodes()[id.0 as usize].loc.expression
    }

    /// The `node.loc` bundle for `id` — Murphy's analog of the parser
    /// gem's `node.loc` accessor. `.expression` is the AST node's full
    /// source range; `.name` is the identifier range (the
    /// `node.loc.name` analog), [`Range::ZERO`] for nodes without
    /// an identifier or for name-bearing nodes the translator did not
    /// annotate. Equivalent to `self.node(id).loc`; provided as a
    /// shorthand so cops can write `cx.loc(node).name`.
    pub fn loc(&self, id: NodeId) -> NodeLoc {
        self.nodes()[id.0 as usize].loc
    }

    /// The parent of `id`; `OptNodeId::NONE` for the root.
    pub fn parent(&self, id: NodeId) -> OptNodeId {
        self.nodes()[id.0 as usize].parent
    }

    /// Resolve a [`NodeList`] to its backing slice of child ids.
    ///
    /// Zero-copy: returns a borrow directly into the arena's `node_lists`
    /// side table. This is the allocation-free counterpart to
    /// [`Self::children`] for the variable-length child field of a single
    /// `NodeKind` variant (e.g. `Send.args`, `Array`'s elements). The
    /// generated code of `def_node_matcher!` (murphy-9cr.18) uses it to bind
    /// `$...` seq captures and to match fixed-length argument lists.
    pub fn list(&self, l: murphy_ast::NodeList) -> &'a [NodeId] {
        let start = l.start as usize;
        &self.lists()[start..start + l.len as usize]
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

    /// The method-name selector of a method-bearing node — the call
    /// selector for `Send`/`Csend`, or the defined name for `Def`/`Defs`.
    /// `None` for any other node kind. Mirrors `node.method_name` on
    /// RuboCop's method-dispatch and def nodes.
    pub fn method_name(&self, id: NodeId) -> Option<&'a str> {
        let sym = match *self.kind(id) {
            NodeKind::Send { method, .. } | NodeKind::Csend { method, .. } => method,
            NodeKind::Def { name, .. } | NodeKind::Defs { name, .. } => name,
            _ => return None,
        };
        Some(self.symbol_str(sym))
    }

    /// `comparison_method?` for the node's selector — see
    /// [`crate::method_predicates::is_comparison_method`]. `false` for
    /// nodes without a selector.
    pub fn is_comparison_method(&self, id: NodeId) -> bool {
        self.method_name(id)
            .is_some_and(crate::method_predicates::is_comparison_method)
    }

    /// `operator_method?` for the node's selector — see
    /// [`crate::method_predicates::is_operator_method`].
    pub fn is_operator_method(&self, id: NodeId) -> bool {
        self.method_name(id)
            .is_some_and(crate::method_predicates::is_operator_method)
    }

    /// `assignment_method?` for the node's selector — see
    /// [`crate::method_predicates::is_assignment_method`].
    pub fn is_assignment_method(&self, id: NodeId) -> bool {
        self.method_name(id)
            .is_some_and(crate::method_predicates::is_assignment_method)
    }

    /// `predicate_method?` for the node's selector — see
    /// [`crate::method_predicates::is_predicate_method`].
    pub fn is_predicate_method(&self, id: NodeId) -> bool {
        self.method_name(id)
            .is_some_and(crate::method_predicates::is_predicate_method)
    }

    /// `bang_method?` for the node's selector — see
    /// [`crate::method_predicates::is_bang_method`].
    pub fn is_bang_method(&self, id: NodeId) -> bool {
        self.method_name(id)
            .is_some_and(crate::method_predicates::is_bang_method)
    }

    /// `camel_case_method?` for the node's selector — see
    /// [`crate::method_predicates::is_camel_case_method`].
    pub fn is_camel_case_method(&self, id: NodeId) -> bool {
        self.method_name(id)
            .is_some_and(crate::method_predicates::is_camel_case_method)
    }

    /// The file's comments, in source order.
    pub fn comments(&self) -> &'a [Comment] {
        unsafe { slice(self.raw.comments, self.raw.comments_len) }
    }

    /// The file's source tokens, in source order.
    pub fn sorted_tokens(&self) -> &'a [SourceToken] {
        unsafe { slice(self.raw.sorted_tokens, self.raw.sorted_tokens_len) }
    }

    /// Decode the current cop's runtime options.
    pub fn options<T: CopOptions>(&self) -> Result<T, ConfigError> {
        let bytes = unsafe { self.raw.options_json.as_bytes() };
        T::from_config_json(bytes)
    }

    /// Decode the current cop's runtime options, falling back to defaults.
    pub fn options_or_default<T: CopOptions>(&self) -> T {
        self.options::<T>().unwrap_or_default()
    }

    /// The source text covered by `range`.
    pub fn raw_source(&self, range: Range) -> &'a str {
        let src: &[u8] = unsafe { slice(self.raw.source, self.raw.source_len) };
        std::str::from_utf8(&src[range.start as usize..range.end as usize])
            .expect("source is valid UTF-8")
    }

    /// Source range of the `.` or `&.` operator for an explicit-dot call
    /// — the parser-gem `node.loc.dot` analog, computed on demand.
    ///
    /// Returns `None` for:
    /// - non-call kinds (anything but `Send` / `Csend`),
    /// - implicit `Send` (no receiver, e.g. a bare `foo` resolved as
    ///   `Kernel#foo`),
    /// - operator and bracket methods (`a + b`, `a[b]`) — the source
    ///   between receiver and selector holds no dot,
    /// - implicit-call `foo.()` where the call has no selector range,
    ///   so the scan window degenerates to empty.
    ///
    /// Scans the bytes between `receiver.expression.end` and the
    /// selector's `loc.name.start`, ignoring `#` line comments. The
    /// window is short in practice (avg 0.6 byte, max ≈ a multi-line
    /// chain), so this is cheaper than maintaining a side-table that
    /// every `Ast` would pay for. Cops that never call it pay nothing.
    pub fn call_operator_loc(&self, id: NodeId) -> Option<Range> {
        let node = &self.nodes()[id.0 as usize];
        let (receiver, name_start) = match node.kind {
            NodeKind::Send { receiver, .. } => (receiver.get()?, node.loc.name.start),
            NodeKind::Csend { receiver, .. } => (receiver, node.loc.name.start),
            _ => return None,
        };
        let scan_start = self.nodes()[receiver.0 as usize].loc.expression.end;
        if scan_start >= name_start {
            return None;
        }
        let src: &[u8] = unsafe { slice(self.raw.source, self.raw.source_len) };
        let window = &src[scan_start as usize..name_start as usize];
        let mut i = 0;
        let mut in_comment = false;
        while i < window.len() {
            let b = window[i];
            if b == b'\n' {
                in_comment = false;
                i += 1;
                continue;
            }
            if in_comment {
                i += 1;
                continue;
            }
            if b == b'#' {
                in_comment = true;
                i += 1;
                continue;
            }
            if b == b'&' && i + 1 < window.len() && window[i + 1] == b'.' {
                let start = scan_start + i as u32;
                return Some(Range {
                    start,
                    end: start + 2,
                });
            }
            if b == b'.' {
                let start = scan_start + i as u32;
                return Some(Range {
                    start,
                    end: start + 1,
                });
            }
            i += 1;
        }
        None
    }

    /// The whole file's source text. A `NodeCop` with `KINDS = &[]`
    /// (file-visit, see `NodeCop` doc) uses this to scan the entire
    /// file — `cx.range(cx.root())` only spans the AST root node,
    /// which can be narrower than the file (leading comments, trailing
    /// whitespace, etc. live outside the root's byte range).
    pub fn source(&self) -> &'a str {
        let src: &[u8] = unsafe { slice(self.raw.source, self.raw.source_len) };
        std::str::from_utf8(src).expect("source is valid UTF-8")
    }

    /// Record an offense. `cop_name` is stamped from the `CxRaw` the host
    /// built for the running cop.
    pub fn emit_offense(&self, range: Range, message: &str, severity: Option<crate::Severity>) {
        let offense = crate::RawOffense {
            cop_name: self.raw.cop_name,
            message: crate::RawSlice {
                ptr: message.as_ptr(),
                len: message.len(),
            },
            range,
            severity: crate::Severity::to_wire(severity),
        };
        // Safety: `fns` is non-null per `from_raw`'s contract; `sink` is
        // an opaque host handle interpreted only by the callback. The
        // message slice outlives this synchronous call.
        let fns = unsafe { &*self.raw.fns };
        unsafe { (fns.emit_offense)(self.raw.sink, &offense) };
    }

    /// Record an autocorrect edit. Offense↔edit correlation is the host's
    /// (murphy-9cr.22) concern.
    pub fn emit_edit(&self, range: Range, replacement: &str) {
        let edit = crate::RawEdit {
            range,
            replacement: crate::RawSlice {
                ptr: replacement.as_ptr(),
                len: replacement.len(),
            },
        };
        // Safety: see `emit_offense`.
        let fns = unsafe { &*self.raw.fns };
        unsafe { (fns.emit_edit)(self.raw.sink, &edit) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CopOptions;
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
            sorted_tokens: p.sorted_tokens.as_ptr(),
            sorted_tokens_len: p.sorted_tokens.len(),
            options_json: RawSlice::from_str("{}"),
        }
    }

    #[derive(Default)]
    struct TestOptions {
        style: String,
    }

    impl CopOptions for TestOptions {
        fn from_config_json(bytes: &[u8]) -> Result<Self, crate::ConfigError> {
            let value: serde_json::Value =
                serde_json::from_slice(bytes).map_err(crate::ConfigError::parse)?;
            let style = value
                .get("style")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("default")
                .to_string();
            Ok(Self { style })
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
    fn list_resolves_node_list_to_a_borrowed_slice() {
        use murphy_ast::{AstBuilder, NodeKind, NodeList, OptNodeId, Range};

        // `foo(1, 2)` — a Send whose `args` NodeList holds two Int nodes.
        let mut b = AstBuilder::new("foo(1, 2)", "t.rb".to_string());
        let one = b.push(NodeKind::Int(1), Range { start: 4, end: 5 });
        let two = b.push(NodeKind::Int(2), Range { start: 7, end: 8 });
        let args = b.push_list(&[one, two]);
        let method = b.intern_symbol("foo");
        let root = b.push(
            NodeKind::Send {
                receiver: OptNodeId::NONE,
                method,
                args,
            },
            Range { start: 0, end: 9 },
        );
        let ast = b.finish(root);

        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };

        // Pull the `args` NodeList back out of the Send and resolve it.
        let NodeKind::Send { args, .. } = *cx.kind(root) else {
            panic!("expected Send");
        };
        assert_eq!(cx.list(args), &[one, two]);
        // An empty NodeList resolves to an empty slice.
        assert_eq!(cx.list(NodeList::EMPTY), &[] as &[murphy_ast::NodeId]);
    }

    #[test]
    fn sorted_tokens_match_the_underlying_ast() {
        let mut b = AstBuilder::new("foo(1)", "t.rb".to_string());
        let root = b.push(NodeKind::Int(1), Range { start: 4, end: 5 });
        b.add_source_token(murphy_ast::SourceToken {
            kind: murphy_ast::SourceTokenKind::LeftParen,
            range: Range { start: 3, end: 4 },
        });
        let ast = b.finish(root);
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };

        assert_eq!(cx.sorted_tokens(), ast.sorted_tokens());
    }

    /// Build a synthetic Send/Csend with the receiver/name ranges a real
    /// parser would emit for `source[recv]` (the receiver text) chained
    /// onto `source[name]` (the selector text). Returns the call's
    /// `NodeId` plus the owned `Ast`.
    fn build_call(
        source: &str,
        recv: Option<Range>,
        name: Range,
        is_csend: bool,
    ) -> (Ast, murphy_ast::NodeId) {
        let mut b = AstBuilder::new(source.to_string(), "t.rb".to_string());
        let recv_id = recv.map(|r| {
            let recv_method = b.intern_symbol("recv");
            b.push_named(
                NodeKind::Send {
                    receiver: OptNodeId::NONE,
                    method: recv_method,
                    args: murphy_ast::NodeList::EMPTY,
                },
                r,
                r,
            )
        });
        let method = b.intern_symbol(&source[name.start as usize..name.end as usize]);
        let expression = Range {
            start: recv.map(|r| r.start).unwrap_or(name.start),
            end: name.end,
        };
        let root = if is_csend {
            let recv_id = recv_id.expect("Csend requires a receiver");
            b.push_named(
                NodeKind::Csend {
                    receiver: recv_id,
                    method,
                    args: murphy_ast::NodeList::EMPTY,
                },
                expression,
                name,
            )
        } else {
            b.push_named(
                NodeKind::Send {
                    receiver: recv_id.map(OptNodeId::some).unwrap_or(OptNodeId::NONE),
                    method,
                    args: murphy_ast::NodeList::EMPTY,
                },
                expression,
                name,
            )
        };
        (b.finish(root), root)
    }

    #[test]
    fn call_operator_loc_finds_explicit_dot() {
        // `foo.bar`
        let (ast, root) = build_call(
            "foo.bar",
            Some(Range { start: 0, end: 3 }),
            Range { start: 4, end: 7 },
            false,
        );
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert_eq!(cx.call_operator_loc(root), Some(Range { start: 3, end: 4 }));
        assert_eq!(cx.raw_source(cx.call_operator_loc(root).unwrap()), ".");
    }

    #[test]
    fn call_operator_loc_finds_safe_navigation() {
        // `foo&.bar`
        let (ast, root) = build_call(
            "foo&.bar",
            Some(Range { start: 0, end: 3 }),
            Range { start: 5, end: 8 },
            true,
        );
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert_eq!(cx.call_operator_loc(root), Some(Range { start: 3, end: 5 }));
        assert_eq!(cx.raw_source(cx.call_operator_loc(root).unwrap()), "&.");
    }

    #[test]
    fn call_operator_loc_handles_multiline_chain() {
        // `foo\n  .bar` — receiver ends at offset 3, name starts at 7.
        let (ast, root) = build_call(
            "foo\n  .bar",
            Some(Range { start: 0, end: 3 }),
            Range { start: 7, end: 10 },
            false,
        );
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert_eq!(cx.call_operator_loc(root), Some(Range { start: 6, end: 7 }));
    }

    #[test]
    fn call_operator_loc_skips_dots_inside_line_comments() {
        // `foo # x.y\n  .bar` — the `.` in the comment must not match.
        let src = "foo # x.y\n  .bar";
        let (ast, root) = build_call(
            src,
            Some(Range { start: 0, end: 3 }),
            Range { start: 13, end: 16 },
            false,
        );
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert_eq!(
            cx.call_operator_loc(root),
            Some(Range { start: 12, end: 13 })
        );
        assert_eq!(cx.raw_source(cx.call_operator_loc(root).unwrap()), ".");
    }

    #[test]
    fn call_operator_loc_returns_none_for_implicit_send() {
        // bare `foo` — Send with receiver = None
        let (ast, root) = build_call("foo", None, Range { start: 0, end: 3 }, false);
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert_eq!(cx.call_operator_loc(root), None);
    }

    #[test]
    fn call_operator_loc_returns_none_for_operator_method() {
        // `foo + bar` — Send with method `:+`. Window is " " (no dot).
        let (ast, root) = build_call(
            "foo + bar",
            Some(Range { start: 0, end: 3 }),
            Range { start: 4, end: 5 },
            false,
        );
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert_eq!(cx.call_operator_loc(root), None);
    }

    #[test]
    fn call_operator_loc_returns_none_for_bracket_method() {
        // `a[b]` — Send with method `:[]`, name range starts at the
        // bracket (= receiver end). Empty window ⇒ None.
        let (ast, root) = build_call(
            "a[b]",
            Some(Range { start: 0, end: 1 }),
            Range { start: 1, end: 3 },
            false,
        );
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert_eq!(cx.call_operator_loc(root), None);
    }

    #[test]
    fn call_operator_loc_returns_none_for_non_call_kinds() {
        // A bare `nil` literal — not a call kind.
        let (ast, root) = fixture();
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        // `root` is Return, child is Nil — both non-call.
        assert_eq!(cx.call_operator_loc(root), None);
        let nil = cx.children(root)[0];
        assert_eq!(cx.call_operator_loc(nil), None);
    }

    #[test]
    fn options_or_default_decodes_current_cop_options() {
        let (ast, _) = fixture();
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let mut raw = cx_raw_for(&ast, &fns);
        raw.options_json = RawSlice::from_str(r#"{"style":"configured"}"#);
        let cx = unsafe { Cx::from_raw(&raw) };

        let options = cx.options_or_default::<TestOptions>();
        assert_eq!(options.style, "configured");
    }

    #[test]
    fn options_or_default_falls_back_on_decode_error() {
        let (ast, _) = fixture();
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let mut raw = cx_raw_for(&ast, &fns);
        raw.options_json = RawSlice::from_str("not json");
        let cx = unsafe { Cx::from_raw(&raw) };

        let options = cx.options_or_default::<TestOptions>();
        assert_eq!(options.style, "");
    }

    use std::cell::RefCell;

    struct Sink {
        offenses: Vec<(String, String, Range, u8)>,
        edits: Vec<(Range, String)>,
    }

    unsafe extern "C" fn record_offense(sink: *mut std::ffi::c_void, o: *const RawOffense) {
        let sink = unsafe { &*(sink as *const RefCell<Sink>) };
        let o = unsafe { &*o };
        sink.borrow_mut().offenses.push((
            String::from_utf8(unsafe { o.cop_name.as_bytes() }.to_vec()).unwrap(),
            String::from_utf8(unsafe { o.message.as_bytes() }.to_vec()).unwrap(),
            o.range,
            o.severity,
        ));
    }

    unsafe extern "C" fn record_edit(sink: *mut std::ffi::c_void, e: *const RawEdit) {
        let sink = unsafe { &*(sink as *const RefCell<Sink>) };
        let e = unsafe { &*e };
        sink.borrow_mut().edits.push((
            e.range,
            String::from_utf8(unsafe { e.replacement.as_bytes() }.to_vec()).unwrap(),
        ));
    }

    #[test]
    fn emit_forwards_offense_and_edit_to_the_fn_table() {
        let (ast, root) = fixture();
        let fns = FnTable {
            emit_offense: record_offense,
            emit_edit: record_edit,
        };
        let sink = RefCell::new(Sink {
            offenses: Vec::new(),
            edits: Vec::new(),
        });

        let mut raw = cx_raw_for(&ast, &fns);
        raw.cop_name = RawSlice::from_str("Plugin/Demo");
        raw.sink = &sink as *const _ as *mut std::ffi::c_void;
        let cx = unsafe { Cx::from_raw(&raw) };

        cx.emit_offense(cx.range(root), "bad return", Some(crate::Severity::Error));
        cx.emit_edit(Range { start: 7, end: 10 }, "false");

        let s = sink.borrow();
        assert_eq!(s.offenses.len(), 1);
        assert_eq!(s.offenses[0].0, "Plugin/Demo");
        assert_eq!(s.offenses[0].1, "bad return");
        assert_eq!(
            s.offenses[0].3,
            crate::Severity::to_wire(Some(crate::Severity::Error))
        );
        assert_eq!(s.offenses[0].2, cx.range(root));
        assert_eq!(
            s.edits,
            vec![(Range { start: 7, end: 10 }, "false".to_string())]
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

    /// Build a bare `def <name>; end` and return its `Def` node id + Ast.
    fn build_def(source: &str, name: &str, name_range: Range) -> (Ast, murphy_ast::NodeId) {
        let mut b = AstBuilder::new(source.to_string(), "t.rb".to_string());
        let args = b.push(NodeKind::Args(murphy_ast::NodeList::EMPTY), name_range);
        let sym = b.intern_symbol(name);
        let root = b.push_named(
            NodeKind::Def {
                receiver: OptNodeId::NONE,
                name: sym,
                args,
                body: OptNodeId::NONE,
            },
            Range {
                start: 0,
                end: source.len() as u32,
            },
            name_range,
        );
        (b.finish(root), root)
    }

    #[test]
    fn method_name_resolves_send_csend_and_def_selectors() {
        // Send: `a == b` — selector `==` at [2, 4).
        let (ast, send) = build_call(
            "a == b",
            Some(Range { start: 0, end: 1 }),
            Range { start: 2, end: 4 },
            false,
        );
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert_eq!(cx.method_name(send), Some("=="));

        // Csend: `a&.foo` — selector `foo` at [3, 6).
        let (ast, csend) = build_call(
            "a&.foo",
            Some(Range { start: 0, end: 1 }),
            Range { start: 3, end: 6 },
            true,
        );
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert_eq!(cx.method_name(csend), Some("foo"));

        // Def: `def foo=(v); end` — selector `foo=`.
        let (ast, def) = build_def("def foo=(v); end", "foo=", Range { start: 4, end: 8 });
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert_eq!(cx.method_name(def), Some("foo="));
    }

    #[test]
    fn method_name_is_none_for_non_method_nodes() {
        // An Int literal has no selector.
        let mut b = AstBuilder::new("42", "t.rb".to_string());
        let root = b.push(NodeKind::Int(42), Range { start: 0, end: 2 });
        let ast = b.finish(root);
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert_eq!(cx.method_name(root), None);
    }

    #[test]
    fn cx_predicate_wrappers_classify_the_node_selector() {
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };

        // `a == b` → comparison + operator, not assignment/predicate/bang/camel.
        let (ast, cmp) = build_call(
            "a == b",
            Some(Range { start: 0, end: 1 }),
            Range { start: 2, end: 4 },
            false,
        );
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.is_comparison_method(cmp));
        assert!(cx.is_operator_method(cmp));
        assert!(!cx.is_assignment_method(cmp));
        assert!(!cx.is_predicate_method(cmp));

        // `def foo=(v); end` → assignment, not comparison.
        let (ast, setter) = build_def("def foo=(v); end", "foo=", Range { start: 4, end: 8 });
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.is_assignment_method(setter));
        assert!(!cx.is_comparison_method(setter));

        // `a.foo?` → predicate.
        let (ast, pred) = build_call(
            "a.foo?",
            Some(Range { start: 0, end: 1 }),
            Range { start: 2, end: 6 },
            false,
        );
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.is_predicate_method(pred));
        assert!(!cx.is_bang_method(pred));

        // `Foo()` → camel-case method.
        let (ast, camel) = build_call("Foo()", None, Range { start: 0, end: 3 }, false);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.is_camel_case_method(camel));
    }

    #[test]
    fn cx_predicate_wrappers_are_false_for_non_method_nodes() {
        let mut b = AstBuilder::new("42", "t.rb".to_string());
        let root = b.push(NodeKind::Int(42), Range { start: 0, end: 2 });
        let ast = b.finish(root);
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(!cx.is_comparison_method(root));
        assert!(!cx.is_operator_method(root));
        assert!(!cx.is_assignment_method(root));
        assert!(!cx.is_predicate_method(root));
        assert!(!cx.is_bang_method(root));
        assert!(!cx.is_camel_case_method(root));
    }
}
