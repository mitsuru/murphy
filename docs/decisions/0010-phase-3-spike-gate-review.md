# ADR 0010 — Phase 3 Spike Gate review (ADR 0008 + 0009; walking skeleton unblocked)

- Date: 2026-05-19
- Status: Accepted — **SPIKE GATE PASSED**
- Issue: `murphy-6ob` (Phase 3 Spike Gate)
- Reviews: ADR 0008 (live handle resolution), ADR 0009 (engine composition)
- Effect: unblocks the Phase 3 walking-skeleton tasks (`murphy-i08` … `murphy-382`)
- Pattern: mirrors ADR 0005 (Phase 0 Gate) — verdict + cross-ADR interlock

## Verdict

**PASS.** The two Phase-3 keystone risks are resolved and mutually consistent.
ADR 0008 proves live handle resolution (single-threaded); ADR 0009 proves it
composes with the rayon × per-cop-watchdog model under real threads, closing
the items ADR 0008 explicitly deferred. The walking-skeleton bite-sized TDD
tasks may now be written against the proven mechanisms (no fabrication on an
unproven foundation — the Phase 0 / Phase 1 discipline).

## Mutual consistency check

- ADR 0008's deferred items (thread safety, `Send+Sync` justification,
  abandon-under-real-threads, rayon composition) are **exactly** ADR 0009's
  scope and are all resolved there. No gap, no contradiction.
- ADR 0008's required correction ("the `ud` `Arc::as_ptr` is not the liveness
  guarantee; the worker must own its Arc clone") is **structurally proven** in
  ADR 0009 (move-captured clone, `strong_count == 2` after the rayon worker
  returned, zombie thread AST-safe). Consistent and reinforcing.
- The ADR 0006/0007 frozen offense-JSON contract is untouched by both: only an
  integer crosses FFI; offenses are built Rust-side; `aggregate` total order
  unchanged. Severity *precedence* for cross-engine 4-tuple collisions is the
  one deliberate dedupe-semantics extension and is **not** done in the spikes —
  it is walking-skeleton Task 6 with its own ADR (renumbered below).

## Cross-ADR interlock (record now; do not re-derive in Phase 3)

Like ADR 0005's Phase-0 interlock, two rules from **different** ADRs are a
**single contract that one component — P3 Task 2 (`mrb_state`/`AstContext`
lifecycle wrapper) — must satisfy together**:

- **ADR 0008 finding 3a:** the `transmute<'pr → 'static>` is sound only because
  `source` outlives `parse_result`; this MUST be enforced by an explicit
  `Drop`/`ManuallyDrop` ordering + `// SAFETY:`, **not** implicit struct field
  order.
- **ADR 0009 rule 1:** each per-cop worker thread MUST `move`-own its own
  `Arc<AstContext>` clone (the `ud` raw pointer is not liveness).

These interlock: the explicit-drop discipline (0008-3a) governs the *normal*
teardown ordering inside one `AstContext`; the worker-owned-Arc-clone rule
(0009-1) governs the *abandon* path where that teardown is deferred until all
clones (incl. the zombie's) are gone. A Task 2 implementer who applies only one
reintroduces UB on the path the other covers. Task 2 must implement **both** in
the one wrapper, with a `// SAFETY:` block citing ADR 0008 finding 3a + ADR
0009 rules 1/3 (the read-only-immutable-arena `Send+Sync` basis).

## ADR numbering correction (resolve a plan collision)

The Phase 3 plan provisionally referenced ADR 0010 for severity-precedence and
ADR 0011 for the Phase 3 gate, which collided with this Spike Gate ADR.
Authoritative numbering from here:

- **ADR 0010** — this Phase 3 Spike Gate review.
- **ADR 0011** — severity-precedence dedupe (walking-skeleton Task 6).
- **ADR 0012** — Phase 3 Gate review (walking-skeleton Task 8).

The plan doc's references are updated to match.

## Walking-skeleton readiness

- Task 1 (cop registry + `cops/` discovery): no spike dependency beyond the
  gate; ADR 0004 mitigation 2 governs the fixed `cops/` path.
- Task 2 (`mrb_state`/`AstContext` wrapper): write against ADR 0008 (index
  handle, explicit drop order) **and** ADR 0009 (worker-owned Arc clone,
  `unsafe impl Send+Sync` with the verbatim SAFETY text) — the interlock above.
- Task 3 (native-primitive IDL): the ADR 0008 `with_call_node` re-walk +
  walk-order-index shape; assert handle→node distinctness (ADR 0008 finding 2).
- Task 4 (`Murphy::Cop` SDK, soft-(a) stored fix): ADR 0006 JSON shape
  unchanged; fix captured, not serialized.
- Task 5 (deadline + exception isolation): ADR 0009 rules 4/6 (watchdog in the
  rayon worker, `mrb_close` normal-path-only, deadline-boundary race handling).
- Task 6 (severity precedence): ADR 0011, deliberate dedupe extension, JSON
  shape unchanged, flips the Phase-1 phase-bound test as predicted.
- Task 7 (pipeline integration): the ADR 0009 composition shape.
- Task 8 (docs + Phase 3 Gate): ADR 0012.

## Carried-forward UNPROVEN (correct to pass open)

ThreadSanitizer verification of the `crates/` mruby path (ADR 0009 honest
limitation) — recommended when a suitable toolchain is available; not a Phase-3
blocker. Spike PoCs (`spikes/live_resolution_poc`, `spikes/composition_poc`)
are throwaway and are NOT promoted into `crates/`; only ADR 0008/0009/0010 and
their rules are load-bearing.
