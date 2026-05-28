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
        sorted_tokens: p.sorted_tokens.as_ptr(),
        sorted_tokens_len: p.sorted_tokens.len(),
        options_json: RawSlice::from_str("{}"),
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

fn puts_int_ast(value: i64) -> (Ast, NodeId, NodeId) {
    let mut b = AstBuilder::new("puts(1)", "t.rb");
    let arg = b.push(NodeKind::Int(value), r());
    let m = b.intern_symbol("puts");
    let args = b.push_list(&[arg]);
    let send = b.push(
        NodeKind::Send {
            receiver: OptNodeId::NONE,
            method: m,
            args,
        },
        r(),
    );
    let ast = b.finish(send);
    (ast, send, arg)
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

node_pattern!(b_send_puts_odd_predicate, "(send nil? :puts odd?)");

#[test]
fn bare_predicate_in_send_arg_slot_agrees() {
    let (odd_ast, odd_send, _) = puts_int_ast(3);
    let (even_ast, even_send, _) = puts_int_ast(4);
    let fns = fns();
    let odd_raw = cx_raw_for(&odd_ast, &fns);
    let even_raw = cx_raw_for(&even_ast, &fns);
    let odd_cx = unsafe { Cx::from_raw(&odd_raw) };
    let even_cx = unsafe { Cx::from_raw(&even_raw) };

    let mut host = PredFixture { cx: &odd_cx };
    assert_c_matches_with(
        "(send nil? :puts odd?)",
        &odd_ast,
        odd_send,
        b_send_puts_odd_predicate(odd_send, &odd_cx),
        &mut host,
    );

    host.cx = &even_cx;
    assert_c_matches_with(
        "(send nil? :puts odd?)",
        &even_ast,
        even_send,
        b_send_puts_odd_predicate(even_send, &even_cx),
        &mut host,
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

// ────────────────────────────────────────────────────────────────────────
// 10. Sequence quantifiers — `pat*` / `pat+` / `pat?` in node child lists
// (murphy-ycx). The B backend emits a compile-time backtracker; the C
// backend uses the runtime backtracker landed in PR #76. Both must agree
// on hit/miss and on the captured slot shape (`Seq` for `*`/`+`,
// `OptNode` for `?`).
// ────────────────────────────────────────────────────────────────────────

/// `[1, 2, 3]` — an array of three ints.
fn array_three_ints_ast() -> (Ast, NodeId) {
    let mut b = AstBuilder::new("[1, 2, 3]", "t.rb");
    let ints: Vec<NodeId> = (1..=3)
        .map(|i| b.push(NodeKind::Int(i as i64), r()))
        .collect();
    let list = b.push_list(&ints);
    let arr = b.push(NodeKind::Array(list), r());
    let ast = b.finish(arr);
    (ast, arr)
}

/// `[]` — an empty array.
fn array_empty_ast() -> (Ast, NodeId) {
    let mut b = AstBuilder::new("[]", "t.rb");
    let list = b.push_list(&[]);
    let arr = b.push(NodeKind::Array(list), r());
    let ast = b.finish(arr);
    (ast, arr)
}

node_pattern!(b_array_int_plus, "(array int+)");
node_pattern!(b_array_int_star, "(array int*)");
node_pattern!(b_array_int_plus_int, "(array int+ int)");
node_pattern!(b_send_pluck_sym_plus, "(send _ :pluck sym+)");
node_pattern!(b_send_uc_hash_q, "(send _ :update_columns hash?)");
node_pattern!(b_send_foo_int_star_str, "(send _ :foo int* str)");

/// Build an `obj.<method>(<args>)` send AST. The closure produces the arg
/// node ids in call order. Receiver is an `Lvar(:obj)` so that an OptNode
/// slot pattern of `_` (which requires `Some`) matches.
fn build_send_ast<F>(method: &str, push_args: F) -> (Ast, NodeId)
where
    F: FnOnce(&mut AstBuilder) -> Vec<NodeId>,
{
    let mut b = AstBuilder::new("", "t.rb");
    let recv_sym = b.intern_symbol("obj");
    let recv = b.push(NodeKind::Lvar(recv_sym), r());
    let args = push_args(&mut b);
    let arg_list = b.push_list(&args);
    let m = b.intern_symbol(method);
    let send = b.push(
        NodeKind::Send {
            receiver: OptNodeId::some(recv),
            method: m,
            args: arg_list,
        },
        r(),
    );
    let ast = b.finish(send);
    (ast, send)
}

fn run_pair(src: &str, ast: &Ast, node: NodeId, b_matched: bool, expect_match: bool, case: &str) {
    let c = assert_c_matches(src, ast, node, b_matched);
    assert_eq!(
        b_matched, expect_match,
        "case `{case}` against `{src}`: expected match={expect_match}, got B={b_matched}"
    );
    let _ = c;
}

#[test]
fn quantifier_plus_matches_one_or_more() {
    {
        let (ast, arr) = array_three_ints_ast();
        let fns = fns();
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        run_pair(
            "(array int+)",
            &ast,
            arr,
            b_array_int_plus(arr, &cx),
            true,
            "[1,2,3]",
        );
    }
    {
        let (ast, arr) = array_empty_ast();
        let fns = fns();
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        run_pair(
            "(array int+)",
            &ast,
            arr,
            b_array_int_plus(arr, &cx),
            false,
            "[]",
        );
    }
}

#[test]
fn quantifier_star_matches_zero_or_more() {
    // Both `[1,2,3]` and `[]` hit `(array int*)`.
    {
        let (ast, arr) = array_three_ints_ast();
        let fns = fns();
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        run_pair(
            "(array int*)",
            &ast,
            arr,
            b_array_int_star(arr, &cx),
            true,
            "[1,2,3]",
        );
    }
    {
        let (ast, arr) = array_empty_ast();
        let fns = fns();
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        run_pair(
            "(array int*)",
            &ast,
            arr,
            b_array_int_star(arr, &cx),
            true,
            "[]",
        );
    }
}

#[test]
fn quantifier_backtracks_to_give_back_to_fixed_suffix() {
    // `(array int+ int)` against `[1,2,3]`: greedy int+ takes all 3, suffix
    // `int` fails, backtrack to int+ = [1,2], suffix `int` = 3 → hit.
    let (ast, arr) = array_three_ints_ast();
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };
    run_pair(
        "(array int+ int)",
        &ast,
        arr,
        b_array_int_plus_int(arr, &cx),
        true,
        "[1,2,3]",
    );
}

#[test]
fn quantifier_pluck_sym_plus() {
    // pluck(:a) — hit.
    {
        let (ast, send) = build_send_ast("pluck", |b| {
            let s = b.intern_symbol("a");
            vec![b.push(NodeKind::Sym(s), r())]
        });
        let fns = fns();
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        run_pair(
            "(send _ :pluck sym+)",
            &ast,
            send,
            b_send_pluck_sym_plus(send, &cx),
            true,
            "pluck(:a)",
        );
    }
    // pluck(:a, :b) — hit.
    {
        let (ast, send) = build_send_ast("pluck", |b| {
            let sa = b.intern_symbol("a");
            let sb = b.intern_symbol("b");
            vec![
                b.push(NodeKind::Sym(sa), r()),
                b.push(NodeKind::Sym(sb), r()),
            ]
        });
        let fns = fns();
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        run_pair(
            "(send _ :pluck sym+)",
            &ast,
            send,
            b_send_pluck_sym_plus(send, &cx),
            true,
            "pluck(:a,:b)",
        );
    }
    // pluck() — miss (min 1).
    {
        let (ast, send) = build_send_ast("pluck", |_| Vec::new());
        let fns = fns();
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        run_pair(
            "(send _ :pluck sym+)",
            &ast,
            send,
            b_send_pluck_sym_plus(send, &cx),
            false,
            "pluck()",
        );
    }
}

#[test]
fn quantifier_optional_hash_arg() {
    use murphy_ast::NodeList;
    // update_columns({}) — hit (one hash arg).
    {
        let (ast, send) = build_send_ast("update_columns", |b| {
            vec![b.push(NodeKind::Hash(NodeList::EMPTY), r())]
        });
        let fns = fns();
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        run_pair(
            "(send _ :update_columns hash?)",
            &ast,
            send,
            b_send_uc_hash_q(send, &cx),
            true,
            "update_columns({})",
        );
    }
    // update_columns() — hit (optional absent).
    {
        let (ast, send) = build_send_ast("update_columns", |_| Vec::new());
        let fns = fns();
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        run_pair(
            "(send _ :update_columns hash?)",
            &ast,
            send,
            b_send_uc_hash_q(send, &cx),
            true,
            "update_columns()",
        );
    }
}

// ─── Capture-bearing quantifier cases (`$int+`, `$str?`, $-backreffed). ───

node_pattern!(b_array_cap_int_plus, "(array $int+)");
node_pattern!(b_send_uc_cap_hash_q, "(send _ :update_columns $hash?)");
node_pattern!(b_array_cap_int_plus_cap_1, "(array $int+ $1)");
node_pattern!(b_send_foo_rest_int_plus, "(send _ :foo ... int+)");
// Nested quantifier lists where an inner capture slot is also visible from
// the outer driver's `collect_capture_slots` walk. The outer driver must
// not double-redirect the slot or both lists race the `__cap{slot}`
// single-assignment binding.
node_pattern!(
    b_nested_outer_q_inner_cap,
    "(send _ :wrap (array $int+) int+)"
);

#[test]
fn quantifier_capture_seq_matches_one_or_more() {
    // `(array $int+)` against `[1,2,3]` — captures Seq([1,2,3]).
    let (ast, arr) = array_three_ints_ast();
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    let b_captured: Option<(&[NodeId],)> = b_array_cap_int_plus(arr, &cx);
    let c = assert_c_matches("(array $int+)", &ast, arr, b_captured.is_some());

    let c_seq = c.expect("C also matched").get(0).cloned();
    match (b_captured, c_seq) {
        (Some((bs,)), Some(CaptureValue::Seq(cs))) => assert_eq!(bs, cs.as_slice()),
        other => panic!("backends disagree on $int+ capture: {other:?}"),
    }
}

#[test]
fn quantifier_capture_optnode_matches_present_and_absent() {
    use murphy_ast::NodeList;
    // `update_columns({})` — `$hash?` captures `Some(hash_node)`.
    {
        let (ast, send) = build_send_ast("update_columns", |b| {
            vec![b.push(NodeKind::Hash(NodeList::EMPTY), r())]
        });
        let fns = fns();
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };

        let b_captured: Option<(Option<NodeId>,)> = b_send_uc_cap_hash_q(send, &cx);
        let c = assert_c_matches(
            "(send _ :update_columns $hash?)",
            &ast,
            send,
            b_captured.is_some(),
        );
        let c_optnode = c.expect("C also matched").get(0).cloned();
        match (b_captured, c_optnode) {
            (Some((Some(b_id),)), Some(CaptureValue::OptNode(Some(c_id)))) => {
                assert_eq!(b_id, c_id, "OptNode-Some id disagrees");
            }
            other => panic!("backends disagree on $hash? present-capture: {other:?}"),
        }
    }
    // `update_columns()` — `$hash?` captures `None` (optional absent).
    {
        let (ast, send) = build_send_ast("update_columns", |_| Vec::new());
        let fns = fns();
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };

        let b_captured: Option<(Option<NodeId>,)> = b_send_uc_cap_hash_q(send, &cx);
        let c = assert_c_matches(
            "(send _ :update_columns $hash?)",
            &ast,
            send,
            b_captured.is_some(),
        );
        let c_optnode = c.expect("C also matched").get(0).cloned();
        match (b_captured, c_optnode) {
            (Some((None,)), Some(CaptureValue::OptNode(None))) => {}
            other => panic!("backends disagree on $hash? absent-capture: {other:?}"),
        }
    }
}

#[test]
fn quantifier_backtracks_with_captures_into_fixed_suffix() {
    // `(array $int+ $1)` against `[1, 2, 1]`:
    // - greedy `$int+` would take all 3, suffix `$1` fails (no elem left)
    // - backtrack to `$int+` = [id1, id2], suffix `$1` matches `1` against id3
    // → captures: Seq([id1, id2]) + Node(id3).
    let mut b = AstBuilder::new("[1, 2, 1]", "t.rb");
    let id1 = b.push(NodeKind::Int(1), r());
    let id2 = b.push(NodeKind::Int(2), r());
    let id3 = b.push(NodeKind::Int(1), r());
    let list = b.push_list(&[id1, id2, id3]);
    let arr = b.push(NodeKind::Array(list), r());
    let ast = b.finish(arr);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    let b_captured: Option<(&[NodeId], NodeId)> = b_array_cap_int_plus_cap_1(arr, &cx);
    let c = assert_c_matches("(array $int+ $1)", &ast, arr, b_captured.is_some());

    let c_seq = c.as_ref().expect("C also matched").get(0).cloned();
    let c_node = c.expect("C also matched").get(1).cloned();
    match (b_captured, c_seq, c_node) {
        (
            Some((b_seq, b_node)),
            Some(CaptureValue::Seq(c_seq)),
            Some(CaptureValue::Node(c_node)),
        ) => {
            assert_eq!(b_seq, c_seq.as_slice(), "Seq capture disagrees");
            assert_eq!(b_node, c_node, "Node capture disagrees");
            // Sanity: backtracker really gave back the trailing `1`.
            assert_eq!(b_seq, &[id1, id2]);
            assert_eq!(b_node, id3);
        }
        other => panic!("backends disagree on $int+ $1 captures: {other:?}"),
    }
}

#[test]
fn quantifier_nested_inner_capture_threads_through_outer_driver() {
    // `(send _ :wrap (array $int+) int+)` — outer driver owns slot 0 (it
    // walks Fixed Node children too), inner driver finds slot 0 already
    // redirected and reuses the outer's `__lcap0`. `obj.wrap([1, 2], 7,
    // 8)` should hit: outer args = [array, int(7), int(8)] → fixed
    // `(array $int+)` matches the array (inner $int+ = [1, 2]), trailing
    // `int+` matches [7, 8].
    let mut b = AstBuilder::new("obj.wrap([1, 2], 7, 8)", "t.rb");
    let recv_sym = b.intern_symbol("obj");
    let recv = b.push(NodeKind::Lvar(recv_sym), r());
    let n1 = b.push(NodeKind::Int(1), r());
    let n2 = b.push(NodeKind::Int(2), r());
    let inner_list = b.push_list(&[n1, n2]);
    let array = b.push(NodeKind::Array(inner_list), r());
    let n7 = b.push(NodeKind::Int(7), r());
    let n8 = b.push(NodeKind::Int(8), r());
    let args = b.push_list(&[array, n7, n8]);
    let m = b.intern_symbol("wrap");
    let send = b.push(
        NodeKind::Send {
            receiver: OptNodeId::some(recv),
            method: m,
            args,
        },
        r(),
    );
    let ast = b.finish(send);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    let b_captured: Option<(&[NodeId],)> = b_nested_outer_q_inner_cap(send, &cx);
    let c = assert_c_matches(
        "(send _ :wrap (array $int+) int+)",
        &ast,
        send,
        b_captured.is_some(),
    );
    let c_seq = c.expect("C also matched").get(0).cloned();
    match (b_captured, c_seq) {
        (Some((bs,)), Some(CaptureValue::Seq(cs))) => {
            assert_eq!(bs, cs.as_slice(), "nested $int+ capture disagrees");
            assert_eq!(bs, &[n1, n2]);
        }
        other => panic!("backends disagree on nested capture: {other:?}"),
    }
}

#[test]
fn quantifier_coexists_with_rest() {
    // `(send _ :foo ... int+)` requires both `...` (zero-or-more, no
    // capture) and a trailing `int+` quantifier. The parser allows at most
    // one rest plus any number of quantifiers, and the backtracker must
    // honour both: `foo(:x, 1, 2)` → `...` = [:x], `int+` = [1, 2].
    {
        let (ast, send) = build_send_ast("foo", |b| {
            let sx = b.intern_symbol("x");
            vec![
                b.push(NodeKind::Sym(sx), r()),
                b.push(NodeKind::Int(1), r()),
                b.push(NodeKind::Int(2), r()),
            ]
        });
        let fns = fns();
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        run_pair(
            "(send _ :foo ... int+)",
            &ast,
            send,
            b_send_foo_rest_int_plus(send, &cx),
            true,
            "foo(:x, 1, 2)",
        );
    }
    // `foo(:x)` — no trailing int, `int+` min=1 fails.
    {
        let (ast, send) = build_send_ast("foo", |b| {
            let sx = b.intern_symbol("x");
            vec![b.push(NodeKind::Sym(sx), r())]
        });
        let fns = fns();
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        run_pair(
            "(send _ :foo ... int+)",
            &ast,
            send,
            b_send_foo_rest_int_plus(send, &cx),
            false,
            "foo(:x)",
        );
    }
}

#[test]
fn quantifier_star_with_fixed_suffix() {
    // foo(1, 2, "x") — hit: int* = [1,2], str = "x".
    {
        let (ast, send) = build_send_ast("foo", |b| {
            let s = b.intern_string("x");
            vec![
                b.push(NodeKind::Int(1), r()),
                b.push(NodeKind::Int(2), r()),
                b.push(NodeKind::Str(s), r()),
            ]
        });
        let fns = fns();
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        run_pair(
            "(send _ :foo int* str)",
            &ast,
            send,
            b_send_foo_int_star_str(send, &cx),
            true,
            "foo(1,2,\"x\")",
        );
    }
    // foo("x") — hit: int* = [], str = "x".
    {
        let (ast, send) = build_send_ast("foo", |b| {
            let s = b.intern_string("x");
            vec![b.push(NodeKind::Str(s), r())]
        });
        let fns = fns();
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        run_pair(
            "(send _ :foo int* str)",
            &ast,
            send,
            b_send_foo_int_star_str(send, &cx),
            true,
            "foo(\"x\")",
        );
    }
    // foo() — miss (no str).
    {
        let (ast, send) = build_send_ast("foo", |_| Vec::new());
        let fns = fns();
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };
        run_pair(
            "(send _ :foo int* str)",
            &ast,
            send,
            b_send_foo_int_star_str(send, &cx),
            false,
            "foo()",
        );
    }
}

// ────────────────────────────────────────────────────────────────────────
// 11. Union-capture sugar `${int float}` (murphy-6ba). A `$` immediately
// before `{...}` desugars to a Union whose every arm is a Capture sharing
// slot 0. Both backends must agree on hit/miss AND return the same NodeId
// for the capture.
// ────────────────────────────────────────────────────────────────────────

node_pattern!(b_sugar_int_or_float, "${int float}");

#[test]
fn union_capture_sugar_returns_same_node_id_for_both_arms() {
    let fns = fns();

    // An Int(42) subject — must hit via the `int` arm and capture itself.
    {
        let mut b = AstBuilder::new("42", "t.rb");
        let int_node = b.push(NodeKind::Int(42), r());
        let ast = b.finish(int_node);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };

        let b_captured: Option<(NodeId,)> = b_sugar_int_or_float(int_node, &cx);
        let c = assert_c_matches("${int float}", &ast, int_node, b_captured.is_some());
        let c_captured = c.expect("C also matched on Int(42)").get(0).cloned();
        match (b_captured, c_captured) {
            (Some((bi,)), Some(CaptureValue::Node(ci))) => {
                assert_eq!(bi, ci, "Int arm: capture id disagrees (B={bi:?}, C={ci:?})");
                assert_eq!(bi, int_node, "Int arm: captured wrong node");
            }
            other => panic!("backends disagree on ${{int float}} / Int(42): {other:?}"),
        }
    }

    // A Float(1.5) subject — must hit via the `float` arm and capture itself.
    {
        let mut b = AstBuilder::new("1.5", "t.rb");
        let float_node = b.push(NodeKind::Float(1.5), r());
        let ast = b.finish(float_node);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };

        let b_captured: Option<(NodeId,)> = b_sugar_int_or_float(float_node, &cx);
        let c = assert_c_matches("${int float}", &ast, float_node, b_captured.is_some());
        let c_captured = c.expect("C also matched on Float(1.5)").get(0).cloned();
        match (b_captured, c_captured) {
            (Some((bi,)), Some(CaptureValue::Node(ci))) => {
                assert_eq!(
                    bi, ci,
                    "Float arm: capture id disagrees (B={bi:?}, C={ci:?})"
                );
                assert_eq!(bi, float_node, "Float arm: captured wrong node");
            }
            other => panic!("backends disagree on ${{int float}} / Float(1.5): {other:?}"),
        }
    }

    // A Sym subject — must miss (neither int nor float).
    {
        let mut b = AstBuilder::new(":foo", "t.rb");
        let sym = b.intern_symbol("foo");
        let sym_node = b.push(NodeKind::Sym(sym), r());
        let ast = b.finish(sym_node);
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };

        let b_matched: Option<(NodeId,)> = b_sugar_int_or_float(sym_node, &cx);
        assert_c_matches("${int float}", &ast, sym_node, b_matched.is_some());
        assert!(b_matched.is_none(), "Sym must not match ${{int float}}");
    }
}

// ────────────────────────────────────────────────────────────────────────
// 12. AnyOrder `<...>` — order-independent sequence matching (murphy-ejd).
//
// The first three tests drive the C-backend only (via `compile`/`matches`).
// The last three are B↔C paired conformance tests that also exercise the
// `node_pattern!` B-backend.
// ────────────────────────────────────────────────────────────────────────

#[test]
fn anyorder_basic_both_orderings() {
    // `(array <int sym>)` must match whether the int or sym comes first.
    let pat = "(array <int sym>)";
    let ir = compile(pat).unwrap_or_else(|e| panic!("compile `{pat}` failed: {e}"));

    // [42, :foo] — int first, sym second.
    let (ast_a, arr_a) = {
        let mut b = AstBuilder::new("", "t.rb");
        let i = b.push(NodeKind::Int(42), r());
        let s_sym = b.intern_symbol("foo");
        let s = b.push(NodeKind::Sym(s_sym), r());
        let list = b.push_list(&[i, s]);
        let arr = b.push(NodeKind::Array(list), r());
        let ast = b.finish(arr);
        (ast, arr)
    };
    assert!(
        matches(&ir, &ast_a, arr_a, &mut NoPredicates).is_some(),
        "`{pat}` must match [int, sym]"
    );

    // [:foo, 42] — sym first, int second.
    let (ast_b, arr_b) = {
        let mut b = AstBuilder::new("", "t.rb");
        let s_sym = b.intern_symbol("foo");
        let s = b.push(NodeKind::Sym(s_sym), r());
        let i = b.push(NodeKind::Int(42), r());
        let list = b.push_list(&[s, i]);
        let arr = b.push(NodeKind::Array(list), r());
        let ast = b.finish(arr);
        (ast, arr)
    };
    assert!(
        matches(&ir, &ast_b, arr_b, &mut NoPredicates).is_some(),
        "`{pat}` must match [sym, int]"
    );

    // [42, 99] — two ints, no sym → miss.
    let (ast_c, arr_c) = {
        let mut b = AstBuilder::new("", "t.rb");
        let i1 = b.push(NodeKind::Int(42), r());
        let i2 = b.push(NodeKind::Int(99), r());
        let list = b.push_list(&[i1, i2]);
        let arr = b.push(NodeKind::Array(list), r());
        let ast = b.finish(arr);
        (ast, arr)
    };
    assert!(
        matches(&ir, &ast_c, arr_c, &mut NoPredicates).is_none(),
        "`{pat}` must NOT match [int, int]"
    );
}

#[test]
fn anyorder_with_rest_absorbs_extras() {
    // `(array <int sym ...>)` must match [42, :foo, :bar, :baz] — int + sym
    // must be found somewhere; the rest absorbs the leftover syms.
    let pat = "(array <int sym ...>)";
    let ir = compile(pat).unwrap_or_else(|e| panic!("compile `{pat}` failed: {e}"));

    let (ast, arr) = {
        let mut b = AstBuilder::new("", "t.rb");
        let i = b.push(NodeKind::Int(42), r());
        let s1 = b.intern_symbol("foo");
        let sym1 = b.push(NodeKind::Sym(s1), r());
        let s2 = b.intern_symbol("bar");
        let sym2 = b.push(NodeKind::Sym(s2), r());
        let s3 = b.intern_symbol("baz");
        let sym3 = b.push(NodeKind::Sym(s3), r());
        let list = b.push_list(&[i, sym1, sym2, sym3]);
        let arr = b.push(NodeKind::Array(list), r());
        let ast = b.finish(arr);
        (ast, arr)
    };
    assert!(
        matches(&ir, &ast, arr, &mut NoPredicates).is_some(),
        "`{pat}` must match [int, sym, sym, sym]"
    );

    // sym first, int in the middle.
    let (ast2, arr2) = {
        let mut b = AstBuilder::new("", "t.rb");
        let s1 = b.intern_symbol("a");
        let sym1 = b.push(NodeKind::Sym(s1), r());
        let i = b.push(NodeKind::Int(1), r());
        let s2 = b.intern_symbol("b");
        let sym2 = b.push(NodeKind::Sym(s2), r());
        let list = b.push_list(&[sym1, i, sym2]);
        let arr = b.push(NodeKind::Array(list), r());
        let ast = b.finish(arr);
        (ast, arr)
    };
    assert!(
        matches(&ir, &ast2, arr2, &mut NoPredicates).is_some(),
        "`{pat}` must match [sym, int, sym] with rest"
    );

    // Only one element: just int — misses because no sym.
    let (ast3, arr3) = {
        let mut b = AstBuilder::new("", "t.rb");
        let i = b.push(NodeKind::Int(7), r());
        let list = b.push_list(&[i]);
        let arr = b.push(NodeKind::Array(list), r());
        let ast = b.finish(arr);
        (ast, arr)
    };
    assert!(
        matches(&ir, &ast3, arr3, &mut NoPredicates).is_none(),
        "`{pat}` must NOT match [int] (no sym)"
    );
}

#[test]
fn anyorder_captures_in_declaration_order() {
    // `(array <$a $b>)` — wildcard captures: the C-backend must assign
    // captures in declaration order (slot 0 = first element assigned to
    // pattern 0, slot 1 = first element assigned to pattern 1).
    // With wildcards and input [sym, int], pattern 0 takes element 0 (sym)
    // and pattern 1 takes element 1 (int).
    let pat = "(array <$a $b>)";
    let ir = compile(pat).unwrap_or_else(|e| panic!("compile `{pat}` failed: {e}"));

    // [:foo, 42] — sym first, int second.
    let (ast, arr) = {
        let mut b = AstBuilder::new("", "t.rb");
        let s_sym = b.intern_symbol("foo");
        let s = b.push(NodeKind::Sym(s_sym), r());
        let i = b.push(NodeKind::Int(42), r());
        let list = b.push_list(&[s, i]);
        let arr = b.push(NodeKind::Array(list), r());
        let ast = b.finish(arr);
        (ast, arr)
    };
    let caps = matches(&ir, &ast, arr, &mut NoPredicates)
        .expect("`(array <$a $b>)` must match [:foo, 42]");
    // Slot 0 = first element (Sym(:foo)), slot 1 = second element (Int(42)).
    let slot0 = caps.get(0).expect("slot 0 must be set");
    let slot1 = caps.get(1).expect("slot 1 must be set");
    match slot0 {
        CaptureValue::Node(nid) => {
            assert!(
                matches!(ast.node(*nid).kind, NodeKind::Sym(_)),
                "slot 0 must be Sym (first element), got {:?}",
                ast.node(*nid).kind
            );
        }
        other => panic!("slot 0 expected Node, got {other:?}"),
    }
    match slot1 {
        CaptureValue::Node(nid) => {
            assert!(
                matches!(ast.node(*nid).kind, NodeKind::Int(_)),
                "slot 1 must be Int (second element), got {:?}",
                ast.node(*nid).kind
            );
        }
        other => panic!("slot 1 expected Node, got {other:?}"),
    }
}

// ─── B↔C paired conformance for AnyOrder ─────────────────────────────────

// Declare B-backend matchers for the paired tests below.
node_pattern!(b_anyorder_basic, "(array <int sym>)");
node_pattern!(b_anyorder_underscore_then_int, "(array <_ int>)");
// Wildcard captures: `$a` captures anything into slot 0, `$b` into slot 1.
node_pattern!(b_anyorder_caps, "(array <$a $b>)");
// Backtracking + capture: `$a` is a wildcard capture, `int` is a type
// discriminator. Against [42, :sym], `$a` must end up on :sym (slot 0)
// after backtracking forces `int` to take 42.
node_pattern!(b_anyorder_backtrack_cap, "(array <$a int>)");

#[test]
fn b_anyorder_basic_both_orderings() {
    // B and C agree: `(array <int sym>)` matches both orderings.
    let pat = "(array <int sym>)";
    let ir = compile(pat).unwrap_or_else(|e| panic!("compile `{pat}` failed: {e}"));
    let fns = fns();

    // [42, :foo] — int first.
    let (ast_a, arr_a) = {
        let mut b = AstBuilder::new("", "t.rb");
        let i = b.push(NodeKind::Int(42), r());
        let s_sym = b.intern_symbol("foo");
        let s = b.push(NodeKind::Sym(s_sym), r());
        let list = b.push_list(&[i, s]);
        let arr = b.push(NodeKind::Array(list), r());
        let ast = b.finish(arr);
        (ast, arr)
    };
    let raw_a = cx_raw_for(&ast_a, &fns);
    let cx_a = unsafe { Cx::from_raw(&raw_a) };
    let b_hit_a: bool = b_anyorder_basic(arr_a, &cx_a);
    let c_hit_a = matches(&ir, &ast_a, arr_a, &mut NoPredicates).is_some();
    assert_eq!(b_hit_a, c_hit_a, "B↔C disagree on [int, sym]");
    assert!(b_hit_a, "must match [int, sym]");

    // [:foo, 42] — sym first.
    let (ast_b, arr_b) = {
        let mut b = AstBuilder::new("", "t.rb");
        let s_sym = b.intern_symbol("foo");
        let s = b.push(NodeKind::Sym(s_sym), r());
        let i = b.push(NodeKind::Int(42), r());
        let list = b.push_list(&[s, i]);
        let arr = b.push(NodeKind::Array(list), r());
        let ast = b.finish(arr);
        (ast, arr)
    };
    let raw_b = cx_raw_for(&ast_b, &fns);
    let cx_b = unsafe { Cx::from_raw(&raw_b) };
    let b_hit_b: bool = b_anyorder_basic(arr_b, &cx_b);
    let c_hit_b = matches(&ir, &ast_b, arr_b, &mut NoPredicates).is_some();
    assert_eq!(b_hit_b, c_hit_b, "B↔C disagree on [sym, int]");
    assert!(b_hit_b, "must match [sym, int]");

    // [42, 99] — no sym → miss.
    let (ast_c, arr_c) = {
        let mut b = AstBuilder::new("", "t.rb");
        let i1 = b.push(NodeKind::Int(42), r());
        let i2 = b.push(NodeKind::Int(99), r());
        let list = b.push_list(&[i1, i2]);
        let arr = b.push(NodeKind::Array(list), r());
        let ast = b.finish(arr);
        (ast, arr)
    };
    let raw_c = cx_raw_for(&ast_c, &fns);
    let cx_c = unsafe { Cx::from_raw(&raw_c) };
    let b_miss_c: bool = b_anyorder_basic(arr_c, &cx_c);
    let c_miss_c = matches(&ir, &ast_c, arr_c, &mut NoPredicates).is_some();
    assert_eq!(b_miss_c, c_miss_c, "B↔C disagree on [int, int]");
    assert!(!b_miss_c, "must NOT match [int, int]");
}

#[test]
fn b_anyorder_underscore_then_int_discriminator() {
    // `(array <_ int>)` against [42, :sym] — backtracking discriminator.
    // A greedy (non-backtracking) B-backend: `_` takes 42, `int` fails on :sym.
    // A correct backtracking B-backend: `_` retries :sym, `int` takes 42. Match.
    let pat = "(array <_ int>)";
    let ir = compile(pat).unwrap_or_else(|e| panic!("compile `{pat}` failed: {e}"));
    let fns = fns();

    let (ast, arr) = {
        let mut b = AstBuilder::new("", "t.rb");
        let i = b.push(NodeKind::Int(42), r());
        let s_sym = b.intern_symbol("sym");
        let s = b.push(NodeKind::Sym(s_sym), r());
        let list = b.push_list(&[i, s]);
        let arr = b.push(NodeKind::Array(list), r());
        let ast = b.finish(arr);
        (ast, arr)
    };
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    let b_hit: bool = b_anyorder_underscore_then_int(arr, &cx);
    let c_hit = matches(&ir, &ast, arr, &mut NoPredicates).is_some();
    assert_eq!(b_hit, c_hit, "B↔C disagree on [int, sym] for `<_ int>`");
    assert!(
        b_hit,
        "`<_ int>` must match [int, sym] (backtracking required)"
    );
}

#[test]
fn b_anyorder_captures_in_declaration_order() {
    // `(array <$a $b>)` — wildcard captures: slot 0 = first matched element,
    // slot 1 = second matched element. B and C must agree.
    let pat = "(array <$a $b>)";
    let ir = compile(pat).unwrap_or_else(|e| panic!("compile `{pat}` failed: {e}"));
    let fns = fns();

    // [:foo, 42] — sym first in array, int is slot 0.
    let (ast, arr) = {
        let mut b = AstBuilder::new("", "t.rb");
        let s_sym = b.intern_symbol("foo");
        let s = b.push(NodeKind::Sym(s_sym), r());
        let i = b.push(NodeKind::Int(42), r());
        let list = b.push_list(&[s, i]);
        let arr = b.push(NodeKind::Array(list), r());
        let ast = b.finish(arr);
        (ast, arr)
    };
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    let b_caps: Option<(NodeId, NodeId)> = b_anyorder_caps(arr, &cx);
    let c_caps = matches(&ir, &ast, arr, &mut NoPredicates);

    assert!(b_caps.is_some(), "B must match [:foo, 42]");
    assert!(c_caps.is_some(), "C must match [:foo, 42]");

    let (b_slot0, b_slot1) = b_caps.unwrap();
    let c = c_caps.unwrap();
    let c_slot0 = match c.get(0).expect("C slot 0") {
        CaptureValue::Node(n) => *n,
        other => panic!("C slot 0 expected Node, got {other:?}"),
    };
    let c_slot1 = match c.get(1).expect("C slot 1") {
        CaptureValue::Node(n) => *n,
        other => panic!("C slot 1 expected Node, got {other:?}"),
    };

    assert_eq!(b_slot0, c_slot0, "B↔C slot 0 disagree");
    assert_eq!(b_slot1, c_slot1, "B↔C slot 1 disagree");

    // Wildcards assign declaration-order: slot 0 = first element (Sym), slot 1 = second (Int).
    assert!(
        matches!(ast.node(b_slot0).kind, NodeKind::Sym(_)),
        "slot 0 must be Sym (first element), got {:?}",
        ast.node(b_slot0).kind
    );
    assert!(
        matches!(ast.node(b_slot1).kind, NodeKind::Int(_)),
        "slot 1 must be Int (second element), got {:?}",
        ast.node(b_slot1).kind
    );
}

#[test]
fn b_anyorder_backtrack_captures_after_backtrack() {
    // `(array <$a int>)` against [42, :sym].
    //
    // Declaration order is: pattern 0 = `$a` (wildcard), pattern 1 = `int`.
    // A greedy implementation tries `$a`=42, `int`=:sym → fails.
    // Backtracking commits `$a`=:sym, `int`=42 → succeeds.
    // Slot 0 ($a) must be the Sym node (:sym), not the Int node.
    // B and C must agree on both the match result and which node is captured.
    let pat = "(array <$a int>)";
    let ir = compile(pat).unwrap_or_else(|e| panic!("compile `{pat}` failed: {e}"));
    let fns = fns();

    let (ast, arr) = {
        let mut b = AstBuilder::new("", "t.rb");
        let i = b.push(NodeKind::Int(42), r());
        let s_sym = b.intern_symbol("sym");
        let s = b.push(NodeKind::Sym(s_sym), r());
        // Array is [42, :sym] — int first, then sym.
        let list = b.push_list(&[i, s]);
        let arr = b.push(NodeKind::Array(list), r());
        let ast = b.finish(arr);
        (ast, arr)
    };
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    let b_cap: Option<(NodeId,)> = b_anyorder_backtrack_cap(arr, &cx);
    let c_cap = matches(&ir, &ast, arr, &mut NoPredicates);

    assert!(b_cap.is_some(), "B must match [42, :sym] for `<$a int>`");
    assert!(c_cap.is_some(), "C must match [42, :sym] for `<$a int>`");

    let (b_slot0,) = b_cap.unwrap();
    let c_slot0 = match c_cap.unwrap().get(0).expect("C slot 0") {
        CaptureValue::Node(n) => *n,
        other => panic!("C slot 0 expected Node, got {other:?}"),
    };

    assert_eq!(b_slot0, c_slot0, "B↔C slot 0 disagree");

    // After backtracking, $a must be bound to the Sym node (:sym), not Int.
    assert!(
        matches!(ast.node(b_slot0).kind, NodeKind::Sym(_)),
        "slot 0 ($a) must be Sym after backtrack, got {:?}",
        ast.node(b_slot0).kind
    );
}
