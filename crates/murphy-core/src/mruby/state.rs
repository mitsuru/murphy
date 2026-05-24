//! `MrubyState` + `AstContext` lifecycle wrapper — Phase 3 Task 2.
//!
//! This is the FIRST real mruby FFI in `crates/` and is SAFETY-CRITICAL. It
//! promotes the *lifecycle / ownership* parts of `spikes/live_resolution_poc`
//! and `spikes/mruby_poc` into production code, satisfying — in ONE component —
//! the ADR 0010 cross-ADR interlock:
//!
//!   * **ADR 0008 finding 3a:** the `transmute<'pr → 'static>` on the stored
//!     `ParseResult` is sound ONLY because `source` outlives `parse_result`.
//!     That ordering MUST be enforced **explicitly** (here: `ManuallyDrop`
//!     fields dropped in an explicit order inside `impl Drop`), NOT by implicit
//!     struct field declaration order — a future contributor reordering fields
//!     must not be able to silently introduce UB.
//!   * **ADR 0009 rule 1:** the `ud` raw pointer (`Arc::as_ptr`) is NOT a
//!     liveness guarantee (it touches no refcount). Each per-cop worker thread
//!     (Task 5) MUST `move`-own its own `Arc<AstContext>` clone. This wrapper
//!     DOCUMENTS that requirement; it does not — and cannot — enforce it.
//!   * **ADR 0009 rule 3:** `unsafe impl Send + Sync for AstContext` carries
//!     the read-only-immutable-arena SAFETY justification verbatim.
//!   * **ADR 0009 rule 4 / ADR 0003 Mechanism A:** `mrb_close` runs on the
//!     NORMAL path only. The abandon path deliberately does NOT close; that is
//!     Task 5's responsibility and is intentionally out of scope here.
//!
//! Scope fence (Phase 3 plan, Task 2): this module is *only* the `AstContext`
//! type + the `MrubyState` RAII wrapper. It deliberately does NOT implement:
//! native primitives / live handle resolution (Task 3), the `Murphy::Cop` SDK
//! / `add_offense` / `fix` (Task 4), the per-cop OS thread + watchdog + abandon
//! (Task 5), or pipeline integration (Task 7).
//!
//! ## `unsafe_op_in_unsafe_fn`
//!
//! Deliberately NOT module-allowed: Task 3's `unsafe extern "C"` primitives
//! must annotate each unsafe op explicitly (per-block `unsafe {}`), not inherit
//! a blanket allow.

use std::ffi::CString;
use std::mem::ManuallyDrop;
use std::os::raw::c_void;
use std::sync::Arc;

use ruby_prism::{ParseResult, parse};

use mruby3_sys::{mrb_load_string, mrb_state};

use crate::mruby::build::{MrubyStateOptions, close_state, open_state};

/// The shared, `Arc`-able AST context handed to each per-cop worker.
///
/// It owns BOTH the source bytes and the prism `ParseResult` produced from
/// them. `ruby_prism::parse(src) -> ParseResult<'pr>` borrows `src`; storing
/// both in one struct would be self-referential, so the stored `ParseResult`
/// is lifetime-laundered `'pr → 'static` via `transmute` in [`AstContext::parse`].
/// **That `'static` is a documented LIE** — the true lifetime is `&self.source`
/// living in the same struct. Validity rests on RUNTIME OWNERSHIP DISCIPLINE,
/// not the borrow checker:
///
///   1. **Explicit drop ordering (ADR 0008 finding 3a):** both fields are
///      [`ManuallyDrop`]; `impl Drop` drops `parse_result` (the prism C arena)
///      FIRST, then `source`. This is *independent of field declaration order*
///      — reordering the fields below cannot change the teardown sequence, so
///      a future contributor adding/reordering fields cannot silently make
///      `source` outlive-from-below `parse_result` and dangle the transmuted
///      `ParseResult<'static>`. "Field order" is explicitly NOT the mechanism.
///   2. **Abandon-path liveness (ADR 0009 rule 1):** the whole context lives
///      behind `Arc`; every owner (the host now; an abandoned cop thread in
///      Task 5) holds its OWN clone, so `source` + `parse_result` die together,
///      never apart, even if the host returns first.
///
/// The arena AST is owned here for read-only mruby primitives. It is independent
/// of the prism `ParseResult` borrow; the explicit `parse_result`-before-`source`
/// drop contract below remains the only load-bearing self-reference rule.
pub struct AstContext {
    ast: Option<murphy_ast::Ast>,
    // SAFETY (ADR 0008 finding 3a + ADR 0009 rules 1 & 3 + ADR 0010 interlock):
    //
    //   * `parse_result` is a `ParseResult<'static>` whose `'static` is a LIE:
    //     its real lifetime is `&self.source`. The `transmute` in
    //     `AstContext::parse` is sound ONLY while `source` outlives
    //     `parse_result`. Both are `ManuallyDrop`, and `impl Drop` (below)
    //     drops `parse_result` BEFORE `source`, EXPLICITLY — not via field
    //     declaration order. This is ADR 0008 finding 3a's required correction:
    //     field order is not an acceptable load-bearing safety mechanism in
    //     production code, so it is not used as one here; reordering these two
    //     fields is provably harmless.
    //
    //   * Abandon path (ADR 0009 rule 1, ADR 0010 interlock): the `ud` raw
    //     pointer set via `MrubyState::set_cop_run` does NOT keep this alive.
    //     Per-cop liveness past a deferred teardown is guaranteed solely by the
    //     worker thread owning its own `Arc<AstContext>` clone (Task 5). The
    //     explicit-drop discipline governs the *normal* teardown ordering
    //     inside one `AstContext`; the worker-owned-Arc-clone rule governs the
    //     *abandon* path where that teardown is deferred until every clone
    //     (including a zombie thread's) is gone. They are ONE contract; this
    //     component satisfies both.
    //
    //   * `Send + Sync` basis (ADR 0009 rule 3, verbatim in the `unsafe impl`
    //     below): the prism C arena is read-only for the lifetime of every cop
    //     run; no `&mut` is ever formed into the parsed tree.
    /// `ParseResult<'static>` — the `'static` is a LIE; real lifetime is
    /// `&self.source`. Dropped FIRST (explicitly, in `impl Drop`).
    parse_result: ManuallyDrop<ParseResult<'static>>,
    /// The owned source buffer the (transmuted) `parse_result` references.
    /// Dropped LAST (explicitly, in `impl Drop`), after `parse_result`.
    source: ManuallyDrop<Box<[u8]>>,
}

impl AstContext {
    /// Parse `source` once and own both the bytes and the resulting tree.
    ///
    /// The caller hands ownership of the source bytes in; the context owns them
    /// so the produced `ParseResult` can be soundly `transmute`d to `'static`
    /// (its true lifetime is the now co-owned `source`). Mirrors the proven
    /// `spikes/live_resolution_poc` shape.
    pub fn new(source: impl Into<Box<[u8]>>) -> Arc<Self> {
        let source: Box<[u8]> = source.into();
        let ast = std::str::from_utf8(&source)
            .ok()
            .map(|source_text| murphy_translate::translate(source_text, "<mruby>"));

        // `result` borrows `source` for real here.
        let result: ParseResult<'_> = parse(&source);

        // LIFETIME LAUNDER: `ParseResult<'_ borrowing source>` →
        // `ParseResult<'static>`.
        //
        // SAFETY (ADR 0008 finding 3a + ADR 0009 rule 1 + ADR 0010 interlock):
        // the `'static` is a documented lie. It is sound ONLY because `source`
        // is moved into the SAME `AstContext` below and `impl Drop` EXPLICITLY
        // drops `parse_result` before `source` (independent of field order),
        // and the whole context lives behind `Arc` so on the abandon path a
        // worker-owned clone keeps `source` alive for any late read (the `ud`
        // raw pointer does not — ADR 0009 rule 1). The arena is only ever read
        // (ADR 0009 rule 3); no `&mut` is formed. Only an integer handle ever
        // crosses FFI (Task 3); no `&'pr` is threaded through `ud`.
        //
        // UNWIND PATH: a panic after this `transmute` but before/within
        // `Arc::new` below drops the partially-built `Self` through the SAME
        // explicit `impl Drop`, so the `parse_result`-before-`source` ordering
        // also holds on the unwind path — `source` is never freed while a
        // `ParseResult` still borrows it, even when construction itself panics.
        let parse_result: ParseResult<'static> =
            unsafe { std::mem::transmute::<ParseResult<'_>, ParseResult<'static>>(result) };

        Arc::new(Self {
            ast,
            parse_result: ManuallyDrop::new(parse_result),
            source: ManuallyDrop::new(source),
        })
    }

    /// Borrow the live prism tree. Read-only — cops never mutate it
    /// (ADR 0009 rule 3). Live handle resolution is Task 3; this is only the
    /// minimal accessor the wrapper needs to *hold* and expose the context.
    pub fn parse_result(&self) -> &ParseResult<'static> {
        &self.parse_result
    }

    /// Borrow the owned arena AST used by the mruby primitive surface.
    pub fn ast(&self) -> Option<&murphy_ast::Ast> {
        self.ast.as_ref()
    }

    /// The owned source bytes (`parse_result` conceptually borrows these).
    pub fn source(&self) -> &[u8] {
        &self.source
    }
}

impl Drop for AstContext {
    fn drop(&mut self) {
        // SAFETY (ADR 0008 finding 3a + ADR 0009 rules 1 & 3 + ADR 0010
        // interlock — the SINGLE contract this component satisfies):
        //
        //   * ADR 0008 finding 3a: the stored `ParseResult` is a transmuted
        //     `'static` whose true lifetime is `self.source`. It is sound only
        //     while `source` outlives `parse_result`. We enforce that EXPLICITLY
        //     here by dropping `parse_result` (the prism C arena —
        //     `pm_node_destroy` + `pm_parser_free`) BEFORE `source` (the bytes
        //     it references). This ordering does NOT depend on struct field
        //     declaration order (both fields are `ManuallyDrop`, so the
        //     compiler drops neither implicitly); reordering the fields is
        //     provably harmless. Field order is explicitly NOT the safety
        //     mechanism — per finding 3a it is not an acceptable one in
        //     production code.
        //
        //   * ADR 0009 rule 1 / ADR 0010 interlock: this explicit-drop
        //     discipline governs the *normal* teardown ordering inside one
        //     `AstContext`. The *abandon* path (Task 5) defers this `drop`
        //     until every `Arc<AstContext>` clone — including an abandoned
        //     zombie cop thread's worker-owned clone — is gone; the `ud` raw
        //     pointer is NOT what keeps the pointee alive there. An
        //     implementer applying only one of these reintroduces UB on the
        //     path the other covers.
        //
        //   * ADR 0009 rule 3: the arena was only ever read during the run; no
        //     `&mut` was formed, so dropping it now is the sole mutation and is
        //     race-free (the host has joined all readers on the normal path; on
        //     the abandon path the last Arc owner — possibly the zombie —
        //     performs this drop after no reader remains).
        //
        //   * `ManuallyDrop::drop` is called exactly once per field, here, and
        //     the fields are never used again (we are in `Drop::drop`).
        //
        // REGRESSION GUARD — DO NOT REORDER THESE TWO LINES. Reversing them
        // (freeing `source` before `parse_result`) is a use-after-free: the
        // transmuted `ParseResult<'static>` would read freed bytes. This
        // ordering's regression is caught by **Miri** (`cargo +nightly miri
        // test -p murphy-core`, recommended in CI — see ADR 0008 / the Phase-3
        // Spike-3.1 plan, which anticipated "Miri or documented-reasoning"). It
        // is NOT caught by ordinary `cargo test`: no safe test reads
        // `parse_result` after teardown, so a swapped order still passes the
        // suite. (Caveat, ADR-0009-honest: `ruby_prism::parse` is itself a C
        // FFI call Miri currently cannot execute — `astcontext_drop_order_is_
        // miri_uaf_target` below documents this; in-tree it is a smoke test,
        // and the Miri-as-regression-detector path is gated on a non-FFI prism
        // path / future tooling. The "do not reorder" rule stands regardless.)
        unsafe {
            ManuallyDrop::drop(&mut self.parse_result);
            ManuallyDrop::drop(&mut self.source);
        }
    }
}

// SAFETY (ADR 0009 rule 3 — carried into crates/ VERBATIM): the prism C arena
// is read-only for the lifetime of every cop run — after `parse()` the tree is
// never mutated, cops are read-only traversal (design §4, no `&mut` ever
// formed, every primitive takes `&AstContext` and only reads shared immutable
// parse/arena state); concurrent shared `&` reads of an immutable C arena from
// many threads are sound; the `Arc` is freed only after all
// reader threads incl. abandoned ones are gone (each owns a clone).
//
// ADR 0008 finding 4: `AstContext` is otherwise `!Send`/`!Sync` because the
// transmuted `ParseResult<'static>` holds `NonNull<pm_parser_t>`. The
// read-only-immutable-arena invariant above is the WHOLE basis for overriding
// that — any future code forming `&mut` into the parsed tree, or a cop
// mutating shared state, breaks it.
unsafe impl Send for AstContext {}
// SAFETY: see the `unsafe impl Send for AstContext` block immediately above —
// the read-only-immutable-arena invariant (ADR 0009 rule 3) is identically the
// basis for `Sync`: concurrent shared `&` reads of the immutable C arena from
// many threads are sound.
unsafe impl Sync for AstContext {}

/// RAII wrapper owning exactly one `mrb_state`, created and closed per cop run
/// on the worker thread (design §6 per-cop isolation; ADR 0009).
///
/// Lifecycle: [`MrubyState::open`] (`mrb_open`) → `MrubyState::set_cop_run`
/// → [`MrubyState::eval`] (`mrb_load_string`) → drop (`mrb_close`).
///
/// `MrubyState` is intentionally NOT `Send`/`Sync`: `*mut mrb_state` is a
/// thread-confined VM handle. Each per-cop worker thread calls
/// [`MrubyState::open`] *itself* (the spawn closure owns the state) — the
/// state never crosses threads. Two cops running concurrently is two
/// independent `MrubyState`s on two threads, never one shared (design §6).
///
/// ## `mrb_close` is NORMAL-path only
///
/// `impl Drop` calls `mrb_close`. This is correct for the normal path: the
/// state is closed BEFORE the borrowed `Arc<AstContext>` is released (a GC
/// finalizer / still-defined native primitive could otherwise deref a freed
/// prism C arena = UB — ADR 0002 item 3 / ADR 0005 / ADR 0008 finding 3). The
/// API shape enforces this by construction: the worker drops its `MrubyState`
/// (running `mrb_close`) and only THEN drops its `Arc<AstContext>` clone.
///
/// The **abandon path** (a timed-out cop thread, ADR 0003 Mechanism A /
/// ADR 0009 rule 4) deliberately does NOT `mrb_close` — the zombie state and
/// its `ud` pointer persist, kept AST-safe purely by the worker-owned `Arc`
/// clone (ADR 0009 rule 1). That path (and the watchdog) is **Task 5** and is
/// intentionally NOT implemented here.
pub struct MrubyState {
    mrb: *mut mrb_state,
}

impl MrubyState {
    /// Open a fresh, independent `mrb_state` (`mrb_open`).
    ///
    /// Panics if `mrb_open` returns null (allocation failure — unrecoverable
    /// for this cop run; the caller treats it as the cop×file error path).
    pub fn open() -> Self {
        Self::open_with_options(MrubyStateOptions::default())
    }

    /// Open a fresh, independent `mrb_state` with pluggable build options.
    pub(crate) fn open_with_options(options: MrubyStateOptions) -> Self {
        // SAFETY: `mrb_open` is the documented mruby constructor; it returns
        // either a valid owned `*mut mrb_state` or null. We null-check below
        // and own the handle until `mrb_close` in `Drop`.
        let mrb = open_state(options);
        assert!(
            !mrb.is_null(),
            "mrb_open() returned null (mruby alloc failed)"
        );
        Self { mrb }
    }

    /// Store an `Arc<AstContext>`-derived raw pointer in `mrb_state.ud` so
    /// native primitives (Task 3) can reconstitute `&AstContext` by deref.
    ///
    /// The pointer is `Arc::as_ptr(ctx) as *mut c_void` — a raw `*const`, NOT
    /// an `Arc` (no refcount is touched) and NOT a `&'pr` reference.
    ///
    /// # Liveness is the CALLER's responsibility (ADR 0009 rule 1)
    ///
    /// This `ud` pointer does **NOT** keep the `AstContext` alive — it touches
    /// no refcount. The caller (the per-cop worker, Task 5) MUST keep an owned
    /// `Arc<AstContext>` clone alive in its spawn closure for at least as long
    /// as any `eval` (and any in-flight or post-deadline native call) can run.
    /// On the abandon path the host may drop its `Arc` and return first; only
    /// the worker-owned clone then keeps the pointee valid. This wrapper does
    /// NOT and CANNOT guarantee that — it is ADR 0009 rule 1 / the ADR 0010
    /// interlock, and it lives in the worker, not here. Passing a pointer to an
    /// `Arc` that is dropped before `eval` completes is UB.
    /// Store the per-cop-run [`crate::mruby::sdk::CopRun`] payload pointer in
    /// `mrb_state.ud`.
    ///
    /// Task 2/3 originally put `Arc::as_ptr(&AstContext)` here; Task 4 widened
    /// the `ud` payload to the cop-run-owned `CopRun` (it carries the
    /// `Arc<AstContext>` AND the cop-run-owned offense sink — ADR 0009 rule 2,
    /// NOT a `thread_local!`). Task-3's `primitives::ctx` now projects
    /// `&(*p).ctx`. This is the anticipated extension point — Task 2's `raw()`
    /// and Task 3's `register` docstrings both say "Task 4 lands the first
    /// non-test caller".
    ///
    /// # Liveness is the CALLER's responsibility (ADR 0009 rule 1)
    ///
    /// The `ud` pointer touches no refcount — it does NOT keep the `CopRun`
    /// (nor its inner `Arc<AstContext>`) alive. The caller ([`crate::mruby::
    /// sdk::run_mruby_cop`]; the per-cop worker in Task 5) MUST keep the owned
    /// `CopRun` alive for at least as long as any `eval` (and any in-flight
    /// native call) can run, and MUST close this state (`mrb_close`, via the
    /// `MrubyState` `Drop`) BEFORE the `CopRun` drops. Passing a pointer to a
    /// `CopRun` dropped before `eval` completes is UB.
    pub(crate) fn set_cop_run(&mut self, run: &crate::mruby::sdk::CopRun) {
        let ud = (run as *const crate::mruby::sdk::CopRun) as *mut c_void;
        // SAFETY: `self.mrb` is a valid owned non-null `mrb_state` (since
        // `open`). Writing the `ud` field is the documented mruby
        // native-callback context mechanism. The pointee's liveness is the
        // caller's contract per the doc above (ADR 0009 rule 1).
        unsafe {
            (*self.mrb).ud = ud;
        }
    }

    /// The raw `*mut mrb_state` this wrapper owns, for registering native
    /// primitives / dispatching cops **on the same thread** (Task 3's
    /// `primitives::register`; Task 4's dispatch). Purely additive — it touches
    /// nothing about lifecycle, ownership, drop ordering, or `Send`/`Sync`.
    ///
    /// The returned pointer is valid only while `&self` is alive and MUST NOT
    /// be passed to `mrb_close` (this wrapper's `Drop` is the unique closer —
    /// normal-path only). `pub(crate)`: an in-crate detail (Tasks 3/4), not
    /// public API.
    ///
    /// `allow(dead_code)`: Task 3's only consumer is the `primitives` module's
    /// `#[cfg(test)]` `.rb`-driven tests; the non-test call site (the per-cop
    /// worker registering primitives) lands in Task 4. The accessor is shipped
    /// now because Task 3 genuinely requires it (registering primitives needs
    /// the raw `mrb_state`), avoiding a fragile layout transmute in the tests.
    #[allow(dead_code)]
    pub(crate) fn raw(&self) -> *mut mrb_state {
        self.mrb
    }

    /// Evaluate a Ruby `script` in this state (`mrb_load_string`).
    ///
    /// Task 2 scope: enough to prove open→run→close. Exception isolation
    /// (`(*mrb).exc` checking → one `error offense`) is ADR 0009 / Task 5 and
    /// is intentionally NOT done here.
    ///
    /// Panics if `script` contains an interior NUL byte (cannot be a C string).
    pub fn eval(&mut self, script: &str) {
        let cscript = CString::new(script).expect("script contains an interior NUL byte");
        // SAFETY: `self.mrb` is a valid owned non-null `mrb_state`; `cscript`
        // is a valid NUL-terminated C string that outlives this call.
        // `mrb_load_string` is the documented mruby string-eval entry point.
        unsafe {
            mrb_load_string(self.mrb, cscript.as_ptr());
        }
    }

    /// Evaluate a Ruby `script` and report whether it left a pending mruby
    /// exception (`(*mrb).exc != NULL`) — Task 5 exception isolation
    /// (design §6 / ADR 0009 "Exception isolation").
    ///
    /// mruby exceptions do NOT unwind into Rust: a `raise` (at cop-file load
    /// OR — I-3 — inside `on_call_node`, surfacing through the dispatch eval)
    /// returns control normally with `(*mrb).exc` set. Task-2's [`eval`]
    /// deliberately did not check this, so an in-visitor `raise` was a silent
    /// no-op. This method checks `(*mrb).exc` after the eval and, if an
    /// exception is pending, **clears it** (`(*mrb).exc = NULL`) so the
    /// next eval on this state starts clean. The caller (the per-cop worker,
    /// [`crate::mruby::run_mruby_cop_isolated`]) maps a `true` return to
    /// exactly one `error offense` for that cop×file and continues.
    ///
    /// Purely ADDITIVE to Task 2: it does not touch `Drop`, `Send`/`Sync`,
    /// the `ud` payload, or the existing [`eval`]; it only adds an
    /// exception-state read+clear.
    ///
    /// Panics if `script` contains an interior NUL byte (cannot be a C string).
    pub fn eval_checked(&mut self, script: &str) -> bool {
        let cscript = CString::new(script).expect("script contains an interior NUL byte");
        // SAFETY: `self.mrb` is a valid owned non-null `mrb_state` (since
        // `open`, never closed before `Drop`); `cscript` is a valid
        // NUL-terminated C string that outlives this call; `mrb_load_string`
        // is the documented string-eval entry point. Reading `(*mrb).exc`
        // (a `*mut RObject`) and writing it back to null is the documented
        // mruby pending-exception state — mruby never unwinds into Rust, so
        // this read is the ONLY way to observe a `raise` (design §6). Writing
        // `null` clears the pending exception so a subsequent eval on the same
        // state is not poisoned; the `RObject` itself is owned by the mruby GC
        // (we never free it here — `mrb_close` in `Drop` reclaims it).
        unsafe {
            mrb_load_string(self.mrb, cscript.as_ptr());
            let raised = !(*self.mrb).exc.is_null();
            if raised {
                (*self.mrb).exc = std::ptr::null_mut();
            }
            raised
        }
    }
}

impl Drop for MrubyState {
    fn drop(&mut self) {
        // NORMAL PATH ONLY (ADR 0009 rule 4 / ADR 0003 Mechanism A): close the
        // state here, which by API shape happens BEFORE the worker drops its
        // `Arc<AstContext>` clone — so no GC finalizer / still-defined native
        // primitive can deref a freed prism C arena (ADR 0002 item 3 /
        // ADR 0005 / ADR 0008 finding 3). The ABANDON path (a timed-out cop
        // thread) intentionally must NOT reach this `Drop` (it leaks/forgets
        // the state instead, keeping it AST-safe via the worker-owned Arc
        // clone) — that is Task 5's responsibility and is out of scope here.
        //
        // SAFETY: `self.mrb` is a valid owned non-null `mrb_state` obtained
        // from `mrb_open` in `open` and not closed before now (this is the
        // unique owner; `MrubyState` is not `Clone`/`Copy`). `mrb_close` is the
        // documented destructor; the handle is never used after this.
        close_state(self.mrb);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    /// Compile-time proof that `Arc<AstContext>` is `Send + Sync` (ADR 0009
    /// rule 3). NOT asserted for `MrubyState` — `*mut mrb_state` is correctly
    /// `!Send`/`!Sync` (thread-confined VM handle; the worker opens its own).
    fn assert_send_sync<T: Send + Sync>() {}

    // M-3 (no false sense of thread-safety coverage): `Send + Sync` soundness
    // here rests on ADR 0009's spike concurrent-stress evidence — there is NO
    // in-tree concurrent stress / TSan on the crates/ mruby path. TSan is
    // recommended future CI (ADR 0009 honest limitation) and gets exercised
    // more once Task 7 wires the rayon pipeline. The assertion below is a
    // compile-time trait check only, not a race detector.
    #[test]
    fn arc_ast_context_is_send_sync() {
        assert_send_sync::<Arc<AstContext>>();
    }

    #[test]
    fn open_eval_trivial_script_close_is_panic_free() {
        // open → run a trivial script → close (Drop). Two trivial scripts to
        // exercise an expression and an assignment.
        let mut st = MrubyState::open();
        st.eval("1 + 1");
        st.eval("x = 1; x");
        // `st` drops here → mrb_close. Reaching this line == no panic.
    }

    #[test]
    fn two_independent_states_run_concurrently_from_two_threads() {
        // design §6 isolation: two cops == two independent `MrubyState`s, each
        // OPENED ON ITS OWN THREAD (the state never crosses threads). Proves
        // per-cop state isolation under real concurrency.
        let h1 = thread::spawn(|| {
            let mut st = MrubyState::open();
            st.eval("a = 1 + 1; a");
            // dropped here on this thread → its own mrb_close.
        });
        let h2 = thread::spawn(|| {
            let mut st = MrubyState::open();
            st.eval("b = 2 * 3; b");
        });
        h1.join()
            .expect("thread 1 (independent mrb_state) panicked");
        h2.join()
            .expect("thread 2 (independent mrb_state) panicked");
    }

    #[test]
    fn ast_context_parses_real_source_and_is_arc_shareable() {
        let src = "puts \"hi\"\nlogger.info(x)\nFoo.bar(1)\n";
        let ctx = AstContext::new(src.as_bytes().to_vec());

        // The transmuted ParseResult is live and references the co-owned
        // source bytes (the explicit drop order keeps this sound).
        assert!(
            ctx.parse_result().errors().next().is_none(),
            "fixture must parse cleanly"
        );
        assert_eq!(ctx.source(), src.as_bytes());

        // Shareable across threads via Arc with the explicit drop order honored
        // (runtime counterpart to the compile-time `assert_send_sync`).
        let worker = Arc::clone(&ctx);
        let handle = thread::spawn(move || {
            // Concurrent shared `&` read of the immutable C arena from another
            // thread (ADR 0009 rule 3): re-walk the live tree.
            worker.parse_result().node().location().start_offset()
        });
        let start = handle.join().expect("reader thread panicked");
        assert_eq!(start, 0, "root node starts at byte 0");

        // Host drops its Arc; a clone alive elsewhere would keep source +
        // parse_result alive together (abandon-path structure, ADR 0009 rule
        // 1). Here the last owner drops → explicit ordered teardown.
        drop(ctx);
    }

    /// Drop-order regression target for `impl Drop for AstContext` (ADR 0008
    /// finding 3a — the entire reason Task 2 exists).
    ///
    /// `impl Drop` MUST drop `parse_result` before `source`. A *reversed*
    /// `ManuallyDrop::drop` order frees the owned `source` bytes while the
    /// transmuted `ParseResult<'static>` still references them — a
    /// use-after-free / freed-read that ordinary `cargo test` does NOT catch
    /// (no safe test reads `parse_result` post-teardown, so a swapped order
    /// still passes the whole suite). The *intended* detector is **Miri**: this
    /// test builds an `AstContext` from real owned source, actually touches the
    /// prism arena (so the transmuted borrow is live), then lets `impl Drop`
    /// run via the explicit `Arc` drop — under Miri a reversed drop order would
    /// surface as a detectable freed-read INSIDE `Drop`, not as a post-drop
    /// deref in safe code (there is none here, by design).
    ///
    /// HONEST LIMITATION (verified 2026-05-19, ADR 0009-style honesty): Miri
    /// IS installed and runs in this environment, but it CANNOT execute this
    /// path — `AstContext::new` calls `ruby_prism::parse`, which calls the C
    /// FFI `pm_parser_init`; `cargo +nightly miri test -p murphy-core` aborts
    /// with "unsupported operation: can't call foreign function
    /// `pm_parser_init`" before the drop ordering is ever exercised. So in-tree
    /// this is a SMOKE TEST only (it proves build + a live arena touch +
    /// panic-free ordered teardown). Turning it into a real Miri regression
    /// detector requires a non-FFI prism path / future tooling — tracked as a
    /// Task-3+ concern. The "do not reorder" rule (commented at `impl Drop`)
    /// stands regardless of Miri's reach.
    #[test]
    fn astcontext_drop_order_is_miri_uaf_target() {
        // Real owned source so the transmuted `ParseResult<'static>` genuinely
        // borrows bytes this struct owns.
        let ctx = AstContext::new(b"puts 1\n".to_vec());

        // Exercise the prism arena via the public surface so the transmuted
        // borrow is actually live (not optimized away): read the root node and
        // touch its location, then re-touch the source bytes.
        let root = ctx.parse_result().node();
        assert_eq!(
            root.location().start_offset(),
            0,
            "root node starts at byte 0"
        );
        assert_eq!(ctx.source(), b"puts 1\n");

        // Final live touch immediately before the explicit drop, to widen the
        // Miri window (the freed-read, were the order reversed, must occur
        // INSIDE `impl Drop` — we never read after drop in safe code).
        let _ = ctx.parse_result().node().location().start_offset();

        // Last owner drops here → `impl Drop for AstContext` runs the explicit
        // `parse_result`-then-`source` `ManuallyDrop::drop` sequence. Reaching
        // the end of this test == ordered teardown was panic-free; under a
        // Miri build that could execute prism FFI, a reversed order would be a
        // detected use-after-free here.
        drop(ctx);
    }

    #[test]
    fn mrb_state_used_with_an_ast_context_normal_path_close_before_arc_drop() {
        // Normal-path ordering enforced BY API SHAPE: the worker owns its own
        // Arc clone (ADR 0009 rule 1); the `MrubyState` is dropped (mrb_close)
        // BEFORE that Arc clone is dropped. We make the ordering explicit and
        // observable here.
        let ctx = AstContext::new(b"x = 1\n".to_vec());
        let worker_clone = Arc::clone(&ctx); // ADR 0009 rule 1: worker owns it.
        // Task-4 ud-payload: `set_cop_run` takes the cop-run-owned `CopRun`
        // (it carries this Arc clone). The CopRun outlives the `MrubyState`
        // (mrb_close) — same normal-path ordering this test pins.
        let cop_run = crate::mruby::sdk::CopRun::for_test(Arc::clone(&worker_clone));

        {
            let mut st = MrubyState::open();
            st.set_cop_run(&cop_run);
            st.eval("y = 1 + 1; y");
            // `st` drops at this block's end → mrb_close runs HERE, while
            // `cop_run`/`worker_clone`/`ctx` are still alive. Close-before-AST-drop.
        }

        // Only now is the AST released — strictly after mrb_close. The CopRun
        // holds one clone (the Arc Task-3's `ctx()` projects), plus the host
        // and `worker_clone` → strong count 3.
        assert_eq!(
            Arc::strong_count(&ctx),
            3,
            "host + worker clone + CopRun's clone all alive after mrb_close"
        );
        drop(cop_run);
        drop(worker_clone);
        drop(ctx);
    }
}
