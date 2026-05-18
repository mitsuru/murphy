# ADR 0005 — Phase 0 Gate review

- Date: 2026-05-19
- Status: Accepted — **GATE PASSED**
- Issue: `murphy-8j5` (Phase 0 Gate)
- Reviews: ADR 0001, 0002, 0003, 0004
- Effect: unblocks Phase 1 (`murphy-03u`)

## Verdict

**PASS.** The four Phase-0 ADRs are mutually consistent. Phase 1 may start. One
documentation defect was found and fixed during review; two cross-ADR
interactions are recorded below because they are easy to misread and would cost
time in Phase 3 / Phase 7 if rediscovered.

## Defect found & fixed

- **ADR 0002 "Findings" list was mis-numbered 1, 2, 3, 5, 4** (the Phase 3
  forward-flag was inserted out of order). Renumbered to 1–5 (4 = Phase 3
  forward-flag, 5 = Spike 0.3 forward-flag); verified the internal reference
  "the drop-order rule (item 3)" still points at the drop-order item. Fixed in
  this gate.

## Cross-ADR interactions (record, do not re-derive later)

1. **ADR 0002 drop rule ⇄ ADR 0003 Arc requirement — apparent contradiction,
   actually an interlock.** ADR 0002: `mrb_close()` must precede AST drop or it
   is UB. ADR 0003: on timeout the worker thread is *abandoned* — `mrb_close()`
   is **never called** for it. These reconcile: on the **normal path** safety
   comes from close-before-drop ordering; on the **abandon path** safety comes
   from `Arc<AstContext>` keeping the AST alive past the host stack frame, so
   the still-open mrb_state / in-flight native call never derefs freed AST.
   Both paths satisfy the single invariant *"no in-flight native call ever sees
   freed AST."* Phase 3 must implement **both** mechanisms; they are not
   alternatives.

2. **Phase 7 needs ONE custom mruby build, not two.** ADR 0003 wants a
   forked/patched mruby for the instruction-hook deadline; ADR 0004 wants a
   capability-restricted mruby (disable `Kernel#system`, `File`, sockets) for
   the sandbox. These are the **same** custom-mruby-build work item from two
   motivations. Phase 7 estimation must count it **once**, not twice.

## Phase 1 readiness (the gate's explicit purpose)

- **Phase 1 does not use the mruby path** (per the implementation plan), so ADR
  0002 / 0003 / 0004 do **not** block Phase 1 — they bind Phase 3.
- **Only ADR 0001 binds Phase 1.** Its constraints all translate concretely
  into P1 Task 3: wrap `ruby_prism::parse`; build a structured
  `ParseError { message, range }` from `result.errors()` (never panic); keep
  the source alive for the AST `'pr` lifetime; offsets are **bytes**. Confirmed
  writable against the Spike 0.1 binding (`ruby-prism =1.9.0`). ✔
- **Phase 3 IDL seed exists** from Spike 0.2 (`node_count`, `node_name`,
  `report`, plus candidate list in ADR 0002 Consequences). ✔
- **Pin discipline is consistent** across ADR 0001 (`=1.9.0`) and ADR 0002
  (`=3.2.0`), same rationale (schema stability on load-bearing offsets).
- **Plan sharpen applied:** P1 Task 9's fixture set must include one file with
  non-ASCII (multibyte) content, so the ADR 0001 byte-offset rule is exercised
  by a real snapshot test before Phase 1 closes. Added to the implementation
  plan in this gate.

## Explicitly carried forward UNRESOLVED (correct to pass open)

- Phase 3 live-resolution (ADR 0002 Finding 4) remains unproven by design; the
  gate does **not** require solving it — that is Phase 3's scope.
