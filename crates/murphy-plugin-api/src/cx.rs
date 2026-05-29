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

    /// `enumerable_method?` for the node's selector — see
    /// [`crate::method_predicates::is_enumerable_method`].
    pub fn is_enumerable_method(&self, id: NodeId) -> bool {
        self.method_name(id)
            .is_some_and(crate::method_predicates::is_enumerable_method)
    }

    /// `enumerator_method?` for the node's selector — see
    /// [`crate::method_predicates::is_enumerator_method`].
    pub fn is_enumerator_method(&self, id: NodeId) -> bool {
        self.method_name(id)
            .is_some_and(crate::method_predicates::is_enumerator_method)
    }

    /// `nonmutating_binary_operator_method?` for the node's selector — see
    /// [`crate::method_predicates::is_nonmutating_binary_operator_method`].
    pub fn is_nonmutating_binary_operator_method(&self, id: NodeId) -> bool {
        self.method_name(id)
            .is_some_and(crate::method_predicates::is_nonmutating_binary_operator_method)
    }

    /// `nonmutating_unary_operator_method?` for the node's selector — see
    /// [`crate::method_predicates::is_nonmutating_unary_operator_method`].
    pub fn is_nonmutating_unary_operator_method(&self, id: NodeId) -> bool {
        self.method_name(id)
            .is_some_and(crate::method_predicates::is_nonmutating_unary_operator_method)
    }

    /// `nonmutating_operator_method?` for the node's selector — see
    /// [`crate::method_predicates::is_nonmutating_operator_method`].
    pub fn is_nonmutating_operator_method(&self, id: NodeId) -> bool {
        self.method_name(id)
            .is_some_and(crate::method_predicates::is_nonmutating_operator_method)
    }

    /// `nonmutating_array_method?` for the node's selector — see
    /// [`crate::method_predicates::is_nonmutating_array_method`].
    pub fn is_nonmutating_array_method(&self, id: NodeId) -> bool {
        self.method_name(id)
            .is_some_and(crate::method_predicates::is_nonmutating_array_method)
    }

    /// `nonmutating_hash_method?` for the node's selector — see
    /// [`crate::method_predicates::is_nonmutating_hash_method`].
    pub fn is_nonmutating_hash_method(&self, id: NodeId) -> bool {
        self.method_name(id)
            .is_some_and(crate::method_predicates::is_nonmutating_hash_method)
    }

    /// `nonmutating_string_method?` for the node's selector — see
    /// [`crate::method_predicates::is_nonmutating_string_method`].
    pub fn is_nonmutating_string_method(&self, id: NodeId) -> bool {
        self.method_name(id)
            .is_some_and(crate::method_predicates::is_nonmutating_string_method)
    }

    /// The receiver of a call node (`Send`/`Csend`), or `OptNodeId::NONE`
    /// for a receiverless `Send` or any non-call node. Mirrors RuboCop's
    /// `node.receiver`.
    pub fn call_receiver(&self, id: NodeId) -> OptNodeId {
        match *self.kind(id) {
            NodeKind::Send { receiver, .. } => receiver,
            NodeKind::Csend { receiver, .. } => OptNodeId::some(receiver),
            _ => OptNodeId::NONE,
        }
    }

    /// The argument list of a call node (`Send`/`Csend`); an empty slice
    /// for a non-call node. Mirrors RuboCop's `node.arguments`.
    pub fn call_arguments(&self, id: NodeId) -> &'a [NodeId] {
        match *self.kind(id) {
            NodeKind::Send { args, .. } | NodeKind::Csend { args, .. } => self.list(args),
            _ => &[],
        }
    }

    /// The first argument of a call node, or `OptNodeId::NONE`. Mirrors
    /// RuboCop's `node.first_argument`.
    pub fn first_argument(&self, id: NodeId) -> OptNodeId {
        self.call_arguments(id)
            .first()
            .copied()
            .map_or(OptNodeId::NONE, OptNodeId::some)
    }

    /// The last argument of a call node, or `OptNodeId::NONE`. Mirrors
    /// RuboCop's `node.last_argument`.
    pub fn last_argument(&self, id: NodeId) -> OptNodeId {
        self.call_arguments(id)
            .last()
            .copied()
            .map_or(OptNodeId::NONE, OptNodeId::some)
    }

    /// Whether a call node has any arguments. Mirrors RuboCop's
    /// `node.arguments?`.
    pub fn has_call_arguments(&self, id: NodeId) -> bool {
        !self.call_arguments(id).is_empty()
    }

    /// `self_receiver?` — the call's receiver is `self`. Mirrors RuboCop's
    /// `node.self_receiver?` (`receiver&.self_type?`).
    pub fn is_self_receiver(&self, id: NodeId) -> bool {
        self.call_receiver(id)
            .get()
            .is_some_and(|r| matches!(self.kind(r), NodeKind::SelfExpr))
    }

    /// `const_receiver?` — the call's receiver is a constant. Mirrors
    /// RuboCop's `node.const_receiver?` (`receiver&.const_type?`).
    pub fn is_const_receiver(&self, id: NodeId) -> bool {
        self.call_receiver(id)
            .get()
            .is_some_and(|r| matches!(self.kind(r), NodeKind::Const { .. }))
    }

    /// `command?(name)` — a receiverless `Send` whose selector is `name`.
    /// Mirrors RuboCop's `node.command?(name)` (`!receiver && method?(name)`).
    /// A `Csend` always has a receiver, so it is never a command.
    pub fn is_command(&self, id: NodeId, name: &str) -> bool {
        matches!(*self.kind(id), NodeKind::Send { receiver, .. } if receiver.get().is_none())
            && self.method_name(id) == Some(name)
    }

    /// `negation_method?` — a call to `!` with a receiver (`!x`, parsed as
    /// `x.!`). Mirrors RuboCop's `node.negation_method?`
    /// (`receiver && method_name == :!`).
    pub fn is_negation_method(&self, id: NodeId) -> bool {
        self.call_receiver(id).get().is_some() && self.method_name(id) == Some("!")
    }

    /// `literal?` — the node is one of RuboCop's `LITERALS`
    /// (`TRUTHY_LITERALS + FALSEY_LITERALS`): string/xstring/dstring,
    /// symbol/dsymbol, integer/float/rational/complex, array, hash,
    /// regexp (+ its `regopt`), range, and `true`/`false`/`nil`.
    ///
    /// RuboCop distinguishes `irange`/`erange`; Murphy folds both into
    /// [`NodeKind::RangeExpr`], which is sound here because both are
    /// literals.
    pub fn is_literal(&self, id: NodeId) -> bool {
        matches!(
            self.kind(id),
            NodeKind::Str(..)
                | NodeKind::Dstr(..)
                | NodeKind::Xstr(..)
                | NodeKind::Int(..)
                | NodeKind::Float(..)
                | NodeKind::Sym(..)
                | NodeKind::Dsym(..)
                | NodeKind::Array(..)
                | NodeKind::Hash(..)
                | NodeKind::Regexp { .. }
                | NodeKind::Regopt(..)
                | NodeKind::True_
                | NodeKind::False_
                | NodeKind::Nil
                | NodeKind::RangeExpr { .. }
                | NodeKind::Rational(..)
                | NodeKind::Complex(..)
        )
    }

    /// The number of source lines the node's expression range spans —
    /// Murphy's analog of RuboCop's `node.line_count`
    /// (`last_line - first_line + 1`), computed from the expression
    /// range's source text.
    fn line_count(&self, id: NodeId) -> usize {
        self.raw_source(self.range(id)).matches('\n').count() + 1
    }

    /// `single_line?` — the node's expression spans exactly one line.
    pub fn is_single_line(&self, id: NodeId) -> bool {
        self.line_count(id) == 1
    }

    /// `multiline?` — the node's expression spans more than one line.
    pub fn is_multiline(&self, id: NodeId) -> bool {
        self.line_count(id) > 1
    }

    // --- typed-node accessors (pure field projections) ---
    //
    // Each returns the relevant child of a specific node kind, or the
    // empty value (`OptNodeId::NONE` / `&[]`) when `id` is a different
    // kind, so a cop can call them without a prior kind check. Mirrors
    // the accessor methods on RuboCop's typed `IfNode` / `HashNode` /
    // `PairNode` / `BlockNode`.

    /// `IfNode#condition` — the `if`/`unless`/ternary condition.
    pub fn if_condition(&self, id: NodeId) -> OptNodeId {
        match *self.kind(id) {
            NodeKind::If { cond, .. } => OptNodeId::some(cond),
            _ => OptNodeId::NONE,
        }
    }

    /// `IfNode#if_branch` — the `then` branch (the body run when the
    /// condition holds). `OptNodeId::NONE` if absent or not an `If`.
    pub fn if_then_branch(&self, id: NodeId) -> OptNodeId {
        match *self.kind(id) {
            NodeKind::If { then_, .. } => then_,
            _ => OptNodeId::NONE,
        }
    }

    /// `IfNode#else_branch` — the `else` branch. `OptNodeId::NONE` if
    /// absent or not an `If`.
    pub fn if_else_branch(&self, id: NodeId) -> OptNodeId {
        match *self.kind(id) {
            NodeKind::If { else_, .. } => else_,
            _ => OptNodeId::NONE,
        }
    }

    /// `HashNode#pairs` — the hash's **`Pair`-type** children only.
    /// Faithful to RuboCop's `pairs` (`each_child_node(:pair)`): a
    /// `kwsplat` such as the `**h` in `{ **h, a: 1 }` is **excluded**
    /// (use [`Self::children`] for every child — verified via
    /// `murphy ast`: `{**h}` parses to `(hash (kwsplat …))`). Empty
    /// `Vec` for a non-`Hash` node. Allocates, like [`Self::children`].
    pub fn hash_pairs(&self, id: NodeId) -> Vec<NodeId> {
        match *self.kind(id) {
            NodeKind::Hash(list) => self
                .list(list)
                .iter()
                .copied()
                .filter(|&c| matches!(self.kind(c), NodeKind::Pair { .. }))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// `PairNode#key`. `OptNodeId::NONE` if not a `Pair`.
    pub fn pair_key(&self, id: NodeId) -> OptNodeId {
        match *self.kind(id) {
            NodeKind::Pair { key, .. } => OptNodeId::some(key),
            _ => OptNodeId::NONE,
        }
    }

    /// `PairNode#value`. `OptNodeId::NONE` if not a `Pair`.
    pub fn pair_value(&self, id: NodeId) -> OptNodeId {
        match *self.kind(id) {
            NodeKind::Pair { value, .. } => OptNodeId::some(value),
            _ => OptNodeId::NONE,
        }
    }

    /// `BlockNode#send_node` — the call the block is attached to.
    /// `OptNodeId::NONE` if not a `Block`.
    pub fn block_call(&self, id: NodeId) -> OptNodeId {
        match *self.kind(id) {
            NodeKind::Block { call, .. } => OptNodeId::some(call),
            _ => OptNodeId::NONE,
        }
    }

    /// `BlockNode#arguments` — the block's `Args` node (always present
    /// for a block, possibly empty). `OptNodeId::NONE` if not a `Block`.
    pub fn block_arguments(&self, id: NodeId) -> OptNodeId {
        match *self.kind(id) {
            NodeKind::Block { args, .. } => OptNodeId::some(args),
            _ => OptNodeId::NONE,
        }
    }

    /// `BlockNode#body` — the block body. `OptNodeId::NONE` for an empty
    /// body or a non-`Block` node.
    pub fn block_body(&self, id: NodeId) -> OptNodeId {
        match *self.kind(id) {
            NodeKind::Block { body, .. } => body,
            _ => OptNodeId::NONE,
        }
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

    #[test]
    fn cx_collection_and_enumerable_wrappers_classify_the_node_selector() {
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };

        // `a.map` → enumerable + enumerator (in set), not a nonmutating
        // collection-specific table.
        let (ast, map) = build_call(
            "a.map",
            Some(Range { start: 0, end: 1 }),
            Range { start: 2, end: 5 },
            false,
        );
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.is_enumerable_method(map));
        assert!(cx.is_enumerator_method(map));

        // `a.each_slice` → enumerator via the `each_` prefix rule.
        let (ast, es) = build_call(
            "a.each_slice",
            Some(Range { start: 0, end: 1 }),
            Range { start: 2, end: 12 },
            false,
        );
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.is_enumerator_method(es));

        // `a.merge` → nonmutating hash method.
        let (ast, merge) = build_call(
            "a.merge",
            Some(Range { start: 0, end: 1 }),
            Range { start: 2, end: 7 },
            false,
        );
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.is_nonmutating_hash_method(merge));

        // `a + b` → nonmutating binary operator (so also nonmutating operator).
        let (ast, plus) = build_call(
            "a + b",
            Some(Range { start: 0, end: 1 }),
            Range { start: 2, end: 3 },
            false,
        );
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.is_nonmutating_binary_operator_method(plus));
        assert!(cx.is_nonmutating_operator_method(plus));
        assert!(!cx.is_nonmutating_unary_operator_method(plus));
    }

    /// Build `<recv-kind>.<sel>(args…)` where the receiver is a chosen
    /// `NodeKind` (self / const / a sub-send), returning the call + Ast.
    fn build_call_with(
        recv_kind: Option<NodeKind>,
        selector: &str,
        arg_ints: &[i64],
    ) -> (Ast, murphy_ast::NodeId) {
        let mut b = AstBuilder::new("x".to_string(), "t.rb".to_string());
        let z = Range { start: 0, end: 1 };
        let receiver = match recv_kind {
            Some(k) => OptNodeId::some(b.push(k, z)),
            None => OptNodeId::NONE,
        };
        let arg_ids: Vec<_> = arg_ints
            .iter()
            .map(|&n| b.push(NodeKind::Int(n), z))
            .collect();
        let args = b.push_list(&arg_ids);
        let method = b.intern_symbol(selector);
        let root = b.push(
            NodeKind::Send {
                receiver,
                method,
                args,
            },
            z,
        );
        (b.finish(root), root)
    }

    #[test]
    fn call_receiver_and_arguments_resolve_send_parts() {
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };

        // `foo(1, 2)` — receiverless, two args.
        let (ast, call) = build_call_with(None, "foo", &[1, 2]);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.call_receiver(call).get().is_none());
        let args = cx.call_arguments(call);
        assert_eq!(args.len(), 2);
        assert!(cx.has_call_arguments(call));
        assert_eq!(cx.first_argument(call).get(), Some(args[0]));
        assert_eq!(cx.last_argument(call).get(), Some(args[1]));

        // `self.bar` — self receiver, no args.
        let (ast, call) = build_call_with(Some(NodeKind::SelfExpr), "bar", &[]);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.call_receiver(call).get().is_some());
        assert!(!cx.has_call_arguments(call));
        assert!(cx.first_argument(call).get().is_none());
        assert!(cx.last_argument(call).get().is_none());
    }

    #[test]
    fn self_and_const_receiver_predicates() {
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };

        // `self.foo`
        let (ast, call) = build_call_with(Some(NodeKind::SelfExpr), "foo", &[]);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.is_self_receiver(call));
        assert!(!cx.is_const_receiver(call));

        // `Foo.bar` — const receiver.
        let const_name = {
            let mut b = AstBuilder::new("x".to_string(), "t.rb".to_string());
            b.intern_symbol("Foo")
        };
        let (ast, call) = build_call_with(
            Some(NodeKind::Const {
                scope: OptNodeId::NONE,
                name: const_name,
            }),
            "bar",
            &[],
        );
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.is_const_receiver(call));
        assert!(!cx.is_self_receiver(call));

        // Receiverless send is neither.
        let (ast, call) = build_call_with(None, "foo", &[]);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(!cx.is_self_receiver(call));
        assert!(!cx.is_const_receiver(call));
    }

    #[test]
    fn command_and_negation_predicates() {
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };

        // `foo` — receiverless ⇒ command?("foo") true, command?("bar") false.
        let (ast, call) = build_call_with(None, "foo", &[]);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.is_command(call, "foo"));
        assert!(!cx.is_command(call, "bar"));
        assert!(!cx.is_negation_method(call));

        // `self.foo` — has a receiver ⇒ not a command.
        let (ast, call) = build_call_with(Some(NodeKind::SelfExpr), "foo", &[]);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(!cx.is_command(call, "foo"));

        // `x.!` — receiver + `!` selector ⇒ negation_method?.
        let (ast, call) = build_call_with(Some(NodeKind::SelfExpr), "!", &[]);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.is_negation_method(call));
        // Bare `!` with no receiver is not a negation method.
        let (ast, call) = build_call_with(None, "!", &[]);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(!cx.is_negation_method(call));
    }

    #[test]
    fn literal_predicate_matches_literal_node_kinds() {
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };

        // An Int literal.
        let mut b = AstBuilder::new("42".to_string(), "t.rb".to_string());
        let root = b.push(NodeKind::Int(42), Range { start: 0, end: 2 });
        let ast = b.finish(root);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.is_literal(root));

        // A `nil` literal.
        let mut b = AstBuilder::new("nil".to_string(), "t.rb".to_string());
        let root = b.push(NodeKind::Nil, Range { start: 0, end: 3 });
        let ast = b.finish(root);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.is_literal(root));

        // A Send is not a literal.
        let (ast, call) = build_call_with(None, "foo", &[]);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(!cx.is_literal(call));
    }

    #[test]
    fn single_and_multiline_count_expression_lines() {
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };

        // `42` — one line.
        let mut b = AstBuilder::new("42".to_string(), "t.rb".to_string());
        let root = b.push(NodeKind::Int(42), Range { start: 0, end: 2 });
        let ast = b.finish(root);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.is_single_line(root));
        assert!(!cx.is_multiline(root));

        // `[\n1,\n2,\n]` — the Array expression spans four lines.
        let src = "[\n1,\n2,\n]";
        let mut b = AstBuilder::new(src.to_string(), "t.rb".to_string());
        let one = b.push(NodeKind::Int(1), Range { start: 2, end: 3 });
        let two = b.push(NodeKind::Int(2), Range { start: 5, end: 6 });
        let elems = b.push_list(&[one, two]);
        let root = b.push(
            NodeKind::Array(elems),
            Range {
                start: 0,
                end: src.len() as u32,
            },
        );
        let ast = b.finish(root);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert!(cx.is_multiline(root));
        assert!(!cx.is_single_line(root));
    }

    #[test]
    fn if_node_accessors_project_branches() {
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let z = Range { start: 0, end: 1 };
        let mut b = AstBuilder::new("x".to_string(), "t.rb".to_string());
        let cond = b.push(NodeKind::True_, z);
        let then_ = b.push(NodeKind::Int(1), z);
        let else_ = b.push(NodeKind::Int(2), z);
        let iff = b.push(
            NodeKind::If {
                cond,
                then_: OptNodeId::some(then_),
                else_: OptNodeId::some(else_),
            },
            z,
        );
        let ast = b.finish(iff);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert_eq!(cx.if_condition(iff).get(), Some(cond));
        assert_eq!(cx.if_then_branch(iff).get(), Some(then_));
        assert_eq!(cx.if_else_branch(iff).get(), Some(else_));
        // Non-If node projects to NONE.
        assert!(cx.if_condition(then_).get().is_none());
    }

    #[test]
    fn hash_and_pair_accessors_project_children() {
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let z = Range { start: 0, end: 1 };
        let mut b = AstBuilder::new("x".to_string(), "t.rb".to_string());
        let key = b.intern_symbol("k");
        let key_node = b.push(NodeKind::Sym(key), z);
        let val_node = b.push(NodeKind::Int(7), z);
        let pair = b.push(
            NodeKind::Pair {
                key: key_node,
                value: val_node,
            },
            z,
        );
        // `{ **h, k => 7 }` — a kwsplat plus a pair. `pairs` must return
        // only the pair (faithful to RuboCop's `each_child_node(:pair)`),
        // excluding the kwsplat — the shape `{**h}` -> (hash (kwsplat …))
        // confirmed via `murphy ast`.
        let h_recv = b.push(
            NodeKind::Send {
                receiver: OptNodeId::NONE,
                method: key,
                args: murphy_ast::NodeList::EMPTY,
            },
            z,
        );
        let kwsplat = b.push(NodeKind::Kwsplat(OptNodeId::some(h_recv)), z);
        let pairs = b.push_list(&[kwsplat, pair]);
        let hash = b.push(NodeKind::Hash(pairs), z);
        let ast = b.finish(hash);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert_eq!(cx.hash_pairs(hash), vec![pair]);
        assert_eq!(cx.children(hash).len(), 2, "children includes the kwsplat");
        assert_eq!(cx.pair_key(pair).get(), Some(key_node));
        assert_eq!(cx.pair_value(pair).get(), Some(val_node));
        // Non-matching kinds project empty.
        assert!(cx.hash_pairs(pair).is_empty());
        assert!(cx.pair_key(hash).get().is_none());
    }

    #[test]
    fn block_accessors_project_call_args_body() {
        let fns = FnTable {
            emit_offense: noop_offense,
            emit_edit: noop_edit,
        };
        let z = Range { start: 0, end: 1 };
        let mut b = AstBuilder::new("x".to_string(), "t.rb".to_string());
        let method = b.intern_symbol("each");
        let call = b.push(
            NodeKind::Send {
                receiver: OptNodeId::NONE,
                method,
                args: murphy_ast::NodeList::EMPTY,
            },
            z,
        );
        let args = b.push(NodeKind::Args(murphy_ast::NodeList::EMPTY), z);
        let body = b.push(NodeKind::Int(1), z);
        let block = b.push(
            NodeKind::Block {
                call,
                args,
                body: OptNodeId::some(body),
            },
            z,
        );
        let ast = b.finish(block);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        assert_eq!(cx.block_call(block).get(), Some(call));
        assert_eq!(cx.block_arguments(block).get(), Some(args));
        assert_eq!(cx.block_body(block).get(), Some(body));
        // Non-Block node projects to NONE.
        assert!(cx.block_call(body).get().is_none());
    }
}
