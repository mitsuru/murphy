# ADR 0021 — Phase 7.2 mruby build hook and instruction budget seam

- Date: 2026-05-20
- Status: Accepted (partial)
- Issue: `murphy-bn3.2`
- Parent: `0020-phase-6-gate-review`
- Scope: `crates/murphy-core`

## Decision

`murphy-core` will keep `mruby3-sys` as the default runtime path for all existing
callers, while adding an additive build-time seam for a custom mruby open path that can
provide instruction-step budgets in future.

## Design

- Add `crates/murphy-core/build.rs` with deterministic cfg/env signaling:
  - `CARGO_FEATURE_MRUBY_CUSTOM_BUILD`
  - `MURPHY_MRUBY_CUSTOM_BUILD`
  - optional `MURPHY_MRUBY_CUSTOM_BUILD_PATH`
- Add `cfg(mruby_custom_build)` via `cargo:rustc-cfg`, guarded by
  `cargo:rustc-check-cfg`, so strict `unexpected cfg` linting remains clean.
- Add `crates/murphy-core/src/mruby/build.rs` that routes runtime opening through
  `open_state(options: MrubyStateOptions)`.
- Add `MrubyStateOptions { instruction_budget: Option<u64> }` and propagate from the
  cop run layer.
- Add `run_mruby_cop_isolated_with_options` in `sdk.rs` and keep existing APIs by
  delegating:
  - `run_mruby_cop_isolated` → options default
  - `run_mruby_cop_isolated_with_deadline` → options with deadline override

## Fallback policy (implemented)

- Default behavior remains unchanged and MUST continue to match current wall-clock
  watchdog semantics.
- When `cfg!(mruby_custom_build)` is active but not yet customized, runtime still
  uses `mruby3-sys::mrb_open()` so behavior remains stable during rollout.

## Proof status

- Build-seam plumbing exists and compiles.
- Regression tests in `crates/murphy-core/src/mruby/sdk.rs` confirm base
  semantics parity for `run_mruby_cop_isolated_with_options` versus
  `run_mruby_cop_isolated_with_deadline`.
- Warning cleanup for custom cfg and instruction budget field usage has been completed.

## Risks and follow-up

- Custom backend is not yet implemented; when introduced, it must preserve current
  thread-affined lifecycle expectations (open + close in same thread, same abort
  behavior) and validate ABI expectations around `mrb_state` ownership.
- Follow-up task: connect the real custom open path in `crates/murphy-core/src/mruby/build.rs`
  and add an integration test that proves budget-based abort semantics.

## Completion note

- `murphy-bn3.2` now has additive API and seam coverage; full budget-enforced
  runtime behavior is still pending behind the next custom build implementation.
