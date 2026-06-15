//! Arena dispatch host (murphy-9cr.22).
//!
//! Walks a [`murphy_ast::Ast`] arena once and invokes every cop registered
//! for each visited node's kind. The host is the single integration point
//! between the arena AST and the plugin API: built-in cops and `.so`-loaded
//! plugin cops are dispatched through the same `PluginCopV1` table (ADR
//! 0038), so there is no Built-In vs Plugin code path here — only one.
//!
//! ## Iteration order
//!
//! Outer = cop, inner = matched node (design §4.4). The per-cop `CxRaw` is
//! built once from `Ast::raw_parts()`, and only the `cop_name` field is
//! restamped before each cop's run; the inner node loop reuses the same
//! `CxRaw` for every dispatch call from that cop. The alternative — outer
//! node, inner cop — would restamp `cop_name` once per (node, cop) pair, an
//! avoidable N×M write.
//!
//! Walk order over the arena is the arena's push order (the translator does
//! a post-order DFS). The aggregator's sort key is content-based (ADR 0006 /
//! 0011), so walk order has no observable effect on the output offense list.
//!
//! ## Per-cop fault isolation
//!
//! A cop's `dispatch` thunk returns `0` on success, non-zero when the thunk
//! caught a panic in the cop's `check()` (the thunk lives in
//! `register_cops!`, murphy-9cr.21). On non-zero the host disables the
//! offending cop for the remainder of the current file, prints a one-line
//! diagnostic to stderr, and continues with the next cop — matching ADR
//! 0033's per-cop fault isolation contract.

use std::ffi::c_void;

use murphy_ast::{Ast, NodeId, NodeKind};
use murphy_plugin_api::var_semantic_model::VarSemanticModel;
use murphy_plugin_api::{
    AllCopsContext, CxRaw, FnTable, NodeKindTag as PluginNodeKindTag, PluginCopV1, RawEdit,
    RawOffense, RawSlice, RubyVersion, SEVERITY_UNSET,
};

/// The `NodeKindTag` for [`NodeKind::Send`] (frozen by ADR 0037).
/// Mirrors `murphy-plugin-api::NodeKindTag::of(&NodeKind::Send {…})`
/// but kept as a free constant so the host pre-filter loop does not
/// pay a per-node tag-of recomputation.
const SEND_TAG: u8 = 17;

use crate::offense::{Autocorrect, Edit, Offense, Range, Severity};

/// Per-file dispatch sink. Owns the offense + edit storage threaded through
/// `FNS` callbacks during a `run_cops` call.
pub struct OffenseSink {
    file: String,
    offenses: Vec<Offense>,
}

impl OffenseSink {
    /// Build a fresh sink for `file`.
    pub fn new(file: impl Into<String>) -> Self {
        Self {
            file: file.into(),
            offenses: Vec::new(),
        }
    }

    /// Borrow the offenses recorded so far.
    pub fn offenses(&self) -> &[Offense] {
        &self.offenses
    }

    /// Take the recorded offenses, consuming the sink.
    pub fn into_offenses(self) -> Vec<Offense> {
        self.offenses
    }
}

/// Convert plugin-api's `Range` (byte range over the source) into core's
/// `Range`. Both are `u32` byte offsets; the field names differ.
fn convert_range(r: murphy_ast::Range) -> Range {
    Range {
        start_offset: r.start,
        end_offset: r.end,
    }
}

/// Decode a severity wire byte. `SEVERITY_UNSET` and unknown bytes fall back
/// to `Warning` — the v1 host default; later issues may consult a cop's
/// `default_severity` here.
fn decode_severity(byte: u8) -> Severity {
    match byte {
        0 => Severity::Warning,
        1 => Severity::Error,
        _ => Severity::Warning,
    }
}

/// Host callback for `FnTable::emit_offense`. Renders a `RawOffense` into a
/// fresh [`Offense`] and appends it to the sink's offense list. Subsequent
/// `emit_edit` callbacks attach to the offense just pushed.
///
/// # Safety
/// `sink_ptr` must point to a live `OffenseSink`; `o_ptr` must point to a
/// `RawOffense` valid for the call. The arena and source the `RawSlice`s
/// reference must outlive the call.
unsafe extern "C" fn host_emit_offense(sink_ptr: *mut c_void, o_ptr: *const RawOffense) {
    let sink = unsafe { &mut *(sink_ptr as *mut OffenseSink) };
    let o = unsafe { &*o_ptr };
    let cop_name = String::from_utf8_lossy(unsafe { o.cop_name.as_bytes() }).into_owned();
    let message = String::from_utf8_lossy(unsafe { o.message.as_bytes() }).into_owned();
    let range = convert_range(o.range);
    let severity = decode_severity(o.severity);
    let file = sink.file.clone();
    sink.offenses
        .push(Offense::new(&file, &cop_name, range, severity, &message));
}

/// Host callback for `FnTable::emit_edit`. Attaches the edit to the most
/// recently pushed offense — the v1 correlation rule (a cop emits one
/// offense and then zero-or-more edits for it). Edits pushed before any
/// offense are dropped; the caller would have nothing to attach them to.
///
/// # Safety
/// See [`host_emit_offense`].
unsafe extern "C" fn host_emit_edit(sink_ptr: *mut c_void, e_ptr: *const RawEdit) {
    let sink = unsafe { &mut *(sink_ptr as *mut OffenseSink) };
    let e = unsafe { &*e_ptr };
    let replacement = String::from_utf8_lossy(unsafe { e.replacement.as_bytes() }).into_owned();
    let range = convert_range(e.range);
    if let Some(latest) = sink.offenses.last_mut() {
        let ac = latest
            .autocorrect
            .get_or_insert_with(|| Autocorrect { edits: Vec::new() });
        ac.edits.push(Edit { range, replacement });
    }
}

/// The host's static FnTable — function pointers do not change during a run.
static FNS: FnTable = FnTable {
    emit_offense: host_emit_offense,
    emit_edit: host_emit_edit,
};

#[derive(Default)]
struct NodeSliceArena {
    slices: Vec<Box<[NodeId]>>,
}

unsafe extern "C" fn alloc_node_slice(
    arena: *mut c_void,
    ptr: *const NodeId,
    len: usize,
) -> *const NodeId {
    let arena = unsafe { &mut *(arena as *mut NodeSliceArena) };
    let elements = unsafe { std::slice::from_raw_parts(ptr, len) };
    let boxed = elements.to_vec().into_boxed_slice();
    let out = boxed.as_ptr();
    arena.slices.push(boxed);
    out
}

/// Per-kind node index over an arena: `nodes_by_kind[tag]` is every node id
/// whose `NodeKind` discriminant byte is `tag`. Built once per arena.
pub(crate) struct DispatchIndex {
    nodes_by_kind: Vec<Vec<NodeId>>,
}

impl DispatchIndex {
    /// Walk the arena once, bucketing each node id by its kind tag.
    pub(crate) fn build(ast: &Ast) -> Self {
        let mut nodes_by_kind: Vec<Vec<NodeId>> = (0..256).map(|_| Vec::new()).collect();
        let n = ast.len();
        for i in 0..n {
            let id = NodeId(i as u32);
            let tag = PluginNodeKindTag::of(ast.kind(id)).0 as usize;
            nodes_by_kind[tag].push(id);
        }
        Self { nodes_by_kind }
    }

    /// Borrow the bucket for `tag`.
    pub(crate) fn nodes_for(&self, tag: PluginNodeKindTag) -> &[NodeId] {
        &self.nodes_by_kind[tag.0 as usize]
    }
}

/// Build the `CxRaw` template used for every dispatch call in one run. Only
/// `cop_name` is restamped per cop (and `sink` is the host's, shared).
fn build_cx_raw(
    ast: &Ast,
    sink: &mut OffenseSink,
    var_model: &VarSemanticModel,
    node_slice_arena: &mut NodeSliceArena,
    ctx: AllCopsContext,
    config_disabled_cops: &[RawSlice],
) -> CxRaw {
    let p = ast.raw_parts();
    let file_path = ast.path().to_str().unwrap_or("");
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
        fns: &FNS as *const FnTable,
        sink: sink as *mut OffenseSink as *mut c_void,
        sorted_tokens: p.sorted_tokens.as_ptr(),
        sorted_tokens_len: p.sorted_tokens.len(),
        options_json: RawSlice::from_str("{}"),
        call_closing_locs: p.call_closing_locs.as_ptr(),
        call_closing_locs_len: p.call_closing_locs.len(),
        call_operator_locs: p.call_operator_locs.as_ptr(),
        call_operator_locs_len: p.call_operator_locs.len(),
        var_model: var_model as *const VarSemanticModel,
        node_slice_arena: node_slice_arena as *mut NodeSliceArena as *mut c_void,
        alloc_node_slice,
        file_path: RawSlice {
            ptr: file_path.as_ptr(),
            len: file_path.len(),
        },
        target_rails_version: RubyVersion::to_wire(ctx.target_rails_version),
        active_support_extensions_enabled: ctx.active_support_extensions_enabled,
        indentation_width: ctx.indentation_width_wire(),
        target_ruby_version: RubyVersion::to_wire(ctx.target_ruby_version),
        config_disabled_cops: config_disabled_cops.as_ptr(),
        config_disabled_cops_len: config_disabled_cops.len(),
        block_forwarding_explicit: ctx.block_forwarding_explicit,
    }
}

/// `true` when `node_id` is a `Send` whose `method` symbol resolves to
/// one of `allow_list`. Used by the host pre-filter (murphy-ip0); a
/// non-Send node is a category error here and returns `false` (the
/// dispatch loop only applies this on tags that are known-Send).
fn send_method_passes(ast: &Ast, node_id: NodeId, allow_list: &[RawSlice]) -> bool {
    let NodeKind::Send { method, .. } = *ast.kind(node_id) else {
        return false;
    };
    let m_bytes = ast.interner().resolve(method.0).as_bytes();
    allow_list
        .iter()
        .any(|slot| unsafe { slot.as_bytes() } == m_bytes)
}

/// Run every cop in `cops` over `ast`, recording offenses + edits into
/// `sink`. The cop order is the order of `cops`; matching nodes are visited
/// in arena push order. A non-zero dispatch return (panic-trap) disables
/// that cop for the rest of the file.
///
/// The cop references' lifetime is intentionally elided: built-ins are
/// `'static` while pack-loaded cops are bounded by the
/// [`crate::CopRegistry`] (which owns the `dlopen` handle). Both flow
/// through this signature without re-asserting `&'static`.
pub fn run_cops(ast: &Ast, cops: &[&PluginCopV1], sink: &mut OffenseSink) {
    run_cops_with_options(ast, cops, sink, |_| b"{}".to_vec());
}

pub fn run_cops_with_options(
    ast: &Ast,
    cops: &[&PluginCopV1],
    sink: &mut OffenseSink,
    options_for: impl FnMut(&str) -> Vec<u8>,
) {
    // Default context: this option-only entry point carries no resolved config;
    // the native SymbolProc consumer reaches the flag via the cli path that
    // calls `run_cops_with_options_and_context` directly.
    run_cops_with_options_and_context(ast, cops, sink, AllCopsContext::default(), &[], options_for);
}

pub fn run_cops_with_options_and_context(
    ast: &Ast,
    cops: &[&PluginCopV1],
    sink: &mut OffenseSink,
    ctx: AllCopsContext,
    // Run-wide config-disabled cop seed for `Cx::extra_enabled_directives()`.
    // Kept as a separate borrowed param rather than bundled into the
    // `Copy`/`Default` `AllCopsContext`, which a `&[RawSlice]` would burden with
    // a lifetime parameter. Bundle here if the positional list grows further.
    config_disabled_cops: &[RawSlice],
    mut options_for: impl FnMut(&str) -> Vec<u8>,
) {
    let var_model = VarSemanticModel::build(ast);
    let index = DispatchIndex::build(ast);
    let mut node_slice_arena = NodeSliceArena::default();
    let mut base = build_cx_raw(
        ast,
        sink,
        &var_model,
        &mut node_slice_arena,
        ctx,
        config_disabled_cops,
    );
    for cop in cops {
        base.cop_name = cop.name;
        let name = std::str::from_utf8(unsafe { cop.name.as_bytes() }).unwrap_or("");
        let options_json = options_for(name);
        base.options_json = RawSlice {
            ptr: options_json.as_ptr(),
            len: options_json.len(),
        };
        // Per-cop kind list. **Empty `KINDS` means file-visit**: the cop
        // is invoked exactly once with `ast.root()` instead of being
        // dispatched per matching node. ADR 0038 deletes the `FileCop`
        // trait — every cop is still a `NodeCop` and still receives a
        // `NodeId` — but a `NodeCop` with `KINDS = &[]` is the
        // intentional degenerate form for whole-file cops like
        // `Layout/TrailingWhitespace`, which scan `cx.raw_source(range)`
        // over `cx.range(root)`. The dispatcher is the contract surface
        // for this semantic; the trait doc on `NodeCop` cross-references
        // back here.
        if cop.kinds_len == 0 {
            let root = ast.root();
            let rc = unsafe { (cop.dispatch)(root, &base) };
            if rc != 0 {
                let name = std::str::from_utf8(unsafe { cop.name.as_bytes() })
                    .unwrap_or("<invalid cop name>");
                eprintln!(
                    "murphy: cop '{name}' returned non-zero ({rc}) on file-visit; \
                     disabling for this file"
                );
            }
            continue;
        }
        let kinds: &[PluginNodeKindTag] =
            unsafe { std::slice::from_raw_parts(cop.kinds_ptr, cop.kinds_len) };
        // Host-level `restrict_on_send` pre-filter (murphy-ip0). Empty
        // slice ⇒ no filter; non-empty ⇒ every Send node is skipped
        // unless its method symbol resolves to one of the listed
        // names. The cop's `dispatch` thunk is **not invoked** for
        // off-list sends — saves the FFI hop + cop body wakeup.
        // Filtering on non-send kinds is meaningless and rejected at
        // the `#[cop]` parse site, so we apply it only on the Send tag.
        let send_methods: &[RawSlice] = if cop.send_methods_len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(cop.send_methods_ptr, cop.send_methods_len) }
        };
        let mut disabled = false;
        for tag in kinds {
            if disabled {
                break;
            }
            let apply_send_filter = !send_methods.is_empty() && tag.0 == SEND_TAG;
            for &node_id in index.nodes_for(*tag) {
                if apply_send_filter && !send_method_passes(ast, node_id, send_methods) {
                    continue;
                }
                let rc = unsafe { (cop.dispatch)(node_id, &base) };
                if rc != 0 {
                    let name = std::str::from_utf8(unsafe { cop.name.as_bytes() })
                        .unwrap_or("<invalid cop name>");
                    eprintln!(
                        "murphy: cop '{name}' returned non-zero ({rc}) dispatching node {}; \
                         disabling for this file",
                        node_id.0
                    );
                    disabled = true;
                    break;
                }
            }
        }
    }
    // Touch the constant so future refactors that drop the use line don't
    // silently lose the `SEVERITY_UNSET` import — it's the documented
    // sentinel for `RawOffense::severity` and likely needed once cops
    // consult `default_severity` here.
    let _ = SEVERITY_UNSET;
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use murphy_ast::{AstBuilder, NodeKind, OptNodeId};
    use murphy_plugin_api::{NodeKindTag as PluginNodeKindTag, PluginCopV1, RawSlice};

    /// Build `nil; 1` — root `Begin([Nil, Int(1)])`.
    fn ast_nil_and_int() -> Ast {
        let mut b = AstBuilder::new("nil; 1", "t.rb");
        let nil = b.push(NodeKind::Nil, murphy_ast::Range { start: 0, end: 3 });
        let one = b.push(NodeKind::Int(1), murphy_ast::Range { start: 5, end: 6 });
        let list = b.push_list(&[nil, one]);
        let root = b.push(
            NodeKind::Begin(list),
            murphy_ast::Range { start: 0, end: 6 },
        );
        b.finish(root)
    }

    /// Build `puts "x"` — root `Send { receiver: None, method: "puts",
    /// args: ["x"] }`. The Str literal is included so the arena has more
    /// than one node and tests can distinguish kinds.
    fn ast_puts_x() -> Ast {
        let mut b = AstBuilder::new("puts \"x\"", "t.rb");
        let s = b.intern_string("x");
        let str_node = b.push(NodeKind::Str(s), murphy_ast::Range { start: 5, end: 8 });
        let args = b.push_list(&[str_node]);
        let method = b.intern_symbol("puts");
        let root = b.push(
            NodeKind::Send {
                receiver: OptNodeId::NONE,
                method,
                args,
            },
            murphy_ast::Range { start: 0, end: 8 },
        );
        b.finish(root)
    }

    // === Test cop scaffolding =================================================
    //
    // Every test that needs a `PluginCopV1` defines `unsafe extern "C" fn`
    // dispatch thunks (the FFI signature is non-negotiable) and wraps them
    // in a `static PluginCopV1`. Atomics observe call counts WITHOUT
    // sharing across tests — `cargo test` runs lib tests in parallel, so a
    // single global `NIL_CALLS` would race between two tests both
    // incrementing it. Per-test atomics keep each assertion local.

    const NIL_TAG: u8 = 1;
    const INT_TAG: u8 = 5;
    const SEND_TAG: u8 = 17;
    const BEGIN_TAG: u8 = 28;

    static NIL_KINDS: &[PluginNodeKindTag] = &[PluginNodeKindTag(NIL_TAG)];
    static SEND_KINDS: &[PluginNodeKindTag] = &[PluginNodeKindTag(SEND_TAG)];

    static TARGET_RAILS_VERSION_SEEN: std::sync::atomic::AtomicU16 =
        std::sync::atomic::AtomicU16::new(0);
    unsafe extern "C" fn target_rails_version_dispatch(_node: NodeId, cx: *const CxRaw) -> i32 {
        let cx = unsafe { &*cx };
        TARGET_RAILS_VERSION_SEEN.store(cx.target_rails_version, Ordering::SeqCst);
        0
    }
    static TARGET_RAILS_VERSION_COP: PluginCopV1 = PluginCopV1 {
        size: std::mem::size_of::<PluginCopV1>(),
        name: RawSlice::from_str("Test/TargetRailsVersion"),
        description: RawSlice::from_str(""),
        default_severity: SEVERITY_UNSET,
        default_enabled: 255,
        safe: 255,
        safe_autocorrect: 255,
        minimum_target_ruby_version: 0,
        options_ptr: std::ptr::null(),
        options_len: 0,
        kinds_ptr: NIL_KINDS.as_ptr(),
        kinds_len: NIL_KINDS.len(),
        dispatch: target_rails_version_dispatch,
        send_methods_ptr: std::ptr::null(),
        send_methods_len: 0,
    };

    #[test]
    fn dispatch_passes_target_rails_version_to_cx_raw() {
        TARGET_RAILS_VERSION_SEEN.store(0, Ordering::SeqCst);
        let ast = ast_nil_and_int();
        let mut sink = OffenseSink::new("t.rb");

        run_cops_with_options_and_context(
            &ast,
            &[&TARGET_RAILS_VERSION_COP],
            &mut sink,
            // this test only exercises target_rails_version threading
            AllCopsContext {
                target_rails_version: Some(RubyVersion::new(5, 2)),
                ..AllCopsContext::default()
            },
            &[],
            |_| b"{}".to_vec(),
        );

        assert_eq!(
            TARGET_RAILS_VERSION_SEEN.load(Ordering::SeqCst),
            RubyVersion::to_wire(Some(RubyVersion::new(5, 2)))
        );
    }

    // Separate per-test atomic (see ACTIVE_SUPPORT_SEEN note on parallelism).
    static TARGET_RUBY_VERSION_SEEN: std::sync::atomic::AtomicU16 =
        std::sync::atomic::AtomicU16::new(0);
    unsafe extern "C" fn target_ruby_version_dispatch(_node: NodeId, cx: *const CxRaw) -> i32 {
        let cx = unsafe { &*cx };
        TARGET_RUBY_VERSION_SEEN.store(cx.target_ruby_version, Ordering::SeqCst);
        0
    }
    static TARGET_RUBY_VERSION_COP: PluginCopV1 = PluginCopV1 {
        size: std::mem::size_of::<PluginCopV1>(),
        name: RawSlice::from_str("Test/TargetRubyVersion"),
        description: RawSlice::from_str(""),
        default_severity: SEVERITY_UNSET,
        default_enabled: 255,
        safe: 255,
        safe_autocorrect: 255,
        minimum_target_ruby_version: 0,
        options_ptr: std::ptr::null(),
        options_len: 0,
        kinds_ptr: NIL_KINDS.as_ptr(),
        kinds_len: NIL_KINDS.len(),
        dispatch: target_ruby_version_dispatch,
        send_methods_ptr: std::ptr::null(),
        send_methods_len: 0,
    };

    #[test]
    fn dispatch_passes_target_ruby_version_to_cx_raw() {
        TARGET_RUBY_VERSION_SEEN.store(0, Ordering::SeqCst);
        let ast = ast_nil_and_int();
        let mut sink = OffenseSink::new("t.rb");

        run_cops_with_options_and_context(
            &ast,
            &[&TARGET_RUBY_VERSION_COP],
            &mut sink,
            AllCopsContext {
                target_ruby_version: Some(RubyVersion::new(3, 2)),
                ..AllCopsContext::default()
            },
            &[],
            |_| b"{}".to_vec(),
        );

        assert_eq!(
            TARGET_RUBY_VERSION_SEEN.load(Ordering::SeqCst),
            RubyVersion::to_wire(Some(RubyVersion::new(3, 2)))
        );
    }

    // Separate per-test atomic: a shared static would race the
    // target_rails_version test under `cargo test`'s parallel lib runs.
    static ACTIVE_SUPPORT_SEEN: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);
    unsafe extern "C" fn active_support_dispatch(_node: NodeId, cx: *const CxRaw) -> i32 {
        let cx = unsafe { &*cx };
        ACTIVE_SUPPORT_SEEN.store(cx.active_support_extensions_enabled, Ordering::SeqCst);
        0
    }
    static ACTIVE_SUPPORT_COP: PluginCopV1 = PluginCopV1 {
        size: std::mem::size_of::<PluginCopV1>(),
        name: RawSlice::from_str("Test/ActiveSupportExtensions"),
        description: RawSlice::from_str(""),
        default_severity: SEVERITY_UNSET,
        default_enabled: 255,
        safe: 255,
        safe_autocorrect: 255,
        minimum_target_ruby_version: 0,
        options_ptr: std::ptr::null(),
        options_len: 0,
        kinds_ptr: NIL_KINDS.as_ptr(),
        kinds_len: NIL_KINDS.len(),
        dispatch: active_support_dispatch,
        send_methods_ptr: std::ptr::null(),
        send_methods_len: 0,
    };

    #[test]
    fn dispatch_passes_active_support_extensions_enabled_to_cx_raw() {
        // `true` path: the flag threaded through build_cx_raw reaches the cop.
        ACTIVE_SUPPORT_SEEN.store(false, Ordering::SeqCst);
        let ast = ast_nil_and_int();
        let mut sink = OffenseSink::new("t.rb");

        run_cops_with_options_and_context(
            &ast,
            &[&ACTIVE_SUPPORT_COP],
            &mut sink,
            // this test only exercises active_support_extensions_enabled threading
            AllCopsContext {
                active_support_extensions_enabled: true,
                ..AllCopsContext::default()
            },
            &[],
            |_| b"{}".to_vec(),
        );

        assert!(ACTIVE_SUPPORT_SEEN.load(Ordering::SeqCst));

        // `false` path: the default flows through as `false`, not a stale `true`.
        ACTIVE_SUPPORT_SEEN.store(true, Ordering::SeqCst);
        let mut sink = OffenseSink::new("t.rb");

        run_cops_with_options_and_context(
            &ast,
            &[&ACTIVE_SUPPORT_COP],
            &mut sink,
            AllCopsContext::default(),
            &[],
            |_| b"{}".to_vec(),
        );

        assert!(!ACTIVE_SUPPORT_SEEN.load(Ordering::SeqCst));
    }

    // Separate per-test atomic: a shared static would race the other
    // context-threading tests under `cargo test`'s parallel lib runs.
    static INDENTATION_WIDTH_SEEN: std::sync::atomic::AtomicU16 =
        std::sync::atomic::AtomicU16::new(0);
    unsafe extern "C" fn indentation_width_dispatch(_node: NodeId, cx: *const CxRaw) -> i32 {
        let cx = unsafe { &*cx };
        INDENTATION_WIDTH_SEEN.store(cx.indentation_width, Ordering::SeqCst);
        0
    }
    static INDENTATION_WIDTH_COP: PluginCopV1 = PluginCopV1 {
        size: std::mem::size_of::<PluginCopV1>(),
        name: RawSlice::from_str("Test/IndentationWidth"),
        description: RawSlice::from_str(""),
        default_severity: SEVERITY_UNSET,
        default_enabled: 255,
        safe: 255,
        safe_autocorrect: 255,
        minimum_target_ruby_version: 0,
        options_ptr: std::ptr::null(),
        options_len: 0,
        kinds_ptr: NIL_KINDS.as_ptr(),
        kinds_len: NIL_KINDS.len(),
        dispatch: indentation_width_dispatch,
        send_methods_ptr: std::ptr::null(),
        send_methods_len: 0,
    };

    #[test]
    fn dispatch_passes_indentation_width_to_cx_raw() {
        // The resolved `Layout/IndentationWidth.Width` threaded through the
        // context reaches the cop's `CxRaw`.
        INDENTATION_WIDTH_SEEN.store(0, Ordering::SeqCst);
        let ast = ast_nil_and_int();
        let mut sink = OffenseSink::new("t.rb");

        run_cops_with_options_and_context(
            &ast,
            &[&INDENTATION_WIDTH_COP],
            &mut sink,
            // this test only exercises indentation_width threading
            AllCopsContext {
                indentation_width: 4,
                ..AllCopsContext::default()
            },
            &[],
            |_| b"{}".to_vec(),
        );

        assert_eq!(INDENTATION_WIDTH_SEEN.load(Ordering::SeqCst), 4);
    }

    // Per-test atomic (one tagged per test) so context-threading tests stay
    // independent under `cargo test` parallelism.
    static BLOCK_FORWARDING_EXPLICIT_SEEN: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);
    unsafe extern "C" fn block_forwarding_explicit_dispatch(
        _node: NodeId,
        cx: *const CxRaw,
    ) -> i32 {
        let cx = unsafe { &*cx };
        BLOCK_FORWARDING_EXPLICIT_SEEN.store(cx.block_forwarding_explicit, Ordering::SeqCst);
        0
    }
    static BLOCK_FORWARDING_EXPLICIT_COP: PluginCopV1 = PluginCopV1 {
        size: std::mem::size_of::<PluginCopV1>(),
        name: RawSlice::from_str("Test/BlockForwardingExplicit"),
        description: RawSlice::from_str(""),
        default_severity: SEVERITY_UNSET,
        default_enabled: 255,
        safe: 255,
        safe_autocorrect: 255,
        minimum_target_ruby_version: 0,
        options_ptr: std::ptr::null(),
        options_len: 0,
        kinds_ptr: NIL_KINDS.as_ptr(),
        kinds_len: NIL_KINDS.len(),
        dispatch: block_forwarding_explicit_dispatch,
        send_methods_ptr: std::ptr::null(),
        send_methods_len: 0,
    };

    #[test]
    fn dispatch_passes_block_forwarding_explicit_to_cx_raw() {
        // The resolved `Naming/BlockForwarding.EnforcedStyle == "explicit"` flag
        // threaded through the context reaches the cop's `CxRaw`.
        BLOCK_FORWARDING_EXPLICIT_SEEN.store(false, Ordering::SeqCst);
        let ast = ast_nil_and_int();
        let mut sink = OffenseSink::new("t.rb");

        run_cops_with_options_and_context(
            &ast,
            &[&BLOCK_FORWARDING_EXPLICIT_COP],
            &mut sink,
            AllCopsContext {
                block_forwarding_explicit: true,
                ..AllCopsContext::default()
            },
            &[],
            |_| b"{}".to_vec(),
        );

        assert!(BLOCK_FORWARDING_EXPLICIT_SEEN.load(Ordering::SeqCst));

        // And the default (false) is faithfully threaded too.
        BLOCK_FORWARDING_EXPLICIT_SEEN.store(true, Ordering::SeqCst);
        let mut sink = OffenseSink::new("t.rb");
        run_cops_with_options_and_context(
            &ast,
            &[&BLOCK_FORWARDING_EXPLICIT_COP],
            &mut sink,
            AllCopsContext::default(),
            &[],
            |_| b"{}".to_vec(),
        );

        assert!(!BLOCK_FORWARDING_EXPLICIT_SEEN.load(Ordering::SeqCst));
    }

    // (1) DispatchIndex correctly buckets the arena's nodes by tag.
    #[test]
    fn dispatch_index_groups_cops_by_kind() {
        let ast = ast_nil_and_int();
        let idx = DispatchIndex::build(&ast);

        // ast_nil_and_int is [Nil, Int, Begin] in push order.
        let nil_bucket = idx.nodes_for(PluginNodeKindTag(NIL_TAG));
        let int_bucket = idx.nodes_for(PluginNodeKindTag(INT_TAG));
        let begin_bucket = idx.nodes_for(PluginNodeKindTag(BEGIN_TAG));

        assert_eq!(nil_bucket, &[NodeId(0)], "Nil should be node 0");
        assert_eq!(int_bucket, &[NodeId(1)], "Int should be node 1");
        assert_eq!(begin_bucket, &[NodeId(2)], "Begin (root) should be node 2");

        // A tag with no nodes resolves to an empty slice — not a panic.
        assert!(idx.nodes_for(PluginNodeKindTag(SEND_TAG)).is_empty());
    }

    // (2) Outer cop / inner node iteration: each matched node is visited
    //     exactly once per cop, no more, no less.
    static ITER_CALLS: AtomicUsize = AtomicUsize::new(0);
    unsafe extern "C" fn iter_dispatch(_node: NodeId, _cx: *const CxRaw) -> i32 {
        ITER_CALLS.fetch_add(1, Ordering::SeqCst);
        0
    }
    static ITER_COP: PluginCopV1 = PluginCopV1 {
        size: std::mem::size_of::<PluginCopV1>(),
        name: RawSlice::from_str("Test/IterCop"),
        description: RawSlice::from_str(""),
        default_severity: SEVERITY_UNSET,
        default_enabled: 255,
        safe: 255,
        safe_autocorrect: 255,
        minimum_target_ruby_version: 0,
        options_ptr: std::ptr::null(),
        options_len: 0,
        kinds_ptr: NIL_KINDS.as_ptr(),
        kinds_len: NIL_KINDS.len(),
        dispatch: iter_dispatch,
        send_methods_ptr: std::ptr::null(),
        send_methods_len: 0,
    };

    #[test]
    fn dispatch_iterates_arena_once_per_node() {
        ITER_CALLS.store(0, Ordering::SeqCst);

        // Build `[Nil, Nil, Begin]` — two Nils so the inner loop runs twice.
        let mut b = AstBuilder::new("nil; nil", "t.rb");
        let n1 = b.push(NodeKind::Nil, murphy_ast::Range { start: 0, end: 3 });
        let n2 = b.push(NodeKind::Nil, murphy_ast::Range { start: 5, end: 8 });
        let list = b.push_list(&[n1, n2]);
        let root = b.push(
            NodeKind::Begin(list),
            murphy_ast::Range { start: 0, end: 8 },
        );
        let ast = b.finish(root);

        let mut sink = OffenseSink::new("t.rb");
        run_cops(&ast, &[&ITER_COP], &mut sink);

        assert_eq!(
            ITER_CALLS.load(Ordering::SeqCst),
            2,
            "IterCop must be invoked exactly once per Nil node in the arena"
        );
    }

    // (3) A cop subscribed to NIL does not see SEND nodes, and vice versa.
    static MATCH_NIL_CALLS: AtomicUsize = AtomicUsize::new(0);
    static MATCH_SEND_CALLS: AtomicUsize = AtomicUsize::new(0);
    unsafe extern "C" fn match_nil_dispatch(_node: NodeId, _cx: *const CxRaw) -> i32 {
        MATCH_NIL_CALLS.fetch_add(1, Ordering::SeqCst);
        0
    }
    unsafe extern "C" fn match_send_dispatch(_node: NodeId, _cx: *const CxRaw) -> i32 {
        MATCH_SEND_CALLS.fetch_add(1, Ordering::SeqCst);
        0
    }
    static MATCH_NIL_COP: PluginCopV1 = PluginCopV1 {
        size: std::mem::size_of::<PluginCopV1>(),
        name: RawSlice::from_str("Test/MatchNil"),
        description: RawSlice::from_str(""),
        default_severity: SEVERITY_UNSET,
        default_enabled: 255,
        safe: 255,
        safe_autocorrect: 255,
        minimum_target_ruby_version: 0,
        options_ptr: std::ptr::null(),
        options_len: 0,
        kinds_ptr: NIL_KINDS.as_ptr(),
        kinds_len: NIL_KINDS.len(),
        dispatch: match_nil_dispatch,
        send_methods_ptr: std::ptr::null(),
        send_methods_len: 0,
    };
    static MATCH_SEND_COP: PluginCopV1 = PluginCopV1 {
        size: std::mem::size_of::<PluginCopV1>(),
        name: RawSlice::from_str("Test/MatchSend"),
        description: RawSlice::from_str(""),
        default_severity: SEVERITY_UNSET,
        default_enabled: 255,
        safe: 255,
        safe_autocorrect: 255,
        minimum_target_ruby_version: 0,
        options_ptr: std::ptr::null(),
        options_len: 0,
        kinds_ptr: SEND_KINDS.as_ptr(),
        kinds_len: SEND_KINDS.len(),
        dispatch: match_send_dispatch,
        send_methods_ptr: std::ptr::null(),
        send_methods_len: 0,
    };

    #[test]
    fn dispatch_invokes_only_matching_kinds() {
        MATCH_NIL_CALLS.store(0, Ordering::SeqCst);
        MATCH_SEND_CALLS.store(0, Ordering::SeqCst);

        let ast = ast_puts_x(); // contains Send + Str — no Nil.
        let mut sink = OffenseSink::new("t.rb");
        run_cops(&ast, &[&MATCH_NIL_COP, &MATCH_SEND_COP], &mut sink);

        assert_eq!(
            MATCH_NIL_CALLS.load(Ordering::SeqCst),
            0,
            "Nil-subscribed cop must not be invoked on Send/Str nodes"
        );
        assert_eq!(
            MATCH_SEND_CALLS.load(Ordering::SeqCst),
            1,
            "Send-subscribed cop must be invoked exactly once on the one Send node"
        );
    }

    // (3b) `send_methods` allow-list on a Send-subscribed cop pre-filters
    //      at the host: the cop's dispatch is **not invoked** for Send
    //      nodes whose method symbol is not in the list (murphy-ip0).
    static SEND_FILTER_CALLS: AtomicUsize = AtomicUsize::new(0);
    unsafe extern "C" fn send_filter_dispatch(_node: NodeId, _cx: *const CxRaw) -> i32 {
        SEND_FILTER_CALLS.fetch_add(1, Ordering::SeqCst);
        0
    }
    static SEND_FILTER_METHODS: &[RawSlice] = &[RawSlice::from_str("describe")];
    static SEND_FILTER_COP: PluginCopV1 = PluginCopV1 {
        size: std::mem::size_of::<PluginCopV1>(),
        name: RawSlice::from_str("Test/SendFilter"),
        description: RawSlice::from_str(""),
        default_severity: SEVERITY_UNSET,
        default_enabled: 255,
        safe: 255,
        safe_autocorrect: 255,
        minimum_target_ruby_version: 0,
        options_ptr: std::ptr::null(),
        options_len: 0,
        kinds_ptr: SEND_KINDS.as_ptr(),
        kinds_len: SEND_KINDS.len(),
        dispatch: send_filter_dispatch,
        send_methods_ptr: SEND_FILTER_METHODS.as_ptr(),
        send_methods_len: SEND_FILTER_METHODS.len(),
    };

    #[test]
    fn dispatch_pre_filters_send_by_method_name() {
        SEND_FILTER_CALLS.store(0, Ordering::SeqCst);

        // Two Sends in the same arena — one `describe`, one `foo`. The
        // cop's allow-list contains only "describe", so the host must
        // call `send_filter_dispatch` exactly once.
        let mut b = AstBuilder::new("describe; foo", "t.rb");
        let describe_method = b.intern_symbol("describe");
        let describe_send = b.push(
            NodeKind::Send {
                receiver: OptNodeId::NONE,
                method: describe_method,
                args: murphy_ast::NodeList::EMPTY,
            },
            murphy_ast::Range { start: 0, end: 8 },
        );
        let foo_method = b.intern_symbol("foo");
        let foo_send = b.push(
            NodeKind::Send {
                receiver: OptNodeId::NONE,
                method: foo_method,
                args: murphy_ast::NodeList::EMPTY,
            },
            murphy_ast::Range { start: 10, end: 13 },
        );
        let list = b.push_list(&[describe_send, foo_send]);
        let root = b.push(
            NodeKind::Begin(list),
            murphy_ast::Range { start: 0, end: 13 },
        );
        let ast = b.finish(root);

        let mut sink = OffenseSink::new("t.rb");
        run_cops(&ast, &[&SEND_FILTER_COP], &mut sink);

        assert_eq!(
            SEND_FILTER_CALLS.load(Ordering::SeqCst),
            1,
            "host must pre-filter Send by method; the cop sees only `describe`, not `foo`"
        );
    }

    // (4) `cop_name` is restamped into the per-cop CxRaw and survives into
    //     emitted offenses.
    static STAMP_COP_A: PluginCopV1 = PluginCopV1 {
        size: std::mem::size_of::<PluginCopV1>(),
        name: RawSlice::from_str("Test/StampA"),
        description: RawSlice::from_str(""),
        default_severity: SEVERITY_UNSET,
        default_enabled: 255,
        safe: 255,
        safe_autocorrect: 255,
        minimum_target_ruby_version: 0,
        options_ptr: std::ptr::null(),
        options_len: 0,
        kinds_ptr: NIL_KINDS.as_ptr(),
        kinds_len: NIL_KINDS.len(),
        dispatch: stamp_emit,
        send_methods_ptr: std::ptr::null(),
        send_methods_len: 0,
    };
    static STAMP_COP_B: PluginCopV1 = PluginCopV1 {
        size: std::mem::size_of::<PluginCopV1>(),
        name: RawSlice::from_str("Test/StampB"),
        description: RawSlice::from_str(""),
        default_severity: SEVERITY_UNSET,
        default_enabled: 255,
        safe: 255,
        safe_autocorrect: 255,
        minimum_target_ruby_version: 0,
        options_ptr: std::ptr::null(),
        options_len: 0,
        kinds_ptr: NIL_KINDS.as_ptr(),
        kinds_len: NIL_KINDS.len(),
        dispatch: stamp_emit,
        send_methods_ptr: std::ptr::null(),
        send_methods_len: 0,
    };

    /// Emit one offense per visited node; cop_name is whatever the host
    /// stamped, which is what this test is verifying.
    unsafe extern "C" fn stamp_emit(_node: NodeId, cx: *const CxRaw) -> i32 {
        let cx = unsafe { &*cx };
        let off = RawOffense {
            cop_name: cx.cop_name,
            message: RawSlice::from_str("touched"),
            range: murphy_ast::Range { start: 0, end: 3 },
            severity: 0,
        };
        let fns = unsafe { &*cx.fns };
        unsafe { (fns.emit_offense)(cx.sink, &off) };
        0
    }

    #[test]
    fn dispatch_stamps_cop_name_into_cx_raw_per_cop() {
        // One Nil node → both cops fire once each → two offenses with
        // different cop_names.
        let mut b = AstBuilder::new("nil", "t.rb");
        let n = b.push(NodeKind::Nil, murphy_ast::Range { start: 0, end: 3 });
        let ast = b.finish(n);

        let mut sink = OffenseSink::new("t.rb");
        run_cops(&ast, &[&STAMP_COP_A, &STAMP_COP_B], &mut sink);

        let names: Vec<_> = sink.offenses().iter().map(|o| o.cop_name.clone()).collect();
        assert_eq!(
            names,
            vec!["Test/StampA".to_string(), "Test/StampB".to_string()],
            "each cop's offense must carry the cop_name the host stamped"
        );
    }

    static OPTION_RECORDS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
    unsafe extern "C" fn options_record_dispatch(_node: NodeId, cx: *const CxRaw) -> i32 {
        let cx = unsafe { &*cx };
        let name = String::from_utf8_lossy(unsafe { cx.cop_name.as_bytes() }).into_owned();
        let options = String::from_utf8_lossy(unsafe { cx.options_json.as_bytes() }).into_owned();
        OPTION_RECORDS.lock().unwrap().push((name, options));
        0
    }

    static OPTIONS_COP_A: PluginCopV1 = PluginCopV1 {
        size: std::mem::size_of::<PluginCopV1>(),
        name: RawSlice::from_str("Test/OptionsA"),
        description: RawSlice::from_str(""),
        default_severity: SEVERITY_UNSET,
        default_enabled: 255,
        safe: 255,
        safe_autocorrect: 255,
        minimum_target_ruby_version: 0,
        options_ptr: std::ptr::null(),
        options_len: 0,
        kinds_ptr: NIL_KINDS.as_ptr(),
        kinds_len: NIL_KINDS.len(),
        dispatch: options_record_dispatch,
        send_methods_ptr: std::ptr::null(),
        send_methods_len: 0,
    };
    static OPTIONS_COP_B: PluginCopV1 = PluginCopV1 {
        size: std::mem::size_of::<PluginCopV1>(),
        name: RawSlice::from_str("Test/OptionsB"),
        description: RawSlice::from_str(""),
        default_severity: SEVERITY_UNSET,
        default_enabled: 255,
        safe: 255,
        safe_autocorrect: 255,
        minimum_target_ruby_version: 0,
        options_ptr: std::ptr::null(),
        options_len: 0,
        kinds_ptr: NIL_KINDS.as_ptr(),
        kinds_len: NIL_KINDS.len(),
        dispatch: options_record_dispatch,
        send_methods_ptr: std::ptr::null(),
        send_methods_len: 0,
    };

    #[test]
    fn run_cops_with_options_injects_per_cop_options_json() {
        OPTION_RECORDS.lock().unwrap().clear();

        let mut b = AstBuilder::new("nil", "t.rb");
        let n = b.push(NodeKind::Nil, murphy_ast::Range { start: 0, end: 3 });
        let ast = b.finish(n);

        let mut sink = OffenseSink::new("t.rb");
        run_cops_with_options(
            &ast,
            &[&OPTIONS_COP_A, &OPTIONS_COP_B],
            &mut sink,
            |name| match name {
                "Test/OptionsA" => br#"{"style":"a"}"#.to_vec(),
                "Test/OptionsB" => br#"{"style":"b"}"#.to_vec(),
                other => panic!("unexpected cop name {other}"),
            },
        );

        let records = OPTION_RECORDS.lock().unwrap().clone();
        assert_eq!(
            records,
            vec![
                ("Test/OptionsA".to_string(), r#"{"style":"a"}"#.to_string()),
                ("Test/OptionsB".to_string(), r#"{"style":"b"}"#.to_string()),
            ],
        );
    }

    // (5) A cop whose dispatch returns non-zero is disabled for the rest of
    //     the file; other cops still run to completion.
    static PANIC_CALLS: AtomicUsize = AtomicUsize::new(0);
    static AFTER_CALLS: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn panicking_dispatch(_node: NodeId, _cx: *const CxRaw) -> i32 {
        PANIC_CALLS.fetch_add(1, Ordering::SeqCst);
        1 // simulate trapped panic
    }
    unsafe extern "C" fn after_dispatch(_node: NodeId, _cx: *const CxRaw) -> i32 {
        AFTER_CALLS.fetch_add(1, Ordering::SeqCst);
        0
    }

    static PANIC_COP: PluginCopV1 = PluginCopV1 {
        size: std::mem::size_of::<PluginCopV1>(),
        name: RawSlice::from_str("Test/PanicCop"),
        description: RawSlice::from_str(""),
        default_severity: SEVERITY_UNSET,
        default_enabled: 255,
        safe: 255,
        safe_autocorrect: 255,
        minimum_target_ruby_version: 0,
        options_ptr: std::ptr::null(),
        options_len: 0,
        kinds_ptr: NIL_KINDS.as_ptr(),
        kinds_len: NIL_KINDS.len(),
        dispatch: panicking_dispatch,
        send_methods_ptr: std::ptr::null(),
        send_methods_len: 0,
    };
    static AFTER_COP: PluginCopV1 = PluginCopV1 {
        size: std::mem::size_of::<PluginCopV1>(),
        name: RawSlice::from_str("Test/AfterCop"),
        description: RawSlice::from_str(""),
        default_severity: SEVERITY_UNSET,
        default_enabled: 255,
        safe: 255,
        safe_autocorrect: 255,
        minimum_target_ruby_version: 0,
        options_ptr: std::ptr::null(),
        options_len: 0,
        kinds_ptr: NIL_KINDS.as_ptr(),
        kinds_len: NIL_KINDS.len(),
        dispatch: after_dispatch,
        send_methods_ptr: std::ptr::null(),
        send_methods_len: 0,
    };

    #[test]
    fn panicking_cop_is_isolated_and_others_complete() {
        PANIC_CALLS.store(0, Ordering::SeqCst);
        AFTER_CALLS.store(0, Ordering::SeqCst);

        // Two Nils so the dispatch loop has more than one node to iterate.
        let mut b = AstBuilder::new("nil; nil", "t.rb");
        let n1 = b.push(NodeKind::Nil, murphy_ast::Range { start: 0, end: 3 });
        let n2 = b.push(NodeKind::Nil, murphy_ast::Range { start: 5, end: 8 });
        let list = b.push_list(&[n1, n2]);
        let root = b.push(
            NodeKind::Begin(list),
            murphy_ast::Range { start: 0, end: 8 },
        );
        let ast = b.finish(root);

        let mut sink = OffenseSink::new("t.rb");
        run_cops(&ast, &[&PANIC_COP, &AFTER_COP], &mut sink);

        // PanicCop disables after the first non-zero return — exactly one call.
        assert_eq!(
            PANIC_CALLS.load(Ordering::SeqCst),
            1,
            "panicking cop must be disabled after the first non-zero return"
        );
        // AfterCop is unaffected and visits BOTH nil nodes.
        assert_eq!(
            AFTER_CALLS.load(Ordering::SeqCst),
            2,
            "subsequent cops must still run to completion"
        );
    }

    // (6) host_emit_offense renders a RawOffense into a fully-formed Offense
    //     with the sink's `file`, the cop's name, and the supplied range +
    //     severity + message.
    unsafe extern "C" fn render_emit(_node: NodeId, cx: *const CxRaw) -> i32 {
        let cx = unsafe { &*cx };
        let off = RawOffense {
            cop_name: cx.cop_name,
            message: RawSlice::from_str("use logger"),
            range: murphy_ast::Range { start: 0, end: 3 },
            severity: 1, // Error wire byte
        };
        let fns = unsafe { &*cx.fns };
        unsafe { (fns.emit_offense)(cx.sink, &off) };
        // Also exercise emit_edit — it attaches to the offense just pushed.
        let edit = RawEdit {
            range: murphy_ast::Range { start: 0, end: 3 },
            replacement: RawSlice::from_str("logger.info"),
        };
        unsafe { (fns.emit_edit)(cx.sink, &edit) };
        0
    }

    static RENDER_COP: PluginCopV1 = PluginCopV1 {
        size: std::mem::size_of::<PluginCopV1>(),
        name: RawSlice::from_str("Test/Render"),
        description: RawSlice::from_str(""),
        default_severity: SEVERITY_UNSET,
        default_enabled: 255,
        safe: 255,
        safe_autocorrect: 255,
        minimum_target_ruby_version: 0,
        options_ptr: std::ptr::null(),
        options_len: 0,
        kinds_ptr: NIL_KINDS.as_ptr(),
        kinds_len: NIL_KINDS.len(),
        dispatch: render_emit,
        send_methods_ptr: std::ptr::null(),
        send_methods_len: 0,
    };

    #[test]
    fn host_emit_offense_renders_into_offense_sink() {
        let mut b = AstBuilder::new("nil", "demo.rb");
        let n = b.push(NodeKind::Nil, murphy_ast::Range { start: 0, end: 3 });
        let ast = b.finish(n);

        let mut sink = OffenseSink::new("demo.rb");
        run_cops(&ast, &[&RENDER_COP], &mut sink);

        let offenses = sink.into_offenses();
        assert_eq!(offenses.len(), 1, "exactly one offense was emitted");
        let o = &offenses[0];
        assert_eq!(o.file, "demo.rb", "sink's file is stamped into the offense");
        assert_eq!(o.cop_name, "Test/Render");
        assert_eq!(o.message, "use logger");
        assert_eq!(o.range.start_offset, 0);
        assert_eq!(o.range.end_offset, 3);
        assert_eq!(o.severity, Severity::Error);

        // emit_edit attached to the offense just pushed.
        let ac = o.autocorrect.as_ref().expect("edit should be attached");
        assert_eq!(ac.edits.len(), 1);
        assert_eq!(ac.edits[0].range.start_offset, 0);
        assert_eq!(ac.edits[0].range.end_offset, 3);
        assert_eq!(ac.edits[0].replacement, "logger.info");
    }

    // (7) Empty `KINDS` = file-visit: the cop is invoked exactly once,
    //     with `node == ast.root()`. Per-test static atomic + node-id
    //     cell to avoid races with the parallel test runner (per the
    //     `Test Parallelism` note in CLAUDE.md).
    use std::sync::atomic::AtomicU32;
    static FILE_VISIT_CALLS: AtomicUsize = AtomicUsize::new(0);
    static FILE_VISIT_SEEN_NODE: AtomicU32 = AtomicU32::new(u32::MAX);
    unsafe extern "C" fn file_visit_dispatch(node: NodeId, _cx: *const CxRaw) -> i32 {
        FILE_VISIT_CALLS.fetch_add(1, Ordering::SeqCst);
        FILE_VISIT_SEEN_NODE.store(node.0, Ordering::SeqCst);
        0
    }
    static FILE_VISIT_KINDS: &[PluginNodeKindTag] = &[];
    static FILE_VISIT_COP: PluginCopV1 = PluginCopV1 {
        size: std::mem::size_of::<PluginCopV1>(),
        name: RawSlice::from_str("Test/FileVisit"),
        description: RawSlice::from_str(""),
        default_severity: SEVERITY_UNSET,
        default_enabled: 255,
        safe: 255,
        safe_autocorrect: 255,
        minimum_target_ruby_version: 0,
        options_ptr: std::ptr::null(),
        options_len: 0,
        kinds_ptr: FILE_VISIT_KINDS.as_ptr(),
        kinds_len: FILE_VISIT_KINDS.len(),
        dispatch: file_visit_dispatch,
        send_methods_ptr: std::ptr::null(),
        send_methods_len: 0,
    };

    #[test]
    fn empty_kinds_dispatches_once_per_file_with_root_id() {
        FILE_VISIT_CALLS.store(0, Ordering::SeqCst);
        FILE_VISIT_SEEN_NODE.store(u32::MAX, Ordering::SeqCst);

        // Arena: `[Nil, Int, Begin]` — three nodes, root is index 2.
        let ast = ast_nil_and_int();
        let expected_root = ast.root().0;

        let mut sink = OffenseSink::new("t.rb");
        run_cops(&ast, &[&FILE_VISIT_COP], &mut sink);

        assert_eq!(
            FILE_VISIT_CALLS.load(Ordering::SeqCst),
            1,
            "a file-visit cop (KINDS = []) must be called exactly once \
             regardless of arena size",
        );
        assert_eq!(
            FILE_VISIT_SEEN_NODE.load(Ordering::SeqCst),
            expected_root,
            "the file-visit dispatch must hand the cop ast.root()",
        );
    }
}
