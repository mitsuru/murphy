# Murphy Phase 3 — mruby Cop Path Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (or superpowers:subagent-driven-development) to implement this plan task-by-task.
> Phase 3 re-opens architecture: ADR 0002 Finding 4 (live handle resolution) and ADR 0003 forward-flag (no instruction hook) are **load-bearing UNPROVEN** assumptions. This plan therefore follows the **Phase 0 pattern: de-risking spikes → ADR → detailed walking skeleton**, NOT bite-sized TDD on an unproven foundation. Writing TDD for the mruby SDK before Spike 3.1/3.2 resolve would fabricate detail (the lesson from the initial plan / Phase 1).

**Goal:** Let a user drop a `.rb` cop into `cops/` and have Murphy run it — in-process via embedded mruby, reading the live shared prism AST through native primitives, per-cop isolated and deadline-guarded — merged with native cops into the same deterministic output, without changing the ADR 0006 frozen offense-JSON shape.

**Architecture:** Per ADR 0002, user cops are `.rb` interpreted by an embedded `mruby3-sys` VM with one isolated `mrb_state` per cop; the prism AST (`Arc<AstContext>`) is shared and read *live* by Rust native primitives the mruby cop calls through `mrb_state.ud` (no serialization). Per ADR 0003 each cop runs on its own OS thread under a wall-clock watchdog (abandon-on-timeout); a crashing/runaway/exception cop degrades to one `error offense` for that cop×file and the run continues. mruby and native cops share a cop registry and the Phase-2 rayon file-parallel pipeline; `aggregate` merges both, with severity precedence introduced for cross-engine 4-tuple collisions.

**Tech Stack:** Existing `crates/murphy-core` + `crates/murphy-cli`; `ruby-prism =1.9.0`, `mruby3-sys =3.2.0` (both already deps of the relevant crate or proven in spikes), `rayon` (Phase 2). No new heavy deps expected.

**Source of truth:** design doc §2/§3/§4/§6/§8; ADR 0002 (bridge, lifetime/drop, Finding 4), ADR 0003 (deadline Mechanism A), ADR 0004 (trust: cops only from `cops/`), ADR 0005 (drop⇄Arc interlock), ADR 0006/0007 (frozen contract). `spikes/mruby_poc` + `spikes/deadline_poc` are the proven-mechanism references.

---

## Scope Fences (decided — do not re-litigate)

1. **fix API = soft-(a): accepted but stored only.** The `Murphy::Cop` SDK provides `add_offense` **and** a `fix` block a cop author can write (forward-compatible), but in Phase 3 the fix is **only captured, never applied**, and the `Offense` JSON stays the **ADR 0006 frozen shape** — `autocorrect` is **absent/empty**, the offense struct is unchanged. **Phase 4** owns autocorrect application, the deliberate `Offense.autocorrect` contract extension, idempotency, and the reparse loop. Phase 3 must NOT add the `autocorrect` field to the serialized contract or apply edits. (Captured-fix storage is internal/in-memory only, dropped after the run — it exists so cop authors write Phase-4-ready cops today.)
2. **Spike-first.** Tasks after the Spike Gate are detailed *bite-sized TDD*; tasks before it are **time-boxed spikes** (deliverable = ADR + throwaway `spikes/*` PoC, NOT TDD). The walking-skeleton code blocks are *representative* — substitute the mechanism the Spike ADRs actually prove (exactly as Phase 1 substituted the Spike 0.1 binding).
3. **Cop loading = fixed `cops/` dir** from the project root (ADR 0004 mitigation 2: "v1 loads cops only from the project's own configured `cops/` path"). **NO `[cops]` in `murphy.toml`** — Phase 2's `#[serde(deny_unknown_fields)]` stays; per-cop config / enable-disable / severity-override / `.rubocop.yml` migration are **Phase 5**. `--debug` listing loaded cop paths (ADR 0004 mitigation 3) is a coarse post-skeleton item.
4. **Cop registry is a Phase 3 deliverable, not Phase 6.** The P2-Task-7 review's deferred "lift `vec![Box::new(NoReceiverPuts)]` to a caller-provided registry" is **required here** — Phase 3 needs one collection holding native + mruby cops. Do not push it further to Phase 6.
5. **Severity precedence is Phase 3's job (ADR 0006 says so explicitly).** Native + mruby offenses now merge; a 4-tuple `(file,cop_name,range,message)` collision across engines must resolve by **severity precedence**, replacing Phase 1's input-order "first wins". This is a **deliberate internal extension of `aggregate`'s dedupe semantics** recorded in a **new ADR 0010** — the offense **JSON shape is unchanged** (only *which* offense survives a collision changes). It flips the Phase-1 `severity_only_dup_collapses_to_first_phase1_behavior` test exactly as that test's phase-bound comment predicted (that flip is correct evolution, not a regression).
6. **Deadline = ADR 0003 Mechanism A only.** Per-cop OS thread + wall-clock watchdog + abandon-on-timeout; the abandoned thread holds `Arc<AstContext>` (ADR 0005 drop⇄Arc interlock — abandoned thread keeps the AST alive, `mrb_close` is NOT called on the abandon path; on the normal path `mrb_close` precedes AST drop). v1 is **wall-clock time only** (no instruction-step budget — the hook is unavailable per ADR 0003; that stays a Phase 7 item). Deadline value: a sane hardcoded default in Phase 3; configurability is coarse/later.
7. **ADR 0006/0007 frozen contract preserved** except the deliberate, ADR-0010-documented severity-precedence dedupe change (JSON shape, exit codes, `SYNTAX_COP_NAME`, byte offsets all unchanged). Regression guard: `integration_snapshot` + `parallel_determinism` use only `NoReceiverPuts` over `sample_project` (no mruby cop, no severity collision there) → those snapshots MUST stay byte-identical. mruby-cop tests use a SEPARATE fixture dir + `cops/`, never `sample_project`.
8. **Phase 3 Gate (exit criteria):** end-to-end — a user writes `cops/no_puts.rb`; `murphy lint <dir>` discovers files (Phase 2) and the cop (from `cops/`), runs it on its own isolated `mrb_state` + watchdog **in parallel with** native `NoReceiverPuts`, reading the live AST via native primitives (no serialization); a deliberately broken/looping cop degrades to one `error offense` for that cop×file and the run continues; output is aggregated + deterministic; the offense JSON shape is the ADR 0006 frozen shape (autocorrect absent). Spike ADRs 0008/0009 + the severity-precedence ADR 0010 written; all quality gates green.

---

## Phase 3 De-risking Spikes (time-boxed; deliverable = ADR + throwaway PoC, NOT TDD)

These resolve the load-bearing UNPROVEN assumptions. Same discipline as Phase 0: box each spike; the ADR records the decision + a minimal working PoC under `spikes/`; PoC code is throwaway and is NOT promoted into `crates/`.

### Spike 3.1 — Live handle resolution (resolves ADR 0002 Finding 4) — THE KEYSTONE

**Deliverable:** `docs/decisions/0008-live-handle-resolution.md` + `spikes/live_resolution_poc/`.

**Question:** Spike 0.2 only proved a *snapshot* model (names pre-collected into a Vec). Prove the real thing: an mruby `.rb` calls a native primitive that resolves an opaque integer handle to a **live** prism node and reads it directly (`name`, `receiver?`, `message_loc` → byte `Range`) from a `ParseResult<'pr>` that borrows the source — with the AST shared as `Arc<AstContext>`, the borrow expressed as `*const T` (the `&'pr` cannot ride an integer handle; this is the E0106 root cause from Spike 0.2), and the drop-order rule (ADR 0002 item 3 / ADR 0005) honored at runtime (not via the type system).

**Done when:** an mruby script walks ≥1 node of a *really parsed* file via native calls that read the **live** tree (not a pre-snapshot), with `Arc<AstContext>` shared into the cop's `mrb_state.ud`; the PoC demonstrates the drop ordering (normal path: `mrb_close` before AST drop; abandon path: Arc keeps AST alive) and a Miri or documented-reasoning argument that no native call dereferences freed AST. The ADR fixes the `*const T`-via-`ud` pattern + the lifetime-discipline rules Phase 3's `crates/` code will follow.

### Spike 3.2 — Composition: rayon file-parallel × per-cop mruby watchdog × Arc

**Deliverable:** `docs/decisions/0009-mruby-engine-composition.md` + `spikes/composition_poc/`.

**Question:** ADR 0003 Mechanism A (per-cop OS thread + wall-clock watchdog + abandon) was proven *standalone*. Prove it **composes** with the Phase 2 rayon file-parallel pipeline: a rayon file-worker parses once, runs native cops, and dispatches mruby cop(s) each on its own isolated `mrb_state` + watchdog thread holding `Arc<AstContext>`; a runaway/`raise`ing cop → exactly one `error offense` for that cop×file, the worker proceeds to the next file, other files unaffected; exit codes (0/1/2/3) and `aggregate` determinism (ADR 0006/0007) hold; the abandoned mruby thread holding the Arc does not dangle (ADR 0005 interlock).

**Done when:** a PoC runs N files in a rayon pool where each file triggers a native cop + an mruby cop; one fixture's mruby cop infinite-loops and another `raise`s; observed: each degrades to one `error offense` for that cop×file, all other offenses present, process exits with the correct code, output deterministic across repeated/shuffled runs. The ADR records the composition shape (where the watchdog thread sits relative to the rayon worker), the exception→error-offense contract, and restates the ADR 0005 drop⇄Arc interlock in Phase-3 terms so it is not rediscovered.

### Phase 3 Spike Gate

Review ADR 0008 + 0009 together (as Phase 0 Gate reviewed its four). Confirm: the walking-skeleton parse/handle/registry/deadline tasks can be written against the proven mechanisms; the ADR 0006 frozen JSON shape is untouched by the bridge; the severity-precedence change (ADR 0010, written in the skeleton) is the only deliberate contract-semantics extension and its scope is understood. Record the verdict in a gate ADR (0011-style, mirroring ADR 0005/0006/0007). Bite-sized tasks below MUST NOT start until this gate passes.

---

## Phase 3 Walking Skeleton (detailed bite-sized TDD — AFTER the Spike Gate)

Vertical slice: **one user mruby cop, end-to-end, merged with the native cop.** Code blocks are representative; substitute the Spike 3.1/3.2 ADR mechanisms.

> Each task: failing test → run-fail → minimal impl → run-pass → commit. Keep `integration_snapshot`/`parallel_determinism` (native-only, `sample_project`) byte-identical throughout — they are the ADR 0006 regression guard. mruby-cop tests use a dedicated fixture dir, never `sample_project`.

### Task 1: Cop registry (native + mruby), `cops/` discovery

**Files:** Create `crates/murphy-core/src/registry.rs` (+ `lib.rs`); Modify `crates/murphy-cli/src/main.rs` (build the registry once, pass `&[Box<dyn Cop>]`-equivalent). Test: `registry.rs` `#[cfg(test)]`.

- Lift the per-call `vec![Box::new(NoReceiverPuts)]` (P2-Task-7 M-3) into a `CopRegistry` that holds native cops now and gains mruby cops in Task 4. Discover `cops/*.rb` from the project root (ADR 0004 mitigation 2) — paths only here; loading is Task 3/4.
- TDD: registry with the one native cop yields it; empty `cops/` → just natives; `cops/*.rb` paths enumerated sorted.
- Commit.

### Task 2: `mrb_state` lifecycle wrapper (isolated, drop-ordered) — per ADR 0008/0009

**Files:** Create `crates/murphy-core/src/mruby/state.rs`. Test: same.

- A safe wrapper owning one `mrb_state`, created/closed per cop run on the worker thread (Spike 3.2 placement). Encodes the ADR 0005 drop rule: `mrb_close` before the borrowed AST is released on the normal path. `Arc<AstContext>` stored in `ud` per Spike 3.1.
- TDD: open→run trivial script→close; a second independent state in parallel (design §6 isolation) — mirrors `spikes/mruby_poc` but in `crates/`.
- Commit.

### Task 3: Native-primitive IDL (live), from Spike 3.1

**Files:** Create `crates/murphy-core/src/mruby/primitives.rs`. Test: same + a minimal `.rb`.

- The native functions an mruby cop calls: resolve handle → live prism node; expose `name`, `receiver_nil?`, `message_loc` → byte `Range` (ADR 0001), `source_slice(range)`. Seeded by Spike 0.2 + finalized by Spike 3.1. **Read-only** (design §4 — no AST mutation).
- TDD: a `.rb` snippet drives each primitive over a real parsed file; byte ranges match hand-derived values (ADR 0001 byte discipline; reuse a multibyte case).
- Commit.

### Task 4: `Murphy::Cop` SDK base + `on_call_node` + `add_offense` + stored `fix` (soft-(a))

**Files:** Create `crates/murphy-core/src/mruby/sdk.rs` + an embedded `Murphy::Cop` prelude `.rb`. Test: a fixture `cops/` with one `.rb`.

- The Ruby-facing base class: `class Murphy::Cop`, `on_call_node(node)` visitor dispatch, `add_offense(range, message:, severity:)`, and a `fix` block that is **captured and stored only** (Scope Fence 1 — NOT applied, NOT serialized; `Offense` stays the ADR 0006 shape). An mruby cop file is loaded into an isolated state (Task 2), walks via primitives (Task 3), emits offenses collected into the same `Vec<Offense>` native cops use.
- TDD: fixture `cops/no_puts.rb` (the design §4 example) over a fixture file → one offense, ADR 0006 JSON shape, `autocorrect` absent; a cop that calls `fix` → offense identical (fix stored, not in JSON).
- Commit.

### Task 5: Per-cop deadline + exception isolation (ADR 0003 Mechanism A, composed per ADR 0009)

**Files:** Modify `crates/murphy-core/src/mruby/*` + the registry dispatch. Test: fixture cops that loop / `raise`.

- Each mruby cop runs on its own OS thread + wall-clock watchdog; timeout → abandon (Arc keeps AST alive), one `error offense` for that cop×file, continue. A Ruby exception is caught → one `error offense`, continue (design §6). Hardcoded sane deadline.
- TDD: `cops/loops.rb` (`while true; end`) → one error offense for it, the native cop's offenses still present, exit code correct; `cops/boom.rb` (`raise`) → one error offense, others unaffected.
- Commit.

### Task 6: Severity-precedence dedupe in `aggregate` (ADR 0010 — deliberate, JSON shape unchanged)

**Files:** Modify `crates/murphy-core/src/aggregator.rs`; Create `docs/decisions/0010-severity-precedence.md`. Test: aggregator + the flipped Phase-1 test.

- Cross-engine 4-tuple collision now resolves by severity precedence (ADR 0006 "Phase 3 owns severity precedence"). Write ADR 0010 first (the decision: precedence order, why JSON shape is unchanged, that it flips `severity_only_dup_collapses_to_first_phase1_behavior`). Update that Phase-1 test to assert the new deterministic precedence (its comment predicted this).
- TDD: native+mruby offense colliding on the 4-tuple, differing severity → higher-precedence survives, deterministic regardless of engine/thread order; `sample_project` snapshots still byte-identical (no collision there).
- Commit.

### Task 7: Pipeline integration + end-to-end

**Files:** Modify `crates/murphy-cli/src/main.rs` (registry → rayon file-worker runs native + mruby cops per ADR 0009); Test: `crates/murphy-cli/tests/mruby_e2e.rs` (dedicated fixture dir + `cops/`).

- Wire the registry into the Phase-2 parallel pipeline: each file worker runs native + mruby cops, deadline-guarded, merged via `aggregate`. Determinism preserved (ADR 0007 total order).
- TDD: a fixture project with `cops/no_puts.rb` + a `.rb` containing `puts` and `print` → both `Murphy/NoReceiverPuts` (native) and the user cop's offense appear, sorted/deterministic, ADR 0006 JSON shape; broken cop → error offense + continue; explicit-file `sample_project` snapshot unchanged.
- Commit. Phase 3 walking skeleton complete.

### Task 8: Docs + Phase 3 Gate

**Files:** `README.md`, `CLAUDE.md` (status → Phase 3; honest scope: user mruby cops work, but fix is captured-not-applied / no autocorrect output yet / no `[cops]` config / cops only from `cops/`); Create `docs/decisions/0011-phase-3-gate-review.md`.

- Verify every documented command. Gate: end-to-end demo (Scope Fence 8), spike + severity ADRs in place, frozen `sample_project` snapshots byte-identical, all gates green. ADR 0011 records the verdict + what stayed frozen + Phase-4/5/6 deferred items.
- Commit. Phase 3 complete.

---

## Phase 3+ Coarse (re-plan in detail after the Phase 3 Gate)

- More `on_<prism_node_type>` visitors (Phase 3 skeleton ships `on_call_node`; the rest grow as cops need them — YAGNI).
- Cop-authoring guide + `--debug` lists loaded cop paths (ADR 0004 mitigation 3).
- Deadline value configurability + per-file vs per-cop scoping (ADR 0003 Phase-3 forward item) — not the hardcoded skeleton default.
- Multi-mruby-cop tie-break proof under parallelism (extends ADR 0007's total-order guarantee to the cross-engine case at scale).
- Node-pattern DSL: **out of scope** (design §4 says v1 ships no pattern DSL — YAGNI).

## Deferred / boundaries (tracked, not built in Phase 3)

- **Autocorrect application, `Offense.autocorrect` contract extension, idempotency, reparse loop** → Phase 4 (`murphy-hwe`). Phase 3's stored-but-unapplied `fix` is the forward-compat seam.
- **Per-cop config / severity override / enable-disable / `.rubocop.yml` migration / `[cops]` in murphy.toml** → Phase 5 (`murphy-3c3`).
- **Native cop suite breadth / name-set scaling / re-export hygiene** → Phase 6 (`murphy-7rg`, `murphy-nkq`).
- **Third-party cop sandbox, instruction-step deadline (custom mruby build)** → Phase 7 (ADR 0003/0004).
- Post-Phase-2 perf items (`murphy-fvh` persistent cache, `murphy-3ui` streaming memo, `murphy-dfl` fast-abort) — unaffected by Phase 3.
