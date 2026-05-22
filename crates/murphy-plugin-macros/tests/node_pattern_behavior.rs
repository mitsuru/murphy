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
