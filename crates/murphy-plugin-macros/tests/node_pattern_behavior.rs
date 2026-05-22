//! Behaviour tests for `node_pattern!`: define matchers, build a small
//! arena, run them, assert the result and captures.

// `NodeId` / `NodeList` / `OptNodeId` / `Symbol` are unused by the
// wildcard-only Task 2 tests but build the arenas later murphy-9cr.18
// tasks add to this file; keep the full import set stable.
#[allow(unused_imports)]
use murphy_ast::{Ast, AstBuilder, NodeId, NodeKind, NodeList, OptNodeId, Range, Symbol};
use murphy_plugin_api::{Cx, CxRaw, FnTable, RawSlice};

unsafe extern "C" fn noop_offense(
    _: *mut std::ffi::c_void,
    _: *const murphy_plugin_api::RawOffense,
) {
}
unsafe extern "C" fn noop_edit(_: *mut std::ffi::c_void, _: *const murphy_plugin_api::RawEdit) {}

/// A `FnTable` whose callbacks are never invoked by matcher code.
fn fns() -> FnTable {
    FnTable {
        emit_offense: noop_offense,
        emit_edit: noop_edit,
    }
}

/// Build a `CxRaw` borrowing `ast` and `fns` for `'a`.
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

fn r() -> Range {
    Range { start: 0, end: 1 }
}

use murphy_plugin_macros::node_pattern;

node_pattern!(any_node, "_");

#[test]
fn wildcard_matches_any_node() {
    let mut b = AstBuilder::new("nil", "t.rb");
    let root = b.push(NodeKind::Nil, r());
    let ast = b.finish(root);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    // Zero captures -> the matcher returns `bool`.
    assert!(any_node(root, &cx));
}

node_pattern!(is_int_42, "42");
node_pattern!(is_any_int, "int");
node_pattern!(is_sym_foo, ":foo");
node_pattern!(is_true_lit, "true");
node_pattern!(is_nil_node, "nil");
node_pattern!(is_nil_test, "nil?");

#[test]
fn literal_and_kind_matching() {
    let mut b = AstBuilder::new("src", "t.rb");
    let i42 = b.push(NodeKind::Int(42), r());
    let i7 = b.push(NodeKind::Int(7), r());
    let sym_foo = {
        let s = b.intern_symbol("foo");
        b.push(NodeKind::Sym(s), r())
    };
    let tru = b.push(NodeKind::True_, r());
    let niln = b.push(NodeKind::Nil, r());
    // Root just needs to own the others; a Begin list keeps them reachable.
    let list = b.push_list(&[i42, i7, sym_foo, tru, niln]);
    let root = b.push(NodeKind::Begin(list), r());
    let ast = b.finish(root);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    assert!(is_int_42(i42, &cx));
    assert!(!is_int_42(i7, &cx));
    assert!(is_any_int(i42, &cx) && is_any_int(i7, &cx));
    assert!(!is_any_int(tru, &cx));
    assert!(is_sym_foo(sym_foo, &cx));
    assert!(is_true_lit(tru, &cx));
    assert!(!is_true_lit(niln, &cx));
    assert!(is_nil_node(niln, &cx));
    assert!(is_nil_test(niln, &cx));
    assert!(!is_nil_test(i42, &cx));
}

node_pattern!(is_nilrecv_foo, "(send nil :foo)");
node_pattern!(is_nested, "(send (send nil :a) :b)");

/// Build `nil.foo` and return (ast-owning) root id.
fn build_nil_dot_foo() -> Ast {
    let mut b = AstBuilder::new("nil.foo", "t.rb");
    let recv = b.push(NodeKind::Nil, r());
    let m = b.intern_symbol("foo");
    let send = b.push(
        NodeKind::Send {
            receiver: OptNodeId::some(recv),
            method: m,
            args: NodeList::EMPTY,
        },
        r(),
    );
    b.finish(send)
}

#[test]
fn node_match_head_exact() {
    let ast = build_nil_dot_foo();
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };
    assert!(is_nilrecv_foo(ast.root(), &cx));

    // `(nil.a).b` — nested send.
    let mut b = AstBuilder::new("nil.a.b", "t.rb");
    let recv = b.push(NodeKind::Nil, r());
    let a = b.intern_symbol("a");
    let inner = b.push(
        NodeKind::Send {
            receiver: OptNodeId::some(recv),
            method: a,
            args: NodeList::EMPTY,
        },
        r(),
    );
    let bb = b.intern_symbol("b");
    let outer = b.push(
        NodeKind::Send {
            receiver: OptNodeId::some(inner),
            method: bb,
            args: NodeList::EMPTY,
        },
        r(),
    );
    let ast2 = b.finish(outer);
    let raw2 = cx_raw_for(&ast2, &fns);
    let cx2 = unsafe { Cx::from_raw(&raw2) };
    assert!(is_nested(ast2.root(), &cx2));
    assert!(!is_nested(inner_of(&ast2), &cx2)); // inner is (send nil :a), not nested
}

/// The inner `(send nil :a)` of the `nil.a.b` fixture.
fn inner_of(ast: &Ast) -> NodeId {
    let NodeKind::Send { receiver, .. } = *ast.kind(ast.root()) else {
        panic!()
    };
    receiver.get().unwrap()
}

node_pattern!(is_if, "(if _ _ _)");
node_pattern!(is_top_const, "(const nil? :Foo)");
node_pattern!(is_array2, "(array 1 2)");
node_pattern!(is_send_or_csend, "({send csend} ...)");
node_pattern!(is_any_paren, "(_ ...)");

#[test]
fn schema_table_and_flexible_heads() {
    let mut b = AstBuilder::new("src", "t.rb");
    // if: cond/then/else all Int
    let c = b.push(NodeKind::Int(0), r());
    let t = b.push(NodeKind::Int(1), r());
    let e = b.push(NodeKind::Int(2), r());
    let iff = b.push(
        NodeKind::If {
            cond: c,
            then_: OptNodeId::some(t),
            else_: OptNodeId::some(e),
        },
        r(),
    );
    // const Foo (no scope)
    let foo = b.intern_symbol("Foo");
    let kons = b.push(
        NodeKind::Const {
            scope: OptNodeId::NONE,
            name: foo,
        },
        r(),
    );
    // [1, 2]
    let a1 = b.push(NodeKind::Int(1), r());
    let a2 = b.push(NodeKind::Int(2), r());
    let alist = b.push_list(&[a1, a2]);
    let arr = b.push(NodeKind::Array(alist), r());
    // bare puts call: (send nil :puts) with no receiver
    let puts = b.intern_symbol("puts");
    let snd = b.push(
        NodeKind::Send {
            receiver: OptNodeId::NONE,
            method: puts,
            args: NodeList::EMPTY,
        },
        r(),
    );
    let list = b.push_list(&[iff, kons, arr, snd]);
    let root = b.push(NodeKind::Begin(list), r());
    let ast = b.finish(root);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    assert!(is_if(iff, &cx));
    assert!(is_top_const(kons, &cx));
    assert!(is_array2(arr, &cx));
    assert!(is_send_or_csend(snd, &cx));
    assert!(!is_send_or_csend(iff, &cx));
    assert!(is_any_paren(iff, &cx) && is_any_paren(arr, &cx));
}
