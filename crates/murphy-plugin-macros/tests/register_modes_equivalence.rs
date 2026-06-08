//! Runtime behaviour of the `register_cops!(mode = dynamic)` +
//! `submit_cop!` registration pair (murphy-9cr.23 §12b, updated for
//! submit_cop! distributed registration).
//!
//! static and dynamic modes use identical `PACK_COPS` collection — the
//! only difference is the export shape of `murphy_plugin_register` — so
//! a single dynamic-mode smoke test is sufficient here.

use murphy_ast::{Ast, AstBuilder, NodeId, NodeKind, OptNodeId, Range};
use murphy_plugin_api::{
    Cop, Cx, CxRaw, FnTable, MURPHY_PLUGIN_ABI_VERSION, NoOptions, NodeCop, NodeKindTag,
    PluginRegistration, RawEdit, RawOffense, RawSlice, Severity,
};
use murphy_plugin_macros::register_cops;

#[derive(Default)]
struct StubCop;

impl Cop for StubCop {
    type Options = NoOptions;
    const NAME: &'static str = "Plugin/StubEquivalence";
    const DESCRIPTION: &'static str = "Shared fixture for the registration smoke test.";
    const DEFAULT_SEVERITY: Option<Severity> = Some(Severity::Warning);
    const DEFAULT_ENABLED: Option<bool> = Some(true);
}

impl NodeCop for StubCop {
    const KINDS: &'static [NodeKindTag] = &[NodeKindTag(1)];
    fn check(&self, _node: NodeId, _cx: &Cx<'_>) {}
}

register_cops!(mode = dynamic);
murphy_plugin_api::submit_cop!(StubCop);

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

#[test]
fn register_entry_point_fills_the_plugin_registration() {
    let mut reg = empty();
    let rc = unsafe { murphy_plugin_register(&mut reg) };

    assert_eq!(rc, 0);
    assert_eq!(reg.abi_version, MURPHY_PLUGIN_ABI_VERSION);
    assert_eq!(reg.cops_len, 1);

    let cops = unsafe { std::slice::from_raw_parts(reg.cops_ptr, reg.cops_len) };

    assert_eq!(
        unsafe { cops[0].name.as_bytes() },
        b"Plugin/StubEquivalence"
    );
    assert_eq!(
        cops[0].size,
        std::mem::size_of::<murphy_plugin_api::PluginCopV1>()
    );
    assert_eq!(cops[0].kinds_len, 1);
    assert_eq!(
        cops[0].default_severity,
        Severity::to_wire(Some(Severity::Warning))
    );
}

#[test]
fn dispatch_thunk_returns_zero_for_noop_cop() {
    let mut reg = empty();
    let _ = unsafe { murphy_plugin_register(&mut reg) };
    let cop = unsafe { &*reg.cops_ptr };

    let (ast, root) = nil_arena();
    let fns = FnTable {
        emit_offense: noop_offense,
        emit_edit: noop_edit,
    };
    let raw = cx_raw_for(&ast, &fns, cop.name);
    let rc = unsafe { (cop.dispatch)(root, &raw as *const CxRaw) };
    assert_eq!(rc, 0);
}
