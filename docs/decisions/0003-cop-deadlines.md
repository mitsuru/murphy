# ADR 0003 — runaway-cop deadline mechanism

- Date: 2026-05-19
- Status: Accepted
- Spike: `murphy-2no` (Phase 0, Spike 0.3)
- Depends on: ADR 0002 (mruby bridge; lifetime/drop rules)
- Feeds: Phase 0 Gate; Phase 3 (cop runner isolation/deadlines); Phase 7 (LSP)

## Context

Design §6 specifies a runaway user cop must be bounded by a "ファイル単位の
実行ステップ/時間デッドライン（mruby 命令フック）" and degrade to an `error
offense` for that cop×file while everything else continues. ADR 0002's Spike
0.3 forward-flag established the assumed mechanism is unavailable: the default
`mruby3-sys` 3.2.0 build exposes **no** `code_fetch_hook` (no
`MRB_USE_DEBUG_HOOK`), and `nm` on `libmruby.a` shows no abort/interrupt/
timeout entry points — mruby has no async preemption.

## Fact established (5-min check, gates the decision)

`mruby3-sys` 3.2.0 `build.rs` (47 lines) runs `make -C mruby` against the
crate-vendored mruby with its **default `build_config`**, hardcoding
`CFLAGS=-fPIE`, and offers **no supported pass-through** (`MRUBY_CONFIG`,
defines, features) to enable `MRB_USE_DEBUG_HOOK`. Enabling the instruction
hook would require forking/patching the crate or its vendored mruby. Per the
spike's time-box rule, that is out of scope here.

→ **Option B (cooperative instruction-hook deadline) is NOT free for v1.**

## Decision

**Mechanism A — OS thread + wall-clock watchdog + abandon-on-timeout — is the
v1 runaway-cop deadline.**

- Each cop runs on its own OS thread with its **own isolated `mrb_state`**
  (per-cop isolation, design §6; mechanically confirmed in Spike 0.2).
- The host waits for the cop's completion with a **wall-clock deadline**
  (channel `recv_timeout`).
- On timeout the worker thread is **abandoned** (never joined), a single
  timeout `error offense` is recorded for that cop×file, and the host
  continues with remaining cops. Process exit reaps the abandoned thread.

**v1 deadline is wall-clock time only.** The design's "execution-step" budget
needs the instruction hook and is deferred (see Consequences).

## Evidence (`spikes/deadline_poc`)

Two cops over a shared real prism AST, 300 ms deadline:

- Pathological cop = `while true; end` — **zero yield points**, no per-iteration
  native callback (the real test: a flag-checking cooperative scheme would
  never catch this).
- Well-behaved cop = `x = 1 + 1`.

Result: runaway → `TimedOut`; good cop (run *after* it) → `Completed`; exactly
one `error offense`; **host elapsed 301 ms** (not held hostage by the infinite
loop). `ALL ASSERTIONS PASSED`. Satisfies design §6 without the instruction
hook.

## Consequences (load-bearing)

1. **Abandoned thread leaks and spins.** A timed-out cop's thread keeps running
   (`while true; end` burns one core) plus its `mrb_state`, until process exit.
   - Acceptable for the **one-shot CLI** (`murphy lint …`): process exits
     promptly and the OS reclaims everything.
   - **Not** acceptable for a long-lived process — LSP / watch mode (Phase 7)
     would accumulate leaked spinning threads per lint pass. **Option B
     (instruction-hook deadline via a forked/patched mruby build) becomes a
     hard prerequisite for Phase 7**, recorded here as a future requirement.
2. **AST/source must outlive the abandoned thread.** The abandoned worker still
   holds the shared AST context; the host must not free it underneath. v1 uses
   `Arc<AstContext>` (the worker holds a clone) so memory is released only when
   *all* threads incl. abandoned ones are gone (process exit for a CLI). This
   directly extends ADR 0002's drop rule: `Box::leak` is the alternative but
   `Arc` is cleaner and demonstrably non-dangling. **Phase 3 cop runner must
   share the AST as `Arc` (or leak), never a borrow the host can drop.**
3. **Composes with the all-core native engine.** Cops get an independent
   thread+deadline each; this layers under the parallel native cop engine
   (each parallel mruby cop carries its own watchdog). The spike ran cops
   sequentially only for clarity.
4. **Pure-Ruby no-yield loops are the binding constraint.** Because a cop can
   spin with no native callback, cooperative flag-checking is provably
   insufficient; thread-abandon is the only mechanism the stock vendored mruby
   supports.

## Rejected alternatives

- **B — cooperative `code_fetch_hook` instruction budget.** Cleanest
  (terminates, no leak, supports a *step* budget per design §6) but requires
  forking/patching `mruby3-sys` or its mruby build config. Deferred to Phase 7
  as the LSP prerequisite, not adopted for v1.
- **Cooperative flag checked from native primitives.** Fails the
  `while true; end` case (no native callback to check the flag). Rejected.
- **Subprocess-per-cop with kill-on-timeout.** Would terminate cleanly, but
  reintroduces the process/IPC model design §2 explicitly rejected (the whole
  point is in-process). Rejected.

## Phase 3 forward items (NOT decided here)

Configurable deadline value + per-file vs per-cop scoping; the runaway
`error offense` shape and its mapping to exit code `2` vs `3` (design §6);
parallel-engine composition specifics. The spike proves the mechanism, not the
cop-API surface. `spikes/deadline_poc` is throwaway; only this ADR is
load-bearing.
