# ADR 0008 — live mruby handle resolution (resolves ADR 0002 Finding 4)

- Date: 2026-05-19
- Status: Accepted — **KEYSTONE PROVEN**
- Spike: `murphy-z6y` (Phase 3, Spike 3.1)
- Resolves: ADR 0002 Finding 4 ("live handle resolution UNPROVEN")
- Depends on: ADR 0002 (bridge), ADR 0005 (drop⇄Arc interlock)
- Feeds: Phase 3 Spike 3.2 (composition), P3 Tasks 2/3 (mrb_state, native-primitive IDL)

## Context

Spike 0.2 only proved a *snapshot* bridge (node names pre-collected into a
`Vec`; mruby read the Vec). ADR 0002 Finding 4 flagged the real question as
UNPROVEN: can an mruby `.rb` cop read the **live** shared prism AST through
native primitives, with the AST `Arc`-shared and the prism borrow (`&'pr`)
unable to ride an integer handle? This is the keystone of the whole "fast core,
scripted glue" architecture (design §2/§3) — if live resolution doesn't work
cleanly, the cop SDK changes fundamentally.

## Decision (proven by `spikes/live_resolution_poc`)

**Live resolution works. Adopt this exact shape for Phase 3 `crates/`:**

- **Handle = opaque walk-order index `0..node_count`.** Nothing about nodes is
  cached — not names, receivers, ranges, nor offsets. Only the integer count is
  stored. Strongest possible "no snapshot".
- **Resolution re-walks the LIVE tree per native call.** `with_call_node(h, f)`
  re-derives a fresh `Node` from `parse_result.node()` (the live prism C arena)
  and visits to the Nth `CallNode`, reading its real `name()` / `receiver()` /
  `message_loc()` on the spot.
- **AST shared as `Arc<AstContext>`;** `mrb_state.ud` carries
  `Arc::as_ptr(&ctx) as *mut c_void` — a raw `*const AstContext`, **not** an
  `Arc` (no refcount touch) and **not** a `&'pr` reference. Native callbacks
  reconstitute `&AstContext` by deref. Soundness rests on **runtime ownership
  discipline** (the host `Arc` provably outlives the `mrb_load_string` call),
  not the type system.
- **The `&'pr` problem:** `parse(src) -> ParseResult<'pr>` borrows `src`. The
  stored `ParseResult` is lifetime-laundered to `'static` via `transmute` — a
  **documented lie**; the true lifetime is `&AstContext.source` held in the
  same struct (declared so the parse arena drops before the source). Only an
  integer ever crosses FFI.

Evidence: `puts "hi"\nlogger.info(x)\nFoo.bar(1)\n` → 5 handles, each live-
resolved distinctly: `puts` (no receiver, msg `[0,4)`), `logger.info` →
`info` (receiver present, `[17,21)`), bare `logger` (no receiver, `[10,16)`),
`x`, `Foo.bar` → `bar`. `ALL LIVE-RESOLUTION ASSERTIONS PASSED`; matches the
Phase-0 `prism_poc` semantics. Workspace tests unaffected (spike excluded).

## Load-bearing findings (Phase 3 `crates/` MUST obey these)

1. **`*const CallNode` is UNSOUND — never store node-wrapper pointers.**
   `CallNode<'pr>` is a walk-time temporary (`{NonNull<parser>, *mut node, …}`,
   no `Clone`); a captured pointer aliases a reused slot (probe deref'd to the
   *wrong* node). Resolution MUST re-walk to an index, not cache a node ptr.
2. **Offset-keyed resolution is REJECTED.** `logger.info(x)` has the bare
   `logger` CallNode and the outer `.info` CallNode sharing `start_offset`;
   offset-first-match aliased two handles to one node. Walk-order **index** is
   the resolution key. The Phase-3 IDL tests MUST assert handle→node
   distinctness so this cannot regress silently.
3. **Drop order (ADR 0002 item 3 / ADR 0005), enforced by field order +
   explicit close.** `AstContext` declares `parse_result` before `source` so
   the prism arena drops first. Normal path: `mrb_close()` runs **before** the
   `Arc<AstContext>` drops (a GC finalizer / still-defined primitive could
   otherwise deref a freed C arena = UB). Phase 3 must encode this in the
   `mrb_state` wrapper's drop/close sequencing.
3a. **The `transmute<'pr → 'static>` is sound ONLY because `AstContext`'s
   field order makes `source` outlive `parse_result` — this is fragile and
   MUST be made non-implicit in `crates/`.** A future contributor adding
   fields (`collected`, `source_digest`, `file_path`, …) and reordering them
   so `source` drops before `parse_result` introduces **silent UB** (the
   transmuted `ParseResult<'static>` would reference freed source bytes). P3
   Task 2 (`mrb_state`/`AstContext` wrapper) MUST NOT rely on implicit field
   order: enforce the drop sequence explicitly (an explicit `impl Drop` that
   drops `parse_result` then `source`, or `ManuallyDrop` with documented
   ordering) plus a `// SAFETY:` block stating the invariant. "Field order"
   is not an acceptable load-bearing safety mechanism in production code.
4. **`AstContext` is `!Send`/`!Sync`** (the transmuted `ParseResult<'static>`
   holds `NonNull<pm_parser_t>`). Phase 3 / Spike 3.2 will need a deliberate
   `unsafe impl Send + Sync` justified by "the prism C arena is read-only
   during a cop run" — **Spike 3.2 must prove this under real threads before
   any `crates/` code relies on it.** Not granted here.
5. **Re-walk is O(N) per native call → O(N²)** for an N-call cop. Acceptable
   for the spike; a Phase-3 perf concern if cops traverse heavily (a resumable
   cursor / single-visit dispatch is the obvious later optimization — YAGNI now,
   note for the IDL task).

## Consequences / forward

- **Abandon path (argued, not thread-proven here) — and the gap a Phase 3
  implementer must NOT misread:** the `Arc::as_ptr(&ctx)` stored in `ud` is a
  **raw pointer that does NOT keep the `Arc` alive** (it touches no refcount).
  The liveness guarantee for a zombie/abandoned cop thread is therefore **NOT**
  the `ud` pointer — it is that **each per-cop worker thread must own its own
  `Arc<AstContext>` clone, moved into the thread closure**, so the `AstContext`
  outlives any late native call from that thread regardless of when the host
  drops its `Arc`. ADR 0003 Mechanism A does not `mrb_close` an abandoned
  thread, so its `mrb_state` (and its `ud` raw pointer) persist; only the
  worker-owned `Arc` clone keeps the pointee valid. "Just put `Arc::as_ptr`
  in `ud`" is **insufficient** for abandon-safety. **Spike 3.2 must prove this
  structurally under real abandoned threads and ADR 0009 must state the
  worker-owns-its-Arc-clone requirement explicitly.**
- P3 Task 3 (native-primitive IDL) builds on this `with_call_node` re-walk +
  index-handle shape; P3 Task 2 (`mrb_state` wrapper) encodes finding 3.
- ADR 0006 frozen offense JSON shape is untouched by the bridge (only an
  integer crosses FFI; offenses are built Rust-side as before).
- `spikes/live_resolution_poc` is throwaway and is NOT promoted into `crates/`;
  only this ADR and its rules are load-bearing.

## Carried-forward UNPROVEN (correct to pass open → Spike 3.2)

Thread safety / `Send + Sync` justification, abandon-under-real-threads, and the
rayon × per-cop-watchdog composition are Spike 3.2's scope. This ADR proves
*single-threaded* live resolution only.
