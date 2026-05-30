//! `MrubyCopProxy` — wrap an arena-shaped mruby user cop as a
//! [`PluginCopV1`] value the single-surface dispatcher (`run_cops`)
//! can call (murphy-9cr.24.1).
//!
//! The arena dispatcher consumes the C-ABI `PluginCopV1` directly:
//! `DispatchFn = unsafe extern "C" fn(NodeId, *const CxRaw) -> i32`
//! has no self-pointer, so all mruby cops share a single thunk
//! ([`mruby_dispatch_thunk`]) that looks up the right proxy by
//! `(*cx).cop_name` in a thread-local map.
//!
//! # Thread-affinity contract (load-bearing)
//!
//! [`CURRENT_MRUBY_PROXIES`] is `thread_local!`, so the proxy map and
//! the dispatcher must be on the **same thread**:
//!
//! 1. The caller invokes [`current_mruby_proxies_populate`] on thread T.
//! 2. The caller invokes [`crate::dispatch::run_cops`] **on thread T**
//!    (synchronously, no offload). The dispatcher invokes the thunk
//!    inline on T, so the lookup hits T's populated map.
//! 3. The caller invokes [`current_mruby_proxies_drain`] on thread T.
//!
//! Violating this contract (populating on T, dispatching on T'):
//! - [`mruby_dispatch_thunk`] on T' finds an empty map, returns `1`
//!   for every node, and the dispatcher prints a `cop 'NAME' returned
//!   non-zero` stderr diagnostic per cop before disabling it.
//! - **No silent miscompile**: every miss is logged.
//! - Per-rayon-worker parallelism is achieved by repeating
//!   `populate → run_cops → drain` on each worker thread (one file per
//!   worker). murphy-9cr.24.9 (cops_path loader) is responsible for
//!   that wiring; this slice owns the per-thread API only.
//!
//! `MrubyCopProxy` itself is `!Send` + `!Sync` (the owned `MrubyState`
//! is thread-confined), so moving a proxy across threads is rejected
//! by the type system before the contract can be broken at runtime.
//!
//! # Lifecycle (ADR 0009)
//!
//! Each `MrubyCopProxy` owns its `MrubyState`. `Drop` closes the state
//! (`mrb_close`) at the end of the populate→drain span, preserving the
//! per-cop-per-file `mrb_open` / `mrb_close` discipline.
//!
//! ## `unsafe_op_in_unsafe_fn`
//!
//! Per `state.rs` discipline: every `unsafe extern "C"` callback and
//! every `unsafe fn` here wraps each FFI call in its own `unsafe {}`
//! block — no module-level blanket allow.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

use murphy_ast::{AstNode, NodeId, NodeKind, NodeList, OptNodeId, Symbol};
use murphy_plugin_api::{
    CxRaw, DispatchFn, FnTable, NodeKindTag, PluginCopV1, RawEdit, RawOffense, RawSlice,
    TRISTATE_UNSET,
};

use mruby3_sys::{
    RClass, mrb_class_get, mrb_class_get_under, mrb_define_module_function, mrb_funcall_argv,
    mrb_get_args, mrb_int, mrb_intern_cstr, mrb_load_string, mrb_obj_new, mrb_state, mrb_sym,
    mrb_value,
};

use crate::mruby::state::MrubyState;

// ---------------------------------------------------------------------
// thread-local proxy map + accessors
// ---------------------------------------------------------------------

thread_local! {
    static CURRENT_MRUBY_PROXIES: RefCell<HashMap<Vec<u8>, Box<MrubyCopProxy>>> =
        RefCell::new(HashMap::new());
}

/// Move `proxies` into THIS thread's mruby proxy map.
///
/// The host then calls [`crate::dispatch::run_cops`] **on the same
/// thread**; the dispatcher invokes [`mruby_dispatch_thunk`] inline,
/// which reads this thread-local. Calling `run_cops` from a different
/// thread will find an empty map there and disable every mruby cop
/// (with a stderr diagnostic — see module docs).
///
/// Call [`current_mruby_proxies_drain`] on the SAME thread when the run
/// completes so `MrubyState::Drop` (`mrb_close`) fires in the
/// expected order (ADR 0009).
pub fn current_mruby_proxies_populate(proxies: HashMap<Vec<u8>, Box<MrubyCopProxy>>) {
    CURRENT_MRUBY_PROXIES.with(|cell| {
        let mut map = cell.borrow_mut();
        debug_assert!(
            map.is_empty(),
            "populate called while a populated map exists"
        );
        *map = proxies;
    });
}

/// Take THIS thread's mruby proxy map. Must be called on the same
/// thread that called [`current_mruby_proxies_populate`]. Dropping the
/// returned map closes every owned `mrb_state` (per-cop-per-file
/// lifecycle, ADR 0009).
pub fn current_mruby_proxies_drain() -> HashMap<Vec<u8>, Box<MrubyCopProxy>> {
    CURRENT_MRUBY_PROXIES.with(|cell| std::mem::take(&mut *cell.borrow_mut()))
}

/// Run `f` with a shared borrow of THIS thread's mruby proxy map.
/// The borrow lasts only for `f`'s body — the dispatch thunk takes a
/// mutable borrow inside `run_cops`, so callers must not hold this
/// shared borrow across a `run_cops` call.
///
/// Useful for callers building a `Vec<PluginCopV1>` from the currently
/// populated proxies (each `PluginCopV1` references proxy-owned memory).
pub fn with_current_mruby_proxies<F, R>(f: F) -> R
where
    F: FnOnce(&HashMap<Vec<u8>, Box<MrubyCopProxy>>) -> R,
{
    CURRENT_MRUBY_PROXIES.with(|cell| f(&cell.borrow()))
}

// ---------------------------------------------------------------------
// MrubyUserData: what mrb_state.ud points to during arena dispatch.
// ---------------------------------------------------------------------

/// Payload stored in `mrb->ud` during a single `MrubyCopProxy::check`
/// call. Primitives (`emit_offense`, `node_range`, `node_kind`)
/// reconstitute the active `CxRaw` from here.
#[repr(C)]
pub(crate) struct MrubyUserData {
    /// Borrowed `*const CxRaw` set by [`MrubyCopProxy::check`] before
    /// each `mrb_funcall_argv`. Valid only for that call's duration.
    pub(crate) cx_raw: *const CxRaw,
    /// Currently-active cop instance (24.4 reads this for `Murphy.match`
    /// to build a `MrubyPredicateHost` rooted at the right cop).
    #[allow(dead_code)]
    pub(crate) current_cop_instance: mrb_value,
}

// ---------------------------------------------------------------------
// MrubyCopProxy
// ---------------------------------------------------------------------

/// One arena-shaped mruby user cop, owning its `MrubyState`.
pub struct MrubyCopProxy {
    /// `mrb_state` owner. `Drop`-closed at the end of the populate→drain
    /// span (per-cop-per-file `mrb_open` / `mrb_close`, ADR 0009).
    state: MrubyState,
    /// Cached `Murphy::Node` class used to build the `node` argument
    /// passed to `on_<kind>(node)` hooks.
    node_class: *mut RClass,
    /// Loaded cop instance (`class.new`).
    instance: mrb_value,
    /// Cop NAME bytes, heap-owned so a `RawSlice` can reference them.
    name: Vec<u8>,
    /// Cop DESCRIPTION bytes, heap-owned.
    description: Vec<u8>,
    /// Severity wire byte. v1 host default `Warning == 0` (see
    /// [`crate::dispatch::decode_severity`]).
    default_severity: u8,
    /// Enablement byte. `1` = enabled (default).
    default_enabled: u8,
    /// Node kinds this cop subscribes to; the dispatcher reads
    /// `kinds.as_ptr() / kinds.len()` via the `PluginCopV1` it built
    /// from this proxy.
    kinds: Vec<NodeKindTag>,
    /// Parallel to `kinds`: `mrb_intern_cstr("on_<snake_kind>")` so the
    /// `check` thunk can find the right method without re-interning.
    sym_table: Vec<mrb_sym>,
    /// Stable-address user data that [`MrubyCopProxy::check`] writes to
    /// before stamping `mrb->ud` for each dispatch.
    user_data: Box<MrubyUserData>,
}

impl MrubyCopProxy {
    /// Test/programmatic constructor. Opens a fresh `MrubyState`,
    /// registers the arena native primitives, evaluates the
    /// `arena_prelude.rb` SDK base, evaluates `cop_source`, looks up
    /// `class_name` and `.new`s an instance.
    ///
    /// `kinds` must contain only kinds whose snake name is currently
    /// resolvable (murphy-9cr.24.1 scope: `Send` only — 24.2 widens the
    /// table). An unknown kind tag returns `Err`.
    pub fn for_test(
        name: &str,
        description: &str,
        cop_source: &str,
        class_name: &str,
        kinds: &[NodeKindTag],
    ) -> Result<Self, String> {
        let mut state = MrubyState::open();
        unsafe {
            register_arena_primitives(state.raw());
        }
        if eval_with_diag(&mut state, ARENA_PRELUDE, "arena prelude") {
            return Err("arena prelude raised during load".into());
        }
        if eval_with_diag(&mut state, cop_source, class_name) {
            return Err(format!("cop source raised during load: {class_name}"));
        }

        let mrb = state.raw();

        // SAFETY: `mrb` is valid + non-null; `class_name` is converted to
        // a NUL-terminated CString; `mrb_class_get` is the documented
        // class-lookup API. Returns null if absent.
        let cstr_class = CString::new(class_name)
            .map_err(|_| format!("class_name {class_name:?} contains an interior NUL"))?;
        let class_ptr: *mut RClass = unsafe { mrb_class_get(mrb, cstr_class.as_ptr()) };
        if class_ptr.is_null() {
            return Err(format!("class {class_name} not found in loaded cop source"));
        }

        // SAFETY: `class_ptr` is the just-resolved class; `mrb_obj_new`
        // creates a new instance. Zero-arg constructor.
        let instance: mrb_value = unsafe { mrb_obj_new(mrb, class_ptr, 0, std::ptr::null()) };

        // SAFETY: arena_prelude.rb defines `class Node` nested inside
        // `class Murphy ... end`. Walk the constant table: look up
        // `Murphy` at the top level, then `Node` inside it.
        // `mrb_class_get` raises NameError on a missing constant; we
        // first probe the load by clearing `(*mrb).exc` if either
        // lookup fails to keep the state usable for a follow-up error.
        let murphy_class: *mut RClass = unsafe { mrb_class_get(mrb, c"Murphy".as_ptr()) };
        if murphy_class.is_null() {
            return Err("Murphy class not found after arena prelude eval".into());
        }
        let node_class: *mut RClass =
            unsafe { mrb_class_get_under(mrb, murphy_class, c"Node".as_ptr()) };
        if node_class.is_null() {
            return Err("Murphy::Node not found after arena prelude eval".into());
        }

        let mut sym_table = Vec::with_capacity(kinds.len());
        for tag in kinds {
            let name = snake_kind_name(*tag)
                .ok_or_else(|| format!("unsupported NodeKindTag({}) in murphy-9cr.24.1", tag.0))?;
            let hook = format!("on_{}", name.to_string_lossy());
            let chook = CString::new(hook).expect("snake name + on_ has no interior NUL");
            // SAFETY: `mrb` valid; `chook` is a live NUL-terminated string.
            let sym: mrb_sym = unsafe { mrb_intern_cstr(mrb, chook.as_ptr()) };
            sym_table.push(sym);
        }

        Ok(Self {
            state,
            node_class,
            instance,
            name: name.as_bytes().to_vec(),
            description: description.as_bytes().to_vec(),
            default_severity: 0, // Warning, v1 host default
            default_enabled: 1,
            kinds: kinds.to_vec(),
            sym_table,
            user_data: Box::new(MrubyUserData {
                cx_raw: std::ptr::null(),
                current_cop_instance: instance,
            }),
        })
    }

    /// Per-node dispatch: invoke the cop's `on_<kind>(node)` hook for
    /// the kind of `node`. Returns the C-ABI dispatch result (0 = ok,
    /// non-zero = cop disabled for the rest of this file).
    ///
    /// # Safety
    ///
    /// `cx_raw` must point to a `CxRaw` valid for the call; its arena
    /// (`nodes`/`source`/…) must outlive the call.
    pub(crate) unsafe fn check(&mut self, node: NodeId, cx_raw: *const CxRaw) -> i32 {
        // Find the symbol for the node's kind.
        let tag = unsafe { node_kind_tag(cx_raw, node) };
        let Some(idx) = self.kinds.iter().position(|k| k.0 == tag.0) else {
            // The dispatcher promises only kinds we subscribed to; an
            // unknown tag here is a contract bug. Treat as a soft skip.
            return 0;
        };
        let sym = self.sym_table[idx];

        // Stamp `mrb->ud` so the arena primitives (`emit_offense`,
        // `node_range`, `node_kind`) can reconstitute the active CxRaw.
        self.user_data.cx_raw = cx_raw;
        self.user_data.current_cop_instance = self.instance;
        let mrb = self.state.raw();
        unsafe {
            (*mrb).ud = self.user_data.as_mut() as *mut MrubyUserData as *mut std::ffi::c_void;
        }

        // Build the node argument: Murphy::Node.new(node_id).
        let id_lit = CString::new(node.0.to_string()).expect("decimal digits, no NUL");
        // SAFETY: `mrb` valid; CString outlives this call; `mrb_load_string`
        // round-trips an integer literal exactly as primitives.rs does
        // (ADR 0002 finding 1: inline mrb_fixnum_value boxer absent from
        // bindgen).
        let id_arg: mrb_value = unsafe { mrb_load_string(mrb, id_lit.as_ptr()) };
        let node_args = [id_arg];
        // SAFETY: `node_class` is the live Murphy::Node class resolved
        // at proxy build; `mrb_obj_new` initializes it with 1 arg.
        let node_obj: mrb_value =
            unsafe { mrb_obj_new(mrb, self.node_class, 1, node_args.as_ptr()) };

        // Call `instance.send(sym, node_obj)`.
        let call_args = [node_obj];
        // SAFETY: `mrb` valid; `self.instance` is a live cop instance;
        // `sym` was interned in this same state at proxy build.
        let _ = unsafe { mrb_funcall_argv(mrb, self.instance, sym, 1, call_args.as_ptr()) };

        // A `raise` from inside the cop hook leaves `(*mrb).exc` set —
        // observe + clear so the next dispatch starts clean. v1 fault
        // isolation reports the raise as a non-zero return so the
        // dispatcher disables the cop for the rest of this file (parent
        // §4 fault isolation contract).
        // SAFETY: `mrb` valid; reading + clearing `exc` is the documented
        // pending-exception state (see MrubyState::eval_checked).
        let raised = unsafe {
            let r = !(*mrb).exc.is_null();
            if r {
                (*mrb).exc = std::ptr::null_mut();
            }
            r
        };

        // Detach `ud` so a stale CxRaw can't be read between dispatches.
        unsafe {
            (*mrb).ud = std::ptr::null_mut();
        }
        self.user_data.cx_raw = std::ptr::null();

        if raised { 1 } else { 0 }
    }

    /// `name` bytes (cop NAME), referenced by `RawSlice` in the built
    /// `PluginCopV1`. Heap-owned by the proxy.
    pub fn name_bytes(&self) -> &[u8] {
        &self.name
    }

    /// `description` bytes.
    pub fn description_bytes(&self) -> &[u8] {
        &self.description
    }

    /// Subscribed kinds. Referenced by `kinds_ptr` in the built
    /// `PluginCopV1`.
    pub fn kinds(&self) -> &[NodeKindTag] {
        &self.kinds
    }

    /// Severity wire byte.
    pub fn default_severity(&self) -> u8 {
        self.default_severity
    }

    /// Enablement byte.
    pub fn default_enabled(&self) -> u8 {
        self.default_enabled
    }
}

// ---------------------------------------------------------------------
// PluginCopV1 builder
// ---------------------------------------------------------------------

/// Build a `PluginCopV1` referencing memory owned by `proxy`.
///
/// The returned value is valid for as long as `proxy` (its `Box` inside
/// the thread-local map) is not moved. The `dispatch` field is the
/// shared [`mruby_dispatch_thunk`] which looks the proxy back up by
/// `cop_name` at call time.
pub fn build_mruby_cop(proxy: &MrubyCopProxy) -> PluginCopV1 {
    PluginCopV1 {
        size: std::mem::size_of::<PluginCopV1>(),
        name: RawSlice {
            ptr: proxy.name.as_ptr(),
            len: proxy.name.len(),
        },
        description: RawSlice {
            ptr: proxy.description.as_ptr(),
            len: proxy.description.len(),
        },
        default_severity: proxy.default_severity,
        default_enabled: proxy.default_enabled,
        safe: TRISTATE_UNSET,
        safe_autocorrect: TRISTATE_UNSET,
        options_ptr: std::ptr::null(),
        options_len: 0,
        kinds_ptr: proxy.kinds.as_ptr(),
        kinds_len: proxy.kinds.len(),
        dispatch: mruby_dispatch_thunk,
        // mruby proxies don't speak the `restrict_on_send` filter
        // surface (no `#[cop]` parse path). Leave empty so the host
        // applies no method allow-list — every Send reaches the proxy.
        send_methods_ptr: std::ptr::null(),
        send_methods_len: 0,
    }
}

// ---------------------------------------------------------------------
// Shared dispatch thunk
// ---------------------------------------------------------------------

/// The shared `extern "C"` dispatch thunk every mruby cop's
/// `PluginCopV1::dispatch` points to. Resolves the proxy from THIS
/// thread's [`CURRENT_MRUBY_PROXIES`] using `(*cx).cop_name` then
/// delegates to [`MrubyCopProxy::check`].
///
/// Returns non-zero on:
/// (a) missing proxy — the host either failed to populate the map on
///     this thread, populated it on a different thread (the thread
///     affinity contract is violated), or built a `PluginCopV1` whose
///     `cop_name` does not match any entry in the map. A stderr
///     diagnostic naming the cop + thread is emitted so the contract
///     break is observable; the dispatcher then disables this cop for
///     the rest of the file.
/// (b) cop hook `raise` — fault isolation per parent §4.
unsafe extern "C" fn mruby_dispatch_thunk(node: NodeId, cx: *const CxRaw) -> i32 {
    if cx.is_null() {
        return 1;
    }
    let cx_ref: &CxRaw = unsafe { &*cx };
    // SAFETY: `cop_name` is a RawSlice over arena-owned bytes that
    // outlive this call (`run_cops` builds it from a heap-owned PluginCopV1
    // value our caller created).
    let cop_name_bytes: Vec<u8> = unsafe { cx_ref.cop_name.as_bytes() }.to_vec();

    CURRENT_MRUBY_PROXIES.with(|cell| {
        let mut map = cell.borrow_mut();
        match map.get_mut(&cop_name_bytes) {
            Some(proxy) => unsafe { proxy.check(node, cx) },
            None => {
                let name = String::from_utf8_lossy(&cop_name_bytes);
                eprintln!(
                    "murphy: mruby cop '{name}' not found in this thread's proxy map \
                     (thread {:?}); was current_mruby_proxies_populate called on a \
                     different thread? Cop will be disabled for this file.",
                    std::thread::current().id()
                );
                1
            }
        }
    })
}

const _: DispatchFn = mruby_dispatch_thunk;

// ---------------------------------------------------------------------
// Arena primitives (Murphy.emit_offense, node_range, node_kind)
// ---------------------------------------------------------------------

/// Read the active `&CxRaw` from `mrb->ud`. Returns null if `ud` is
/// unset (which should never happen inside a dispatch).
unsafe fn cx_from_ud(mrb: *mut mrb_state) -> Option<*const CxRaw> {
    let ud = unsafe { (*mrb).ud as *const MrubyUserData };
    if ud.is_null() {
        return None;
    }
    let cx_raw = unsafe { (*ud).cx_raw };
    if cx_raw.is_null() {
        return None;
    }
    Some(cx_raw)
}

/// `Murphy.emit_offense(start, end, message, severity, edit_blob)` —
/// arena path. Decodes the edit blob and forwards (offense + edits) to
/// the host `FnTable` carried by the active `CxRaw`.
unsafe extern "C" fn arena_emit_offense(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    let mut start: mrb_int = -1;
    let mut end: mrb_int = -1;
    let mut msg_ptr: *const c_char = std::ptr::null();
    let mut msg_len: mrb_int = 0;
    let mut sev_ptr: *const c_char = std::ptr::null();
    let mut sev_len: mrb_int = 0;
    let mut blob_ptr: *const c_char = std::ptr::null();
    let mut blob_len: mrb_int = 0;
    let fmt = c"iisss";
    unsafe {
        mrb_get_args(
            mrb,
            fmt.as_ptr(),
            &mut start,
            &mut end,
            &mut msg_ptr,
            &mut msg_len,
            &mut sev_ptr,
            &mut sev_len,
            &mut blob_ptr,
            &mut blob_len,
        );
    }

    let cx = match unsafe { cx_from_ud(mrb) } {
        Some(p) => unsafe { &*p },
        None => return unsafe { mrb_load_string(mrb, c"nil".as_ptr()) },
    };

    // Bad ranges drop silently — a user cop must not crash the engine.
    if start < 0
        || end < 0
        || start > end
        || start > u32::MAX as mrb_int
        || end > u32::MAX as mrb_int
    {
        return unsafe { mrb_load_string(mrb, c"nil".as_ptr()) };
    }

    let message = unsafe { copy_string(msg_ptr, msg_len) };
    let severity_bytes = unsafe { copy_string(sev_ptr, sev_len) };
    let severity_wire: u8 = match severity_bytes.as_slice() {
        b"error" => 1,
        _ => 0, // warning + unknown → host default
    };

    // SAFETY: cx.fns is a FnTable pointer set by build_cx_raw with
    // 'static fn pointers; cx.sink is the host's OffenseSink.
    let fns: &FnTable = unsafe { &*cx.fns };

    #[allow(clippy::cast_possible_truncation)]
    let range = murphy_ast::Range {
        start: start as u32,
        end: end as u32,
    };

    let raw_offense = RawOffense {
        cop_name: cx.cop_name,
        message: RawSlice {
            ptr: message.as_ptr(),
            len: message.len(),
        },
        range,
        severity: severity_wire,
    };
    unsafe {
        (fns.emit_offense)(cx.sink, &raw_offense);
    }

    if !blob_ptr.is_null() && blob_len > 0 {
        let blob = unsafe { std::slice::from_raw_parts(blob_ptr as *const u8, blob_len as usize) };
        for (e_range, replacement) in decode_edit_blob_iter(blob) {
            let raw_edit = RawEdit {
                range: e_range,
                replacement: RawSlice {
                    ptr: replacement.as_ptr(),
                    len: replacement.len(),
                },
            };
            unsafe {
                (fns.emit_edit)(cx.sink, &raw_edit);
            }
        }
    }

    unsafe { mrb_load_string(mrb, c"nil".as_ptr()) }
}

/// `Murphy.node_range(id) -> [start, end]`.
unsafe extern "C" fn arena_node_range(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    let mut id_int: mrb_int = -1;
    let fmt = c"i";
    unsafe {
        mrb_get_args(mrb, fmt.as_ptr(), &mut id_int);
    }
    let cx = match unsafe { cx_from_ud(mrb) } {
        Some(p) => unsafe { &*p },
        None => return unsafe { mrb_load_string(mrb, c"nil".as_ptr()) },
    };
    if id_int < 0 || (id_int as usize) >= cx.nodes_len {
        return unsafe { mrb_load_string(mrb, c"nil".as_ptr()) };
    }
    let node_ptr = unsafe { cx.nodes.add(id_int as usize) };
    let range = unsafe { (*node_ptr).loc.expression };
    let lit = CString::new(format!("[{},{}]", range.start, range.end))
        .expect("decimal digits and brackets, no NUL");
    unsafe { mrb_load_string(mrb, lit.as_ptr()) }
}

/// `Murphy.node_kind(id) -> Symbol`. Minimal table for murphy-9cr.24.1
/// — 24.2 widens to all NodeKind variants.
unsafe extern "C" fn arena_node_kind(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    let mut id_int: mrb_int = -1;
    let fmt = c"i";
    unsafe {
        mrb_get_args(mrb, fmt.as_ptr(), &mut id_int);
    }
    let cx = match unsafe { cx_from_ud(mrb) } {
        Some(p) => unsafe { &*p },
        None => return unsafe { mrb_load_string(mrb, c"nil".as_ptr()) },
    };
    if id_int < 0 || (id_int as usize) >= cx.nodes_len {
        return unsafe { mrb_load_string(mrb, c"nil".as_ptr()) };
    }
    let node_ptr = unsafe { cx.nodes.add(id_int as usize) };
    let kind: &NodeKind = unsafe { &(*node_ptr).kind };
    let tag = NodeKindTag::of(kind);
    let name = snake_kind_name(tag).unwrap_or(c"unknown");
    let lit = CString::new(format!(":{}", name.to_string_lossy())).expect("snake name has no NUL");
    unsafe { mrb_load_string(mrb, lit.as_ptr()) }
}

/// Define the `Murphy` class (if not already defined) plus the arena
/// native primitives required by 24.1. Idempotent: a `Murphy` class
/// previously defined by the prism path's `primitives::register` is
/// reused.
pub(crate) unsafe fn register_arena_primitives(mrb: *mut mrb_state) {
    // SAFETY: `mrb` valid; `Object` is built-in. `mrb_define_class` is
    // idempotent: it returns the existing class if defined with the
    // same parent, else creates it.
    let object_class: *mut RClass = unsafe { mrb_class_get(mrb, c"Object".as_ptr()) };
    let murphy_class: *mut RClass =
        unsafe { mruby3_sys::mrb_define_class(mrb, c"Murphy".as_ptr(), object_class) };

    let aspec_req = |argc: u32| -> u32 {
        // `MRB_ARGS_REQ(n) == n << 18` per mruby's macro (ADR 0002 finding 1).
        (argc & 0x1f) << 18
    };

    unsafe {
        mrb_define_module_function(
            mrb,
            murphy_class,
            c"emit_offense".as_ptr(),
            Some(arena_emit_offense),
            aspec_req(5),
        );
        mrb_define_module_function(
            mrb,
            murphy_class,
            c"node_range".as_ptr(),
            Some(arena_node_range),
            aspec_req(1),
        );
        mrb_define_module_function(
            mrb,
            murphy_class,
            c"node_kind".as_ptr(),
            Some(arena_node_kind),
            aspec_req(1),
        );
    }
}

// ---------------------------------------------------------------------
// Helpers: kind tag → snake name, edit blob decode, string copy.
// ---------------------------------------------------------------------

/// Resolve `node`'s `NodeKindTag` from a raw `CxRaw` arena.
///
/// # Safety
///
/// `cx_raw` must be a valid pointer; the node id must be in bounds.
unsafe fn node_kind_tag(cx_raw: *const CxRaw, node: NodeId) -> NodeKindTag {
    let cx = unsafe { &*cx_raw };
    debug_assert!((node.0 as usize) < cx.nodes_len);
    let node_ptr: *const AstNode = unsafe { cx.nodes.add(node.0 as usize) };
    let kind: &NodeKind = unsafe { &(*node_ptr).kind };
    NodeKindTag::of(kind)
}

/// Snake-case name for the kinds murphy-9cr.24.1 can dispatch.
///
/// The full table (every NodeKind variant) is murphy-9cr.24.2's
/// responsibility; here we wire just enough for the on_send hook the
/// integration test exercises.
pub(crate) fn snake_kind_name(tag: NodeKindTag) -> Option<&'static CStr> {
    // `NodeKindTag::of` reads the `repr(C, u8)` discriminant byte; we
    // probe each kind we know about with a payload-free sentinel and
    // compare its tag against the requested one. Linear in table size;
    // table is tiny for this slice.
    let send_tag = NodeKindTag::of(&NodeKind::Send {
        receiver: OptNodeId::NONE,
        method: Symbol(0),
        args: NodeList { start: 0, len: 0 },
    });
    if tag.0 == send_tag.0 {
        return Some(c"send");
    }
    None
}

/// Iterator over the binary edit blob produced by
/// `Murphy::Fix#to_blob`. Yields `(Range, replacement)` pairs; invalid
/// records (bad range / non-UTF-8 replacement) are silently skipped
/// (PIN B: degrade-not-panic).
fn decode_edit_blob_iter(blob: &[u8]) -> Vec<(murphy_ast::Range, Vec<u8>)> {
    let mut edits = Vec::new();
    let mut cursor = blob;
    while let Some((start, rest1)) = read_decimal_i64(cursor) {
        let Some((end, rest2)) = read_decimal_i64(rest1) else {
            break;
        };
        let Some((replen, rest3)) = read_decimal_i64(rest2) else {
            break;
        };
        if replen < 0 {
            break;
        }
        let replen = replen as usize;
        if rest3.len() < replen {
            break;
        }
        let replacement_bytes = &rest3[..replen];
        let remaining = &rest3[replen..];
        if start < 0 || end < 0 || start > end || start > u32::MAX as i64 || end > u32::MAX as i64 {
            cursor = remaining;
            continue;
        }
        if std::str::from_utf8(replacement_bytes).is_err() {
            cursor = remaining;
            continue;
        }
        #[allow(clippy::cast_possible_truncation)]
        let range = murphy_ast::Range {
            start: start as u32,
            end: end as u32,
        };
        edits.push((range, replacement_bytes.to_vec()));
        cursor = remaining;
    }
    edits
}

fn read_decimal_i64(bytes: &[u8]) -> Option<(i64, &[u8])> {
    let space = bytes.iter().position(|&b| b == b' ')?;
    let digits = &bytes[..space];
    let rest = &bytes[space + 1..];
    if digits.is_empty() {
        return None;
    }
    let s = std::str::from_utf8(digits).ok()?;
    let val: i64 = s.parse().ok()?;
    Some((val, rest))
}

/// Copy a `(ptr, len)` mruby string view into an owned `Vec<u8>`.
///
/// # Safety
///
/// `ptr` must be valid for `len` bytes (the `mrb_get_args` `s` guarantee);
/// `len >= 0`.
unsafe fn copy_string(ptr: *const c_char, len: mrb_int) -> Vec<u8> {
    if ptr.is_null() || len <= 0 {
        return Vec::new();
    }
    let slice = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };
    slice.to_vec()
}

const ARENA_PRELUDE: &str = include_str!("arena_prelude.rb");

// `MrubyCopProxy` is `!Send + !Sync` by inference: it owns an
// `MrubyState` (which carries `*mut mrb_state` and is itself `!Send`),
// plus `*mut RClass` and `mrb_value` fields that hold thread-confined
// VM handles. We deliberately do NOT `unsafe impl Send` it; the
// thread-affinity contract documented at the top of this module rests
// on the type system rejecting cross-thread moves at compile time. A
// future contributor adding `unsafe impl Send for MrubyCopProxy` must
// first prove the contract is no longer needed.

/// Evaluate `script` and print the mruby exception to stderr if one
/// was raised. Returns `true` on raise (the caller maps that to an
/// `Err`). Provisional until murphy-9cr.24.9 wires a structured
/// `CopLoadError` — at which point the stderr print becomes a host
/// diagnostic and the bool collapses into the `Err` payload.
fn eval_with_diag(state: &mut MrubyState, script: &str, label: &str) -> bool {
    let cscript = CString::new(script).expect("script has no interior NUL");
    let mrb = state.raw();
    unsafe {
        mruby3_sys::mrb_load_string(mrb, cscript.as_ptr());
        let raised = !(*mrb).exc.is_null();
        if raised {
            eprintln!("murphy: mruby raised while loading {label}:");
            mruby3_sys::mrb_print_error(mrb);
            (*mrb).exc = std::ptr::null_mut();
        }
        raised
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_kind_name_resolves_send() {
        let send_tag = NodeKindTag::of(&NodeKind::Send {
            receiver: OptNodeId::NONE,
            method: Symbol(0),
            args: NodeList { start: 0, len: 0 },
        });
        assert_eq!(
            snake_kind_name(send_tag).map(|c| c.to_bytes()),
            Some(b"send".as_slice())
        );
    }

    #[test]
    fn snake_kind_name_unknown_returns_none() {
        // Tag 0 is `Error`, currently outside the 24.1 dispatch table.
        let error_tag = NodeKindTag(0);
        assert!(snake_kind_name(error_tag).is_none());
    }

    #[test]
    fn build_mruby_cop_populates_plugin_cop_v1_descriptor() {
        let send_tag = NodeKindTag::of(&NodeKind::Send {
            receiver: OptNodeId::NONE,
            method: Symbol(0),
            args: NodeList { start: 0, len: 0 },
        });
        let cop_source = r#"
            class BuildCopV1Probe < Murphy::Cop
              def on_send(node); end
            end
        "#;
        let proxy = MrubyCopProxy::for_test(
            "Murphy/BuildProbe",
            "Smokes build_mruby_cop wiring",
            cop_source,
            "BuildCopV1Probe",
            &[send_tag],
        )
        .expect("proxy build must succeed");

        let plugin_cop = build_mruby_cop(&proxy);
        assert_eq!(plugin_cop.size, std::mem::size_of::<PluginCopV1>());
        assert_eq!(plugin_cop.kinds_len, 1);
        assert_eq!(plugin_cop.name.len, b"Murphy/BuildProbe".len());
        unsafe {
            assert_eq!(plugin_cop.name.as_bytes(), b"Murphy/BuildProbe");
        }
        let kinds_slice =
            unsafe { std::slice::from_raw_parts(plugin_cop.kinds_ptr, plugin_cop.kinds_len) };
        assert_eq!(kinds_slice[0].0, send_tag.0);
    }

    #[test]
    fn arena_node_range_layout_pins_loc_expression() {
        // `arena_node_range` reads `(*node_ptr).loc.expression`. Reproduce
        // the same pointer dance from Rust so an AST layout change that
        // moves `expression` out of `loc`, renames it, or swaps it with
        // `loc.name` breaks here alongside the C primitive — flagging
        // drift in this file rather than as a silently wrong value
        // returned to user cops.
        use murphy_ast::{AstBuilder, NodeKind, Range};

        let mut b = AstBuilder::new("foo", "t.rb".to_string());
        let expr = Range { start: 7, end: 10 };
        let name = Range { start: 1, end: 4 };
        let id = b.push_named(NodeKind::Nil, expr, name);
        let ast = b.finish(id);
        let raw = ast.raw_parts();
        let node_ptr = raw.nodes.as_ptr();

        // The exact field walk `arena_node_range` performs.
        let range = unsafe { (*node_ptr.add(id.0 as usize)).loc.expression };
        assert_eq!(range, expr);

        // Pin separation: `loc.name` and `loc.expression` must stay
        // independent so a future field-order swap is detectable.
        let name_range = unsafe { (*node_ptr.add(id.0 as usize)).loc.name };
        assert_eq!(name_range, name);
        assert_ne!(range, name_range);
    }

    #[test]
    fn populate_and_drain_round_trip() {
        let send_tag = NodeKindTag::of(&NodeKind::Send {
            receiver: OptNodeId::NONE,
            method: Symbol(0),
            args: NodeList { start: 0, len: 0 },
        });
        let cop_source = r#"
            class RoundTripProbe < Murphy::Cop
              def on_send(node); end
            end
        "#;
        let proxy = MrubyCopProxy::for_test(
            "Murphy/RoundTrip",
            "",
            cop_source,
            "RoundTripProbe",
            &[send_tag],
        )
        .expect("proxy build");
        let mut map: HashMap<Vec<u8>, Box<MrubyCopProxy>> = HashMap::new();
        map.insert(b"Murphy/RoundTrip".to_vec(), Box::new(proxy));

        current_mruby_proxies_populate(map);
        let count = with_current_mruby_proxies(|m| m.len());
        assert_eq!(count, 1);

        let drained = current_mruby_proxies_drain();
        assert_eq!(drained.len(), 1);
        let empty = with_current_mruby_proxies(|m| m.len());
        assert_eq!(empty, 0);
        drop(drained);
    }
}
