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

node_pattern!(cap_receiver, "(send $_ :foo)");
node_pattern!(cap_subpat, "$(send nil :foo)");
node_pattern!(cap_two, "(if $_ $_ _)");

#[test]
fn node_captures_return_tuple() {
    let ast = build_nil_dot_foo(); // (send nil :foo), receiver = Nil node
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };
    let send = ast.root();
    let NodeKind::Send { receiver, .. } = *ast.kind(send) else {
        panic!()
    };
    let recv = receiver.get().unwrap();

    // $_ at the receiver slot binds the Nil node id.
    assert_eq!(cap_receiver(send, &cx), Some((recv,)));
    // anonymous $(...) capturing the whole send.
    assert_eq!(cap_subpat(send, &cx), Some((send,)));
    // a non-match returns None.
    assert_eq!(cap_receiver(recv, &cx), None);
}

node_pattern!(cap_args, "(send nil? :foo $...)");
node_pattern!(rest_then_cap, "(array ... $_)");
node_pattern!(cap_then_rest, "(array $_ ...)");

#[test]
fn seq_capture_and_rest() {
    let mut b = AstBuilder::new("src", "t.rb");
    // foo(1, 2, 3) with no receiver
    let a1 = b.push(NodeKind::Int(1), r());
    let a2 = b.push(NodeKind::Int(2), r());
    let a3 = b.push(NodeKind::Int(3), r());
    let args = b.push_list(&[a1, a2, a3]);
    let foo = b.intern_symbol("foo");
    let send = b.push(
        NodeKind::Send {
            receiver: OptNodeId::NONE,
            method: foo,
            args,
        },
        r(),
    );
    // Distinct nodes for the array — reusing a1/a2/a3 across two parents
    // would make `finish` overwrite their parent links.
    let e1 = b.push(NodeKind::Int(1), r());
    let e2 = b.push(NodeKind::Int(2), r());
    let e3 = b.push(NodeKind::Int(3), r());
    let earr = b.push_list(&[e1, e2, e3]);
    let arr = b.push(NodeKind::Array(earr), r());
    let list = b.push_list(&[send, arr]);
    let root = b.push(NodeKind::Begin(list), r());
    let ast = b.finish(root);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    // $... binds the whole args slice.
    assert_eq!(cap_args(send, &cx), Some((&[a1, a2, a3][..],)));
    // trailing capture after a leading rest.
    assert_eq!(rest_then_cap(arr, &cx), Some((e3,)));
    // leading capture before a trailing rest.
    assert_eq!(cap_then_rest(arr, &cx), Some((e1,)));
}

node_pattern!(mid_bare, "(array $_ ... $_)");
node_pattern!(mid_cap, "(array $_ $... $_)");

#[test]
fn mid_position_rest() {
    let mut b = AstBuilder::new("src", "t.rb");
    // [e1, e2, e3, e4] — a four-element array.
    let e1 = b.push(NodeKind::Int(1), r());
    let e2 = b.push(NodeKind::Int(2), r());
    let e3 = b.push(NodeKind::Int(3), r());
    let e4 = b.push(NodeKind::Int(4), r());
    let earr = b.push_list(&[e1, e2, e3, e4]);
    let arr = b.push(NodeKind::Array(earr), r());
    // [f1, f2] — a two-element array (empty middle).
    let f1 = b.push(NodeKind::Int(5), r());
    let f2 = b.push(NodeKind::Int(6), r());
    let farr = b.push_list(&[f1, f2]);
    let arr2 = b.push(NodeKind::Array(farr), r());
    // [g1] — a one-element array (too short for a prefix + suffix).
    let g1 = b.push(NodeKind::Int(7), r());
    let garr = b.push_list(&[g1]);
    let arr3 = b.push(NodeKind::Array(garr), r());
    let list = b.push_list(&[arr, arr2, arr3]);
    let root = b.push(NodeKind::Begin(list), r());
    let ast = b.finish(root);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    // Mid-position rest with non-empty prefix and suffix: leading/trailing
    // `$_` bind the edges, the `...`/`$...` covers everything between.
    assert_eq!(mid_bare(arr, &cx), Some((e1, e4)));
    assert_eq!(mid_cap(arr, &cx), Some((e1, &[e2, e3][..], e4)));
    // Empty middle: the seq capture binds an empty slice.
    assert_eq!(mid_cap(arr2, &cx), Some((f1, &[][..], f2)));
    // Too short: a single element cannot fill both a prefix and a suffix.
    assert_eq!(mid_cap(arr3, &cx), None);
    assert_eq!(mid_bare(arr3, &cx), None);
}

node_pattern!(rest_only, "(array ...)");

#[test]
fn bare_rest_only_list() {
    let mut b = AstBuilder::new("src", "t.rb");
    // [e1, e2] — a two-element array.
    let e1 = b.push(NodeKind::Int(1), r());
    let e2 = b.push(NodeKind::Int(2), r());
    let earr = b.push_list(&[e1, e2]);
    let arr = b.push(NodeKind::Array(earr), r());
    // [] — an empty array.
    let empty = b.push_list(&[]);
    let empty_arr = b.push(NodeKind::Array(empty), r());
    let list = b.push_list(&[arr, empty_arr]);
    let root = b.push(NodeKind::Begin(list), r());
    let ast = b.finish(root);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    // A bare `...` as the only child matches any length, including zero.
    assert!(rest_only(arr, &cx));
    assert!(rest_only(empty_arr, &cx));
}

node_pattern!(is_send_or_int, "{send int}");
node_pattern!(not_nil, "!nil");
node_pattern!(send_nonnil_recv, "(send !nil? :foo)");

#[test]
fn union_and_negation() {
    let mut b = AstBuilder::new("src", "t.rb");
    let i = b.push(NodeKind::Int(9), r());
    let niln = b.push(NodeKind::Nil, r());
    let foo = b.intern_symbol("foo");
    // `snd`: a `foo` send with NO receiver (`OptNodeId::NONE`).
    let snd = b.push(
        NodeKind::Send {
            receiver: OptNodeId::NONE,
            method: foo,
            args: NodeList::EMPTY,
        },
        r(),
    );
    // `nil_recv_send`: a `foo` send whose receiver IS a `Nil` node — the
    // `!nil?` negation must reject it.
    let nil_recv = b.push(NodeKind::Nil, r());
    let nil_recv_send = b.push(
        NodeKind::Send {
            receiver: OptNodeId::some(nil_recv),
            method: foo,
            args: NodeList::EMPTY,
        },
        r(),
    );
    // `int_recv_send`: a `foo` send whose receiver is a (non-`Nil`) `Int`
    // node — the `!nil?` negation must accept it.
    let int_recv = b.push(NodeKind::Int(3), r());
    let int_recv_send = b.push(
        NodeKind::Send {
            receiver: OptNodeId::some(int_recv),
            method: foo,
            args: NodeList::EMPTY,
        },
        r(),
    );
    let list = b.push_list(&[i, niln, snd, nil_recv_send, int_recv_send]);
    let root = b.push(NodeKind::Begin(list), r());
    let ast = b.finish(root);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    assert!(is_send_or_int(i, &cx) && is_send_or_int(snd, &cx));
    assert!(!is_send_or_int(niln, &cx));
    assert!(not_nil(i, &cx));
    assert!(!not_nil(niln, &cx));

    // `(send !nil? :foo)` — `!`-at-an-`OptNode`-slot runtime path.
    // Receiver IS a `nil` node: `!nil?` rejects it.
    assert!(!send_nonnil_recv(nil_recv_send, &cx));
    // Receiver is present and not a `nil` node: `!nil?` accepts it.
    assert!(send_nonnil_recv(int_recv_send, &cx));
    // No receiver at all: `!nil?` requires the receiver present, so reject.
    assert!(!send_nonnil_recv(snd, &cx));
}

// `#is_big` is a bare predicate applied directly to `node`. (`int` has no
// schema entry, so `(int #is_big)` would be an unsupported-kind error —
// the predicate is a child position, not a head.)
node_pattern!(is_big_int, "#is_big");
node_pattern!(big_or_nil, "{#is_big nil}");

/// User-provided predicate: a free fn in scope at the matcher call site.
/// Both `is_big_int` and `big_or_nil` resolve `#is_big` to this fn.
fn is_big(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(*cx.kind(node), NodeKind::Int(v) if v >= 100)
}

#[test]
fn predicate_calls_a_free_function() {
    let mut b = AstBuilder::new("src", "t.rb");
    let big = b.push(NodeKind::Int(500), r());
    let small = b.push(NodeKind::Int(3), r());
    let list = b.push_list(&[big, small]);
    let root = b.push(NodeKind::Begin(list), r());
    let ast = b.finish(root);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    assert!(is_big_int(big, &cx));
    assert!(!is_big_int(small, &cx));
}

#[test]
fn predicate_inside_union() {
    let mut b = AstBuilder::new("src", "t.rb");
    let big = b.push(NodeKind::Int(500), r());
    let niln = b.push(NodeKind::Nil, r());
    let small = b.push(NodeKind::Int(1), r());
    let list = b.push_list(&[big, niln, small]);
    let root = b.push(NodeKind::Begin(list), r());
    let ast = b.finish(root);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };
    assert!(big_or_nil(big, &cx) && big_or_nil(niln, &cx));
    assert!(!big_or_nil(small, &cx));
}
