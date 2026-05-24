//! `register_cops!(mode = static, …)` and `register_cops!(mode = dynamic, …)`
//! must produce equivalent `PluginRegistration`s for the same cop list
//! (murphy-9cr.23 §12b, design §5).
//!
//! The completion criteria check four points:
//!
//! 1. **ABI version** — both modes return `MURPHY_PLUGIN_ABI_VERSION`.
//! 2. **Cop metadata content** — name / description / default_severity /
//!    default_enabled / kinds / options match byte-for-byte.
//! 3. **Function table shape** — both `PluginCopV1` rows have the same
//!    `size`, `kinds_len`, `options_len`, and a non-null `dispatch` fn
//!    pointer (the *literal* fn pointer address may differ between modes —
//!    each `register_cops!` expansion generates its own thunk — so we do
//!    not require bit equality, per the issue spec).
//! 4. **Observable behaviour** — invoking the per-cop `dispatch` thunk on
//!    the same input produces the same return code (here `0` for a no-op
//!    cop).

use murphy_ast::{Ast, AstBuilder, NodeId, NodeKind, OptNodeId, Range};
use murphy_plugin_api::{
    Cop, Cx, CxRaw, FnTable, MURPHY_PLUGIN_ABI_VERSION, NoOptions, NodeCop, NodeKindTag,
    PluginRegistration, RawEdit, RawOffense, RawSlice, Severity,
};
use murphy_plugin_macros::register_cops;

/// Shared cop used by both modes. Stateless, no options, default-severity
/// `Warning`, dispatches on `Nil` (`NodeKindTag(1)` — declaration order is
/// frozen by ADR 0037).
#[derive(Default)]
struct StubCop;

impl Cop for StubCop {
    type Options = NoOptions;
    const NAME: &'static str = "Plugin/StubEquivalence";
    const DESCRIPTION: &'static str = "Shared fixture for the static/dynamic equivalence test.";
    const DEFAULT_SEVERITY: Option<Severity> = Some(Severity::Warning);
    const DEFAULT_ENABLED: Option<bool> = Some(true);
}

impl NodeCop for StubCop {
    const KINDS: &'static [NodeKindTag] = &[NodeKindTag(1)];
    fn check(&self, _node: NodeId, _cx: &Cx<'_>) {}
}

mod static_pack {
    use super::StubCop;
    use murphy_plugin_macros::register_cops;
    register_cops!(mode = static, StubCop);
}

register_cops!(mode = dynamic, StubCop);

unsafe extern "C" {
    fn murphy_plugin_register(out: *mut PluginRegistration) -> i32;
}

fn empty() -> PluginRegistration {
    PluginRegistration {
        abi_version: 0,
        cops_ptr: std::ptr::null(),
        cops_len: 0,
    }
}

/// Build an arena that just contains a `Nil` root so the dispatch thunk
/// has a real node to dereference through `Cx`.
fn nil_arena() -> (Ast, NodeId) {
    let mut b = AstBuilder::new("nil", "t.rb".to_string());
    let root = b.push(NodeKind::Nil, Range { start: 0, end: 3 });
    let _ = OptNodeId::NONE;
    (b.finish(root), root)
}

unsafe extern "C" fn noop_offense(_: *mut std::ffi::c_void, _: *const RawOffense) {}
unsafe extern "C" fn noop_edit(_: *mut std::ffi::c_void, _: *const RawEdit) {}

fn cx_raw_for<'a>(ast: &'a Ast, fns: &'a FnTable, cop_name: RawSlice) -> CxRaw {
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
        cop_name,
        fns: fns as *const FnTable,
        sink: std::ptr::null_mut(),
        sorted_tokens: p.sorted_tokens.as_ptr(),
        sorted_tokens_len: p.sorted_tokens.len(),
    }
}

#[test]
fn static_and_dynamic_modes_produce_equivalent_registrations() {
    let mut s_reg = empty();
    let s_rc = unsafe { static_pack::murphy_plugin_register(&mut s_reg) };

    let mut d_reg = empty();
    let d_rc = unsafe { murphy_plugin_register(&mut d_reg) };

    assert_eq!(s_rc, 0, "static-mode register must return 0 on success");
    assert_eq!(d_rc, 0, "dynamic-mode register must return 0 on success");

    // (i) ABI version equal.
    assert_eq!(s_reg.abi_version, MURPHY_PLUGIN_ABI_VERSION);
    assert_eq!(s_reg.abi_version, d_reg.abi_version);

    // (ii) cops count equal.
    assert_eq!(s_reg.cops_len, 1);
    assert_eq!(s_reg.cops_len, d_reg.cops_len);

    let s_cops = unsafe { std::slice::from_raw_parts(s_reg.cops_ptr, s_reg.cops_len) };
    let d_cops = unsafe { std::slice::from_raw_parts(d_reg.cops_ptr, d_reg.cops_len) };

    for (s, d) in s_cops.iter().zip(d_cops.iter()) {
        // (iii) function table shape — sizes match, kinds_len / options_len
        // match. The dispatch fn pointers are type-system non-null; their
        // literal addresses may differ (per task spec) so we do not assert
        // pointer equality.
        assert_eq!(s.size, d.size);
        assert_eq!(
            s.size,
            std::mem::size_of::<murphy_plugin_api::PluginCopV1>()
        );
        assert_eq!(s.kinds_len, d.kinds_len);
        assert_eq!(s.options_len, d.options_len);

        // (ii) cop metadata content byte-equal.
        assert_eq!(unsafe { s.name.as_bytes() }, unsafe { d.name.as_bytes() });
        assert_eq!(unsafe { s.name.as_bytes() }, b"Plugin/StubEquivalence");
        assert_eq!(unsafe { s.description.as_bytes() }, unsafe {
            d.description.as_bytes()
        },);
        assert_eq!(s.default_severity, d.default_severity);
        assert_eq!(
            s.default_severity,
            Severity::to_wire(Some(Severity::Warning))
        );
        assert_eq!(s.default_enabled, d.default_enabled);

        // (ii continued) kinds slice content-equal.
        let s_kinds = unsafe { std::slice::from_raw_parts(s.kinds_ptr, s.kinds_len) };
        let d_kinds = unsafe { std::slice::from_raw_parts(d.kinds_ptr, d.kinds_len) };
        assert_eq!(s_kinds, d_kinds);
        assert_eq!(s_kinds, &[NodeKindTag(1)]);
    }
}

#[test]
fn static_and_dynamic_dispatch_observably_equal() {
    // (iv) Calling the per-cop dispatch thunk for a no-op cop on a real
    // arena node must return 0 (success) from both modes. The thunk uses
    // `catch_unwind`; a non-zero return would indicate it trapped a panic
    // somewhere in `Cx::from_raw` / `StubCop::check`, which would tell us
    // the two emissions had diverged.
    let mut s_reg = empty();
    let _ = unsafe { static_pack::murphy_plugin_register(&mut s_reg) };
    let s_cop = unsafe { &*s_reg.cops_ptr };

    let mut d_reg = empty();
    let _ = unsafe { murphy_plugin_register(&mut d_reg) };
    let d_cop = unsafe { &*d_reg.cops_ptr };

    let (ast, root) = nil_arena();
    let fns = FnTable {
        emit_offense: noop_offense,
        emit_edit: noop_edit,
    };
    let raw = cx_raw_for(&ast, &fns, s_cop.name);

    let s_rc = unsafe { (s_cop.dispatch)(root, &raw as *const CxRaw) };
    let d_rc = unsafe { (d_cop.dispatch)(root, &raw as *const CxRaw) };

    assert_eq!(s_rc, 0);
    assert_eq!(s_rc, d_rc);
}
