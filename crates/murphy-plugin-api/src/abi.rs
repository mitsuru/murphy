//! `#[repr(C)]` types that cross the plugin ABI boundary (ADR 0038).
//!
//! Every struct here has a frozen layout: the `#[cfg(test)]` `offset_of!`
//! assertions are the freeze guard. New fields append at the end only.

use std::ffi::c_void;

use murphy_ast::{
    AstNode, CallClosingLoc, CallOperatorLoc, Comment, NodeId, NodeKindTag, Range, SourceToken,
};

/// The ABI's borrowed-slice primitive: a `#[repr(C)]` pointer+length pair.
///
/// `len == 0` is valid with any `ptr` (including null); accessors check
/// `len` before dereferencing.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RawSlice {
    /// Start pointer. Meaningful only when `len > 0`.
    pub ptr: *const u8,
    /// Byte length.
    pub len: usize,
}

// Safety: a RawSlice is an immutable, non-owning view. The pointee's
// validity and thread-safety are the host's responsibility under the
// ADR 0038 safety contract. This covers both shapes a RawSlice takes:
// a view into the immutable arena during dispatch, and a pack's exported
// `static RawSlice` (e.g. `MURPHY_PLUGIN_DEFAULT_CONFIG`) pointing at
// immutable `'static` rodata inside the loaded `.so`. Both pointees are
// read-only and never mutated, so sharing the view across threads is sound.
unsafe impl Sync for RawSlice {}
unsafe impl Send for RawSlice {}

impl RawSlice {
    /// The empty slice.
    pub const EMPTY: RawSlice = RawSlice {
        ptr: std::ptr::null(),
        len: 0,
    };

    /// Borrow a `&'static str`.
    pub const fn from_str(s: &'static str) -> RawSlice {
        RawSlice {
            ptr: s.as_ptr(),
            len: s.len(),
        }
    }

    /// Reconstruct the byte slice.
    ///
    /// # Safety
    /// When `len > 0`, `ptr` must point to `len` initialized bytes valid
    /// for `'a`.
    pub unsafe fn as_bytes<'a>(self) -> &'a [u8] {
        if self.len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
        }
    }
}

/// `#[repr(C)]` schema entry for one cop option. Re-implements the
/// option-metadata struct (murphy-9cr.2 concept) for the single-surface
/// ABI. The validation gate (murphy-9cr.9) reads `CopOptions::SCHEMA`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OptionSpec {
    /// Option key in `[cops.rules."Name"]`.
    pub name: RawSlice,
    /// Wire type: `"bool"` / `"int"` / `"string"` / `"string_list"`.
    pub ty: RawSlice,
    /// Default value, JSON-encoded. `EMPTY` when the option is required.
    pub default_json: RawSlice,
    /// One-line human description.
    pub description: RawSlice,
    /// Allowed values for an enum `string` (JSON array); `EMPTY` if free.
    pub enum_values_json: RawSlice,
    /// Suggested replacement when this option is deprecated.
    pub replacement: RawSlice,
    /// Why the option exists / its deprecation reason.
    pub reason: RawSlice,
}

// Safety: OptionSpec is an immutable aggregate of non-owning RawSlice
// views; it lives only in &'static schemas. Sharing across threads is
// sound for the same reason RawSlice is Sync. Not Send: never moved
// across threads, so the stronger bound is left off deliberately.
unsafe impl Sync for OptionSpec {}

/// `#[repr(C)]` offense payload passed to [`FnTable::emit_offense`].
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RawOffense {
    /// Reporting cop's `NAME`.
    pub cop_name: RawSlice,
    /// Human-readable offense message.
    pub message: RawSlice,
    /// Source byte range of the offense.
    pub range: Range,
    /// Severity wire byte (see [`Severity::to_wire`](crate::Severity::to_wire));
    /// `SEVERITY_UNSET` defers to the host default.
    pub severity: u8,
}

/// `#[repr(C)]` autocorrect edit passed to [`FnTable::emit_edit`].
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RawEdit {
    /// Source byte range the edit replaces.
    pub range: Range,
    /// Replacement text.
    pub replacement: RawSlice,
}

/// `#[repr(C)]` table of host operations a cop cannot perform by direct
/// memory read — i.e. writing into the host's offense sink.
///
/// Everything else a cop needs (traversal, `NodeKind` matching, interner
/// resolution, comments, source text) is a pure read of the immutable
/// arena and lives on `Cx` directly, off the ABI's hot path.
///
/// Callbacks receive pointers valid only for the duration of the
/// synchronous call; an implementation must not retain any pointer
/// (including the `RawSlice`s inside the payload) past return.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FnTable {
    /// Record one offense into `sink`.
    pub emit_offense: unsafe extern "C" fn(*mut c_void, *const RawOffense),
    /// Record one autocorrect edit into `sink`.
    pub emit_edit: unsafe extern "C" fn(*mut c_void, *const RawEdit),
}

// Safety: FnTable holds only `extern "C"` function pointers, which are
// themselves Sync. Sharing it across threads is sound provided the host
// keeps the `sink` state reachable through these callbacks thread-safe —
// guaranteed by the ADR 0038 safety contract.
unsafe impl Sync for FnTable {}

/// Host allocator for short-lived `NodeId` slices returned from generated
/// matchers. The returned pointer must stay valid for the enclosing dispatch
/// call's `Cx<'a>` lifetime.
pub type AllocNodeSliceFn =
    unsafe extern "C" fn(*mut c_void, *const NodeId, usize) -> *const NodeId;

/// `#[repr(C)]` bundle the host passes per dispatch call. `Cx<'a>` is
/// the safe wrapper built from a borrowed `&CxRaw`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CxRaw {
    /// Arena node array.
    pub nodes: *const AstNode,
    pub nodes_len: usize,
    /// `node_lists` side table (variable-length children).
    pub lists: *const NodeId,
    pub lists_len: usize,
    /// Interner blob.
    pub interner_blob: *const u8,
    pub interner_blob_len: usize,
    /// Interner per-entry offsets.
    pub interner_offsets: *const Range,
    pub interner_offsets_len: usize,
    /// Source comments.
    pub comments: *const Comment,
    pub comments_len: usize,
    /// Source text (UTF-8).
    pub source: *const u8,
    pub source_len: usize,
    /// Arena root node.
    pub root: NodeId,
    /// Reporting cop's `NAME`, stamped into every emitted `RawOffense`.
    pub cop_name: RawSlice,
    /// Host operation table.
    pub fns: *const FnTable,
    /// Opaque host offense sink, passed back to `fns` callbacks.
    pub sink: *mut c_void,
    /// Source tokens in source order.
    pub sorted_tokens: *const SourceToken,
    pub sorted_tokens_len: usize,
    /// JSON object for the current cop's runtime options.
    pub options_json: RawSlice,
    /// Sparse parser-provided closing parens for call nodes.
    pub call_closing_locs: *const CallClosingLoc,
    pub call_closing_locs_len: usize,
    /// Sparse parser-provided call operators for call nodes.
    ///
    /// These trailing fields were added under ABI v2 without a numeric ABI bump
    /// by explicit project decision. During this evolving ABI phase, v2 assumes
    /// host/plugin lockstep rather than cross-version layout compatibility.
    pub call_operator_locs: *const CallOperatorLoc,
    pub call_operator_locs_len: usize,
    /// File-level variable scope model. Always non-null during native cop dispatch.
    pub var_model: *const crate::var_semantic_model::VarSemanticModel,
    /// Opaque host arena used for derived matcher result slices.
    pub node_slice_arena: *mut c_void,
    /// Copy `len` node ids from `ptr` into `node_slice_arena` and return the copy.
    pub alloc_node_slice: AllocNodeSliceFn,
    /// Current source file path as UTF-8 bytes, empty when unavailable.
    pub file_path: RawSlice,
    /// Optional `AllCops.TargetRailsVersion` for per-cop Rails feature gates.
    pub target_rails_version: u16,
    /// `AllCops.ActiveSupportExtensionsEnabled` (default false). Tail-appended
    /// under ABI v4 lockstep (murphy-pfcb); fits existing padding so
    /// `size_of::<CxRaw>()` is unchanged. Per project policy the numeric ABI is
    /// not bumped for tail-appended CxRaw fields.
    pub active_support_extensions_enabled: bool,
}

/// The plugin ABI version. A fresh v1 (ADR 0038-8): the pre-reboot ABI
/// was never frozen, so this is a new ABI starting at 1, not a bump.
///
/// Bumped to 2 (murphy-es99.8): the `SourceToken.kind` carried across the
/// ABI gained the `Comma`/`LeftBrace`/`RightBrace` variants. The addition
/// is tail-only (existing discriminants unchanged), but a plugin built
/// against v1 must still be rejected so it never observes a token kind it
/// cannot decode.
///
/// Bumped to 3 (murphy-es99.4): `PluginCopV1` gained `safe` and
/// `safe_autocorrect` tail fields, increasing the descriptor size.
/// `CxRaw::call_operator_locs` fields were also added under v3 lockstep.
///
/// Bumped to 4 (es99.5): `CxRaw` gained the `var_model` field — a
/// `*const VarSemanticModel` tail-appended at offset 200. Plugins built
/// against v1–v3 must be rejected so they never observe a layout they
/// were not compiled against.
///
/// `CxRaw::node_slice_arena` and `alloc_node_slice` were tail-appended under
/// ABI v4 lockstep for murphy-7nze. Per project policy, do not bump the
/// numeric ABI without explicit approval.
///
/// `CxRaw::file_path` was tail-appended under ABI v4 lockstep for murphy-vmg5.
/// `CxRaw::target_rails_version` was tail-appended under ABI v4 lockstep for murphy-8iym.
/// `CxRaw::active_support_extensions_enabled` was tail-appended into existing
/// padding under ABI v4 lockstep for murphy-pfcb (size unchanged).
pub const MURPHY_PLUGIN_ABI_VERSION: u32 = 4;

/// Ruby language version used for TargetRubyVersion gating.
///
/// Stored as major/minor only, matching RuboCop's accepted `TargetRubyVersion`
/// shape (`3.1`, not teeny versions). The ABI wire value is `major * 100 +
/// minor`; `0` means unset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RubyVersion {
    pub major: u16,
    pub minor: u16,
}

impl RubyVersion {
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }

    pub const fn to_wire(version: Option<Self>) -> u16 {
        match version {
            Some(v) => {
                let wire = (v.major as u32) * 100 + (v.minor as u32);
                if wire > u16::MAX as u32 {
                    u16::MAX
                } else {
                    wire as u16
                }
            }
            None => 0,
        }
    }

    pub const fn from_wire(wire: u16) -> Option<Self> {
        if wire == 0 {
            None
        } else {
            Some(Self::new(wire / 100, wire % 100))
        }
    }
}

/// Host-resolved `AllCops.*` context scalars threaded into every cop's
/// `CxRaw` for one run. Bundles the growing set of run-wide AllCops knobs
/// (`TargetRailsVersion`, `ActiveSupportExtensionsEnabled`, …) so dispatch
/// and test-support signatures don't grow one positional parameter per knob.
///
/// This is a plain host-side convenience struct — **not** `#[repr(C)]` and
/// **not** part of the plugin wire ABI. `build_cx_raw` unpacks it into the
/// individual `CxRaw` fields; do not add it to the `CxRaw` offset assertions.
#[derive(Debug, Clone, Copy, Default)]
pub struct AllCopsContext {
    /// `AllCops.TargetRailsVersion` (`None` = unset).
    pub target_rails_version: Option<RubyVersion>,
    /// `AllCops.ActiveSupportExtensionsEnabled` (default `false`).
    pub active_support_extensions_enabled: bool,
}

/// The dispatch entry for one cop: invoked once per matching node.
///
/// The thunk (generated by `register_cops!`, murphy-9cr.21) wraps a
/// `NodeCop::check`. It must not unwind across the boundary (ADR 0038
/// safety contract) and returns `0` on success, non-zero on a trapped
/// panic.
pub type DispatchFn = unsafe extern "C" fn(node: NodeId, cx: *const CxRaw) -> i32;

/// `#[repr(C)]` registration descriptor for one cop.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PluginCopV1 {
    /// `size_of::<PluginCopV1>()`, written by `register_cops!`
    /// (murphy-9cr.21). The loader (murphy-9cr.22) compares it against
    /// its own `size_of::<PluginCopV1>()` and rejects a plugin built
    /// against a divergent struct layout.
    pub size: usize,
    /// Cop `NAME`.
    pub name: RawSlice,
    /// Cop `DESCRIPTION`.
    pub description: RawSlice,
    /// Default severity wire byte.
    pub default_severity: u8,
    /// Default enablement tristate byte.
    pub default_enabled: u8,
    /// `CopOptions::SCHEMA`.
    pub options_ptr: *const OptionSpec,
    pub options_len: usize,
    /// `NodeCop::KINDS`.
    pub kinds_ptr: *const NodeKindTag,
    pub kinds_len: usize,
    /// Per-node dispatch entry.
    pub dispatch: DispatchFn,
    /// Allow-list of method symbol names for `kind = "send"` dispatch
    /// (murphy-ip0, RuboCop's `restrict_on_send` analogue). When
    /// non-empty, the host dispatcher peeks at each Send node's
    /// `method` symbol and skips invoking `dispatch` when the resolved
    /// string is not in this list — the cop never sees off-list
    /// Sends. When the slice is empty (the historical default), every
    /// Send subscribed via `KINDS` reaches `dispatch`. Filtering on
    /// non-send kinds is meaningless and is rejected at the `#[cop]`
    /// macro parse site, not here.
    pub send_methods_ptr: *const RawSlice,
    pub send_methods_len: usize,
    /// Safe lint execution tristate byte.
    pub safe: u8,
    /// Safe autocorrect tristate byte.
    pub safe_autocorrect: u8,
    /// Minimum `AllCops.TargetRubyVersion` required for this cop. `0` means
    /// no minimum, so the cop always participates after normal enablement.
    pub minimum_target_ruby_version: u16,
}

// Safety: PluginCopV1 is an immutable descriptor of non-owning views and
// `extern "C"` function pointers; it lives only in &'static cop tables.
// Sharing it across threads is sound for the same reason RawSlice and
// FnTable are Sync, under the ADR 0038 safety contract.
unsafe impl Sync for PluginCopV1 {}

/// `#[repr(C)]` table the plugin's single entry point fills in.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PluginRegistration {
    /// Must equal [`MURPHY_PLUGIN_ABI_VERSION`]; the loader rejects a mismatch.
    pub abi_version: u32,
    /// The plugin's cop table.
    pub cops_ptr: *const PluginCopV1,
    pub cops_len: usize,
}

// PluginRegistration is deliberately `!Sync` (and `!Send`): it is a
// loader-local out-parameter, filled by the plugin entry point on one
// thread and never shared. No `unsafe impl` is added, by design.

/// The one symbol a plugin `.so` exports, generated by `register_cops!`
/// (murphy-9cr.21). The loader calls it to obtain the cop table. It must
/// not unwind across the boundary (ADR 0038 safety contract) and returns
/// `0` on success, non-zero on a trapped panic or registration failure.
pub type MurphyPluginRegister = unsafe extern "C" fn(*mut PluginRegistration) -> i32;

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;

    #[test]
    fn raw_slice_from_str_round_trips() {
        let s = RawSlice::from_str("send");
        assert_eq!(unsafe { s.as_bytes() }, b"send");
        assert_eq!(unsafe { RawSlice::EMPTY.as_bytes() }, b"");
    }

    #[test]
    fn ruby_version_to_wire_saturates_on_overflow() {
        assert_eq!(
            RubyVersion::to_wire(Some(RubyVersion::new(700, 0))),
            u16::MAX
        );
    }

    #[test]
    fn raw_slice_field_offsets_are_frozen() {
        use std::mem::offset_of;
        assert_eq!(offset_of!(RawSlice, ptr), 0);
        assert_eq!(offset_of!(RawSlice, len), std::mem::size_of::<usize>());
    }

    #[test]
    fn fn_table_field_offsets_are_frozen() {
        use std::mem::offset_of;
        // Two function pointers; reordering them must fail this test.
        assert_eq!(offset_of!(FnTable, emit_offense), 0);
        assert_eq!(offset_of!(FnTable, emit_edit), size_of::<usize>());
    }

    #[test]
    fn raw_offense_field_offsets_are_frozen() {
        use std::mem::offset_of;
        assert_eq!(offset_of!(RawOffense, cop_name), 0);
        assert_eq!(offset_of!(RawOffense, message), size_of::<RawSlice>());
        assert_eq!(offset_of!(RawOffense, range), 2 * size_of::<RawSlice>());
    }

    #[test]
    fn cx_raw_field_offsets_are_frozen() {
        use std::mem::offset_of;
        assert_eq!(offset_of!(CxRaw, nodes), 0);
        assert_eq!(offset_of!(CxRaw, nodes_len), 8);
        assert_eq!(offset_of!(CxRaw, lists), 16);
        assert_eq!(offset_of!(CxRaw, lists_len), 24);
        assert_eq!(offset_of!(CxRaw, interner_blob), 32);
        assert_eq!(offset_of!(CxRaw, interner_blob_len), 40);
        assert_eq!(offset_of!(CxRaw, interner_offsets), 48);
        assert_eq!(offset_of!(CxRaw, interner_offsets_len), 56);
        assert_eq!(offset_of!(CxRaw, comments), 64);
        assert_eq!(offset_of!(CxRaw, comments_len), 72);
        assert_eq!(offset_of!(CxRaw, source), 80);
        assert_eq!(offset_of!(CxRaw, source_len), 88);
        assert_eq!(offset_of!(CxRaw, root), 96);
        assert_eq!(offset_of!(CxRaw, cop_name), 104);
        assert_eq!(offset_of!(CxRaw, fns), 120);
        assert_eq!(offset_of!(CxRaw, sink), 128);
        assert_eq!(offset_of!(CxRaw, sorted_tokens), 136);
        assert_eq!(offset_of!(CxRaw, sorted_tokens_len), 144);
        assert_eq!(offset_of!(CxRaw, options_json), 152);
        assert_eq!(offset_of!(CxRaw, call_closing_locs), 168);
        assert_eq!(offset_of!(CxRaw, call_closing_locs_len), 176);
        assert_eq!(offset_of!(CxRaw, call_operator_locs), 184);
        assert_eq!(offset_of!(CxRaw, call_operator_locs_len), 192);
        // es99.5: var_model appended after call_operator_locs_len
        assert_eq!(offset_of!(CxRaw, var_model), 200);
        assert_eq!(offset_of!(CxRaw, node_slice_arena), 208);
        assert_eq!(offset_of!(CxRaw, alloc_node_slice), 216);
        assert_eq!(offset_of!(CxRaw, file_path), 224);
        assert_eq!(offset_of!(CxRaw, target_rails_version), 240);
        assert_eq!(offset_of!(CxRaw, active_support_extensions_enabled), 242);
        assert_eq!(size_of::<CxRaw>(), 248);
    }

    #[test]
    fn option_spec_is_repr_c_seven_slices() {
        use std::mem::offset_of;
        assert_eq!(size_of::<OptionSpec>(), 7 * size_of::<RawSlice>());
        assert_eq!(offset_of!(OptionSpec, name), 0);
        assert_eq!(offset_of!(OptionSpec, ty), size_of::<RawSlice>());
        assert_eq!(
            offset_of!(OptionSpec, default_json),
            2 * size_of::<RawSlice>()
        );
        assert_eq!(
            offset_of!(OptionSpec, description),
            3 * size_of::<RawSlice>()
        );
        assert_eq!(
            offset_of!(OptionSpec, enum_values_json),
            4 * size_of::<RawSlice>()
        );
        assert_eq!(
            offset_of!(OptionSpec, replacement),
            5 * size_of::<RawSlice>()
        );
        assert_eq!(offset_of!(OptionSpec, reason), 6 * size_of::<RawSlice>());
    }

    #[test]
    fn abi_version_is_four() {
        // Bumped from 1 → 2 in murphy-es99.8 (SourceTokenKind gained
        // Comma/LeftBrace/RightBrace; additive but the loader must still
        // reject v1 plugins that predate the new token kinds).
        // Bumped from 2 → 3 in murphy-es99.4 (PluginCopV1 gained safe
        // metadata tail fields; size mismatch must reject old plugins).
        // Bumped from 3 → 4 in es99.5 (CxRaw gained `var_model` at offset 200).
        // Kept at 4 for murphy-7nze by explicit project policy despite
        // tail-appending CxRaw node-slice allocator fields.
        // Kept at 4 for murphy-vmg5 by the same policy despite tail-appending
        // CxRaw file_path.
        assert_eq!(MURPHY_PLUGIN_ABI_VERSION, 4);
    }

    #[test]
    fn plugin_cop_v1_field_offsets_are_frozen() {
        use std::mem::offset_of;
        // Every field offset is pinned: swapping any two fields — even two
        // same-typed siblings — must fail at least one of these asserts.
        assert_eq!(offset_of!(PluginCopV1, size), 0);
        assert_eq!(offset_of!(PluginCopV1, name), 8);
        assert_eq!(offset_of!(PluginCopV1, description), 24);
        assert_eq!(offset_of!(PluginCopV1, default_severity), 40);
        assert_eq!(offset_of!(PluginCopV1, default_enabled), 41);
        assert_eq!(offset_of!(PluginCopV1, options_ptr), 48);
        assert_eq!(offset_of!(PluginCopV1, options_len), 56);
        assert_eq!(offset_of!(PluginCopV1, kinds_ptr), 64);
        assert_eq!(offset_of!(PluginCopV1, kinds_len), 72);
        assert_eq!(offset_of!(PluginCopV1, dispatch), 80);
        // Host-level send-method allow-list (murphy-ip0). Added at the
        // end of the struct so existing field offsets stay frozen and
        // the `size`-field rejection in the plugin loader continues to
        // catch divergent struct layouts.
        assert_eq!(offset_of!(PluginCopV1, send_methods_ptr), 88);
        assert_eq!(offset_of!(PluginCopV1, send_methods_len), 96);
        assert_eq!(offset_of!(PluginCopV1, safe), 104);
        assert_eq!(offset_of!(PluginCopV1, safe_autocorrect), 105);
        assert_eq!(offset_of!(PluginCopV1, minimum_target_ruby_version), 106);
        assert_eq!(size_of::<PluginCopV1>(), 112);
    }

    #[test]
    fn plugin_registration_field_offsets_are_frozen() {
        use std::mem::offset_of;
        // Every field offset is pinned: swapping any two fields must fail
        // at least one of these asserts.
        assert_eq!(offset_of!(PluginRegistration, abi_version), 0);
        assert_eq!(offset_of!(PluginRegistration, cops_ptr), 8);
        assert_eq!(offset_of!(PluginRegistration, cops_len), 16);
        assert_eq!(size_of::<PluginRegistration>(), 24);
    }
}
