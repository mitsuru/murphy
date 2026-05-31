//! Behaviour tests for `def_node_matcher!`: define matchers, build a small
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
        sorted_tokens: p.sorted_tokens.as_ptr(),
        sorted_tokens_len: p.sorted_tokens.len(),
        options_json: RawSlice::from_str("{}"),
        call_closing_locs: p.call_closing_locs.as_ptr(),
        call_closing_locs_len: p.call_closing_locs.len(),
        call_operator_locs: p.call_operator_locs.as_ptr(),
        call_operator_locs_len: p.call_operator_locs.len(),
        var_model: std::ptr::null(),
        node_slice_arena: std::ptr::null_mut(),
        alloc_node_slice: murphy_plugin_api::unavailable_alloc_node_slice,
    }
}

fn r() -> Range {
    Range { start: 0, end: 1 }
}

use murphy_plugin_macros::def_node_matcher;

def_node_matcher!(any_node, "_");

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

def_node_matcher!(is_int_42, "42");
def_node_matcher!(is_any_int, "int");
def_node_matcher!(is_sym_foo, ":foo");
def_node_matcher!(is_true_lit, "true");
def_node_matcher!(is_nil_node, "nil");
def_node_matcher!(is_nil_test, "nil?");

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

def_node_matcher!(is_nilrecv_foo, "(send nil :foo)");
def_node_matcher!(is_nested, "(send (send nil :a) :b)");

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

def_node_matcher!(is_if, "(if _ _ _)");
def_node_matcher!(is_top_const, "(const nil? :Foo)");
def_node_matcher!(is_array2, "(array 1 2)");
def_node_matcher!(is_send_or_csend, "({send csend} ...)");
def_node_matcher!(is_any_paren, "(_ ...)");

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

def_node_matcher!(cap_receiver, "(send $_ :foo)");
def_node_matcher!(cap_subpat, "$(send nil :foo)");
def_node_matcher!(cap_two, "(if $_ $_ _)");

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

def_node_matcher!(cap_args, "(send nil? :foo $...)");
def_node_matcher!(rest_then_cap, "(array ... $_)");
def_node_matcher!(cap_then_rest, "(array $_ ...)");

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

def_node_matcher!(mid_bare, "(array $_ ... $_)");
def_node_matcher!(mid_cap, "(array $_ $... $_)");

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

def_node_matcher!(rest_only, "(array ...)");

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

def_node_matcher!(is_send_or_int, "{send int}");
def_node_matcher!(not_nil, "!nil");
def_node_matcher!(send_nonnil_recv, "(send !nil? :foo)");

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
def_node_matcher!(is_big_int, "#is_big");
def_node_matcher!(big_or_nil, "{#is_big nil}");

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

// `#odd?` / `#save!` — predicate names carrying a Ruby-style `?` / `!`
// suffix (murphy-bj7). The macro mangles the call site: `?` → `_p`
// (predicate, mruby convention), `!` → `_bang`. `#save` and `#save?`
// resolve to *different* Rust fns, mirroring how Ruby's `save` and
// `save?` are distinct methods (the use case from murphy-bj7).
def_node_matcher!(is_odd_int, "#odd?");
def_node_matcher!(is_bang_int, "#bang!");

fn odd_p(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(*cx.kind(node), NodeKind::Int(v) if v % 2 != 0)
}

fn bang_bang(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(*cx.kind(node), NodeKind::Int(v) if v == 42)
}

#[test]
fn predicate_question_suffix_mangles_to_p_call() {
    let mut b = AstBuilder::new("src", "t.rb");
    let odd = b.push(NodeKind::Int(3), r());
    let even = b.push(NodeKind::Int(4), r());
    let ast = b.finish(odd);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };
    assert!(is_odd_int(odd, &cx));
    assert!(!is_odd_int(even, &cx));
}

#[test]
fn predicate_bang_suffix_mangles_to_bang_call() {
    let mut b = AstBuilder::new("src", "t.rb");
    let hit = b.push(NodeKind::Int(42), r());
    let miss = b.push(NodeKind::Int(0), r());
    let ast = b.finish(hit);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };
    assert!(is_bang_int(hit, &cx));
    assert!(!is_bang_int(miss, &cx));
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

def_node_matcher!(parent_is_if, "^if");
def_node_matcher!(has_nil_descendant, "`nil");

#[test]
fn parent_and_descendant() {
    // if(cond=nil, then=int, else=none)
    let mut b = AstBuilder::new("src", "t.rb");
    let cond = b.push(NodeKind::Nil, r());
    let then_ = b.push(NodeKind::Int(1), r());
    let iff = b.push(
        NodeKind::If {
            cond,
            then_: OptNodeId::some(then_),
            else_: OptNodeId::NONE,
        },
        r(),
    );
    let ast = b.finish(iff);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    // cond's parent is the if node.
    assert!(parent_is_if(cond, &cx));
    assert!(!parent_is_if(iff, &cx)); // if has no parent
    // the if subtree contains a nil descendant.
    assert!(has_nil_descendant(iff, &cx));
    assert!(!has_nil_descendant(then_, &cx)); // an Int leaf has none
}

// ── Assignment variants ────────────────────────────────────────────────
// `Lvasgn` / `Ivasgn` / `Gvasgn` / `Cvasgn` share the `{ name, value }`
// schema; `Casgn` has `{ scope, name, value }`. Verifying each variant
// guards against schema drift (Sym-in-slot-0 vs slot-1, scope vs no-scope).
def_node_matcher!(is_lvasgn_x, "(lvasgn :x _)");
def_node_matcher!(is_ivasgn_at_x, "(ivasgn :@x _)");
def_node_matcher!(is_casgn_top_foo, "(casgn nil? :Foo _)");
def_node_matcher!(is_gvasgn_dollar_x, "(gvasgn :$x _)");
def_node_matcher!(is_cvasgn_atat_x, "(cvasgn :@@x _)");

#[test]
fn assignment_variants_match_each_kind() {
    let mut b = AstBuilder::new("src", "t.rb");
    let one = b.push(NodeKind::Int(1), r());
    let lv_name = b.intern_symbol("x");
    let lvasgn = b.push(
        NodeKind::Lvasgn {
            name: lv_name,
            value: OptNodeId::some(one),
        },
        r(),
    );
    let iv_one = b.push(NodeKind::Int(1), r());
    let iv_name = b.intern_symbol("@x");
    let ivasgn = b.push(
        NodeKind::Ivasgn {
            name: iv_name,
            value: OptNodeId::some(iv_one),
        },
        r(),
    );
    let ca_one = b.push(NodeKind::Int(1), r());
    let ca_name = b.intern_symbol("Foo");
    let casgn = b.push(
        NodeKind::Casgn {
            scope: OptNodeId::NONE,
            name: ca_name,
            value: OptNodeId::some(ca_one),
        },
        r(),
    );
    let gv_one = b.push(NodeKind::Int(1), r());
    let gv_name = b.intern_symbol("$x");
    let gvasgn = b.push(
        NodeKind::Gvasgn {
            name: gv_name,
            value: OptNodeId::some(gv_one),
        },
        r(),
    );
    let cv_one = b.push(NodeKind::Int(1), r());
    let cv_name = b.intern_symbol("@@x");
    let cvasgn = b.push(
        NodeKind::Cvasgn {
            name: cv_name,
            value: OptNodeId::some(cv_one),
        },
        r(),
    );
    let list = b.push_list(&[lvasgn, ivasgn, casgn, gvasgn, cvasgn]);
    let root = b.push(NodeKind::Begin(list), r());
    let ast = b.finish(root);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    assert!(is_lvasgn_x(lvasgn, &cx));
    assert!(!is_lvasgn_x(ivasgn, &cx)); // wrong kind
    assert!(is_ivasgn_at_x(ivasgn, &cx));
    assert!(!is_ivasgn_at_x(lvasgn, &cx));
    assert!(is_casgn_top_foo(casgn, &cx));
    assert!(!is_casgn_top_foo(lvasgn, &cx)); // wrong kind
    assert!(is_gvasgn_dollar_x(gvasgn, &cx));
    assert!(!is_gvasgn_dollar_x(cvasgn, &cx));
    assert!(is_cvasgn_atat_x(cvasgn, &cx));
    assert!(!is_cvasgn_atat_x(gvasgn, &cx));
}

// ── Block / Hash / Pair ────────────────────────────────────────────────
// `Block` has all-required fields (`call: Node`, `args: Node`, `body:
// OptNode`); `Hash` is a single-list tuple like `Array`; `Pair` is two
// required `Node` fields.
def_node_matcher!(is_block_each, "(block (send nil? :each) _ _)");
def_node_matcher!(is_hash_one_pair, "(hash (pair _ _))");
def_node_matcher!(is_pair_key_a, "(pair :a _)");

#[test]
fn block_hash_pair_variants() {
    let mut b = AstBuilder::new("src", "t.rb");
    // `each { ... }` shaped as a Block over a Send.
    let each = b.intern_symbol("each");
    let call = b.push(
        NodeKind::Send {
            receiver: OptNodeId::NONE,
            method: each,
            args: NodeList::EMPTY,
        },
        r(),
    );
    let args = b.push(NodeKind::Args(NodeList::EMPTY), r());
    let body = b.push(NodeKind::Int(1), r());
    let blk = b.push(
        NodeKind::Block {
            call,
            args,
            body: OptNodeId::some(body),
        },
        r(),
    );
    // `{:a => 1}` shaped as a Hash with one Pair.
    let key = {
        let s = b.intern_symbol("a");
        b.push(NodeKind::Sym(s), r())
    };
    let val = b.push(NodeKind::Int(1), r());
    let pair = b.push(NodeKind::Pair { key, value: val }, r());
    let pairs = b.push_list(&[pair]);
    let hash = b.push(NodeKind::Hash(pairs), r());
    // Standalone pair to test bare `(pair :a _)` against a Pair root.
    let key2 = {
        let s = b.intern_symbol("a");
        b.push(NodeKind::Sym(s), r())
    };
    let val2 = b.push(NodeKind::Int(2), r());
    let pair2 = b.push(
        NodeKind::Pair {
            key: key2,
            value: val2,
        },
        r(),
    );
    let key3 = {
        let s = b.intern_symbol("b");
        b.push(NodeKind::Sym(s), r())
    };
    let val3 = b.push(NodeKind::Int(3), r());
    let pair3 = b.push(
        NodeKind::Pair {
            key: key3,
            value: val3,
        },
        r(),
    );
    let list = b.push_list(&[blk, hash, pair2, pair3]);
    let root = b.push(NodeKind::Begin(list), r());
    let ast = b.finish(root);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    assert!(is_block_each(blk, &cx));
    assert!(!is_block_each(hash, &cx));
    assert!(is_hash_one_pair(hash, &cx));
    assert!(!is_hash_one_pair(blk, &cx));
    assert!(is_pair_key_a(pair2, &cx));
    assert!(!is_pair_key_a(pair3, &cx)); // key is :b
}

// ── Case / When ────────────────────────────────────────────────────────
// `Case` and `When` both have `covers_all_fields=false`: the generated
// destructure ends with `..` so `Case::else_` / `When::body` (which follow
// the trailing `NodeList`) stay out of the schema. The macro expansion
// itself is the compile-time guard; the runtime check is a smoke test.
def_node_matcher!(is_case_any, "(case _ ...)");
def_node_matcher!(is_when_any, "(when ...)");

#[test]
fn case_and_when_trailing_dotdot_paths() {
    let mut b = AstBuilder::new("src", "t.rb");
    let subj = b.push(NodeKind::Int(0), r());
    let cond = b.push(NodeKind::Int(1), r());
    let body = b.push(NodeKind::Int(2), r());
    let conds = b.push_list(&[cond]);
    let wh = b.push(
        NodeKind::When {
            conds,
            body: OptNodeId::some(body),
        },
        r(),
    );
    let whens = b.push_list(&[wh]);
    // `else_` is present but the pattern schema must ignore it (the `..`
    // path in the generated destructure).
    let else_node = b.push(NodeKind::Int(3), r());
    let cs = b.push(
        NodeKind::Case {
            subject: OptNodeId::some(subj),
            whens,
            else_: OptNodeId::some(else_node),
        },
        r(),
    );
    let list = b.push_list(&[cs]);
    let root = b.push(NodeKind::Begin(list), r());
    let ast = b.finish(root);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    assert!(is_case_any(cs, &cx));
    assert!(!is_case_any(wh, &cx));
    assert!(is_when_any(wh, &cx));
    assert!(!is_when_any(cs, &cx));
}

// ── Return — Pos(1,0) OptNode tuple variant destructure ────────────────
// `Return(OptNodeId)` is the only `Pos`-tuple variant whose single slot is
// an `OptNode`. The two branches of `lower_fixed_slot`'s `OptNode` arm
// (`Some(n) => …` and `None => {}` via `nil?`) must both run.
def_node_matcher!(is_return_any, "(return _)");
def_node_matcher!(is_return_nil_test, "(return nil?)");

#[test]
fn return_optnode_both_branches() {
    let mut b = AstBuilder::new("src", "t.rb");
    let v = b.push(NodeKind::Int(1), r());
    let ret_some = b.push(NodeKind::Return(OptNodeId::some(v)), r());
    let ret_none = b.push(NodeKind::Return(OptNodeId::NONE), r());
    let list = b.push_list(&[ret_some, ret_none]);
    let root = b.push(NodeKind::Begin(list), r());
    let ast = b.finish(root);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    // `(return _)` requires `Some(..)` — present value branch.
    assert!(is_return_any(ret_some, &cx));
    assert!(!is_return_any(ret_none, &cx));
    // `(return nil?)` matches both `None` and `Some(Nil)`; here `None`
    // exercises the `None => {}` arm.
    assert!(is_return_nil_test(ret_none, &cx));
    assert!(!is_return_nil_test(ret_some, &cx)); // value present and not Nil
}

// ── And / Or ───────────────────────────────────────────────────────────
def_node_matcher!(is_and_two, "(and _ _)");
def_node_matcher!(is_or_two, "(or _ _)");

#[test]
fn and_or_two_node_slots() {
    let mut b = AstBuilder::new("src", "t.rb");
    let lhs = b.push(NodeKind::Int(1), r());
    let rhs = b.push(NodeKind::Int(2), r());
    let and_ = b.push(NodeKind::And { lhs, rhs }, r());
    let lhs2 = b.push(NodeKind::Int(3), r());
    let rhs2 = b.push(NodeKind::Int(4), r());
    let or_ = b.push(
        NodeKind::Or {
            lhs: lhs2,
            rhs: rhs2,
        },
        r(),
    );
    let list = b.push_list(&[and_, or_]);
    let root = b.push(NodeKind::Begin(list), r());
    let ast = b.finish(root);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    assert!(is_and_two(and_, &cx));
    assert!(!is_and_two(or_, &cx));
    assert!(is_or_two(or_, &cx));
    assert!(!is_or_two(and_, &cx));
}

// ── Def / Class / Module ───────────────────────────────────────────────
// `Def` has `covers_all_fields=false` (omits `receiver`). `Class` and
// `Module` are fully covered.
def_node_matcher!(is_def_foo, "(def :foo _ nil?)");
def_node_matcher!(is_class_any, "(class _ nil? nil?)");
def_node_matcher!(is_module_any, "(module _ nil?)");

#[test]
fn def_class_module_variants() {
    let mut b = AstBuilder::new("src", "t.rb");
    // `def foo(); end` — no receiver, empty args, empty body.
    let def_name = b.intern_symbol("foo");
    let def_args = b.push(NodeKind::Args(NodeList::EMPTY), r());
    let def_ = b.push(
        NodeKind::Def {
            receiver: OptNodeId::NONE,
            name: def_name,
            args: def_args,
            body: OptNodeId::NONE,
        },
        r(),
    );
    // `class Foo; end` — `Const Foo`, no superclass, no body.
    let cls_name_sym = b.intern_symbol("Foo");
    let cls_name = b.push(
        NodeKind::Const {
            scope: OptNodeId::NONE,
            name: cls_name_sym,
        },
        r(),
    );
    let cls = b.push(
        NodeKind::Class {
            name: cls_name,
            superclass: OptNodeId::NONE,
            body: OptNodeId::NONE,
        },
        r(),
    );
    // `module Bar; end` — `Const Bar`, no body.
    let mod_name_sym = b.intern_symbol("Bar");
    let mod_name = b.push(
        NodeKind::Const {
            scope: OptNodeId::NONE,
            name: mod_name_sym,
        },
        r(),
    );
    let mdl = b.push(
        NodeKind::Module {
            name: mod_name,
            body: OptNodeId::NONE,
        },
        r(),
    );
    let list = b.push_list(&[def_, cls, mdl]);
    let root = b.push(NodeKind::Begin(list), r());
    let ast = b.finish(root);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    assert!(is_def_foo(def_, &cx));
    assert!(!is_def_foo(cls, &cx));
    assert!(is_class_any(cls, &cx));
    assert!(!is_class_any(mdl, &cx));
    assert!(is_module_any(mdl, &cx));
    assert!(!is_module_any(cls, &cx));
}

// ── While / Until ──────────────────────────────────────────────────────
// `While { cond, body, post: bool }` and `Until { ... }`: `post` is a flag,
// not a child slot — `covers_all_fields=false` and the macro emits a
// trailing `..` in the destructure.
def_node_matcher!(is_while_any, "(while _ _)");
def_node_matcher!(is_until_any, "(until _ _)");

#[test]
fn while_and_until_skip_post_flag() {
    let mut b = AstBuilder::new("src", "t.rb");
    let c1 = b.push(NodeKind::Int(1), r());
    let body1 = b.push(NodeKind::Int(2), r());
    // `post = true` should NOT affect matching — the schema ignores it.
    let wh = b.push(
        NodeKind::While {
            cond: c1,
            body: OptNodeId::some(body1),
            post: true,
        },
        r(),
    );
    let c2 = b.push(NodeKind::Int(3), r());
    let body2 = b.push(NodeKind::Int(4), r());
    let un = b.push(
        NodeKind::Until {
            cond: c2,
            body: OptNodeId::some(body2),
            post: false,
        },
        r(),
    );
    let list = b.push_list(&[wh, un]);
    let root = b.push(NodeKind::Begin(list), r());
    let ast = b.finish(root);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    assert!(is_while_any(wh, &cx));
    assert!(!is_while_any(un, &cx));
    assert!(is_until_any(un, &cx));
    assert!(!is_until_any(wh, &cx));
}

// ── Head::OneOf — actually exercise the csend branch ───────────────────
// The existing `is_send_or_csend` test only built a `Send`. This test
// builds an actual `Csend` node and asserts the OneOf head accepts it.
def_node_matcher!(is_send_or_csend_any, "({send csend} ...)");

#[test]
fn oneof_head_accepts_csend_arm() {
    let mut b = AstBuilder::new("src", "t.rb");
    let recv = b.push(NodeKind::Nil, r());
    let m = b.intern_symbol("foo");
    let cs = b.push(
        NodeKind::Csend {
            receiver: recv,
            method: m,
            args: NodeList::EMPTY,
        },
        r(),
    );
    let int_node = b.push(NodeKind::Int(7), r());
    let list = b.push_list(&[cs, int_node]);
    let root = b.push(NodeKind::Begin(list), r());
    let ast = b.finish(root);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    assert!(is_send_or_csend_any(cs, &cx));
    assert!(!is_send_or_csend_any(int_node, &cx));
}

// ── `$ident` and `$:sym` capture spellings ─────────────────────────────
// `$ident` is a named capture with an implicit Wildcard body — it binds
// any node id at its slot. `$:sym` is an anonymous capture whose body is
// a `Sym` literal — it binds the Sym node id when its body matches.
def_node_matcher!(cap_recv_ident, "(send $recv :foo)");
def_node_matcher!(cap_sym_lit_in_array, "(array $:foo)");

#[test]
fn named_ident_and_sym_literal_captures() {
    let mut b = AstBuilder::new("src", "t.rb");
    let recv = b.push(NodeKind::Int(7), r());
    let m = b.intern_symbol("foo");
    let send = b.push(
        NodeKind::Send {
            receiver: OptNodeId::some(recv),
            method: m,
            args: NodeList::EMPTY,
        },
        r(),
    );
    // `[:foo]` and `[:bar]` — the second must not match `$:foo`.
    let foo_sym_id = b.intern_symbol("foo");
    let foo_node = b.push(NodeKind::Sym(foo_sym_id), r());
    let foo_arr_list = b.push_list(&[foo_node]);
    let foo_arr = b.push(NodeKind::Array(foo_arr_list), r());
    let bar_sym_id = b.intern_symbol("bar");
    let bar_node = b.push(NodeKind::Sym(bar_sym_id), r());
    let bar_arr_list = b.push_list(&[bar_node]);
    let bar_arr = b.push(NodeKind::Array(bar_arr_list), r());
    let list = b.push_list(&[send, foo_arr, bar_arr]);
    let root = b.push(NodeKind::Begin(list), r());
    let ast = b.finish(root);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    // `$recv` binds the Int(7) node id.
    assert_eq!(cap_recv_ident(send, &cx), Some((recv,)));
    // `$:foo` matches and binds the Sym :foo node; `[:bar]` doesn't match.
    assert_eq!(cap_sym_lit_in_array(foo_arr, &cx), Some((foo_node,)));
    assert_eq!(cap_sym_lit_in_array(bar_arr, &cx), None);
}

// ── Float / Str / False literals ───────────────────────────────────────
// `is_true_lit` and `is_nil_node` already cover `True_` / `Nil`. Add the
// remaining literal lowerings: `Float`, `Str`, `False_`.
def_node_matcher!(is_float_one_five, "1.5");
def_node_matcher!(is_str_hello, "\"hello\"");
def_node_matcher!(is_false_lit, "false");

// ── Predicate with string-literal arg: `#starts_with?("foo")` ─────────
// B-backend acceptance criterion (murphy-jyi §2): `Lit::Str` arg is lowered
// to a `&str` literal. The generated call is:
//   `starts_with_p(node, cx, "foo")`
def_node_matcher!(is_starts_with_foo, "#starts_with?(\"foo\")");

fn starts_with_p(node: NodeId, cx: &Cx<'_>, prefix: &str) -> bool {
    if let NodeKind::Str(id) = *cx.kind(node) {
        cx.string_str(id).starts_with(prefix)
    } else {
        false
    }
}

// ── Predicate with symbol-literal arg: `#sym_eq?(:foo)` ───────────────
// B-backend acceptance criterion (murphy-jyi §3): `Lit::Sym` arg is lowered
// to a `&str` literal (the symbol name without the `:` prefix). The generated
// call is:
//   `sym_eq_p(node, cx, "foo")`
def_node_matcher!(is_sym_eq_foo, "#sym_eq?(:foo)");

fn sym_eq_p(node: NodeId, cx: &Cx<'_>, expected: &str) -> bool {
    if let NodeKind::Sym(sym) = *cx.kind(node) {
        cx.symbol_str(sym) == expected
    } else {
        false
    }
}

#[test]
fn float_str_false_literal_lowerings() {
    let mut b = AstBuilder::new("src", "t.rb");
    let one_five = b.push(NodeKind::Float(1.5), r());
    let other = b.push(NodeKind::Float(2.5), r());
    let hello_id = b.intern_string("hello");
    let hello = b.push(NodeKind::Str(hello_id), r());
    let world_id = b.intern_string("world");
    let world = b.push(NodeKind::Str(world_id), r());
    let f = b.push(NodeKind::False_, r());
    let t = b.push(NodeKind::True_, r());
    let list = b.push_list(&[one_five, other, hello, world, f, t]);
    let root = b.push(NodeKind::Begin(list), r());
    let ast = b.finish(root);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    assert!(is_float_one_five(one_five, &cx));
    assert!(!is_float_one_five(other, &cx));
    assert!(is_str_hello(hello, &cx));
    assert!(!is_str_hello(world, &cx));
    assert!(is_false_lit(f, &cx));
    assert!(!is_false_lit(t, &cx));
}

#[test]
fn predicate_with_str_literal_arg_calls_fn_with_str_ref() {
    // `#starts_with?("foo")` — B backend lowers `Lit::Str("foo")` to a
    // `&str` literal and passes it as the third argument to `starts_with_p`.
    let mut b = AstBuilder::new("src", "t.rb");
    let foobar_id = b.intern_string("foobar");
    let foobar = b.push(NodeKind::Str(foobar_id), r());
    let xyz_id = b.intern_string("xyz");
    let xyz = b.push(NodeKind::Str(xyz_id), r());
    let list = b.push_list(&[foobar, xyz]);
    let root = b.push(NodeKind::Begin(list), r());
    let ast = b.finish(root);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    // "foobar".starts_with("foo") → true
    assert!(is_starts_with_foo(foobar, &cx));
    // "xyz".starts_with("foo") → false
    assert!(!is_starts_with_foo(xyz, &cx));
}

#[test]
fn predicate_with_sym_literal_arg_calls_fn_with_str_ref() {
    // `#sym_eq?(:foo)` — B backend lowers `Lit::Sym("foo")` to a `&str`
    // literal (stripping the `:` prefix) and passes it to `sym_eq_p`.
    let mut b = AstBuilder::new("src", "t.rb");
    let foo_sym = b.intern_symbol("foo");
    let foo_node = b.push(NodeKind::Sym(foo_sym), r());
    let bar_sym = b.intern_symbol("bar");
    let bar_node = b.push(NodeKind::Sym(bar_sym), r());
    let list = b.push_list(&[foo_node, bar_node]);
    let root = b.push(NodeKind::Begin(list), r());
    let ast = b.finish(root);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    // :foo == :foo → true
    assert!(is_sym_eq_foo(foo_node, &cx));
    // :bar == :foo → false
    assert!(!is_sym_eq_foo(bar_node, &cx));
}
