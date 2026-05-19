//! `Murphy::Cop` mruby SDK base — Phase 3 Task 4.
//!
//! This is the THIN Ruby-glue layer ("fast core, scripted glue", design
//! §2/§4) that turns a user's `cops/*.rb` into offenses, on top of:
//!
//!   * Task 2 — [`crate::mruby::state`]: the `AstContext` carrier + the
//!     `MrubyState` RAII wrapper (open → set ud → eval → `mrb_close` on the
//!     normal path, before the AST drops).
//!   * Task 3 — [`crate::mruby::primitives`]: the read-only LIVE native IDL
//!     (`Murphy.node_count` / `node_name` / `node_receiver_nil?` /
//!     `node_msg_range` / `source_slice`). Reused, not reimplemented.
//!
//! What Task 4 adds:
//!
//!   * The embedded **`cop_prelude.rb`** (`include_str!` of the sibling
//!     `cop_prelude.rb`): `Murphy::Cop` base, the `Node` handle-wrapper, a
//!     `Murphy::Range` value object, the captured-only `Murphy::Fix` recorder,
//!     and `Cop#__run` (walk `0...node_count`, dispatch `on_call_node`).
//!   * The **`Murphy.__emit_offense`** native: a cop's `add_offense` crosses
//!     here; the host builds a Rust [`crate::Offense`] and pushes it into the
//!     **cop-run-owned** sink (NOT a `thread_local!` — see [`CopRun`]).
//!   * [`run_mruby_cop`]: load+run ONE mruby cop `.rb` over a parsed
//!     `AstContext`, returning `Vec<Offense>` — the same `Vec<Offense>` shape
//!     native cops produce.
//!
//! ## Scope fence (Phase 3 plan, Task 4)
//!
//!   * **Soft-(a) (Scope Fence 1):** the SDK provides `add_offense` AND a
//!     `fix` block, but in Phase 3 the fix is **captured-stored-only** — never
//!     applied, never serialized. The emitted [`crate::Offense`] is the
//!     ADR 0006 frozen shape; **no `autocorrect` field is ever added** to the
//!     struct or JSON. Phase 4 owns autocorrect application. The captured-fix
//!     count is kept in-memory on [`CopRun`] purely so cop authors write
//!     Phase-4-ready cops today; it is dropped when the run ends.
//!   * Deliberately NOT here: the per-cop OS thread + watchdog + deadline +
//!     abandon + Ruby-exception→error-offense (Task 5 — Task 4 loads+runs a
//!     cop synchronously, in-process, for its own tests); severity-precedence
//!     dedupe (Task 6); registry/pipeline/rayon wiring (Task 7 — the CLI does
//!     not run mruby cops yet, `sample_project` is unchanged); `[cops]` config.
//!
//! ## `unsafe_op_in_unsafe_fn`
//!
//! Per the Task-2 I-2 / Task-3 discipline there is **NO** module-wide
//! `#![allow(unsafe_op_in_unsafe_fn)]`. Every unsafe op inside the
//! `unsafe extern "C"` callback is its own `unsafe { }` + `// SAFETY:`.

use std::cell::UnsafeCell;
use std::sync::Arc;

use mruby3_sys::{
    RClass, mrb_class_get, mrb_define_module_function, mrb_get_args, mrb_int, mrb_state, mrb_value,
};

use crate::mruby::{AstContext, MrubyState};
use crate::{Offense, Range, Severity};

/// A single captured text-edit suggestion from a cop's `fix` block.
///
/// **Captured-stored-only (Scope Fence 1, soft-(a)).** This is internal,
/// in-memory state on [`CopRun`], dropped when the run ends. It is NEVER
/// applied to source and NEVER serialized into the [`Offense`] contract — the
/// `Offense` JSON stays the ADR 0006 frozen 5-field shape with no
/// `autocorrect` field. Phase 4 owns autocorrect application + the contract
/// extension. It exists so a cop author can write a Phase-4-ready `fix` block
/// today and have it recorded (not silently dropped at parse time).
#[derive(Debug, Clone)]
#[allow(dead_code)] // Phase 4 reads this; Phase 3 only proves it is recorded.
pub(crate) struct FixEdit {
    /// Byte offset the edit starts at (ADR 0001).
    pub(crate) start_offset: u32,
    /// Byte offset the edit ends at (ADR 0001).
    pub(crate) end_offset: u32,
    /// Replacement text (empty == deletion).
    pub(crate) replacement: String,
}

/// The **cop-run-owned** payload reachable from the native callbacks via
/// `mrb_state.ud` (ADR 0009 rule 2).
///
/// ## Why this is NOT a `thread_local!`
///
/// ADR 0009 rule 2 requires the offense sink to be a cop-instance-owned local
/// bound to the `mrb_state`/cop-run lifecycle. Task 5 runs cops on **reused
/// rayon OS threads**: a `thread_local!` sink would bleed offenses across
/// successive cop runs sharing a worker thread (and lose cop identity). This
/// `CopRun` is owned by the [`run_mruby_cop`] stack frame for exactly one cop
/// run; its raw `*const` is what `ud` carries, exactly where Task 2/3 used to
/// put `Arc::as_ptr(&AstContext)`. Reconstituting `&AstContext` (Task-3
/// `primitives::ctx`) now goes through `&(*p).ctx`.
///
/// ## ud carries this, ctx is `.ctx`
///
/// Task-3's `primitives` read `(*mrb).ud as *const CopRun` and project
/// `&(*p).ctx`. Liveness is the caller's contract (ADR 0009 rule 1, carried
/// from `AstContext`): the owning `CopRun` (and its `Arc<AstContext>` clone)
/// MUST outlive every `eval` and any in-flight native call. [`run_mruby_cop`]
/// guarantees this by owning the `CopRun` for the whole `eval` scope and
/// closing the `MrubyState` (`mrb_close`) BEFORE the `CopRun` drops.
///
/// ## Interior mutability
///
/// The native `__emit_offense` callback only holds `*const CopRun` (it is the
/// raw `ud`), so the sink/fixes use [`UnsafeCell`] to be pushed to without
/// forming `&mut` through a shared `&`. SAFETY: a `CopRun` is accessed by
/// exactly ONE thread for exactly one synchronous cop run; the only writer is
/// the synchronous `__emit_offense` callback, which cannot be re-entered
/// concurrently (mruby is single-threaded per state). Task 5 keeps this:
/// one `CopRun` per worker thread, still single-writer-per-`CopRun`.
pub(crate) struct CopRun {
    /// The shared parsed tree. Task-3 primitives reach this via `&(*p).ctx`.
    /// The worker owns this `Arc` clone (ADR 0009 rule 1) for the whole run.
    ctx: Arc<AstContext>,
    /// Fully-qualified cop name for [`Offense::cop_name`] (host-fixed per run;
    /// the `.rb` names the class, the host names the cop).
    cop_name: String,
    /// Path of the linted file, for [`Offense::file`].
    file: String,
    /// The cop-run-owned offense sink (ADR 0009 rule 2 — NOT a
    /// `thread_local!`). Drained back to the caller after the run.
    sink: UnsafeCell<Vec<Offense>>,
    /// Captured-only fix edits (soft-(a)). In-memory; dropped after the run;
    /// never applied/serialized. Phase 4 owns the contract extension.
    fixes: UnsafeCell<Vec<FixEdit>>,
}

impl CopRun {
    fn new(ctx: Arc<AstContext>, cop_name: &str, file: &str) -> Self {
        Self {
            ctx,
            cop_name: cop_name.to_owned(),
            file: file.to_owned(),
            sink: UnsafeCell::new(Vec::new()),
            fixes: UnsafeCell::new(Vec::new()),
        }
    }

    /// The shared parsed tree — Task-3's `primitives::ctx` projects this out
    /// of the `ud` payload. Read-only (ADR 0009 rule 3).
    pub(crate) fn ctx(&self) -> &Arc<AstContext> {
        &self.ctx
    }

    /// Construct a minimal `CopRun` for Task-3's `#[cfg(test)]` primitive
    /// tests, which only need the `ctx` projection (no offenses/fixes). Keeps
    /// those tests calling the same `ud`-payload contract Task 4 ships,
    /// instead of a fragile layout transmute.
    #[cfg(test)]
    pub(crate) fn for_test(ctx: Arc<AstContext>) -> Self {
        Self::new(ctx, "Test/Cop", "test.rb")
    }
}

// SAFETY: `CopRun` is `Send` so Task 5 can `move` exactly one `CopRun` into a
// per-cop worker thread (the `Arc<AstContext>` is `Send + Sync`; `String` is
// `Send`; `UnsafeCell<Vec<_>>` is `Send` when its contents are `Send`, which
// `Offense`/`FixEdit` are). It is deliberately NOT `Sync`: a `CopRun` is only
// ever touched by ONE thread for one synchronous cop run (the single-writer
// invariant the `UnsafeCell` SAFETY rests on) — mirroring `MrubyState`'s
// thread-confinement. `UnsafeCell` is already `!Sync`; we add no `unsafe impl
// Sync`, so the compiler enforces "never shared across threads".
unsafe impl Send for CopRun {}

/// `MRB_ARGS_REQ(n)` — absent from the bindgen output (ADR 0002 finding 1);
/// same reproduction Task 3 uses: `((mrb_aspec)((n)&0x1f) << 18)`.
const fn args_req(n: u32) -> u32 {
    (n & 0x1f) << 18
}

/// Reconstitute `&CopRun` from `mrb_state.ud`.
///
/// # Safety
///
/// The caller ([`run_mruby_cop`]) MUST have stored `&CopRun as *const _` in
/// `(*mrb).ud` and the owning `CopRun` MUST be alive for the whole native
/// call (ADR 0009 rule 1 — the `ud` raw pointer is not itself a liveness
/// guarantee). Only `&` is formed here; the interior `UnsafeCell`s document
/// their own single-writer SAFETY at the push site.
unsafe fn cop_run<'a>(mrb: *mut mrb_state) -> &'a CopRun {
    // SAFETY: `mrb` is a valid non-null `mrb_state` passed by mruby into the
    // native callback; reading `ud` is the documented native-callback context
    // mechanism.
    let ud = unsafe { (*mrb).ud } as *const CopRun;
    assert!(
        !ud.is_null(),
        "mrb_state.ud must hold the CopRun pointer (set via set_cop_run)"
    );
    // SAFETY: `ud` is `&CopRun as *const _` set by `run_mruby_cop`: a valid,
    // aligned, initialized `*const CopRun` whose owner outlives the whole
    // native call (ADR 0009 rule 1, caller contract). Only a shared `&` is
    // formed; interior mutation goes through the `UnsafeCell`s with their own
    // single-writer SAFETY justification.
    unsafe { &*ud }
}

/// `Murphy.__emit_offense(start, end, message, severity, fix_count)`.
///
/// A cop's `add_offense` (in `cop_prelude.rb`) crosses here. The host builds a
/// Rust [`crate::Offense`] (ADR 0006 frozen shape) into the cop-run-owned sink
/// and records the captured-fix count (soft-(a): stored-only, never
/// serialized). Returns `nil`.
///
/// Arg shape (mruby `mrb_get_args` `"iissi"`): two `i` byte offsets, an `s`
/// message (ptr+len, NUL-safe), an `s` severity name, and an `i` captured-fix
/// edit count. A bad/inverted range degrades to no offense (a user cop must
/// not be able to crash the engine).
unsafe extern "C" fn native_emit_offense(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    let mut start: mrb_int = -1;
    let mut end: mrb_int = -1;
    let mut msg_ptr: *const std::os::raw::c_char = std::ptr::null();
    let mut msg_len: mrb_int = 0;
    let mut sev_ptr: *const std::os::raw::c_char = std::ptr::null();
    let mut sev_len: mrb_int = 0;
    let mut fix_count: mrb_int = 0;

    let fmt = c"iissi";
    // SAFETY: `mrb` is a valid non-null `mrb_state`; `fmt` requests exactly
    // two `mrb_int`s, two (ptr,len) string pairs, and one `mrb_int`; every
    // out-pointer is a live, correctly-typed local that outlives the call.
    // `mrb_get_args` is the documented argument extractor.
    unsafe {
        mrb_get_args(
            mrb,
            fmt.as_ptr(),
            &mut start as *mut mrb_int,
            &mut end as *mut mrb_int,
            &mut msg_ptr as *mut *const std::os::raw::c_char,
            &mut msg_len as *mut mrb_int,
            &mut sev_ptr as *mut *const std::os::raw::c_char,
            &mut sev_len as *mut mrb_int,
            &mut fix_count as *mut mrb_int,
        );
    }

    // SAFETY: native callback; `mrb` valid & non-null; `ud` is the live
    // `CopRun` set by `run_mruby_cop`, alive for the whole call.
    let run = unsafe { cop_run(mrb) };

    // A user cop must not crash the engine: a negative/inverted range is
    // dropped (no offense emitted) rather than panicking.
    if start < 0 || end < 0 || start > end {
        // SAFETY: `mrb` valid & non-null; `c"nil"` is a trivial literal.
        return unsafe { eval_nil(mrb) };
    }

    // SAFETY: when `mrb_get_args` succeeds with `s`, mruby guarantees the
    // pointer is valid for `len` bytes for the duration of the callback. The
    // bytes are copied into an owned `String`; the borrow ends here.
    let message = unsafe { owned_string(msg_ptr, msg_len) };
    // SAFETY: same `s` guarantee for the severity (ptr, len) pair.
    let severity_name = unsafe { owned_string(sev_ptr, sev_len) };
    let severity = match severity_name.as_str() {
        "error" => Severity::Error,
        // Default + any unknown token → Warning (a user typo must not crash;
        // the documented surface is :warning / :error).
        _ => Severity::Warning,
    };

    let range = Range {
        start_offset: start as u32,
        end_offset: end as u32,
    };
    let offense = Offense::new(&run.file, &run.cop_name, range, severity, &message);

    // SAFETY (ADR 0009 rule 2 + the `CopRun` interior-mutability contract):
    // `run.sink` is only ever written here, from the single synchronous
    // `__emit_offense` callback of a single-threaded `mrb_state`; there is no
    // concurrent access (Task 5 keeps one `CopRun` per worker thread). We form
    // a single short-lived `&mut Vec<Offense>` from the `UnsafeCell`, push,
    // and drop it before returning — no aliasing `&`/`&mut` is live across the
    // boundary.
    unsafe {
        (*run.sink.get()).push(offense);
    }

    // Soft-(a): record the captured-fix edits as count-only sentinels. This is
    // internal, dropped after the run, NEVER serialized — proving the fix was
    // recorded without extending the ADR 0006 `Offense` contract. (The real
    // edit payload threads through in Phase 4; Phase 3 only needs "recorded".)
    if fix_count > 0 {
        // SAFETY: same single-writer-per-`CopRun` contract as `sink` above.
        let fixes = unsafe { &mut *run.fixes.get() };
        for _ in 0..fix_count {
            fixes.push(FixEdit {
                start_offset: range.start_offset,
                end_offset: range.end_offset,
                replacement: String::new(),
            });
        }
    }

    // SAFETY: `mrb` valid & non-null.
    unsafe { eval_nil(mrb) }
}

/// Copy a `(ptr, len)` mruby string view into an owned `String`
/// (lossy — a user cop's message is display text, not a contract key).
///
/// # Safety
///
/// `ptr` must be valid for `len` bytes (the `mrb_get_args` `s` guarantee for
/// the callback's duration); `len >= 0`.
unsafe fn owned_string(ptr: *const std::os::raw::c_char, len: mrb_int) -> String {
    if ptr.is_null() || len <= 0 {
        return String::new();
    }
    // SAFETY: caller guarantees `ptr` valid for `len` bytes; `len > 0` here.
    let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };
    String::from_utf8_lossy(bytes).into_owned()
}

/// Evaluate `nil` and return it. ADR 0002 finding 1: the inline `mrb_value`
/// boxers are absent from bindgen, so a trivial literal is round-tripped (same
/// pattern Task 3's `eval_literal` uses).
///
/// # Safety
///
/// `mrb` must be a valid non-null `mrb_state` inside a native callback.
unsafe fn eval_nil(mrb: *mut mrb_state) -> mrb_value {
    // SAFETY: `mrb` valid & non-null; `c"nil"` is a trivial constant literal;
    // `mrb_load_string` is the documented string-eval entry point.
    unsafe { mruby3_sys::mrb_load_string(mrb, c"nil".as_ptr()) }
}

/// Register the Task-4 SDK natives on the `Murphy` class.
///
/// MUST be called AFTER [`crate::mruby::primitives::register`] (which defines
/// the `Murphy` class + the read-only node primitives) and BEFORE the prelude
/// / cop `.rb` is evaluated. Only `Murphy.__emit_offense` is added here; the
/// rest of the SDK surface (`Murphy::Cop`, `Node`, `Range`, `Fix`) is the
/// Ruby [`PRELUDE`].
///
/// # Safety
///
/// `mrb` must be a valid non-null `mrb_state`; the `Murphy` class must already
/// exist (call `primitives::register` first). Defines a function only — reads
/// nothing.
unsafe fn register_sdk(mrb: *mut mrb_state) {
    // SAFETY: `mrb` valid & non-null; `Murphy` exists because the caller ran
    // `primitives::register` first; `c"Murphy"` is a static NUL-terminated
    // identifier; `mrb_class_get` is the documented class lookup.
    let murphy: *mut RClass = unsafe { mrb_class_get(mrb, c"Murphy".as_ptr()) };
    // SAFETY: `mrb` valid & non-null; `murphy` is the existing class; the name
    // is a static NUL-terminated id; the fn pointer matches the mruby
    // native-callback ABI; `args_req(5)` reproduces `MRB_ARGS_REQ` (ADR 0002
    // finding 1) for the `(start, end, message, severity, fix_count)` arity.
    unsafe {
        mrb_define_module_function(
            mrb,
            murphy,
            c"__emit_offense".as_ptr(),
            Some(native_emit_offense),
            args_req(5),
        );
    }
}

/// The embedded `Murphy::Cop` SDK prelude (the sibling `cop_prelude.rb`),
/// `include_str!`-baked into the binary so a cop author needs no toolchain —
/// they just drop a `.rb` into `cops/` (design §2/§4). Loaded into the
/// isolated `mrb_state` before the cop `.rb`.
const PRELUDE: &str = include_str!("cop_prelude.rb");

/// The host bootstrap eval'd after the prelude + cop `.rb`: run every cop the
/// `.rb` defined (each `Murphy::Cop` subclass registered itself via
/// `inherited`). One `.rb` is normally one cop; if it defines several they all
/// run (offenses merge — same as multiple native cops).
const BOOTSTRAP: &str = "Murphy::Cop.__registered.each { |k| k.new.__run }";

/// Load and run ONE mruby user cop `.rb` over a parsed `AstContext`, returning
/// the offenses it emitted as the SAME `Vec<Offense>` shape native cops
/// produce (ADR 0006).
///
/// `cop_name` is the fully-qualified name the host attributes the offenses to
/// (e.g. `Murphy/NoPuts`) — the `.rb` names its Ruby class; the host names the
/// cop. `file` is the linted file's path (for [`Offense::file`]).
///
/// Synchronous + in-process (Task 4 scope): NO per-cop OS thread, watchdog,
/// deadline, abandon, or Ruby-exception→error-offense — that hardening is
/// Task 5. Soft-(a): a cop's `fix` block is captured-stored-only on the
/// internal [`CopRun`] and dropped here; it is never applied or serialized,
/// and the returned `Offense`s are the ADR 0006 frozen shape (no
/// `autocorrect`).
///
/// Drop order (ADR 0009 rule 4 / Task-2 normal path): the `MrubyState` is
/// closed (`mrb_close`) at the end of the inner scope, BEFORE the `CopRun`
/// (and its `Arc<AstContext>` clone) drops — so no still-defined native /
/// GC finalizer can deref a freed tree.
pub fn run_mruby_cop(
    ctx: &Arc<AstContext>,
    cop_source: &str,
    cop_name: &str,
    file: &str,
) -> Vec<Offense> {
    // The worker owns its OWN `Arc<AstContext>` clone (ADR 0009 rule 1): the
    // `ud` raw pointer is not the liveness guarantee.
    let cop_run = CopRun::new(Arc::clone(ctx), cop_name, file);

    {
        let mut st = MrubyState::open();
        st.set_cop_run(&cop_run);
        // SAFETY: `st.raw()` is a valid non-null `mrb_state` living as long as
        // `st`; `ud` was set to `&cop_run` (alive for this whole scope, which
        // ends AFTER `st` drops). `primitives::register` defines the
        // read-only node IDL; `register_sdk` then defines `__emit_offense`
        // (it requires `Murphy` to already exist, hence the order). Both only
        // define functions — they read nothing.
        unsafe {
            crate::mruby::primitives::register(st.raw());
            register_sdk(st.raw());
        }
        st.eval(PRELUDE);
        st.eval(cop_source);
        st.eval(BOOTSTRAP);
        // `st` drops here → `mrb_close`, BEFORE `cop_run` (and its Arc clone)
        // drop below — the Task-2 normal-path ordering (ADR 0009 rule 4).
    }

    // Drain the cop-run-owned sink. `cop_run` is solely owned here and the
    // `mrb_state` is closed, so no native callback can be running: taking the
    // `Vec` out of the `UnsafeCell` is race-free.
    let offenses = cop_run.sink.into_inner();
    // `cop_run` (incl. the captured-only `fixes`) drops here — soft-(a): the
    // recorded fix is in-memory only and dropped, never applied/serialized.
    offenses
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The captured-fix is NEVER reflected in the `Offense` — a cop that
    /// writes a `fix` and one that does not emit byte-identical serialized
    /// offenses, and neither serialization contains an `autocorrect` field
    /// (ADR 0006 frozen shape; soft-(a)). This is the in-crate unit-level
    /// guard; `tests/cop_no_puts_mruby.rs` is the integration-level guard.
    #[test]
    fn fix_is_captured_only_and_offense_json_has_no_autocorrect() {
        let ctx = AstContext::new(b"puts 1\n".to_vec());

        const NOFIX: &str = r#"
class NoFix < Murphy::Cop
  def on_call_node(n)
    add_offense(n.message_loc, message: "m") if n.name == :puts && n.receiver_nil?
  end
end
"#;
        const WITHFIX: &str = r#"
class WithFix < Murphy::Cop
  def on_call_node(n)
    return unless n.name == :puts && n.receiver_nil?
    add_offense(n.message_loc, message: "m") { |f| f.replace(n.message_loc, "x") }
  end
end
"#;

        let a = run_mruby_cop(&ctx, NOFIX, "Murphy/T", "t.rb");
        let b = run_mruby_cop(&ctx, WITHFIX, "Murphy/T", "t.rb");
        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 1);

        let ja = serde_json::to_string(&a[0]).unwrap();
        let jb = serde_json::to_string(&b[0]).unwrap();
        assert_eq!(ja, jb, "fix captured-only → byte-identical offense JSON");
        assert!(
            !jb.contains("autocorrect"),
            "ADR 0006: no autocorrect in the serialized contract: {jb}"
        );
    }
}
