//! B↔C cross-backend semantic conformance for `node_pattern!` /
//! `murphy_pattern::matches`.
//!
//! The two backends share one grammar (`murphy-pattern::parse`) and MUST
//! agree on whether a given pattern matches a given node, including the
//! capture slot values. The B-backend (`node_pattern!` proc macro,
//! murphy-9cr.18) lowers patterns to typed Rust at compile time; the
//! C-backend (`murphy_pattern::matches`, murphy-9cr.19) interprets
//! `PatternIr` at runtime. This file exercises a representative pattern
//! per backend feature against a small arena and asserts both backends
//! reach the same yes/no decision and (where applicable) the same
//! captured node id.
//!
//! When a new backend feature lands, add a paired matcher here — that is
//! the explicit drift guard for design §4 ("1 grammar, 2 backends").

use murphy_ast::{Ast, AstBuilder, NodeId, NodeKind, OptNodeId, Range};
use murphy_pattern::{CaptureValue, Captures, NoPredicates, PredicateHost, compile, matches};
use murphy_plugin_api::{Cx, CxRaw, FnTable, RawSlice};
use murphy_plugin_macros::node_pattern;

// ────────────────────────────────────────────────────────────────────────
// `Cx` plumbing identical to `node_pattern_behavior.rs` — kept here to
// keep this file self-contained as a single conformance vehicle.
// ────────────────────────────────────────────────────────────────────────

unsafe extern "C" fn noop_offense(
    _: *mut std::ffi::c_void,
    _: *const murphy_plugin_api::RawOffense,
) {
}
unsafe extern "C" fn noop_edit(_: *mut std::ffi::c_void, _: *const murphy_plugin_api::RawEdit) {}

fn fns() -> FnTable {
    FnTable {
        emit_offense: noop_offense,
        emit_edit: noop_edit,
    }
}

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

// ────────────────────────────────────────────────────────────────────────
// Fixtures
// ────────────────────────────────────────────────────────────────────────

/// `puts(1)` — implicit receiver, one int arg.
fn puts_one_ast() -> (Ast, NodeId, NodeId) {
    let mut b = AstBuilder::new("puts(1)", "t.rb");
    let one = b.push(NodeKind::Int(1), r());
    let m = b.intern_symbol("puts");
    let args = b.push_list(&[one]);
    let send = b.push(
        NodeKind::Send {
            receiver: OptNodeId::NONE,
            method: m,
            args,
        },
        r(),
    );
    let ast = b.finish(send);
    (ast, send, one)
}

/// `foo.bar(1, 2, 3)` — explicit receiver, three int args.
fn dotcall_three_args_ast() -> (Ast, NodeId, NodeId) {
    let mut b = AstBuilder::new("foo.bar(1,2,3)", "t.rb");
    let recv_sym = b.intern_symbol("foo");
    let recv = b.push(NodeKind::Lvar(recv_sym), r());
    let m = b.intern_symbol("bar");
    let ints: Vec<NodeId> = (1..=3)
        .map(|i| b.push(NodeKind::Int(i as i64), r()))
        .collect();
    let args = b.push_list(&ints);
    let send = b.push(
        NodeKind::Send {
            receiver: OptNodeId::some(recv),
            method: m,
            args,
        },
        r(),
    );
    let ast = b.finish(send);
    (ast, send, recv)
}

// ────────────────────────────────────────────────────────────────────────
// Drift-guard helper: assert C's `matches` agrees with a B-side bool.
// ────────────────────────────────────────────────────────────────────────

fn assert_c_matches(src: &str, ast: &Ast, node: NodeId, b_matched: bool) -> Option<Captures> {
    let ir = compile(src).unwrap_or_else(|e| panic!("compile `{src}` failed: {e}"));
    let c = matches(&ir, ast, node, &mut NoPredicates);
    assert_eq!(
        c.is_some(),
        b_matched,
        "B/C disagree on `{src}` against node {node:?}: B={b_matched}, C={}",
        c.is_some()
    );
    c
}

/// Like [`assert_c_matches`] but drives the C-backend matcher with a
/// custom [`PredicateHost`]. Used by the section-9 predicate-suffix
/// pairings, which require both backends to evaluate the *same*
/// predicate semantics — the default `NoPredicates` always returns
/// `false` and would mask a real disagreement.
fn assert_c_matches_with<P: PredicateHost>(
    src: &str,
    ast: &Ast,
    node: NodeId,
    b_matched: bool,
    host: &mut P,
) -> Option<Captures> {
    let ir = compile(src).unwrap_or_else(|e| panic!("compile `{src}` failed: {e}"));
    let c = matches(&ir, ast, node, host);
    assert_eq!(
        c.is_some(),
        b_matched,
        "B/C disagree on `{src}` against node {node:?}: B={b_matched}, C={}",
        c.is_some()
    );
    c
}

// ────────────────────────────────────────────────────────────────────────
// 1. Wildcard / bare kind / literal — no captures.
// ────────────────────────────────────────────────────────────────────────

node_pattern!(b_wildcard, "_");
node_pattern!(b_send_kind, "send");
node_pattern!(b_array_kind, "array");
node_pattern!(b_int_42, "42");
node_pattern!(b_sym_puts, ":puts");

#[test]
fn wildcard_kind_and_literal_match_consistently() {
    let (ast, send, one) = puts_one_ast();
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    // Wildcard against the send node.
    assert_c_matches("_", &ast, send, b_wildcard(send, &cx));
    // `send` matches the send, `array` does not.
    assert_c_matches("send", &ast, send, b_send_kind(send, &cx));
    assert_c_matches("array", &ast, send, b_array_kind(send, &cx));
    // Literal `42` does NOT match `Int(1)`.
    assert_c_matches("42", &ast, one, b_int_42(one, &cx));
    // `:puts` symbol literal does NOT match `Int(1)`.
    assert_c_matches(":puts", &ast, one, b_sym_puts(one, &cx));
}

// ────────────────────────────────────────────────────────────────────────
// 2. Send node match — fixed slots (`nil?` + `:sym`) + trailing rest.
// ────────────────────────────────────────────────────────────────────────

node_pattern!(b_send_nil_puts_wild, "(send nil? :puts _)");
node_pattern!(b_send_nil_raise_wild, "(send nil? :raise _)");

#[test]
fn send_match_with_nil_test_and_sym_slot_agrees() {
    let (ast, send, _) = puts_one_ast();
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    assert_c_matches(
        "(send nil? :puts _)",
        &ast,
        send,
        b_send_nil_puts_wild(send, &cx),
    );
    assert_c_matches(
        "(send nil? :raise _)",
        &ast,
        send,
        b_send_nil_raise_wild(send, &cx),
    );
}

// ────────────────────────────────────────────────────────────────────────
// 3. Heads — Any / OneOf — kind-only with optional `...`.
// ────────────────────────────────────────────────────────────────────────

node_pattern!(b_any_head, "(_ ...)");
node_pattern!(b_oneof_send_csend, "({send csend} ...)");
node_pattern!(b_oneof_array_hash, "({array hash} ...)");

#[test]
fn head_any_and_oneof_agree() {
    let (ast, send, _) = puts_one_ast();
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    assert_c_matches("(_ ...)", &ast, send, b_any_head(send, &cx));
    assert_c_matches(
        "({send csend} ...)",
        &ast,
        send,
        b_oneof_send_csend(send, &cx),
    );
    assert_c_matches(
        "({array hash} ...)",
        &ast,
        send,
        b_oneof_array_hash(send, &cx),
    );
}

// ────────────────────────────────────────────────────────────────────────
// 4. Union / Not.
// ────────────────────────────────────────────────────────────────────────

node_pattern!(b_union_array_send, "{array send}");
node_pattern!(b_union_array_hash, "{array hash}");
node_pattern!(b_not_array, "!array");
node_pattern!(b_not_send, "!send");

#[test]
fn union_and_negation_agree() {
    let (ast, send, _) = puts_one_ast();
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    assert_c_matches("{array send}", &ast, send, b_union_array_send(send, &cx));
    assert_c_matches("{array hash}", &ast, send, b_union_array_hash(send, &cx));
    assert_c_matches("!array", &ast, send, b_not_array(send, &cx));
    assert_c_matches("!send", &ast, send, b_not_send(send, &cx));
}

// ────────────────────────────────────────────────────────────────────────
// 5. Captures — Node and Seq slots — values, not just hit/miss.
// ────────────────────────────────────────────────────────────────────────

node_pattern!(b_capture_arg, "(send nil? :puts $_)");
node_pattern!(b_seq_capture_bar, "(send _ :bar $...)");

#[test]
fn anonymous_node_capture_returns_same_id() {
    let (ast, send, one) = puts_one_ast();
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    // B always returns the capture tuple wrapped in `Option<(..,)>`; C
    // returns it in `Captures.get(0)` as `CaptureValue::Node`.
    let b_captured: Option<(NodeId,)> = b_capture_arg(send, &cx);
    let c = assert_c_matches("(send nil? :puts $_)", &ast, send, b_captured.is_some());

    let c_captured = c.expect("C also matched").get(0).cloned();
    match (b_captured, c_captured) {
        (Some((bi,)), Some(CaptureValue::Node(ci))) => {
            assert_eq!(bi, ci, "capture id disagrees")
        }
        other => panic!("backends disagree on capture: {other:?}"),
    }
    let _ = one;
}

#[test]
fn seq_capture_collects_same_args() {
    let (ast, send, _) = dotcall_three_args_ast();
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    // B's seq capture comes wrapped in `Option<(&[NodeId],)>`; C's is a
    // `Vec<NodeId>` inside `CaptureValue::Seq`.
    let b_slice: Option<(&[NodeId],)> = b_seq_capture_bar(send, &cx);
    let c = assert_c_matches("(send _ :bar $...)", &ast, send, b_slice.is_some());

    let c_seq = c.expect("C also matched").get(0).cloned();
    match (b_slice, c_seq) {
        (Some((bs,)), Some(CaptureValue::Seq(cs))) => assert_eq!(bs, cs.as_slice()),
        other => panic!("backends disagree on seq capture: {other:?}"),
    }
}

// ────────────────────────────────────────────────────────────────────────
// 6. Unsupported kinds — `(int)` is rejected at compile time by B (so it
// is not added as a matcher here), and reported as a no-match at runtime
// by C. The runtime behaviour is exercised in
// `matcher::tests::unsupported_kind_node_pattern_silently_fails`; this
// file pairs only patterns B will accept.
// ────────────────────────────────────────────────────────────────────────

// ────────────────────────────────────────────────────────────────────────
// 7. Atom-payload var kinds (gvar/lvar/ivar/cvar) — single sym slot via
// `(gvar :$name)` / `(ivar :@n)` / `(cvar :@@c)` / `(lvar :n)` (murphy-o5k).
// ────────────────────────────────────────────────────────────────────────

node_pattern!(b_gvar_stdout, "(gvar :$stdout)");
node_pattern!(b_gvar_any, "(gvar _)");
node_pattern!(b_lvar_x, "(lvar :x)");
node_pattern!(b_ivar_at_x, "(ivar :@x)");
node_pattern!(b_cvar_atat_c, "(cvar :@@c)");

#[test]
fn gvar_sym_slot_match_agrees() {
    let mut b = AstBuilder::new("$stdout", "t.rb");
    let stdout = b.intern_symbol("$stdout");
    let g = b.push(NodeKind::Gvar(stdout), r());
    let ast = b.finish(g);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    assert_c_matches("(gvar :$stdout)", &ast, g, b_gvar_stdout(g, &cx));
    // Wildcard sym slot accepts any name.
    assert_c_matches("(gvar _)", &ast, g, b_gvar_any(g, &cx));
}

#[test]
fn gvar_sym_slot_rejects_wrong_name() {
    let mut b = AstBuilder::new("$other", "t.rb");
    let other = b.intern_symbol("$other");
    let g = b.push(NodeKind::Gvar(other), r());
    let ast = b.finish(g);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    assert_c_matches("(gvar :$stdout)", &ast, g, b_gvar_stdout(g, &cx));
}

#[test]
fn lvar_ivar_cvar_sym_slot_match_agrees() {
    let mut b = AstBuilder::new("ignored", "t.rb");
    let lx = b.intern_symbol("x");
    let l = b.push(NodeKind::Lvar(lx), r());
    let iat = b.intern_symbol("@x");
    let i = b.push(NodeKind::Ivar(iat), r());
    let cat = b.intern_symbol("@@c");
    let c = b.push(NodeKind::Cvar(cat), r());
    // Root has to be some node; pick `l`.
    let ast = b.finish(l);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    assert_c_matches("(lvar :x)", &ast, l, b_lvar_x(l, &cx));
    assert_c_matches("(ivar :@x)", &ast, i, b_ivar_at_x(i, &cx));
    assert_c_matches("(cvar :@@c)", &ast, c, b_cvar_atat_c(c, &cx));
    // A wrong-kind subject must miss.
    assert_c_matches("(lvar :x)", &ast, i, b_lvar_x(i, &cx));
}

// ────────────────────────────────────────────────────────────────────────
// 8. Symbol-slot literal union `{:a :b :c}` for method-table cops
// (murphy-rs7). The Send method slot accepts a union of sym literals;
// it hits when the method name matches any arm.
// ────────────────────────────────────────────────────────────────────────

node_pattern!(b_send_puts_or_print, "(send nil? {:puts :print} ...)");
node_pattern!(b_gvar_stdout_or_stderr, "(gvar {:$stdout :$stderr})");
// `!(gvar {:$stdout :$stderr})` routes the sym-union check through
// the B-backend's `lower_bool_fixed_slot` (the value-form sibling of
// `lower_fixed_slot`). Without it the bool-form rewrite has no test
// coverage and a future change to it would silently regress. A
// `gvar` kind is used here, not `send`, because `send` has a trailing
// `List` slot whose unconstrained semantics differ between B's
// `lower_bool` (slot floats free) and C's matcher (slot must be
// empty when no list pattern children are given) — a pre-existing
// gap, independent of sym-union.
node_pattern!(b_not_gvar_stdout_or_stderr, "!(gvar {:$stdout :$stderr})");

#[test]
fn send_method_slot_union_matches_any_listed_name() {
    let (ast, send, _) = puts_one_ast();
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    // `puts` is in the union — must hit.
    assert_c_matches(
        "(send nil? {:puts :print} ...)",
        &ast,
        send,
        b_send_puts_or_print(send, &cx),
    );
}

#[test]
fn send_method_slot_union_misses_unlisted_name() {
    // `foo.bar(...)` — `bar` is NOT in `{:puts :print}`.
    let (ast, send, _) = dotcall_three_args_ast();
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    assert_c_matches(
        "(send nil? {:puts :print} ...)",
        &ast,
        send,
        b_send_puts_or_print(send, &cx),
    );
}

#[test]
fn negated_gvar_with_sym_union_routes_through_bool_form() {
    // `!(gvar {:$stdout :$stderr})` — `Not` lowers its body via the
    // B-backend's `lower_bool` route, which dispatches the sym slot
    // to `lower_bool_fixed_slot`. C's matcher reaches the same union
    // arm through `Not` + `match_fixed_slot` + the `IrNode::Union`
    // branch. The matcher returns `true` (negation succeeds) when the
    // gvar's name is NOT in the union, and `false` when it is.
    let fns = fns();

    // `$stdout` — in the union; both backends must report `false`.
    let mut b = AstBuilder::new("$stdout", "t.rb");
    let s = b.intern_symbol("$stdout");
    let g = b.push(NodeKind::Gvar(s), r());
    let ast = b.finish(g);
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };
    assert_c_matches(
        "!(gvar {:$stdout :$stderr})",
        &ast,
        g,
        b_not_gvar_stdout_or_stderr(g, &cx),
    );

    // `$log` — outside the union; both backends must report `true`.
    let mut b = AstBuilder::new("$log", "t.rb");
    let s = b.intern_symbol("$log");
    let g = b.push(NodeKind::Gvar(s), r());
    let ast = b.finish(g);
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };
    assert_c_matches(
        "!(gvar {:$stdout :$stderr})",
        &ast,
        g,
        b_not_gvar_stdout_or_stderr(g, &cx),
    );
}

#[test]
fn gvar_sym_slot_union_matches_any_listed_name() {
    let mut b = AstBuilder::new("$stderr", "t.rb");
    let stderr = b.intern_symbol("$stderr");
    let g = b.push(NodeKind::Gvar(stderr), r());
    let ast = b.finish(g);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    assert_c_matches(
        "(gvar {:$stdout :$stderr})",
        &ast,
        g,
        b_gvar_stdout_or_stderr(g, &cx),
    );

    // A non-listed gvar misses.
    let mut b = AstBuilder::new("$log", "t.rb");
    let log = b.intern_symbol("$log");
    let g = b.push(NodeKind::Gvar(log), r());
    let ast = b.finish(g);
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };
    assert_c_matches(
        "(gvar {:$stdout :$stderr})",
        &ast,
        g,
        b_gvar_stdout_or_stderr(g, &cx),
    );
}

// ────────────────────────────────────────────────────────────────────────
// 9. `#predicate?` / `#predicate!` suffix mangling (murphy-bj7). The B
// backend emits a call to `name_p` / `name_bang`; the C backend keeps
// the original source name and dispatches via [`PredicateHost`]. A
// matching pair must agree on hit/miss, so the test's host returns the
// same answer as the Rust fn for each name.
// ────────────────────────────────────────────────────────────────────────

node_pattern!(b_pred_odd_q, "#odd?");
node_pattern!(b_pred_save_bang, "#save!");
node_pattern!(b_pred_in_union, "{#odd? #save!}");

/// Free fns the B backend's mangled call sites resolve to.
fn odd_p(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(*cx.kind(node), NodeKind::Int(v) if v % 2 != 0)
}
fn save_bang(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(*cx.kind(node), NodeKind::Int(v) if v == 42)
}

/// C-backend host that dispatches the *source* predicate names (with
/// the `?` / `!` suffix intact) onto the same predicates the B backend
/// reaches via the mangled call site. A real cop would wire this
/// through the mruby bridge — here we hard-code the test fixture so
/// the conformance assertion is meaningful.
struct PredFixture<'a, 'cx> {
    cx: &'a Cx<'cx>,
}
impl PredicateHost for PredFixture<'_, '_> {
    fn call(&mut self, name: &str, node: NodeId) -> bool {
        match name {
            "odd?" => odd_p(node, self.cx),
            "save!" => save_bang(node, self.cx),
            _ => false,
        }
    }
}

#[test]
fn predicate_question_suffix_agrees_across_backends() {
    let mut b = AstBuilder::new("3", "t.rb");
    let odd = b.push(NodeKind::Int(3), r());
    let even = b.push(NodeKind::Int(4), r());
    let ast = b.finish(odd);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    let mut host = PredFixture { cx: &cx };
    assert_c_matches_with("#odd?", &ast, odd, b_pred_odd_q(odd, &cx), &mut host);
    assert_c_matches_with("#odd?", &ast, even, b_pred_odd_q(even, &cx), &mut host);
}

#[test]
fn predicate_bang_suffix_agrees_across_backends() {
    let mut b = AstBuilder::new("42", "t.rb");
    let hit = b.push(NodeKind::Int(42), r());
    let miss = b.push(NodeKind::Int(0), r());
    let ast = b.finish(hit);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    let mut host = PredFixture { cx: &cx };
    assert_c_matches_with("#save!", &ast, hit, b_pred_save_bang(hit, &cx), &mut host);
    assert_c_matches_with("#save!", &ast, miss, b_pred_save_bang(miss, &cx), &mut host);
}

#[test]
fn predicate_suffix_inside_union_agrees() {
    // `{#odd? #save!}` — the union flows through `lower_bool`, which
    // also routes predicates through the mangled call site. C reaches
    // the same predicates via `match_pat`'s Union arm.
    let mut b = AstBuilder::new("42", "t.rb");
    let n42 = b.push(NodeKind::Int(42), r());
    let n3 = b.push(NodeKind::Int(3), r());
    let n4 = b.push(NodeKind::Int(4), r());
    let ast = b.finish(n42);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    let mut host = PredFixture { cx: &cx };
    // 42 matches via `#save!`; 3 matches via `#odd?`; 4 misses both.
    assert_c_matches_with(
        "{#odd? #save!}",
        &ast,
        n42,
        b_pred_in_union(n42, &cx),
        &mut host,
    );
    assert_c_matches_with(
        "{#odd? #save!}",
        &ast,
        n3,
        b_pred_in_union(n3, &cx),
        &mut host,
    );
    assert_c_matches_with(
        "{#odd? #save!}",
        &ast,
        n4,
        b_pred_in_union(n4, &cx),
        &mut host,
    );
}
