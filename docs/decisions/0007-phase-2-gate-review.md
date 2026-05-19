# ADR 0007 — Phase 2 Gate review (native engine scale-out complete; ADR 0006 contract preserved)

- Date: 2026-05-19
- Status: Accepted — **GATE PASSED**
- Epic: `murphy-hgz` (Phase 2 — native engine scale-out)
- Reviews: P2 Tasks 1–8 (`murphy-pux, dab, 5of, 0gz, aom, 6lv, 0fw, tkp`)
- Preserves: ADR 0006 (Phase 1 frozen contract)
- Effect: Phase 3 may start; the offense/exit/determinism contract is unchanged

## Verdict

**PASS.** Phase 2 scaled the native engine from "one file, one cop, single-thread" to "discover a project, lint it across all cores, deterministically, parsing duplicate content once" **without changing the Phase 1 frozen contract**. Every task ran implement → independent spec review → independent code-quality review → fix loop → independent re-verification on the project toolchain.

## End-to-end demo (binary, observed)

- explicit file args → **unchanged** Phase-1 behavior (offense JSON, exit 1)
- `murphy lint <dir>` and `murphy lint` (zero args) → recursive `.rb` discovery, offenses sorted by `(file, …)`, exit 1
- `murphy.toml` `[files] exclude = ["sub/**"]` → matching files pruned
- `.murphyignore` prunes; ambient `.gitignore` **not** honored (verified)
- malformed / unknown-key `murphy.toml` → exit 2 (structured, no panic)
- missing file in a multi-file/discovered set → exit 2 (abort preserved across the read/parallel split)
- identical-content files → parsed once per run, output identical to non-memoized

Full suite: **43 tests, 0 failed** (`murphy-cli`: main unit 2 + cli 12 + integration_snapshot 1 + parallel_determinism 1; `murphy-core`: lib 26 + cop integration 1). `cargo fmt --check` 0; `cargo clippy --all-targets -- -D warnings` clean.

## Frozen-contract proof (the load-bearing check)

`crates/murphy-cli/tests/snapshots/sample_project.json` has exactly **one** change commit in its entire history — `8a820ca` (Phase 1 Task 9, where it was blessed). `git diff b410b44 HEAD -- …/sample_project.json` (Phase 1 Gate → end of Phase 2) is **empty**. The snapshot is byte-identical through the aggregator total-order change, `Severity: Ord`, rayon parallelism, discovery, and in-run memoization. ADR 0006's offense-JSON shape, exit codes, byte offsets, and `SYNTAX_COP_NAME` are unchanged. `integration_snapshot` + `parallel_determinism` both green.

## What Phase 2 added (and what stayed frozen)

- **Determinism keystone:** `aggregate` sort extended to a **total order** over all five `Offense` fields (`file, start, end, cop_name, message, severity`); `Severity` gained `Ord` (non-contract — serde output unchanged). Dedupe key unchanged (4-tuple, severity excluded; Phase 3 owns precedence). This made parallelism observably deterministic.
  **Load-bearing for Phase 6:** this total order is the **sole guarantor** of deterministic output once multiple cops run concurrently (today only `NoReceiverPuts` ships, so ties are not yet exercised). A Phase 6 author who trims the `aggregate` sort key — or relies on cop-registration / thread order — reintroduces flaky parallel output. Do not weaken the `aggregate` comparator; it is the determinism contract, not an optimization.
- **`Cop: Send + Sync`** (ADR 0002 phase-2 flag), minimal bound, kept satisfiable by Phase-3 mruby worker-thread cops (the trait doc records the "do not store `mrb_state` in a field" trap).
- **rayon file-level parallelism** over `lint_one_file`; abort-on-first-Err → exit 2 preserved.
- **Discovery** (`ignore` + `globset` + `toml`): discovery-only `murphy.toml` `[files] include/exclude` (`#[serde(deny_unknown_fields)]`) + `.murphyignore`; all ambient `ignore` filters disabled (regression-tested).
- **In-run memoization**: `lint_one_file` split into `read_source` (abort→exit 2) + `lint_source` (path-independent); identical content parsed once, fanned per path; output byte-identical.

## Phase-2-deferred items (tracked, none weaken the contract)

- `murphy-fvh` — persistent on-disk `source_digest` cache (cross-run skip).
- `murphy-3ui` — streaming memo to bound peak memory (Phase A reads all contents).
- `murphy-dfl` — fast-abort: cancel in-flight rayon tasks on first setup error.
- `murphy-nkq` — native cop suite hygiene → re-deferred to **Phase 6** (no 2nd cop shipped).
- `murphy-3c3` (Phase 5) noted: structured `ConfigError` refactor; `[cops]` becomes a known field then (one-line, not a schema break).
- Phase 6 (`murphy-7rg`) noted: lift the per-call `vec![Box::new(NoReceiverPuts)]` to a caller-provided cop registry (also needed for Phase 3 native↔mruby composition).

## Carried-forward UNRESOLVED (correct to pass open)

Phase 3 live mruby handle resolution (ADR 0002 Finding 4) and the Phase 7 custom-mruby build (ADR 0003/0004) remain unproven by design; Phase 2 is native-only and does not touch the mruby path. The `Cop: Send + Sync` bound was kept mruby-compatible but mruby cops are not implemented here.
