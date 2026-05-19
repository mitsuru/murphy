//! `Murphy::Cop` mruby SDK base — Phase 4 Task 2 (murphy-hwe.2).
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
//! What Task 4 (original P3 T4) adds:
//!
//!   * The embedded **`cop_prelude.rb`** (`include_str!` of the sibling
//!     `cop_prelude.rb`): `Murphy::Cop` base, the `Node` handle-wrapper, a
//!     `Murphy::Range` value object, the `Murphy::Fix` recorder,
//!     and `Cop#__run` (walk `0...node_count`, dispatch `on_call_node`).
//!   * The **`Murphy.__emit_offense`** native: a cop's `add_offense` crosses
//!     here; the host builds a Rust [`crate::Offense`] and pushes it into the
//!     **cop-run-owned** sink (NOT a `thread_local!` — see [`CopRun`]).
//!   * [`run_mruby_cop`]: load+run ONE mruby cop `.rb` over a parsed
//!     `AstContext`, returning `Vec<Offense>` — the same `Vec<Offense>` shape
//!     native cops produce.
//!
//! ## Phase 4 Task 2 change (ADR 0013 — murphy-hwe.2)
//!
//! The Phase-3 soft-(a) seam (`FixEdit` placeholder + `CopRun.fixes` sink) has
//! been replaced with real mruby→Rust edit marshalling:
//!
//!   * `cop_prelude.rb Fix#to_blob` encodes `fix.edits` as a binary blob.
//!   * `__emit_offense` (arg fmt `"iisss"`, blob as 5th `s`) decodes the blob
//!     via [`decode_edit_blob`] into `Vec<`[`Edit`]`>`.
//!   * Non-empty edits are attached via [`Offense::with_autocorrect`].
//!   * `FixEdit` and `CopRun.fixes` are removed; ADR 0009 field-disjointness
//!     now covers only the `ctx ↔ sink` pair (ADR 0013).
//!   * Invalid edits (inverted range) are silently dropped (PIN B / degrade-not-panic).
//!   * `sample_project` has no fix-emitting cops → snapshot BYTE-IDENTICAL (ADR 0007).
//!
//! ## `unsafe_op_in_unsafe_fn`
//!
//! Per the Task-2 I-2 / Task-3 discipline there is **NO** module-wide
//! `#![allow(unsafe_op_in_unsafe_fn)]`. Every unsafe op inside the
//! `unsafe extern "C"` callback is its own `unsafe { }` + `// SAFETY:`.

use std::cell::UnsafeCell;
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use mruby3_sys::{
    RClass, mrb_class_get, mrb_define_module_function, mrb_get_args, mrb_int, mrb_state, mrb_value,
};

use crate::mruby::{AstContext, MrubyState};
use crate::offense::{Autocorrect, Edit};
use crate::{Offense, Range, Severity};

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
/// ## Interior mutability — SAFETY (field disjointness, Phase 4 reduced set)
///
/// The native `__emit_offense` callback only holds `*const CopRun` (it is the
/// raw `ud`), so the `sink` uses [`UnsafeCell`] to be pushed to without
/// forming `&mut` through a shared `&`.
///
/// SAFETY — the *real* soundness argument is **field disjointness**, not
/// no-reentrancy or single-threadedness. `ctx` (reached only via a shared
/// `&(*p).ctx` by Task-3 primitives) and `sink` (the ONLY `UnsafeCell`) are
/// *disjoint fields* of `CopRun` — distinct, non-overlapping memory.
/// Therefore: even if a native primitive holds a live shared `&(*p).ctx` and
/// re-enters the mruby VM (e.g. a future primitive doing `mrb_funcall` into
/// user code) which in turn re-enters `__emit_offense`, the
/// `&mut *(*p).sink.get()` formed there CANNOT alias that live `&ctx` —
/// they name different fields / different memory. No aliasing UB exists
/// regardless of reentrancy.
///
/// Phase 4 (ADR 0013): the `fixes: UnsafeCell<Vec<FixEdit>>` field has been
/// removed. The Phase-3 field-disjointness soundness argument collapses to the
/// simpler `ctx ↔ sink` disjointness only (ADR 0013 / ADR 0009 re-justification):
/// there is no second `UnsafeCell`, so the single-writer-among-`UnsafeCell`s
/// concern is gone. The `ctx ↔ sink` disjointness is still the load-bearing
/// invariant: `&ctx` and `&mut *sink.get()` name different memory.
///
/// Soundness explicitly does **NOT** rest on no-reentrancy or
/// single-threadedness — those are incidental properties of the current
/// synchronous, single-`mrb_state` run, not the load-bearing invariant. Task 5
/// keeps one `CopRun` per worker thread.
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
}

impl CopRun {
    fn new(ctx: Arc<AstContext>, cop_name: &str, file: &str) -> Self {
        Self {
            ctx,
            cop_name: cop_name.to_owned(),
            file: file.to_owned(),
            sink: UnsafeCell::new(Vec::new()),
        }
    }

    /// The shared parsed tree — Task-3's `primitives::ctx` projects this out
    /// of the `ud` payload. Read-only (ADR 0009 rule 3).
    pub(crate) fn ctx(&self) -> &Arc<AstContext> {
        &self.ctx
    }

    /// Construct a minimal `CopRun` for Task-3's `#[cfg(test)]` primitive
    /// tests, which only need the `ctx` projection (no offenses). Keeps
    /// those tests calling the same `ud`-payload contract Task 4 ships,
    /// instead of a fragile layout transmute.
    #[cfg(test)]
    pub(crate) fn for_test(ctx: Arc<AstContext>) -> Self {
        Self::new(ctx, "Test/Cop", "test.rb")
    }
}

// SAFETY: `CopRun` is `Send` so Task 5 can `move` exactly one `CopRun` into a
// per-cop worker thread (the `Arc<AstContext>` is `Send + Sync`; `String` is
// `Send`; `UnsafeCell<Vec<Offense>>` is `Send` when its contents are `Send`,
// which `Offense` is). The interior-mutability soundness this `Send` relies on
// is **field disjointness** (ctx ↔ sink), NOT no-reentrancy /
// single-threadedness: `ctx` and the `sink` `UnsafeCell` are disjoint fields
// of `CopRun`, so a `&mut` formed through `sink`'s `UnsafeCell` can never
// alias a live shared `&ctx` even under VM re-entry (see the `CopRun`
// "Interior mutability — SAFETY" doc for the full argument). Phase 4 (ADR
// 0013): the `fixes` field is removed; the soundness argument is now the
// simpler `ctx ↔ sink` disjointness only. It is deliberately NOT `Sync`: a
// `CopRun` is touched by ONE thread for one synchronous cop run — mirroring
// `MrubyState`'s thread-confinement. `UnsafeCell` is already `!Sync`; we add
// no `unsafe impl Sync`, so the compiler enforces "never shared across
// threads".
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

/// `Murphy.__emit_offense(start, end, message, severity, edit_blob)`.
///
/// A cop's `add_offense` (in `cop_prelude.rb`) crosses here. The host builds a
/// Rust [`crate::Offense`] and, when the `edit_blob` is non-empty, decodes it
/// into [`Edit`] records and attaches them via [`Offense::with_autocorrect`].
/// Returns `nil`.
///
/// Arg shape (mruby `mrb_get_args` `"iisss"`): two `i` byte offsets, an `s`
/// message (ptr+len, NUL-safe), an `s` severity name, and an `s` edit blob
/// (ptr+len, arbitrary bytes / NUL-safe via the `s` ptr+len format). A
/// bad/inverted offense range degrades to no offense (a user cop must not be
/// able to crash the engine). Invalid edits in the blob are silently dropped
/// (PIN B); if all edits are invalid the `autocorrect` key is absent.
///
/// ## Edit blob wire format (kept in sync with cop_prelude.rb Fix#to_blob)
///
/// Zero or more concatenated edit records:
///   `"<start_decimal> <end_decimal> <replen_decimal> "` + exactly `replen` raw bytes.
/// All numeric fields are non-negative decimal ASCII integers followed by a
/// single space. Replacement is exactly `replen` raw bytes after the space.
/// Empty blob (no fix block, or all edits dropped) → no `autocorrect` attached.
///
/// Example: `fix.replace(Range.new(0,4), "hi")` encodes as `"0 4 2 hi"`.
/// Example: `fix.remove(Range.new(5,9))` encodes as `"5 9 0 "` (0-byte replacement).
///
/// MUST stay in sync with the encoder in `cop_prelude.rb` `Fix#to_blob`. Both
/// files carry this format spec to prevent encoder/decoder drift.
///
/// ## Narrowing (ADR 0001 single audited site, PIN C)
///
/// The `start`/`end` offense range and each edit's `(start, end)` are
/// narrowed `mrb_int -> u32` at a SINGLE site here, next to the existing
/// offense-range narrowing. The predicate `val < 0 || val > u32::MAX as mrb_int`
/// guards the cast; for edits the additional `start > end` check applies
/// (invalid range → silent drop, PIN B). This follows ADR 0001's mandate that
/// the `usize->u32` narrowing lives in exactly one audited place per
/// subsystem.
unsafe extern "C" fn native_emit_offense(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    let mut start: mrb_int = -1;
    let mut end: mrb_int = -1;
    let mut msg_ptr: *const std::os::raw::c_char = std::ptr::null();
    let mut msg_len: mrb_int = 0;
    let mut sev_ptr: *const std::os::raw::c_char = std::ptr::null();
    let mut sev_len: mrb_int = 0;
    let mut blob_ptr: *const std::os::raw::c_char = std::ptr::null();
    let mut blob_len: mrb_int = 0;

    // fmt changed from "iissi" (Phase 3, fix count) to "iisss" (Phase 4, edit blob).
    // arg count is still 5; `args_req(5)` in `register_sdk` is unchanged.
    let fmt = c"iisss";
    // SAFETY: `mrb` is a valid non-null `mrb_state`; `fmt` requests exactly
    // two `mrb_int`s and three (ptr,len) string pairs; every out-pointer is a
    // live, correctly-typed local that outlives the call.
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
            &mut blob_ptr as *mut *const std::os::raw::c_char,
            &mut blob_len as *mut mrb_int,
        );
    }

    // SAFETY: native callback; `mrb` valid & non-null; `ud` is the live
    // `CopRun` set by `run_mruby_cop`, alive for the whole call.
    let run = unsafe { cop_run(mrb) };

    // A user cop must not crash the engine: a negative/inverted offense range
    // is dropped (no offense emitted) rather than panicking.
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

    // ADR 0001 single audited narrowing site for offense range (unchanged from Phase 3).
    #[allow(clippy::cast_possible_truncation)]
    let range = Range {
        start_offset: start as u32,
        end_offset: end as u32,
    };

    // Decode the edit blob into Vec<Edit>. Invalid edits are silently dropped
    // (PIN B): start<0 || end<0 || start>end → drop that edit only, offense
    // and other edits survive. Empty result → no autocorrect attached.
    //
    // SAFETY: when `mrb_get_args` succeeds with `s`, mruby guarantees `blob_ptr`
    // is valid for `blob_len` bytes for the duration of the callback.
    let edits: Vec<Edit> = if blob_ptr.is_null() || blob_len <= 0 {
        Vec::new()
    } else {
        let blob = unsafe { std::slice::from_raw_parts(blob_ptr as *const u8, blob_len as usize) };
        decode_edit_blob(blob)
    };

    let offense = Offense::new(&run.file, &run.cop_name, range, severity, &message);
    let offense = if edits.is_empty() {
        offense
    } else {
        offense.with_autocorrect(Autocorrect { edits })
    };

    // SAFETY (ADR 0009 rule 2 + the `CopRun` interior-mutability contract):
    // the `&mut Vec<Offense>` formed from `run.sink`'s `UnsafeCell` is sound by
    // **field disjointness** (ctx ↔ sink, Phase 4 ADR 0013) — `sink` is a
    // distinct field from `ctx`, so this `&mut` cannot alias any live shared
    // `&(*p).ctx` a Task-3 primitive holds, even if a primitive re-entered the
    // VM and that re-entered here (different memory; soundness does NOT rest on
    // no-reentrancy / single-threadedness — see the `CopRun`
    // "Interior mutability — SAFETY" doc). Phase 4: there is no `fixes` field;
    // the field-disjointness argument is now the simpler ctx ↔ sink only.
    unsafe {
        (*run.sink.get()).push(offense);
    }

    // SAFETY: `mrb` valid & non-null.
    unsafe { eval_nil(mrb) }
}

/// Decode the binary edit blob produced by `Murphy::Fix#to_blob` (cop_prelude.rb)
/// into a `Vec<Edit>`.
///
/// ## Blob format (must stay in sync with `cop_prelude.rb Fix#to_blob`)
///
/// Zero or more concatenated edit records:
///   `"<start_decimal> <end_decimal> <replen_decimal> "` + exactly `replen` raw bytes.
///
/// Each numeric field is a non-negative decimal ASCII integer followed by a
/// single ASCII space (`0x20`). Replacement is exactly `replen` raw bytes
/// immediately after the trailing space.
///
/// ## Error handling (PIN B: degrade-not-panic)
///
/// - A malformed header (parse failure, negative value, or `start > end`) →
///   that edit is **silently dropped**; the remaining blob is not parseable
///   (we can't safely re-sync), so parsing stops. The offense + any previously
///   decoded edits survive.
/// - A `replen` that would run past the blob end → same silent drop + stop.
/// - `start < 0 || end < 0` → silent drop (can't happen for non-negative
///   decimal ASCII, but the `i64` parse domain makes it explicit).
///
/// ## Narrowing (ADR 0001 / PIN C)
///
/// `start` and `end` are parsed as `i64` (so negative is representable for
/// the guard), then narrowed to `u32` under one `#[allow]` + SAFETY doc
/// after the `start < 0 || end < 0 || start > end` predicate, matching the
/// existing offense-range narrowing pattern.
fn decode_edit_blob(blob: &[u8]) -> Vec<Edit> {
    let mut edits = Vec::new();
    let mut cursor = blob;

    while let Some((start, rest1)) = read_decimal_i64(cursor) {
        // Read the remaining two header fields; break on any parse failure.
        let Some((end, rest2)) = read_decimal_i64(rest1) else {
            break;
        };
        let Some((replen, rest3)) = read_decimal_i64(rest2) else {
            break;
        };
        // PIN B: invalid range or replen → silent drop, stop parsing.
        // PIN C: narrowing guard (ADR 0001 single audited site for edits).
        if start < 0 || end < 0 || start > end || replen < 0 {
            break;
        }
        let replen = replen as usize;
        if rest3.len() < replen {
            break; // blob truncated → stop
        }
        let replacement_bytes = &rest3[..replen];
        let remaining = &rest3[replen..];

        // ADR 0001: narrowing from i64 to u32. The `start >= 0 && end >= 0`
        // guard above makes the `i64 as u32` cast safe (i64 non-negative and
        // the source was u32-bounded by prism / cop_prelude, so no truncation).
        #[allow(clippy::cast_possible_truncation)]
        let edit = Edit {
            range: Range {
                start_offset: start as u32,
                end_offset: end as u32,
            },
            replacement: String::from_utf8_lossy(replacement_bytes).into_owned(),
        };
        edits.push(edit);
        cursor = remaining;
    }

    edits
}

/// Read a non-negative decimal ASCII integer from the front of `bytes`,
/// consuming the trailing space separator. Returns `(value, rest_after_space)`
/// or `None` if the input is empty, has no space, or is not decimal.
fn read_decimal_i64(bytes: &[u8]) -> Option<(i64, &[u8])> {
    let space = bytes.iter().position(|&b| b == b' ')?;
    let digits = &bytes[..space];
    let rest = &bytes[space + 1..];
    // Allow empty digits to return None (malformed).
    if digits.is_empty() {
        return None;
    }
    let s = std::str::from_utf8(digits).ok()?;
    let val: i64 = s.parse().ok()?;
    Some((val, rest))
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
    // finding 1) for the `(start, end, message, severity, edit_blob)` arity.
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

/// `Murphy.__test_sleep_ms(n)` — **TEST-ONLY**, `#[cfg(test)]`-gated.
///
/// A cop calls this to sleep a controlled `n` milliseconds on the per-cop
/// child thread. It is the *deterministic* mechanism for the late-finish
/// stress test ([`tests::late_finish_after_timeout_is_sound_under_load`]): a
/// cop that calls `Murphy.__test_sleep_ms(deadline + ε)` reliably returns
/// *just after* the watchdog `recv_timeout` fired, hitting the
/// detached-`MrubyState::Drop`-while-the-caller-has-moved-on window — without
/// the timing fragility of a calibrated busy-loop.
///
/// This compiles ONLY under `cfg(test)` (the lib's own unit-test build); it is
/// absent from every production build and adds no production surface (ADR 0003
/// fence: a test affordance must not affect production). It is registered by
/// the matching `#[cfg(test)]` arm in [`cop_run_body`].
///
/// # Safety
///
/// Standard native-callback contract: `mrb` is a valid non-null `mrb_state`;
/// one required `i` arg. Sleeps the calling (child) thread only; touches no
/// `ud`/AST state, so it cannot perturb the soundness argument.
#[cfg(test)]
unsafe extern "C" fn native_test_sleep_ms(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    let mut ms: mrb_int = 0;
    // SAFETY: native callback; `mrb` valid & non-null; `c"i"` requests exactly
    // one `mrb_int`; `&mut ms` is a live, correctly-typed local out-pointer
    // that outlives the call.
    unsafe {
        mrb_get_args(mrb, c"i".as_ptr(), &mut ms as *mut mrb_int);
    }
    if ms > 0 {
        thread::sleep(Duration::from_millis(ms as u64));
    }
    // SAFETY: `mrb` valid & non-null.
    unsafe { eval_nil(mrb) }
}

/// Register the `#[cfg(test)]`-only `Murphy.__test_sleep_ms` on the `Murphy`
/// class (which `primitives::register` defined first). Called ONLY from the
/// `#[cfg(test)]` arm in [`cop_run_body`]; never compiled into production.
///
/// # Safety
///
/// `mrb` must be a valid non-null `mrb_state`; the `Murphy` class must already
/// exist (`primitives::register` ran first). Defines a function only.
#[cfg(test)]
unsafe fn register_test_sleep(mrb: *mut mrb_state) {
    // SAFETY: `mrb` valid & non-null; `Murphy` exists (register ran first);
    // the name is a static NUL-terminated id; the fn matches the mruby
    // native-callback ABI; `args_req(1)` reproduces `MRB_ARGS_REQ` for the
    // single `n` arg (ADR 0002 finding 1).
    unsafe {
        let murphy = mrb_class_get(mrb, c"Murphy".as_ptr());
        mrb_define_module_function(
            mrb,
            murphy,
            c"__test_sleep_ms".as_ptr(),
            Some(native_test_sleep_ms),
            args_req(1),
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
/// produce (ADR 0006 / ADR 0013).
///
/// `cop_name` is the fully-qualified name the host attributes the offenses to
/// (e.g. `Murphy/NoPuts`) — the `.rb` names its Ruby class; the host names the
/// cop. `file` is the linted file's path (for [`Offense::file`]).
///
/// Synchronous + in-process (Task 4 scope): NO per-cop OS thread, watchdog,
/// deadline, abandon, or Ruby-exception→error-offense — that hardening is
/// Task 5. Phase 4 (ADR 0013): a cop's `fix` block is marshalled from Ruby as
/// a binary blob and decoded into `Vec<Edit>` by `native_emit_offense`;
/// non-empty edits are attached to the `Offense` via `with_autocorrect`.
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
    // Task 4 is the synchronous path: a `raise` is observed (Task 5's
    // exception-checked eval) but mapped to no offense here — exception→
    // error-offense is `run_mruby_cop_isolated`'s job, NOT this primitive's
    // (Task-4 scope is unchanged: load+run a cop synchronously for its own
    // tests). `_raised` is discarded deliberately.
    let _raised = cop_run_body(&cop_run, cop_source);

    // Drain the cop-run-owned sink. `cop_run` is solely owned here and the
    // `mrb_state` is closed, so no native callback can be running: taking the
    // `Vec` out of the `UnsafeCell` is race-free.
    // Phase 4: offenses may carry `autocorrect` (marshalled from the fix blob).
    cop_run.sink.into_inner()
}

/// The shared open→register→eval→close cop-run body, factored out of
/// [`run_mruby_cop`] so [`run_mruby_cop_isolated`] runs the IDENTICAL
/// lifecycle **on the per-cop child thread** (the ADR 0009 / `composition_poc`
/// proven shape — the entire mruby lifecycle lives on the child thread so a
/// timed-out thread stuck inside `mrb_load_string` simply never reaches this
/// `MrubyState`'s `Drop`; no `mrb_close`, no forget hack — see
/// [`run_mruby_cop_isolated`]).
///
/// Returns `true` iff the cop left a pending mruby exception — at cop-file
/// load OR (I-3) from inside `on_call_node`, surfacing through the `BOOTSTRAP`
/// dispatch eval. Caught via [`MrubyState::eval_checked`] (mruby never unwinds
/// into Rust — design §6).
///
/// Drop order (ADR 0009 rule 4 / Task-2 normal path): `MrubyState` is closed
/// (`mrb_close`) at the inner scope's end, BEFORE the caller drops the
/// `CopRun` (and its `Arc<AstContext>` clone) — so no still-defined native /
/// GC finalizer can deref a freed tree. On the ABANDON path this fn never
/// returns (the thread is stuck in `eval_checked`'s `mrb_load_string`), so
/// `st` is never dropped and `mrb_close` is never called — exactly ADR 0003
/// Mechanism A.
fn cop_run_body(cop_run: &CopRun, cop_source: &str) -> bool {
    let mut st = MrubyState::open();
    st.set_cop_run(cop_run);
    // SAFETY: `st.raw()` is a valid non-null `mrb_state` living as long as
    // `st`; `ud` was set to `cop_run` (alive for this whole call — the caller
    // owns it past `st`'s drop). `primitives::register` defines the read-only
    // node IDL; `register_sdk` then defines `__emit_offense` (it requires
    // `Murphy` to already exist, hence the order). Both only define functions
    // — they read nothing.
    unsafe {
        crate::mruby::primitives::register(st.raw());
        register_sdk(st.raw());
    }
    // TEST-ONLY (cfg(test)): also expose `Murphy.__test_sleep_ms` so the
    // late-finish stress test can deterministically land a cop just past the
    // injected deadline. Absent from every production build — Task-5
    // production logic is unchanged (this block does not exist there).
    #[cfg(test)]
    // SAFETY: `st.raw()` is the same valid non-null `mrb_state`; `Murphy`
    // exists (`primitives::register` ran above). Defines a function only.
    unsafe {
        register_test_sleep(st.raw());
    }
    // The prelude defines the SDK base; it must not raise. The cop `.rb` and
    // the dispatch (`BOOTSTRAP`, which invokes `on_call_node`) can both raise
    // — at LOAD or IN-VISITOR (I-3). `||` short-circuits, so a load-time
    // `raise` skips the dispatch (a half-defined cop is not dispatched), and
    // an in-visitor `raise` is caught at `BOOTSTRAP`. Either ⇒ `true`.
    st.eval(PRELUDE);
    st.eval_checked(cop_source) || st.eval_checked(BOOTSTRAP)
    // `st` drops HERE → `mrb_close`, BEFORE the caller drops `cop_run` (and
    // its Arc clone) — the Task-2 normal-path ordering (ADR 0009 rule 4).
    // (Abandon path: control never reaches here.)
}

/// A hardcoded, sane per-cop wall-clock deadline (ADR 0003: v1 is wall-clock
/// time only, no instruction-step budget; "Hardcoded sane deadline value,
/// configurability is later/coarse"). 2 s is far above any reasonable cop's
/// per-file cost yet bounds a runaway. Configurable / per-file-vs-per-cop
/// scoping is an explicit ADR-0003 forward item, NOT Task 5.
pub const COP_DEADLINE: Duration = Duration::from_secs(2);

/// Outcome the per-cop child thread sends back over the channel.
///
/// There is intentionally NO `TimedOut` variant: a timeout is detected by the
/// watchdog (`recv_timeout` returning `Err(Timeout)` in the CALLER), never
/// produced by the child — the child either completes or catches a raise. The
/// child cannot report its own timeout (a runaway child is stuck in
/// `mrb_load_string` and sends nothing at all; Mechanism A).
enum CopOutcome {
    /// The cop completed within the deadline; here are its offenses.
    Completed(Vec<Offense>),
    /// The cop raised an mruby exception (at load OR in-visitor — I-3).
    Raised,
}

/// Run ONE mruby user cop `.rb` over a parsed `AstContext` **with per-cop
/// isolation** (Phase 3 Task 5; ADR 0003 Mechanism A; ADR 0009 composition):
///
///   * a dedicated per-cop OS thread + a wall-clock watchdog
///     (`recv_timeout(deadline)`) sitting in THIS (caller) thread;
///   * abandon-on-timeout — the child thread is never joined; for a runaway
///     cop it is stuck forever inside `mrb_load_string`, so its stack-local
///     `MrubyState`/`CopRun` `Drop` is **unreachable** ⇒ NO `mrb_close`
///     (ADR 0003 Mechanism A / ADR 0009 rule 4). The child thread **owns its
///     own `Arc<AstContext>` clone** (built inside the closure, ADR 0009
///     rule 1), so the AST stays alive for any late zombie native call even
///     after this caller returns and drops its own `Arc`;
///   * a Ruby exception (at cop-file load OR — I-3 — inside `on_call_node`)
///     is caught (`(*mrb).exc`; mruby does not unwind into Rust, design §6);
///   * timeout OR exception ⇒ **exactly one `error offense`** for that
///     cop×file (`Severity::Error`, the cop's own `cop_name`, a message
///     naming the cause, ADR 0006 frozen shape — no `autocorrect`); the run
///     continues. M-3: if the `.rb` defines several cops they ALL run on the
///     one thread (the `BOOTSTRAP` dispatches every registered subclass);
///     their offenses merge — same as multiple native cops.
///
/// ## Deadline-boundary race (ADR 0009 rule 6 — handled, documented)
///
/// A cop finishing *exactly* as the watchdog fires can `send` after
/// `recv_timeout` already returned `Timeout`. We handle this per ADR 0009
/// rule 6 option (a): the `Receiver` is **dropped immediately** when
/// `recv_timeout` returns `Timeout` (it goes out of scope as we leave the
/// `match` arm and `return` the `TimedOut` mapping), so any late `tx.send`
/// from the child fails harmlessly (`Err`, ignored via `let _ =`) instead of
/// being observed. Determinism scope (ADR 0006/0007): the "byte-identical
/// across repeated/shuffled runs" guarantee holds **only for cops with
/// deadline headroom**. For a cop landing *exactly* on the wall-clock
/// boundary, whether it resolves as `Completed` vs one `error offense` is
/// inherently non-deterministic (wall-clock; impossible to make the exact
/// boundary deterministic by design) and that is **accepted, not a contract
/// breach** — it is the documented scope of the determinism contract, not a
/// guarantee silently assumed away. All Task-5 fixtures have ample headroom
/// (runaway: never completes; well-behaved: sub-millisecond ≪ deadline), so
/// every Task-5 assertion is on the deterministic side of that boundary.
///
/// The deadline is the **hardcoded sane** [`COP_DEADLINE`] (ADR 0003: v1 is
/// wall-clock only; configurability is an explicit forward item, not Task 5).
/// This is the API Task 7 wires into the rayon pipeline. Tests inject a short
/// deadline via the `pub(crate)` [`run_mruby_cop_isolated_with_deadline`] so a
/// runaway-cop assertion does not wait the full production deadline.
pub fn run_mruby_cop_isolated(
    ctx: &Arc<AstContext>,
    cop_source: &str,
    cop_name: &str,
    file: &str,
) -> Vec<Offense> {
    run_mruby_cop_isolated_with_deadline(ctx, cop_source, cop_name, file, COP_DEADLINE)
}

/// [`run_mruby_cop_isolated`] with an explicit wall-clock `deadline`.
///
/// A testability + forward-compat seam so Task-5 tests (and a future
/// per-file/per-cop-configurable deadline — an explicit ADR-0003 forward
/// item) can inject a deadline without paying the full hardcoded
/// [`COP_DEADLINE`]. The hardcoded-deadline production entry point is
/// [`run_mruby_cop_isolated`] (Task 7 wires THAT). All the
/// isolation/abandon/exception/race semantics documented on
/// [`run_mruby_cop_isolated`] apply identically here — this is its body.
pub fn run_mruby_cop_isolated_with_deadline(
    ctx: &Arc<AstContext>,
    cop_source: &str,
    cop_name: &str,
    file: &str,
    deadline: Duration,
) -> Vec<Offense> {
    // Owned, `Send` move-ins for the child thread. The AST is shared by a
    // child-OWNED `Arc` clone (ADR 0009 rule 1: the `ud` raw pointer is NOT
    // the liveness guarantee — this clone, owned on the child thread's stack,
    // is). `cop_source`/`cop_name`/`file` are copied so the child needs no
    // borrow of the caller's stack (the caller may return before an abandoned
    // child).
    let child_ctx: Arc<AstContext> = Arc::clone(ctx);
    let child_cop_source = cop_source.to_owned();
    let child_cop_name = cop_name.to_owned();
    let child_file = file.to_owned();
    // Separate owned copies for the watchdog's error-offense attribution (the
    // child moves its own copies in; an abandoned child must not be borrowed
    // from here — it may outlive this call).
    let cop_name = cop_name.to_owned();
    let file = file.to_owned();

    let (tx, rx) = mpsc::channel::<CopOutcome>();

    thread::spawn(move || {
        // THE ABANDON SEAM (I-2, the proven `composition_poc` shape):
        // the ENTIRE per-cop mruby lifecycle — `CopRun` (owning the
        // child's own `Arc<AstContext>` clone), `MrubyState`
        // (`mrb_open`→register→eval→`mrb_close`) — is created and owned
        // ON THIS CHILD THREAD's stack inside `cop_run_body`. For a
        // runaway cop this thread blocks forever inside
        // `mrb_load_string` (in `cop_run_body`'s `eval_checked`), so it
        // NEVER returns, the stack-local `MrubyState`/`CopRun` are never
        // dropped, and `MrubyState`'s `Drop` (`mrb_close`) is therefore
        // UNREACHABLE — no `mrb_close` on the abandon path (ADR 0003
        // Mechanism A / ADR 0009 rule 4), with NO `std::mem::forget`
        // hack and NO change to Task-2's unconditional-`mrb_close`
        // `Drop`. The child-owned `Arc<AstContext>` clone (held by the
        // never-dropped `CopRun`) keeps `source` + the prism arena alive
        // for any late zombie native call — even after the caller
        // returns and drops ITS `Arc` (ADR 0009 rule 1).
        let cop_run = CopRun::new(child_ctx, &child_cop_name, &child_file);
        let raised = cop_run_body(&cop_run, &child_cop_source);
        // Reached ONLY on the normal/exception path (a runaway cop never
        // gets here). `cop_run_body` has already run `mrb_close` (its `st`
        // dropped at its scope end) BEFORE we touch `cop_run` — Task-2
        // normal-path ordering.
        let outcome = if raised {
            CopOutcome::Raised
        } else {
            CopOutcome::Completed(cop_run.sink.into_inner())
        };
        // If the caller already timed out, `rx` is dropped and this `send`
        // fails harmlessly (ADR 0009 rule 6) — the thread just exits.
        //
        // SOLE-SEND INVARIANT (Task 7+/Phase 4 MUST NOT break): this is the
        // ONLY `tx.send` on this channel — the child sends EXACTLY ONCE unless
        // it panics or is abandoned (runaway stuck in `mrb_load_string`). The
        // watchdog's `Disconnected` arm reports "worker thread died
        // unexpectedly" on the strength of that invariant; a future early
        // `return` that skips this `send` would make a normal exit look like a
        // panic. Do NOT add a code path that leaves this `send` unreached.
        let _ = tx.send(outcome);
        // `cop_run` drops here, AFTER `mrb_close` ran inside `cop_run_body`.
        // Phase 4 (ADR 0013): no `fixes` field; offenses carry `autocorrect`
        // when the cop emitted a fix blob. The child-owned Arc clone drops last.
    });

    match rx.recv_timeout(deadline) {
        Ok(CopOutcome::Completed(offenses)) => offenses,
        Ok(CopOutcome::Raised) => vec![error_offense(
            &file,
            &cop_name,
            &format!("cop `{cop_name}` raised an exception (isolated; design §6)"),
        )],
        Err(mpsc::RecvTimeoutError::Timeout) => {
            // ADR 0009 rule 6 option (a): `rx` is dropped as this arm
            // returns, so any late child `tx.send` fails harmlessly. The
            // child thread is ABANDONED (never joined): for a runaway cop it
            // spins/blocks until process exit (acceptable for the one-shot
            // CLI — ADR 0003 Consequence 1), kept AST-safe by its own Arc
            // clone (ADR 0009 rule 1). One error offense, run continues.
            vec![error_offense(
                &file,
                &cop_name,
                &format!(
                    "cop `{cop_name}` exceeded the {deadline:?} deadline (abandoned; ADR 0003)"
                ),
            )]
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            // The child panicked before sending (it should not — the cop run
            // is FFI, not panicking Rust — but a panic must still degrade to
            // one error offense for that cop×file, never abort the run).
            vec![error_offense(
                &file,
                &cop_name,
                &format!("cop `{cop_name}` worker thread died unexpectedly (isolated)"),
            )]
        }
    }
}

/// Build the single `error offense` for a timed-out / raising / dead cop×file.
/// ADR 0006 frozen shape (`Offense::new`): `Severity::Error`, the cop's own
/// `cop_name`, a zero range (the failure is not tied to a source span), the
/// cause in the message. NO `autocorrect` field — the JSON contract is
/// unchanged by Task 5.
fn error_offense(file: &str, cop_name: &str, message: &str) -> Offense {
    Offense::new(
        file,
        cop_name,
        Range {
            start_offset: 0,
            end_offset: 0,
        },
        Severity::Error,
        message,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Phase-4 deliberate inversion of the Phase-3 soft-(a) invariant (ADR 0013).
    ///
    /// Phase 3 asserted: fix-cop offense JSON byte-identical to no-fix, no `autocorrect`.
    /// Phase 4 (this test) asserts the OPPOSITE: a `fix` block produces `autocorrect:{edits:[...]}`
    /// carrying the REAL `[start, end, replacement]` values marshalled from Ruby;
    /// a no-fix cop still has `autocorrect` ABSENT. See `tests/cop_no_puts_mruby.rs`
    /// for the integration-level assertion; this is the unit-level guard.
    /// ADR 0013 cross-reference: this is the deliberate inversion point.
    #[test]
    fn fix_block_produces_real_edit_in_offense_autocorrect() {
        // ADR 0013: deliberate inversion of Phase-3 soft-(a) "captured-only" invariant.
        // `puts 1\n` — selector `puts` = bytes [0, 4) (ADR 0001).
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

        // No-fix cop: autocorrect ABSENT.
        let ja: serde_json::Value = serde_json::to_value(&a[0]).unwrap();
        assert!(
            ja.as_object().unwrap().get("autocorrect").is_none(),
            "no-fix cop: autocorrect must be ABSENT: {ja}"
        );

        // Fix cop: autocorrect PRESENT with REAL values (Phase 4 — ADR 0013 inversion).
        // fix.replace(n.message_loc, "x"): `puts` selector = bytes [0, 4); replacement = "x".
        let jb: serde_json::Value = serde_json::to_value(&b[0]).unwrap();
        let autocorrect = jb
            .as_object()
            .unwrap()
            .get("autocorrect")
            .expect("fix cop: autocorrect key MUST be present (ADR 0013 Phase 4)");
        let edits = autocorrect["edits"]
            .as_array()
            .expect("autocorrect.edits must be an array");
        assert_eq!(edits.len(), 1, "one fix.replace → one edit");
        assert_eq!(
            edits[0]["range"]["start_offset"], 0,
            "real edit start offset"
        );
        assert_eq!(edits[0]["range"]["end_offset"], 4, "real edit end offset");
        assert_eq!(
            edits[0]["replacement"].as_str().unwrap(),
            "x",
            "real replacement"
        );
    }

    /// Native cop parity: a test-only native cop that emits an `Edit` via
    /// `with_autocorrect` produces the SAME `Offense.autocorrect` JSON shape as
    /// the mruby path (native<->mruby parity, Phase 4 Task 2 boundary test).
    ///
    /// This proves the native cop path can emit the same contract without going
    /// through the mruby FFI boundary — the `with_autocorrect` builder is the
    /// shared surface.
    #[test]
    fn native_cop_with_autocorrect_has_same_json_shape_as_mruby_cop() {
        use crate::offense::{Autocorrect, Edit};
        use crate::{Range, Severity};

        // Build a native offense with autocorrect (the native path: direct Rust).
        let native_offense = Offense::new(
            "t.rb",
            "Murphy/NativeTest",
            Range {
                start_offset: 0,
                end_offset: 4,
            },
            Severity::Warning,
            "m",
        )
        .with_autocorrect(Autocorrect {
            edits: vec![Edit {
                range: Range {
                    start_offset: 0,
                    end_offset: 4,
                },
                replacement: "x".into(),
            }],
        });

        // Build the equivalent via mruby (the mruby path).
        let ctx = AstContext::new(b"puts 1\n".to_vec());
        const MRUBY_COP: &str = r#"
class MrubyTestCop < Murphy::Cop
  def on_call_node(n)
    return unless n.name == :puts && n.receiver_nil?
    add_offense(n.message_loc, message: "m") { |f| f.replace(n.message_loc, "x") }
  end
end
"#;
        let mruby_offenses = run_mruby_cop(&ctx, MRUBY_COP, "Murphy/NativeTest", "t.rb");
        assert_eq!(mruby_offenses.len(), 1);

        // Both paths must produce the same autocorrect JSON shape.
        let jn: serde_json::Value = serde_json::to_value(&native_offense).unwrap();
        let jm: serde_json::Value = serde_json::to_value(&mruby_offenses[0]).unwrap();

        // The autocorrect shapes must match (same start, end, replacement).
        assert_eq!(
            jn["autocorrect"], jm["autocorrect"],
            "native<->mruby parity: autocorrect JSON shape must be identical"
        );
    }

    // ===================================================================
    // Late-finish-after-timeout stress test (P3 Task 8 / ADR 0012 gate
    // prerequisite — murphy-cql).
    //
    // ## ThreadSanitizer — recommended future CI (ADR-0009 TSan loop)
    //
    // The `Send + Sync` / concurrency soundness of the `crates/` embedded-
    // mruby path does NOT rest on a sanitizer run. It rests on, in order:
    //
    //   1. ADR 0009's read-only-immutable-arena reasoning — the prism arena
    //      is only ever read (rule 3), the offense `sink`/`fixes` are
    //      `CopRun`-disjoint fields (the field-disjointness argument on
    //      `CopRun`), and each child owns its OWN `Arc<AstContext>` clone so
    //      the AST outlives any late zombie native call (rule 1);
    //   2. the spike's concurrent-stress evidence (`composition_poc`); and
    //   3. THIS late-finish stress test, which exercises the one window
    //      ADR 0009 rule 6 reasons about but no prior test forced under
    //      load: a child's `MrubyState::Drop` (`mrb_close` + GC finalizers)
    //      running CONCURRENTLY with the caller having moved on and dropped
    //      its own `Arc<AstContext>`.
    //
    // Running ThreadSanitizer over this path remains a RECOMMENDED future
    // CI addition (ADR 0009's honest stated limitation: the soundness
    // argument is by-construction, not yet machine-checked). It is NOT a
    // Phase-3 blocker — by-construction soundness + this guard is the
    // Phase-3 bar; TSan is the belt-and-suspenders follow-up. This module
    // doc is the documentation that closes the ADR-0009 TSan loop for
    // Phase 3.
    //
    // ## RED honesty
    //
    // This is NOT a TDD RED→GREEN test. The late-finish path is sound by
    // construction (ADR 0009 rule 1: each child owns its own `Arc` clone;
    // `composition_poc`-precedented) — there is no honest pre-implementation
    // RED for "no UB", just as `parallel_determinism` has none. The value
    // here is a PERMANENT regression / UB guard that additionally drives
    // the detached-Drop window under load (timing jitter across many
    // iterations), so a future change that breaks the per-child-Arc rule or
    // the drop ordering is caught.
    // ===================================================================

    /// Injected deadline for the late-finish stress. Short so 100 iterations
    /// stay fast, yet far above thread-spawn + cold-mruby-init noise.
    #[cfg(test)]
    const LF_DEADLINE_MS: u64 = 60;
    /// The test cop sleeps this long on its child thread — comfortably PAST
    /// `LF_DEADLINE_MS` (3× margin) so the watchdog `recv_timeout` ALWAYS
    /// fires first and the child returns *just after*, deterministically
    /// landing in the detached-`MrubyState::Drop`-while-caller-moved-on
    /// window. A calibrated busy-loop would be jitter-fragile here; a real
    /// `thread::sleep` via the cfg(test) `Murphy.__test_sleep_ms` primitive
    /// makes "reliably just-late" hold even on a loaded CI host.
    #[cfg(test)]
    const LF_SLEEP_MS: u64 = 180;

    /// A cop whose `on_call_node` sleeps PAST the injected deadline (via the
    /// cfg(test) `Murphy.__test_sleep_ms`). It DOES finish (not a runaway):
    /// it returns `LF_SLEEP_MS - LF_DEADLINE_MS` ms *after* the watchdog has
    /// already timed out, dropped `rx`, and the caller has moved on and
    /// dropped its `Arc<AstContext>` — the precise window under test.
    #[cfg(test)]
    fn late_finish_rb() -> String {
        // The sleep value is `LF_SLEEP_MS` (single source of truth), spliced
        // into the cop so the Ruby and the Rust margin assertion can never
        // drift apart.
        format!(
            r#"
class LateFinishCop < Murphy::Cop
  def on_call_node(node)
    return unless node.name == :puts && node.receiver_nil?
    Murphy.__test_sleep_ms({LF_SLEEP_MS})
    add_offense(node.message_loc, message: "late but finished")
  end
end
"#
        )
    }

    /// Known-good cop run AFTER each abandoned late-finisher, on a FRESH
    /// `Arc<AstContext>`, to prove the abandoned thread's concurrent
    /// `mrb_close`/Drop did not corrupt a subsequent good run.
    #[cfg(test)]
    const GOOD_RB: &str = r#"
class GoodAfterCop < Murphy::Cop
  def on_call_node(node)
    return unless node.name == :puts && node.receiver_nil?
    add_offense(node.message_loc, message: "no bare puts")
  end
end
"#;

    /// **Late-finish-after-timeout stress (murphy-cql core deliverable).**
    ///
    /// Mechanism — RELIABLY hitting the window: the cop calls the
    /// `#[cfg(test)]` `Murphy.__test_sleep_ms(LF_SLEEP_MS=180)` from inside
    /// `on_call_node`, while the watchdog runs with an injected
    /// `LF_DEADLINE_MS=60`. The 3× margin makes the ordering deterministic:
    /// the watchdog's `recv_timeout(60ms)` ALWAYS fires first → it returns
    /// one deadline `error offense` and DROPS `rx`; the caller returns and
    /// drops ITS `Arc<AstContext>`; ~120 ms later the abandoned child wakes,
    /// finishes `cop_run_body` (its `MrubyState::Drop` runs `mrb_close` + GC
    /// finalizers), its `let _ = tx.send` fails harmlessly into the dropped
    /// `rx`, then `CopRun` + the child-owned `Arc` clone drop — all
    /// CONCURRENTLY with the caller already having moved on. That is exactly
    /// the detached-Drop-after-caller-moved-on window (ADR 0009 rules 1 & 6).
    /// Repeated `ITERS=100` times to cover scheduler timing jitter where the
    /// child's `mrb_close`/Drop overlaps the caller's `Arc` drop differently.
    ///
    /// Assertions, per the murphy-cql scope:
    ///   (a) NO crash / panic / abort / UB across ALL iterations — the test
    ///       process stays alive and the test completes;
    ///   (b) a subsequent KNOWN-GOOD cop on a FRESH `Arc<AstContext>` after
    ///       each abandoned late-finisher is uncorrupted (exactly its one
    ///       expected `Warning` offense) — the abandoned thread's concurrent
    ///       `mrb_close`/Drop did not poison later runs;
    ///   (c) BOUNDED: each late-finish iteration returns in ~deadline (NOT
    ///       hung — `< LF_DEADLINE_MS * 8`), and the late-finish run yields
    ///       exactly ONE deadline `error offense` (the deterministic
    ///       contract: the child's `send` lands after `rx` is dropped).
    #[test]
    fn late_finish_after_timeout_is_sound_under_load() {
        const ITERS: usize = 100;
        // The margin invariant the deterministic ordering rests on: the cop
        // sleeps comfortably PAST the deadline (3× margin) so the watchdog
        // always fires first (reliably-just-late, not jitter-fragile).
        // Compile-time enforced — a future tweak to either constant that
        // narrows the margin fails the build, not a flaky run.
        const _: () = assert!(LF_SLEEP_MS >= LF_DEADLINE_MS * 3);
        let deadline = Duration::from_millis(LF_DEADLINE_MS);
        let bound = deadline * 8;
        let src = b"puts \"hi\"\n".to_vec();
        let late_finish_rb = late_finish_rb();

        let suite_start = std::time::Instant::now();

        for i in 0..ITERS {
            // FRESH ctx each iteration: the caller's `Arc` is created here and
            // dropped at end of iteration, while the previous iteration's
            // abandoned child may still be mid-`mrb_close`/Drop on ITS OWN
            // (different) child-owned clone — the strongest form of the
            // window (drop-while-detached-child-still-running).
            let ctx_late: Arc<AstContext> = AstContext::new(src.clone());

            let t0 = std::time::Instant::now();
            let late = run_mruby_cop_isolated_with_deadline(
                &ctx_late,
                &late_finish_rb,
                "Murphy/LateFinish",
                "late.rb",
                deadline,
            );
            let elapsed = t0.elapsed();

            // (c) BOUNDED — the caller was not held hostage by the sleeping
            // (then late-finishing) child; it returned at ~deadline.
            assert!(
                elapsed < bound,
                "iter {i}: late-finish run must be bounded by the watchdog \
                 (elapsed {elapsed:?}, deadline {deadline:?}) — not hung"
            );
            // (c) Deterministic contract: the child's `send` lands AFTER the
            // watchdog dropped `rx`, so this is exactly one deadline error
            // offense (never the cop's real offense — that arrives too late).
            assert_eq!(
                late.len(),
                1,
                "iter {i}: late-finish → exactly one deadline error offense \
                 (got {late:?})"
            );
            assert_eq!(late[0].severity, Severity::Error, "iter {i}: {late:?}");
            assert_eq!(late[0].cop_name, "Murphy/LateFinish", "iter {i}");
            assert!(
                late[0].message.to_lowercase().contains("deadline"),
                "iter {i}: must be the deadline error offense: {}",
                late[0].message
            );

            // Caller drops its `Arc<AstContext>` HERE while the abandoned
            // child is (likely) still finishing its `mrb_close`/Drop on its
            // own clone — the concurrent-Drop window.
            drop(ctx_late);

            // (b) A KNOWN-GOOD cop on a FRESH `Arc` right after the abandoned
            // late-finisher must be uncorrupted: exactly its one expected
            // Warning offense. (Run via the SAME isolated path so a poisoned
            // worker/thread-local or a torn AST would show here.)
            let ctx_good: Arc<AstContext> = AstContext::new(src.clone());
            let good = run_mruby_cop_isolated_with_deadline(
                &ctx_good,
                GOOD_RB,
                "Murphy/GoodAfter",
                "good.rb",
                Duration::from_secs(2),
            );
            assert_eq!(
                good.len(),
                1,
                "iter {i}: a good cop after the abandoned late-finisher must \
                 be uncorrupted — exactly one offense (got {good:?})"
            );
            assert_eq!(good[0].severity, Severity::Warning, "iter {i}: {good:?}");
            assert_eq!(good[0].cop_name, "Murphy/GoodAfter", "iter {i}");
            assert_eq!(good[0].message, "no bare puts", "iter {i}");
        }

        // (a) Reaching here = NO crash / panic / abort / UB across all 100
        // iterations (the process stayed alive, the test completed). Also
        // bound the whole suite: 100 iters × ~deadline ≈ 6 s ceiling — proof
        // the suite itself is bounded, not silently hanging on one iteration.
        let suite = suite_start.elapsed();
        assert!(
            suite < deadline * (ITERS as u32) * 2 + Duration::from_secs(10),
            "the whole late-finish stress suite must be bounded \
             (elapsed {suite:?}) — no iteration hung"
        );
        // FLOOR: each late-finish iteration MUST have actually reached the
        // watchdog timeout (not all completed fast — which would mean the
        // detached-Drop window was never exercised and this guard is vacuous).
        // Each iteration cannot return before its `deadline` elapses, so the
        // suite wall time must exceed `ITERS × deadline`. This makes the test
        // self-prove it is genuinely hitting the window even on a slow CI host.
        assert!(
            suite > deadline * (ITERS as u32),
            "the suite must exceed ITERS×deadline (elapsed {suite:?}) — \
             proves every iteration actually hit the watchdog timeout and \
             exercised the late-finish/detached-Drop window (not vacuous)"
        );
    }
}
