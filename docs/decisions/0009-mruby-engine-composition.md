# ADR 0009 — mruby engine composition (rayon × per-cop watchdog × Arc, under real threads)

- Date: 2026-05-19
- Status: Accepted — **COMPOSITION PROVEN**
- Spike: `murphy-iet` (Phase 3, Spike 3.2)
- Resolves: the thread-safety / abandon-under-threads / `Send+Sync` items ADR 0008 deferred
- Depends on: ADR 0002, ADR 0003 (Mechanism A), ADR 0005 (drop⇄Arc interlock), ADR 0008 (live resolution), ADR 0006/0007 (frozen contract)
- Feeds: Phase 3 Spike Gate; P3 Tasks 2/5/7 (mrb_state wrapper, deadline+isolation, pipeline integration)

## Context

ADR 0008 proved live handle resolution **single-threaded** and explicitly
deferred: thread safety, the `unsafe impl Send + Sync` justification, abandon
under *real* threads, and composition with the Phase-2 rayon file-parallel
pipeline. ADR 0003 Mechanism A (per-cop OS thread + wall-clock watchdog +
abandon-on-timeout) was proven *standalone*. This spike proves they compose.

## Decision (proven by `spikes/composition_poc`)

**The composition works. Adopt this shape for Phase 3 `crates/`:**

- **Pipeline:** `files.par_iter().map(lint_one_file).collect::<Result<…>>()`
  then `aggregate` — Phase 2's exact rayon shape, unchanged.
- **Per file (rayon worker):** parse once → `Arc<AstContext>` (the ADR 0008
  layout verbatim: `parse_result` declared before `source`; walk-order-index
  handles; nothing cached) → run native cops synchronously → dispatch each
  mruby cop.
- **Per mruby cop:** the rayon worker `thread::spawn`s a dedicated OS thread
  that **moves in its own `Arc::clone`** of the file's `AstContext`, then does
  `mrb_open` → `ud = Arc::as_ptr(&worker_clone)` → define primitives →
  `mrb_load_string` → check `(*mrb).exc` → drain a **thread-local** offense
  sink → `mrb_close`, and sends `Result` over an mpsc channel. **The watchdog
  sits in the rayon worker** (`rx.recv_timeout(deadline)`); the mruby lifecycle
  and all live AST reads sit entirely on the per-cop child thread.
- **Abandon-safety (closes the ADR 0008 gap, proven under real threads):**
  because each per-cop thread *owns* its `Arc` clone, a timed-out / abandoned
  (never-joined) cop thread keeps `source` + `ParseResult` alive for any late
  native call **even after the rayon worker returned and dropped its Arc**.
  Observed: looping cop abandoned at the deadline, `strong_count == 2` after
  the worker returned, host wall time ≈ one deadline (not infinite). The
  `ud` raw pointer is *not* the liveness guarantee — the worker-owned `Arc`
  clone is (ADR 0008's required correction, now structurally proven).
- **`unsafe impl Send + Sync for AstContext`** is required (ADR 0008 finding 4
  — `NonNull<pm_parser_t>` makes it `!Send`/`!Sync`; verified: removing the
  impls fails E0277). **SAFETY justification (carry into `crates/`):** the
  prism C arena is read-only for the lifetime of every cop run — after
  `parse()` the tree is never mutated, cops are read-only traversal (design §4,
  no `&mut` ever formed, every primitive takes `&AstContext` and only reads via
  `parse_result.node()` re-walks); concurrent shared `&` reads of an immutable
  C arena from many threads are sound; the `Arc` is freed only after all reader
  threads incl. abandoned ones are gone (each owns a clone).
- **Offense transport:** cops report via a **thread-local** sink drained
  between `mrb_load_string` and `mrb_close`, sent out-of-band over the channel
  — never through the shared read-only `AstContext` (a shared-mutable sink
  there would be a data race; ADR 0008's spike-only `collected` Vec was
  removed here).
- **Exception isolation (design §6):** mruby exceptions do not unwind into
  Rust; the worker checks `(*mrb).exc` after `mrb_load_string` → exactly one
  `error offense` for that cop×file, run continues.

## Evidence

`spikes/composition_poc`: 6 fixtures incl. `looping_cop.rb` (`while true;
end`, zero yield points) and `raising_cop.rb` (`raise` after real native
work). Observed: looping cop → 1 Error offense (abandoned, host ~153 ms not
infinite), sibling good cop on same file still produced its Warning; raising
cop → 1 Error offense, sibling unaffected; **exactly 2 Error offenses** total
(discriminator). Determinism: 5 shuffled/repeated parallel runs →
byte-identical output via the Phase-2 total-order key. `Send+Sync` stress:
80 files × 2 cops × 5 rounds = 11,145 concurrent native re-walks of the same
Arc'd C arena, every parallel result byte-identical to a single-threaded
reference, no torn/incorrect read. `ALL COMPOSITION ASSERTIONS PASSED`,
stable across 7 runs. Root `Cargo.*` clean; Phase-2 workspace tests
unaffected (spike excluded).

## Load-bearing rules for Phase 3 `crates/`

1. Each per-cop worker thread MUST `move` its own `Arc<AstContext>` clone into
   the closure. The `ud` `Arc::as_ptr` is a convenience raw pointer, **not** a
   liveness guarantee. (Proven; ADR 0008's required correction.)
2. The cop offense sink MUST be a **cop-instance-owned local** bound to the
   `mrb_state` lifecycle (a heap `Vec<Offense>` reachable from the cop run,
   e.g. via the `ud` payload, or a value captured in the spawn closure),
   drained then channel-transported. It MUST NOT be a Rust `thread_local!`
   (process-wide TLS): rayon reuses OS threads, so a `thread_local!` slot
   bleeds residue across consecutive cop runs on the same worker, loses
   cop-identity (native callbacks identify the cop via the `ud` payload, not
   thread identity), and is a hazard once a thread is abandoned and the OS
   thread later reused. Never a shared field on the shared `AstContext`
   (that would be a data race on the read-only Arc).
3. `unsafe impl Send + Sync for AstContext` carries the SAFETY text above
   verbatim; the read-only-immutable-arena invariant is the whole basis — any
   future code forming `&mut` into the parsed tree, or a cop mutating shared
   state, breaks it.
4. Watchdog in the rayon worker (`recv_timeout`), mruby lifecycle on the child
   thread; `mrb_close` on the normal path only (NOT on the abandoned path —
   ADR 0003 Mechanism A; the zombie keeps running until process/file scope ends
   but is harmless and AST-safe via its Arc clone).
5. ADR 0006/0007 contract preserved: offenses built Rust-side, total-order
   `aggregate` unchanged, deterministic across parallel + abandoned threads.

6. **Deadline-boundary race (Phase 3 must handle explicitly; not a contract
   violation):** when a cop finishes *exactly* as the watchdog fires, a late
   `send` can arrive after `recv_timeout` already returned `Timeout`. Phase 3
   MUST either (a) drop the `Receiver`/`Sender` immediately after
   `recv_timeout` so a post-timeout `send` fails harmlessly, or (b) explicitly
   ignore post-timeout sends. Consequence for ADR 0006/0007: the
   "byte-identical across repeated/shuffled runs" determinism guarantee holds
   **only for cops with deadline headroom**; for a cop landing exactly on the
   wall-clock boundary, whether it resolves as `Completed` vs one
   `error offense` is inherently non-deterministic (wall-clock; making the
   exact boundary deterministic is impossible by design) and that is
   **accepted, not a contract breach** — it must be documented as the scope of
   the determinism contract, not silently assumed away. The spike's 5
   shuffled runs were byte-identical because the fixtures had ample deadline
   headroom; do not generalize that to the boundary case.

## Limitation (honest; not a blocker)

ThreadSanitizer was **not** run (no nightly/instrumented std in the sandbox).
`Send+Sync` soundness rests on (a) the read-only-immutable-arena reasoning and
(b) heavy concurrent stress (11k+ concurrent native re-walks, all results ==
single-threaded reference, no symptom). **ADR 0009 recommends running TSan on
the Phase-3 `crates/` mruby path when a suitable toolchain is available** —
recommended future verification, not a Phase-3 blocker.

## Carried-forward

`spikes/composition_poc` is throwaway, NOT promoted into `crates/`. With ADR
0008 + 0009 the Phase 3 Spike Gate may pass: the walking-skeleton tasks can be
written against these proven mechanisms. The native-primitive IDL breadth and
the SDK surface are detailed-plan work, not further spikes.
