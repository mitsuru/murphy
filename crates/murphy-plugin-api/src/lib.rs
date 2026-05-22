//! Safe, plugin-author-facing surface over the Murphy native plugin ABI.
//!
//! This crate is what third-party native plugins import. It re-exports the
//! raw `#[repr(C)]` ABI types from `murphy-core`, defines safe traits
//! ([`Cop`] for metadata, [`FileCop`] / [`NodeCop`] / [`CallCop`] for
//! callbacks) that authors implement on plain Rust structs, exposes the
//! generic thunks the proc-macro layer plugs into `MurphyPluginCopV1`, and
//! ships the [`kinds`] module of node-kind string constants.
//!
//! `register_cops!`, `#[derive(CopOptions)]`, and `#[murphy::cop]` live in
//! the separate `murphy-plugin-macros` crate (murphy-9cr.6 / .7 / .8) and
//! consume the surface defined here.

use std::ffi::c_void;
use std::marker::PhantomData;

pub use murphy_core::{
    CopOptionMetadata, MURPHY_CALL_ARGUMENT_KIND_OTHER, MURPHY_CALL_ARGUMENT_KIND_STRING,
    MURPHY_CALL_ARGUMENT_KIND_SYMBOL, MURPHY_CALL_RECEIVER_FLOAT, MURPHY_CALL_RECEIVER_INTEGER,
    MURPHY_CALL_RECEIVER_NONE, MURPHY_CALL_RECEIVER_OTHER, MURPHY_PLUGIN_ABI_VERSION,
    MURPHY_SEVERITY_ERROR, MURPHY_SEVERITY_UNSET, MURPHY_SEVERITY_WARNING, MURPHY_TRISTATE_FALSE,
    MURPHY_TRISTATE_TRUE, MURPHY_TRISTATE_UNSET, MurphyCallContext, MurphyCallDispatchV1,
    MurphyCopOptionV1, MurphyEmitOffense, MurphyFileContext, MurphyNodeContext,
    MurphyNodeDispatchV1, MurphyPluginAutocorrect, MurphyPluginCallArgument, MurphyPluginCopV1,
    MurphyPluginEdit, MurphyPluginOffense, MurphyPluginV1, MurphyRange, MurphyRunCallDispatch,
    MurphyRunFile, MurphyRunNodeDispatch, MurphySlice, Severity,
};

mod config_error;
pub mod kinds;

pub use config_error::{ConfigError, ConfigErrorKind};

/// A cop, as authored against the plugin API.
///
/// `Cop` is **metadata-only**: every field is an associated `const` so the
/// `register_cops!` macro can assemble the static `MurphyPluginCopV1`
/// table at const-eval time. Runtime callbacks live on the separate
/// [`FileCop`] / [`NodeCop`] / [`CallCop`] traits; a cop opts in to a
/// callback by implementing the relevant trait *and* setting the matching
/// `RUN_*` const to `Some(run_*_thunk::<Self>)`.
///
/// `murphy-9cr.8`'s `#[murphy::cop]` attribute macro removes that
/// boilerplate; in the meantime authors write it by hand.
///
/// # Example
///
/// ```
/// use murphy_plugin_api::{
///     Cop, FileCop, FileContext, Emitter, MurphyRunFile, NoOptions, Severity,
///     run_file_thunk,
/// };
///
/// struct NoTabs;
///
/// impl Cop for NoTabs {
///     type Options = NoOptions;
///     const NAME: &'static str = "Plugin/NoTabs";
///     const DESCRIPTION: &'static str = "Forbids tab indentation.";
///     const DEFAULT_SEVERITY: Option<Severity> = Some(Severity::Warning);
///     const RUN_FILE: Option<MurphyRunFile> = Some(run_file_thunk::<NoTabs>);
/// }
///
/// impl FileCop for NoTabs {
///     fn run_file(_ctx: &FileContext<'_>, _emit: &mut Emitter<'_>) {
///         // ... lint logic
///     }
/// }
/// ```
pub trait Cop: Send + Sync + 'static {
    /// Option struct backing this cop's `[cops.rules."Name"]` table.
    ///
    /// Defaults to [`NoOptions`] for cops that take no configuration
    /// beyond `enabled` / `severity`.
    type Options: CopOptions;

    /// The cop identifier, e.g. `"Plugin/MyCop"`. Must match the runtime
    /// name visible in `murphy.toml` and in offense JSON.
    const NAME: &'static str;

    /// One-line human-readable description. Surfaced by future `murphy
    /// plugins list` diagnostics and editor hover. Empty by default.
    const DESCRIPTION: &'static str = "";

    /// Default severity used when the user does not override it. `None`
    /// leaves Murphy's built-in fallback (typically warning).
    const DEFAULT_SEVERITY: Option<Severity> = None;

    /// Default enablement. `None` keeps Murphy's built-in default
    /// (`Style` / `Lint` / `Murphy` namespaces on, niche cops off).
    const DEFAULT_ENABLED: Option<bool> = None;

    /// File-scope callback fn pointer. Defaults to `None`; cops that
    /// implement [`FileCop`] set this to
    /// `Some(run_file_thunk::<Self>)`.
    const RUN_FILE: Option<MurphyRunFile> = None;

    /// Node-dispatch callback fn pointer. Defaults to `None`; cops that
    /// implement [`NodeCop`] set this to
    /// `Some(run_node_thunk::<Self>)`.
    const RUN_NODE: Option<MurphyRunNodeDispatch> = None;

    /// Call-dispatch callback fn pointer. Defaults to `None`; cops that
    /// implement [`CallCop`] set this to
    /// `Some(run_call_thunk::<Self>)`.
    const RUN_CALL: Option<MurphyRunCallDispatch> = None;
}

/// Cops that scan whole files. Implementations are stateless (no
/// `&self`); Murphy invokes [`Self::run_file`] once per matched file
/// through the [`run_file_thunk`].
pub trait FileCop: Cop {
    /// Invoked once per file the cop is configured to inspect.
    fn run_file(ctx: &FileContext<'_>, emit: &mut Emitter<'_>);
}

/// Cops that subscribe to specific AST node kinds. Implementations are
/// stateless; Murphy invokes [`Self::run_node`] for each dispatched
/// node through [`run_node_thunk`].
pub trait NodeCop: Cop {
    /// Invoked once per matched node. The node kind, source range, and
    /// other context live on `ctx`.
    fn run_node(ctx: &NodeContext<'_>, emit: &mut Emitter<'_>);
}

/// Cops that subscribe to specific call expressions. Implementations are
/// stateless; Murphy invokes [`Self::run_call`] for each matched call
/// through [`run_call_thunk`].
pub trait CallCop: Cop {
    /// Invoked once per matched call.
    fn run_call(ctx: &CallContext<'_>, emit: &mut Emitter<'_>);
}

/// Plugin-side counterpart of [`CopOptionMetadata`].
///
/// Plugin authors usually derive this trait via `#[derive(CopOptions)]`
/// (murphy-9cr.7). Direct implementation is supported but requires
/// hand-maintaining the schema slice.
///
/// `Default` is required so the runtime can hand the cop an `Options`
/// instance even when the user supplied no `[cops.rules."X"]` table.
///
/// `SCHEMA` is an associated `const` (not a method) so it can be read
/// from `static` and `const fn` contexts — that's what
/// [`register_cops!`](../../murphy_plugin_macros/macro.register_cops.html)
/// needs to build its static cop table.
pub trait CopOptions: Default + Sized + 'static {
    /// Static schema describing each option. The validation gate
    /// (murphy-9cr.9) reads this to diff against the user's config.
    const SCHEMA: &'static [MurphyCopOptionV1] = &[];

    /// Decode an `Options` value from a cop's `[cops.rules."Name"]`
    /// config table, serialised as a JSON object.
    ///
    /// The default implementation ignores the input and returns
    /// [`Default::default`], which is correct for [`NoOptions`] and any
    /// cop that takes no configuration. `#[derive(CopOptions)]`
    /// (murphy-9cr.7) overrides it with field-by-field decoding.
    fn from_config_json(_bytes: &[u8]) -> Result<Self, ConfigError> {
        Ok(Self::default())
    }
}

/// Marker type for cops that declare no options.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoOptions;

impl CopOptions for NoOptions {}

// --- Safe context wrappers ------------------------------------------------
//
// These wrap the raw `Murphy*Context` ABI structs for the duration of a
// thunk callback. They expose the minimal accessors needed by murphy-9cr.6
// itself; richer accessors land with the typed-wrapper work in
// murphy-9cr.5 / .8.

/// Borrowed view of a `MurphyFileContext` valid for the lifetime of a
/// single [`FileCop::run_file`] invocation.
pub struct FileContext<'a> {
    raw: &'a MurphyFileContext,
}

impl<'a> FileContext<'a> {
    /// Wrap a raw ABI context. Used by [`run_file_thunk`].
    ///
    /// # Safety
    /// `raw` must point to a `MurphyFileContext` whose backing buffers
    /// remain valid for `'a`. Murphy's loader guarantees this for the
    /// duration of a thunk invocation.
    pub unsafe fn from_raw(raw: &'a MurphyFileContext) -> Self {
        Self { raw }
    }

    /// UTF-8 file path (loader has already validated encoding).
    pub fn file(&self) -> &'a [u8] {
        slice_bytes(&self.raw.file)
    }

    /// Source bytes for the file being inspected.
    pub fn source(&self) -> &'a [u8] {
        slice_bytes(&self.raw.source)
    }

    /// Per-cop config payload (`[cops.rules."Name"]` serialised as JSON).
    pub fn config(&self) -> &'a [u8] {
        slice_bytes(&self.raw.config)
    }
}

/// Borrowed view of a `MurphyNodeContext` valid for the lifetime of a
/// single [`NodeCop::run_node`] invocation.
pub struct NodeContext<'a> {
    raw: &'a MurphyNodeContext,
}

impl<'a> NodeContext<'a> {
    /// Wrap a raw ABI context. Used by [`run_node_thunk`].
    ///
    /// # Safety
    /// See [`FileContext::from_raw`].
    pub unsafe fn from_raw(raw: &'a MurphyNodeContext) -> Self {
        Self { raw }
    }

    /// File path for the source under analysis.
    pub fn file(&self) -> &'a [u8] {
        slice_bytes(&self.raw.file)
    }

    /// Full source bytes.
    pub fn source(&self) -> &'a [u8] {
        slice_bytes(&self.raw.source)
    }

    /// Per-cop config payload.
    pub fn config(&self) -> &'a [u8] {
        slice_bytes(&self.raw.config)
    }

    /// Snake-case node kind (matches the [`kinds`] constants).
    pub fn node_kind(&self) -> &'a [u8] {
        slice_bytes(&self.raw.node_kind)
    }

    /// Source range covered by this node.
    pub fn range(&self) -> MurphyRange {
        self.raw.range
    }

    /// Dispatch id assigned by `#[on_node]` (murphy-9cr.8). Plugins can
    /// use this to demultiplex multiple `#[on_node]` attributes that
    /// share a `run_node` body.
    pub fn dispatch_id(&self) -> usize {
        self.raw.dispatch_id
    }
}

/// Borrowed view of a `MurphyCallContext` valid for the lifetime of a
/// single [`CallCop::run_call`] invocation.
pub struct CallContext<'a> {
    raw: &'a MurphyCallContext,
}

impl<'a> CallContext<'a> {
    /// Wrap a raw ABI context. Used by [`run_call_thunk`].
    ///
    /// # Safety
    /// See [`FileContext::from_raw`].
    pub unsafe fn from_raw(raw: &'a MurphyCallContext) -> Self {
        Self { raw }
    }

    /// File path for the source under analysis.
    pub fn file(&self) -> &'a [u8] {
        slice_bytes(&self.raw.file)
    }

    /// Full source bytes.
    pub fn source(&self) -> &'a [u8] {
        slice_bytes(&self.raw.source)
    }

    /// Per-cop config payload.
    pub fn config(&self) -> &'a [u8] {
        slice_bytes(&self.raw.config)
    }

    /// Method name of the matched call.
    pub fn name(&self) -> &'a [u8] {
        slice_bytes(&self.raw.name)
    }

    /// Dispatch id (see [`NodeContext::dispatch_id`]).
    pub fn dispatch_id(&self) -> usize {
        self.raw.dispatch_id
    }

    /// Source range covering the method-name token.
    pub fn message_range(&self) -> MurphyRange {
        self.raw.message_range
    }
}

/// Offense-emission handle valid for the lifetime of one callback.
///
/// In murphy-9cr.6 this is a minimal stub; richer accessors (emit
/// offenses with autocorrect, suppress offenses, etc.) land with the
/// attribute-macro work in murphy-9cr.8.
pub struct Emitter<'a> {
    emit: MurphyEmitOffense,
    sink: *mut c_void,
    _phantom: PhantomData<&'a mut ()>,
}

impl<'a> Emitter<'a> {
    /// Wrap a raw emit callback. Used by every `run_*_thunk`.
    ///
    /// # Safety
    /// `emit` must remain callable with `sink` for the duration of `'a`.
    /// Murphy's loader provides this for the duration of a thunk
    /// invocation.
    pub unsafe fn from_raw(emit: MurphyEmitOffense, sink: *mut c_void) -> Self {
        Self {
            emit,
            sink,
            _phantom: PhantomData,
        }
    }

    /// Forward a pre-built offense to Murphy's collector. Higher-level
    /// builders land in murphy-9cr.8.
    ///
    /// # Safety
    /// `offense.cop_name` / `offense.message` slices must remain valid
    /// for the duration of this call. The lifetimes are not yet encoded
    /// in the API; tighter signatures arrive with the attribute macro.
    pub unsafe fn emit(&mut self, offense: &MurphyPluginOffense) {
        unsafe { (self.emit)(self.sink, offense) }
    }
}

// --- Thunks ---------------------------------------------------------------
//
// These are the actual ABI-shaped fn pointers Murphy's loader calls back.
// `register_cops!` plants `Some(run_file_thunk::<MyCop>)` (etc.) into
// `MurphyPluginCopV1.run_file` so the loader sees a normal `extern "C"`
// callable, while the user code stays on the safe side of the boundary.

/// Bridges [`MurphyRunFile`] to [`FileCop::run_file`].
///
/// # Safety
/// Called by Murphy's loader through the ABI. `ctx` must point to a
/// valid `MurphyFileContext`; `emit` / `sink` form a valid callback
/// pair. These invariants hold by construction inside Murphy.
pub unsafe extern "C" fn run_file_thunk<C: FileCop>(
    ctx: *const MurphyFileContext,
    emit: MurphyEmitOffense,
    sink: *mut c_void,
) -> i32 {
    if ctx.is_null() {
        return 1;
    }
    let safe_ctx = unsafe { FileContext::from_raw(&*ctx) };
    let mut emitter = unsafe { Emitter::from_raw(emit, sink) };
    C::run_file(&safe_ctx, &mut emitter);
    0
}

/// Bridges [`MurphyRunNodeDispatch`] to [`NodeCop::run_node`].
///
/// # Safety
/// See [`run_file_thunk`].
pub unsafe extern "C" fn run_node_thunk<C: NodeCop>(
    ctx: *const MurphyNodeContext,
    emit: MurphyEmitOffense,
    sink: *mut c_void,
) -> i32 {
    if ctx.is_null() {
        return 1;
    }
    let safe_ctx = unsafe { NodeContext::from_raw(&*ctx) };
    let mut emitter = unsafe { Emitter::from_raw(emit, sink) };
    C::run_node(&safe_ctx, &mut emitter);
    0
}

/// Bridges [`MurphyRunCallDispatch`] to [`CallCop::run_call`].
///
/// # Safety
/// See [`run_file_thunk`].
pub unsafe extern "C" fn run_call_thunk<C: CallCop>(
    ctx: *const MurphyCallContext,
    emit: MurphyEmitOffense,
    sink: *mut c_void,
) -> i32 {
    if ctx.is_null() {
        return 1;
    }
    let safe_ctx = unsafe { CallContext::from_raw(&*ctx) };
    let mut emitter = unsafe { Emitter::from_raw(emit, sink) };
    C::run_call(&safe_ctx, &mut emitter);
    0
}

// --- Internal helpers used by the `register_cops!` macro -----------------
//
// `__internal` is doc(hidden) but public so the macro can name it from
// outside the crate. Stable in spirit because the macro is the only
// expected caller.

#[doc(hidden)]
pub mod __internal {
    use super::{
        Cop, CopOptions, MURPHY_SEVERITY_ERROR, MURPHY_SEVERITY_UNSET, MURPHY_SEVERITY_WARNING,
        MURPHY_TRISTATE_FALSE, MURPHY_TRISTATE_TRUE, MURPHY_TRISTATE_UNSET, MurphyPluginCopV1,
        MurphySlice, Severity,
    };

    /// Build a [`MurphyPluginCopV1`] for cop `C` at const-eval time.
    ///
    /// Called by `register_cops!` once per cop in the type list.
    pub const fn build_cop<C: Cop>() -> MurphyPluginCopV1 {
        let schema = <<C as Cop>::Options as CopOptions>::SCHEMA;
        MurphyPluginCopV1 {
            size: std::mem::size_of::<MurphyPluginCopV1>(),
            name: str_to_slice(C::NAME),
            run_file: C::RUN_FILE,
            description: str_to_slice(C::DESCRIPTION),
            default_severity: severity_to_wire(C::DEFAULT_SEVERITY),
            default_enabled: tristate_to_wire(C::DEFAULT_ENABLED),
            options_ptr: schema.as_ptr(),
            options_len: schema.len(),
        }
    }

    /// Reject duplicate cop NAMEs at const-eval time. Called from inside
    /// the `const _: () = ...;` block generated by `register_cops!`, so
    /// any duplicate surfaces as a compile error pointing at that block.
    pub const fn assert_unique_cop_names<const N: usize>(names: [&'static str; N]) {
        let mut i = 0;
        while i < N {
            let mut j = i + 1;
            while j < N {
                if str_eq(names[i], names[j]) {
                    panic!("register_cops!: duplicate cop NAME");
                }
                j += 1;
            }
            i += 1;
        }
    }

    pub const fn str_to_slice(s: &'static str) -> MurphySlice {
        MurphySlice {
            ptr: s.as_ptr(),
            len: s.len(),
        }
    }

    pub const fn severity_to_wire(s: Option<Severity>) -> u8 {
        match s {
            Some(Severity::Warning) => MURPHY_SEVERITY_WARNING,
            Some(Severity::Error) => MURPHY_SEVERITY_ERROR,
            None => MURPHY_SEVERITY_UNSET,
        }
    }

    pub const fn tristate_to_wire(s: Option<bool>) -> u8 {
        match s {
            Some(false) => MURPHY_TRISTATE_FALSE,
            Some(true) => MURPHY_TRISTATE_TRUE,
            None => MURPHY_TRISTATE_UNSET,
        }
    }

    const fn str_eq(a: &str, b: &str) -> bool {
        if a.len() != b.len() {
            return false;
        }
        let a = a.as_bytes();
        let b = b.as_bytes();
        let mut i = 0;
        while i < a.len() {
            if a[i] != b[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

fn slice_bytes(slice: &MurphySlice) -> &[u8] {
    if slice.len == 0 {
        return &[];
    }
    // Safety: ABI contract — caller (Murphy loader) provides a valid
    // pointer/length pair when len > 0.
    unsafe { std::slice::from_raw_parts(slice.ptr, slice.len) }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubCop;

    impl Cop for StubCop {
        type Options = NoOptions;
        const NAME: &'static str = "Plugin/Stub";
    }

    #[test]
    fn no_options_has_empty_schema() {
        assert!(<NoOptions as CopOptions>::SCHEMA.is_empty());
    }

    #[test]
    fn no_options_from_config_json_ignores_input() {
        // The default `from_config_json` returns Default regardless of
        // input, even malformed JSON.
        let ok = <NoOptions as CopOptions>::from_config_json(b"not json at all");
        assert!(ok.is_ok());
    }

    #[test]
    fn config_error_kinds_round_trip() {
        assert!(matches!(
            ConfigError::not_an_object().kind(),
            ConfigErrorKind::NotAnObject
        ));
        assert!(matches!(
            ConfigError::type_mismatch("f", "int").kind(),
            ConfigErrorKind::TypeMismatch { field, expected }
                if field == "f" && *expected == "int"
        ));
        assert!(matches!(
            ConfigError::enum_violation("f", "v").kind(),
            ConfigErrorKind::EnumViolation { field, value }
                if field == "f" && value == "v"
        ));
        assert!(matches!(
            ConfigError::missing_required("f").kind(),
            ConfigErrorKind::MissingRequired { field } if field == "f"
        ));
    }

    #[test]
    fn build_cop_defaults_use_unset_sentinels() {
        let cop = __internal::build_cop::<StubCop>();
        assert_eq!(cop.size, std::mem::size_of::<MurphyPluginCopV1>());
        assert_eq!(cop.default_severity, MURPHY_SEVERITY_UNSET);
        assert_eq!(cop.default_enabled, MURPHY_TRISTATE_UNSET);
        assert!(cop.run_file.is_none());
        assert_eq!(cop.options_len, 0);
        // options_ptr is allowed to be a dangling-but-aligned `&[]::as_ptr()`
        // value when options_len == 0; the loader only dereferences when
        // options_len > 0.
        // name pointer wraps the &'static str's bytes directly.
        let name_bytes = unsafe { std::slice::from_raw_parts(cop.name.ptr, cop.name.len) };
        assert_eq!(name_bytes, b"Plugin/Stub");
    }

    #[test]
    fn severity_to_wire_round_trips_each_variant() {
        assert_eq!(__internal::severity_to_wire(None), MURPHY_SEVERITY_UNSET);
        assert_eq!(
            __internal::severity_to_wire(Some(Severity::Warning)),
            MURPHY_SEVERITY_WARNING
        );
        assert_eq!(
            __internal::severity_to_wire(Some(Severity::Error)),
            MURPHY_SEVERITY_ERROR
        );
    }

    #[test]
    fn tristate_to_wire_round_trips_each_variant() {
        assert_eq!(__internal::tristate_to_wire(None), MURPHY_TRISTATE_UNSET);
        assert_eq!(
            __internal::tristate_to_wire(Some(false)),
            MURPHY_TRISTATE_FALSE
        );
        assert_eq!(
            __internal::tristate_to_wire(Some(true)),
            MURPHY_TRISTATE_TRUE
        );
    }

    #[test]
    fn assert_unique_cop_names_accepts_distinct() {
        // const-eval — failure would be a compile error, so reaching the
        // assertion at runtime is itself the success signal.
        __internal::assert_unique_cop_names::<3>(["Plugin/A", "Plugin/B", "Plugin/C"]);
    }

    #[test]
    #[should_panic(expected = "duplicate cop NAME")]
    fn assert_unique_cop_names_rejects_duplicates() {
        __internal::assert_unique_cop_names::<2>(["Plugin/Same", "Plugin/Same"]);
    }

    #[test]
    fn kinds_count_matches_all_slice_length() {
        assert_eq!(kinds::ALL.len(), kinds::COUNT);
    }

    #[test]
    fn kinds_all_has_no_duplicates() {
        let mut seen = std::collections::BTreeSet::new();
        for kind in kinds::ALL {
            assert!(seen.insert(*kind), "duplicate node kind: {kind}");
        }
    }

    #[test]
    fn kinds_consts_resolve_to_known_examples() {
        assert_eq!(kinds::CALL_NODE, "call");
        assert_eq!(kinds::ALIAS_GLOBAL_VARIABLE_NODE, "alias_global_variable");
        assert_eq!(kinds::IF_NODE, "if");
        assert_eq!(kinds::UNLESS_NODE, "unless");
        assert_eq!(kinds::CLASS_NODE, "class");
    }
}
