//! Behaviour tests for the `#[cop]` / `#[on_node]` proc-macro (murphy-9cr.8.4).
//!
//! These are **runtime** integration tests (not trybuild). Each test builds a
//! small AST with `AstBuilder`, constructs a `Cx` over it, and drives the
//! `NodeCop` implementations produced by the macro.

use std::sync::atomic::{AtomicU32, Ordering};

use murphy_ast::{AstBuilder, NodeId, NodeKind, NodeList, OptNodeId, Range};
use murphy_plugin_api::{
    Cop, CopOptions, Cx, CxRaw, FnTable, NoOptions, NodeCop, NodeKindTag, RawEdit, RawOffense,
    RawSlice, RubyVersion, Severity,
};
use murphy_plugin_macros::cop;

// ── Tag constants (declaration-order discriminants, frozen by ADR 0037) ──────
//
// send=17, if=25, case=26, when=27, def=32
const TAG_SEND: u8 = 17;
const TAG_IF: u8 = 25;
const TAG_CASE: u8 = 26;
const TAG_WHEN: u8 = 27;
const TAG_DEF: u8 = 32;

// ── Noop ABI callbacks ────────────────────────────────────────────────────────

unsafe extern "C" fn noop_offense(_: *mut std::ffi::c_void, _: *const RawOffense) {}
unsafe extern "C" fn noop_edit(_: *mut std::ffi::c_void, _: *const RawEdit) {}

/// Build a `CxRaw` borrowing from `ast` and `fns`.
///
/// # Safety
/// The returned `CxRaw` contains raw pointers into `ast` and `fns`; the
/// caller must keep both alive for the `CxRaw`'s lifetime.
fn cx_raw_for<'a>(ast: &'a murphy_ast::Ast, fns: &'a FnTable) -> CxRaw {
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
        file_path: RawSlice::from_str("t.rb"),
        target_rails_version: 0,
        active_support_extensions_enabled: false,
        config_disabled_cops: std::ptr::null(),
        config_disabled_cops_len: 0,
    }
}

// ── Fixture helpers ───────────────────────────────────────────────────────────

/// Push a `Send` node (`recv.method(args…)`) onto `b` and return its `NodeId`.
fn push_send(b: &mut AstBuilder) -> NodeId {
    push_send_named(b, "foo")
}

/// Push a bare `Send` (no receiver) whose method symbol is `name`.
fn push_send_named(b: &mut AstBuilder, name: &str) -> NodeId {
    let method = b.intern_symbol(name);
    b.push(
        NodeKind::Send {
            receiver: OptNodeId::NONE,
            method,
            args: NodeList::EMPTY,
        },
        Range { start: 0, end: 5 },
    )
}

/// Push an `If` node (`if true; nil; end`) onto `b` and return its `NodeId`.
fn push_if(b: &mut AstBuilder) -> NodeId {
    let cond = b.push(NodeKind::True_, Range { start: 3, end: 7 });
    b.push(
        NodeKind::If {
            cond,
            then_: OptNodeId::NONE,
            else_: OptNodeId::NONE,
        },
        Range { start: 0, end: 10 },
    )
}

/// Push a `Nil` node onto `b` and return its `NodeId`.
fn push_nil(b: &mut AstBuilder) -> NodeId {
    b.push(NodeKind::Nil, Range { start: 0, end: 3 })
}

// ── Cop definitions (must live at module scope — `#[cop]` emits `impl` items) ─

// --- T1: three methods, three different kinds --------------------------------

#[derive(Default)]
struct T1;

#[cop(name = "Plugin/T1")]
impl T1 {
    #[on_node(kind = "send")]
    fn check_send(&self, _node: NodeId, _cx: &Cx<'_>) {}

    #[on_node(kind = "if")]
    fn check_if(&self, _node: NodeId, _cx: &Cx<'_>) {}

    #[on_node(kind = "def")]
    fn check_def(&self, _node: NodeId, _cx: &Cx<'_>) {}
}

// --- T2: one method, three #[on_node] attrs ----------------------------------

#[derive(Default)]
struct T2;

#[cop(name = "Plugin/T2")]
impl T2 {
    #[on_node(kind = "if")]
    #[on_node(kind = "case")]
    #[on_node(kind = "when")]
    fn check_branch(&self, _node: NodeId, _cx: &Cx<'_>) {}
}

// --- T3: dispatch counter by kind -------------------------------------------

// `AtomicU32` lets a `&self` method mutate a counter while satisfying the
// `Cop: Send + Sync + 'static` bound naturally — no `unsafe impl` needed.
// Real cops are stateless (see ADR 0035 / `dispatch_thunk`'s `C::default()`);
// these counters are only here to observe dispatch in tests.

#[derive(Default)]
struct T3 {
    send_count: AtomicU32,
    if_count: AtomicU32,
}

#[cop(name = "Plugin/T3")]
impl T3 {
    #[on_node(kind = "send")]
    fn check_send(&self, _node: NodeId, _cx: &Cx<'_>) {
        self.send_count.fetch_add(1, Ordering::Relaxed);
    }

    #[on_node(kind = "if")]
    fn check_if(&self, _node: NodeId, _cx: &Cx<'_>) {
        self.if_count.fetch_add(1, Ordering::Relaxed);
    }
}

// --- T4: no-op for kinds outside KINDS array --------------------------------

#[derive(Default)]
struct T4 {
    count: AtomicU32,
}

#[cop(name = "Plugin/T4")]
impl T4 {
    #[on_node(kind = "send")]
    fn check_send(&self, _node: NodeId, _cx: &Cx<'_>) {
        self.count.fetch_add(1, Ordering::Relaxed);
    }
}

// --- T5: full metadata propagation ------------------------------------------

#[derive(Default)]
struct T5;

#[cop(
    name = "Plugin/T5",
    description = "Catches T5 issues.",
    default_severity = "warning",
    default_enabled = false,
    minimum_target_ruby_version = "3.2",
    safe = false,
    safe_autocorrect = false,
    options = NoOptions
)]
impl T5 {
    #[on_node(kind = "send")]
    fn check_send(&self, _node: NodeId, _cx: &Cx<'_>) {}
}

// --- T6: build_cop / register_cops integration ------------------------------

#[derive(Default)]
struct T6;

#[cop(name = "Plugin/T6")]
impl T6 {
    #[on_node(kind = "send")]
    fn check_send(&self, _node: NodeId, _cx: &Cx<'_>) {}
}

// Emit the `murphy_plugin_register` entry point and register T6.
// Compilation success proves `register_cops!` + `#[cop]` + `submit_cop!` compose correctly.
murphy_plugin_macros::register_cops!(mode = dynamic);
murphy_plugin_api::submit_cop!(T6);

// --- T7: methods filter on send (murphy-34d) --------------------------------

#[derive(Default)]
struct T7 {
    hits: AtomicU32,
}

#[cop(name = "Plugin/T7")]
impl T7 {
    #[on_node(kind = "send", methods = ["describe", "context"])]
    fn check_send(&self, _node: NodeId, _cx: &Cx<'_>) {
        self.hits.fetch_add(1, Ordering::Relaxed);
    }
}

// --- T8/T9: macro-injected runtime options ----------------------------------

#[derive(CopOptions)]
struct InjectedOptions {
    #[option(default = "default")]
    mode: String,
}

#[derive(Default)]
struct T8 {
    hits: AtomicU32,
}

#[cop(name = "Plugin/T8", options = InjectedOptions)]
impl T8 {
    #[on_node(kind = "send")]
    fn check_send(&self, _node: NodeId, _cx: &Cx<'_>, options: &InjectedOptions) {
        if options.mode == "configured" {
            self.hits.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[derive(Default)]
struct T9 {
    hits: AtomicU32,
}

#[cop(name = "Plugin/T9", options = InjectedOptions)]
impl T9 {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>, options: &InjectedOptions) {
        if options.mode == "configured" {
            self.hits.fetch_add(1, Ordering::Relaxed);
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[test]
fn kinds_array_contains_expected_tags_in_method_order() {
    assert_eq!(
        <T1 as NodeCop>::KINDS,
        &[
            NodeKindTag(TAG_SEND),
            NodeKindTag(TAG_IF),
            NodeKindTag(TAG_DEF),
        ]
    );
}

#[test]
fn kinds_array_unions_multiple_on_node_attrs_on_one_method() {
    assert_eq!(
        <T2 as NodeCop>::KINDS,
        &[
            NodeKindTag(TAG_IF),
            NodeKindTag(TAG_CASE),
            NodeKindTag(TAG_WHEN),
        ]
    );
}

#[test]
fn check_dispatches_to_correct_method_by_node_kind_tag() {
    // Build a tree: `begin(send_node, if_node)`.
    let mut b = AstBuilder::new("foo; if true; end", "t.rb".to_string());
    let send_id = push_send(&mut b);
    let if_id = push_if(&mut b);
    let begin_list = b.push_list(&[send_id, if_id]);
    let root = b.push(NodeKind::Begin(begin_list), Range { start: 0, end: 17 });
    let ast = b.finish(root);

    let fns = FnTable {
        emit_offense: noop_offense,
        emit_edit: noop_edit,
    };
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    let t3 = T3::default();

    // Dispatch send node — only send_count must increase.
    t3.check(send_id, &cx);
    assert_eq!(
        t3.send_count.load(Ordering::Relaxed),
        1,
        "send_count should be 1 after send dispatch"
    );
    assert_eq!(
        t3.if_count.load(Ordering::Relaxed),
        0,
        "if_count should be 0 after send dispatch"
    );

    // Dispatch if node — only if_count must increase.
    t3.check(if_id, &cx);
    assert_eq!(
        t3.send_count.load(Ordering::Relaxed),
        1,
        "send_count should still be 1 after if dispatch"
    );
    assert_eq!(
        t3.if_count.load(Ordering::Relaxed),
        1,
        "if_count should be 1 after if dispatch"
    );
}

#[test]
fn check_is_no_op_for_kinds_outside_kinds_array() {
    // T4 only registers "send"; dispatching a Nil must not increment its counter.
    let mut b = AstBuilder::new("nil", "t.rb".to_string());
    let nil_id = push_nil(&mut b);
    let ast = b.finish(nil_id);

    let fns = FnTable {
        emit_offense: noop_offense,
        emit_edit: noop_edit,
    };
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    let t4 = T4::default();
    t4.check(nil_id, &cx);

    assert_eq!(
        t4.count.load(Ordering::Relaxed),
        0,
        "count should remain 0 for a Nil node (not in KINDS)"
    );
}

#[test]
fn methods_filter_lowers_to_send_methods_const() {
    // The host applies the filter via `PluginCopV1::send_methods_*`
    // (murphy-ip0); the cop body itself unconditionally invokes its
    // check method. So the macro contract this test pins is "the
    // `methods = [...]` array is lowered into the `SEND_METHODS`
    // associated const in declaration order". The actual host-level
    // filter behaviour is covered by `dispatch_pre_filters_send_by_method_name`
    // in `murphy_core::dispatch`.
    let lowered: Vec<&str> = <T7 as NodeCop>::SEND_METHODS
        .iter()
        .map(|s| std::str::from_utf8(unsafe { s.as_bytes() }).unwrap())
        .collect();
    assert_eq!(lowered, vec!["describe", "context"]);
}

#[test]
fn unfiltered_send_cop_has_empty_send_methods() {
    // T4 declares `#[on_node(kind = "send")]` without `methods`. The
    // associated const must default to empty so the host applies no
    // filter — preserving the historical "every Send reaches the cop"
    // contract for cops that don't opt in to `restrict_on_send`.
    assert!(<T4 as NodeCop>::SEND_METHODS.is_empty());
}

#[test]
fn on_node_method_can_receive_decoded_options() {
    let mut b = AstBuilder::new("foo", "t.rb".to_string());
    let send_id = push_send(&mut b);
    let ast = b.finish(send_id);

    let fns = FnTable {
        emit_offense: noop_offense,
        emit_edit: noop_edit,
    };
    let mut raw = cx_raw_for(&ast, &fns);
    raw.options_json = RawSlice::from_str(r#"{"mode":"configured"}"#);
    let cx = unsafe { Cx::from_raw(&raw) };

    let t8 = T8::default();
    t8.check(send_id, &cx);

    assert_eq!(t8.hits.load(Ordering::Relaxed), 1);
}

#[test]
fn on_new_investigation_method_can_receive_decoded_options() {
    let mut b = AstBuilder::new("nil", "t.rb".to_string());
    let root = push_nil(&mut b);
    let ast = b.finish(root);

    let fns = FnTable {
        emit_offense: noop_offense,
        emit_edit: noop_edit,
    };
    let mut raw = cx_raw_for(&ast, &fns);
    raw.options_json = RawSlice::from_str(r#"{"mode":"configured"}"#);
    let cx = unsafe { Cx::from_raw(&raw) };

    let t9 = T9::default();
    t9.check(root, &cx);

    assert_eq!(t9.hits.load(Ordering::Relaxed), 1);
}

#[test]
fn cop_metadata_propagates_to_trait_consts() {
    assert_eq!(<T5 as Cop>::NAME, "Plugin/T5");
    assert_eq!(<T5 as Cop>::DESCRIPTION, "Catches T5 issues.");
    assert_eq!(
        <T5 as Cop>::DEFAULT_SEVERITY,
        Some(Severity::Warning),
        "default_severity should be Warning"
    );
    assert_eq!(
        <T5 as Cop>::DEFAULT_ENABLED,
        Some(false),
        "default_enabled should be Some(false)"
    );
    assert_eq!(<T5 as Cop>::SAFE, Some(false), "safe should be Some(false)");
    assert_eq!(
        <T5 as Cop>::MINIMUM_TARGET_RUBY_VERSION,
        Some(RubyVersion::new(3, 2)),
        "minimum_target_ruby_version should be 3.2"
    );
    assert_eq!(
        <T5 as Cop>::SAFE_AUTOCORRECT,
        Some(false),
        "safe_autocorrect should be Some(false)"
    );
    // NoOptions has an empty SCHEMA.
    assert_eq!(
        <<T5 as Cop>::Options as CopOptions>::SCHEMA.len(),
        0,
        "NoOptions schema must be empty"
    );
}

#[test]
fn register_cops_integration_emits_pluginv1_table_via_internal_build_cop() {
    use murphy_plugin_api::__internal::build_cop;

    // `build_cop` is `const fn`; calling it at runtime is also fine.
    let cop_v1 = build_cop::<T6>();

    // name == "Plugin/T6"
    let name_bytes = unsafe { cop_v1.name.as_bytes() };
    assert_eq!(
        std::str::from_utf8(name_bytes).unwrap(),
        "Plugin/T6",
        "cop name must be Plugin/T6"
    );
    assert_eq!(
        cop_v1.safe,
        murphy_plugin_api::tristate_to_wire(None),
        "safe metadata defaults to unset"
    );
    assert_eq!(
        cop_v1.safe_autocorrect,
        murphy_plugin_api::tristate_to_wire(None),
        "safe_autocorrect metadata defaults to unset"
    );

    // kinds_len == 1 (only "send")
    assert_eq!(cop_v1.kinds_len, 1, "T6 registers exactly one kind (send)");

    // The single kind tag is TAG_SEND.
    let kinds: &[NodeKindTag] =
        unsafe { std::slice::from_raw_parts(cop_v1.kinds_ptr, cop_v1.kinds_len) };
    assert_eq!(kinds, &[NodeKindTag(TAG_SEND)]);

    // dispatch function pointer is non-null.
    assert!(
        (cop_v1.dispatch as usize) != 0,
        "dispatch function pointer must not be null"
    );
}
