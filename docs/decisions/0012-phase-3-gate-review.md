# ADR 0012 — Phase 3 Gate review (mruby cop path complete; frozen contract preserved)

- Date: 2026-05-20
- Status: Accepted — **GATE PASSED**
- Epic: `murphy-5gf` (Phase 3 — mruby cop path)
- Reviews: ADR 0008 (live handle resolution), 0009 (engine composition), 0010 (Phase 3 Spike Gate), 0011 (severity precedence); P3 Tasks 1–8 + `murphy-cql`
- Preserves: ADR 0006/0007 (frozen offense-JSON / determinism contract)
- Effect: Phase 4 (`murphy-hwe`, autocorrect) may start

## Verdict

**PASS.** A user can drop a `.rb` cop into `cops/` and Murphy runs it — in-process via embedded mruby, per-cop isolated, deadline-guarded, exception-isolated, reading the live shared prism AST through native primitives with no serialization — merged with native cops into one deterministic JSON array, without changing the ADR 0006 frozen offense shape. Every task ran implement → independent spec review → independent code-quality review → fix loop → independent re-verification. The two keystone risks (live resolution, composition) were de-risked spike-first (Phase-0 pattern) before any `crates/` code.

## End-to-end demo (binary, observed)

One `app.rb` (receiver-less `puts`) + a `cops/` containing a good user cop, a `raise`-in-`on_call_node` cop, and a `while true` cop → a single deterministic JSON array:

- `Murphy/Boom` `severity:"error"` — "raised an exception (isolated; design §6)"
- `Murphy/Loopc` `severity:"error"` — "exceeded the 2s deadline (abandoned; ADR 0003)"
- `Murphy/NoPuts` `warning` — the user cop ran
- `Murphy/NoReceiverPuts` `warning` — the native cop ran alongside

exit `1`, **host wall time ~2s** (the infinite-loop cop was abandoned at the deadline, host NOT hung). ADR-0006 5-key shape, no `autocorrect`. A reserved-name collision (`cops/no_receiver_puts.rb`) → exit `2` (a user cop cannot silently shadow an engine cop). Full suite **77 tests, 0 failed**; `cargo fmt --check` 0; `cargo clippy --all-targets -- -D warnings` clean.

## Frozen-contract proof (the load-bearing check)

`crates/murphy-cli/tests/snapshots/sample_project.json` has exactly **one** change commit in its entire history — `8a820ca` (Phase 1 Task 9). `git diff b410b44 HEAD -- …/sample_project.json` (Phase 1 Gate → end of Phase 3, spanning **all of Phase 2 and Phase 3**) is **empty**. The native-only offense JSON / exit codes / `SYNTAX_COP_NAME` / byte offsets are byte-identical through the entire mruby cop path. `integration_snapshot` + `parallel_determinism` pass. The one deliberate dedupe-semantics change (severity precedence, ADR 0011) does not alter the JSON shape and has no cross-engine collision in `sample_project` (single engine) — hence the byte-identity is itself the proof the contract held.

## ADR consistency + cross-ADR interlocks (record; do not re-derive)

- ADR 0008 (live resolution) + ADR 0009 (composition) are mutually consistent and were ratified together by ADR 0010 (Spike Gate). ADR 0010's recorded interlock — ADR 0008-3a (explicit `Drop` ordering, not field order) ⇄ ADR 0009-rule-1 (each per-cop worker owns its own `Arc` clone) — was implemented **together** in P3 Task 2's `AstContext`/`MrubyState` wrapper and verified (spec+code-quality) there.
- ADR 0009 rule 6 (deadline-boundary race) + the late-finish detached-Drop sub-path: implemented in Task 5, and the previously-untested late-finish path is now a permanent stress guard (`murphy-cql`, empirically non-flaky under 4× CPU oversubscription). This **closes ADR 0009's honest TSan/late-finish limitation for Phase 3**; ThreadSanitizer on the `crates/` mruby path remains a **recommended future CI** item (not a Phase-3 blocker), carried forward.
- ADR 0011 (severity precedence Error>Warning) flipped the Phase-1 phase-bound test exactly as that test predicted; the `Severity`-`Ord`⇄precedence coupling is documented at the enum (offense.rs) + a compile-time assertion guards variant reordering.
- soft-(a) honored end-to-end: the `Murphy::Cop` `fix` block is captured but **not applied**; `Offense` JSON is the ADR-0006 frozen shape with **no `autocorrect`** field. Phase 4 owns autocorrect application + the deliberate `Offense.autocorrect` contract extension.

## Known limitation (record for Phase 4/5)

When linting a **directory**, the `cops/*.rb` files are themselves discovered and linted as ordinary `.rb` source (Phase-2 discovery globs `**/*.rb`), in addition to being run as cops — so a broken cop's error offense also appears against the cop file. Not a frozen-contract violation (`sample_project` has no `cops/`); a UX/semantics gap. Tracked: discovery should exclude the configured `cops/` path (natural Phase-5 `[cops]`/discovery work, or a Phase-2-discovery follow-up).

## Phase-deferred (tracked, none weaken the frozen contract)

- **Phase 4 (`murphy-hwe`):** autocorrect application + `Offense.autocorrect` contract + idempotency + reparse loop; the captured-`fix` is the forward-compat seam. Also tracked there: mid-FFI late-finish sub-window variant, `MURPHY_LATE_FINISH_ITERS` override, I-2 redundant-parse free win (parse()'s `exceeds_offset_domain` u32 guard subtlety).
- **Phase 5 (`murphy-3c3`):** `[cops]` config / per-cop enable/severity-override / `.rubocop.yml` migration; structured `ConfigError`; the `cops/`-self-lint exclusion; document/normalize the `Murphy/<PascalCase(stem)>` derived-name contract.
- **Phase 6 (`murphy-7rg`, `murphy-nkq`):** native cop suite breadth; lift `lint_source`'s per-call cop vec to a registry; node-message-loc stringly-IDL hardening.
- **Phase 7 (ADR 0003/0004):** third-party cop sandbox; instruction-step deadline via a custom mruby build.
- Post-Phase-2 perf: `murphy-fvh` persistent cache, `murphy-3ui` streaming memo, `murphy-dfl` fast-abort.

## Carried-forward UNPROVEN (correct to pass open)

ThreadSanitizer verification of the `crates/` mruby path — recommended future CI; soundness rests on ADR 0009's read-only-immutable-arena reasoning + field-disjointness + spike concurrent-stress + the `murphy-cql` late-finish stress guard. Not a Phase-3 blocker. Spike PoCs (`spikes/live_resolution_poc`, `spikes/composition_poc`) are throwaway, NOT promoted into `crates/`; only ADRs 0008–0012 and their rules are load-bearing.
