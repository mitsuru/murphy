# Murphy Phase 7: mruby Step Budget + LSP Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add instruction-step budget support for mruby cops using a custom build path (without breaking current wall-clock behavior), then expose Murphy offenses through a first-pass LSP server in `murphy`.

**Architecture:** `murphy-core` stays the single runtime surface for parsing, registry, offense model, and cop execution. `murphy-cli` adds an LSP subcommand that streams canonical `Run`/`Diagnostic` results into JSON-RPC. `murphy-core` gains a swappable mruby backend hook so command builds can switch between upstream `mruby3-sys` and a custom build exposing instruction counters.

**Tech Stack:** Rust workspace (`murphy-core`, `murphy-cli`), `mruby3-sys` today + optional forked/custom build, planned `tower-lsp`-based protocol loop.

---

## File Structure

- Add: `crates/murphy-core/build.rs` to manage vendored/custom mruby feature resolution.
- Add: `crates/murphy-core/src/mruby/build.rs` (`mod` for build-specific helpers and optional hook metadata).
- Modify: `crates/murphy-core/Cargo.toml` to permit feature- or build-profile-specific mruby linkage.
- Add: `crates/murphy-cli/src/lsp.rs` for JSON-RPC transport, config bootstrap, and diagnostics publish flow.
- Modify: `crates/murphy-cli/src/main.rs` to dispatch `murphy lsp`.
- Add: `crates/murphy-cli/tests/lsp_*` integration cases for startup/shutdown and diagnostics shape mapping.
- Add: `docs/decisions/0021-phase-7-p72-mruby-build.md` capturing hook availability, budget semantics, and fallback policy.
- Add: `docs/decisions/0022-phase-7-p73-lsp-contract.md` capturing protocol/version/diagnostic mapping decisions.
- Update: `docs/plans/2026-05-20-murphy-post-fmw-roadmap-design.md` Phase7 completion criteria notes for these subtasks.

### Task 1: `murphy-bn3.2` — custom mruby build for instruction-step budget

**Files:**
- Modify: `crates/murphy-core/Cargo.toml`
- Add: `crates/murphy-core/build.rs`
- Add: `crates/murphy-core/src/mruby/build.rs`
- Modify: `crates/murphy-core/src/mruby/state.rs`
- Modify: `crates/murphy-core/src/mruby/sdk.rs`
- Add: `docs/decisions/0021-phase-7-p72-mruby-build.md`
- Test: `crates/murphy-core/tests/deadline_budget.rs`

- [ ] Claim the issue.

Run:

```bash
bd update murphy-bn3.2 --claim --json
```

- [ ] Add feature-gated build plumbing.

In `crates/murphy-core/build.rs`, emit cfg/output knobs (`mruby_custom_build`, source path checks) and validate custom source is available when enabled. In `Cargo.toml`, add a clear feature switch that defaults to existing `mruby3-sys`.

- [ ] Introduce instruction-budget API without changing host contract.

In `crates/murphy-core/src/mruby/sdk.rs`, keep current `run_mruby_cop_isolated_with_deadline` behavior and add a bounded run path that can request both wall-clock and instruction budget. The public API shape should remain additive: existing callsites keep working unchanged, and new budget parameters are behind an explicit feature type.

- [ ] Gate feature usage on LSP-path only.

In the same module, add a non-default path that is enabled by Phase 7 config and only used by new caller paths. Keep `COP_DEADLINE` and watchdog abandon unchanged for `murphy lint`.

- [ ] Add decision record.

Create `docs/decisions/0021-phase-7-p72-mruby-build.md` with: proof status of custom hook availability, observed fallback behavior when feature is disabled, and migration risk when build system changes.

- [ ] Create regression test.

In `crates/murphy-core/tests/deadline_budget.rs`, add a controlled test that toggles budget-instrumented runner and asserts the same one-offense timeout/degrade behavior appears with both wall-clock and budget controls active.

- [ ] Run verification.

Run:

```bash
cargo test -p murphy-core deadline_budget -- --nocapture
cargo test -p murphy-core mruby::sdk::tests::late_finish_is_bounded -- --nocapture
```

- [ ] Close and annotate.

Run:

```bash
git add crates/murphy-core/Cargo.toml crates/murphy-core/build.rs crates/murphy-core/src/mruby crates/murphy-core/tests/deadline_budget.rs docs/decisions/0021-phase-7-p72-mruby-build.md && git commit -m "feat(core): add custom mruby build hook for budgeted runs"
bd close murphy-bn3.2 --reason "Added Phase 7 mruby build hook and budget API path while preserving lint behavior." --json
```

### Task 2: `murphy-bn3.3` — LSP integration for diagnostics + quick-fix payload

**Files:**
- Modify: `crates/murphy-cli/Cargo.toml`
- Add: `crates/murphy-cli/src/lsp.rs`
- Modify: `crates/murphy-cli/src/main.rs`
- Add: `crates/murphy-cli/tests/lsp_smoke.rs`
- Add: `docs/decisions/0022-phase-7-p73-lsp-contract.md`

- [ ] Claim the issue.

Run:

```bash
bd update murphy-bn3.3 --claim --json
```

- [ ] Add LSP command entry and runtime config.

In `main.rs`, add `murphy lsp` parsing branch and pass `--stdio` stdin/stdout channels into a new LSP module.

- [ ] Implement a minimal server loop with workspace diagnostics.

In `lsp.rs`, implement initialize/shutdown, open/change/doLint, and publish diagnostics translated from `Offense` fields into LSP ranges.

- [ ] Add baseline quick-fix envelope.

Expose `codeAction` for offenses with autocorrect edits using UTF-8-safe range and replacement mapping. If an offense is non-fixable, return empty actions.

- [ ] Add protocol and mapping tests.

Add an integration test that creates a temp Ruby file, runs one lint cycle through the in-process LSP transport mock, and asserts one expected diagnostic code/severity/message.

- [ ] Run verification.

Run:

```bash
cargo test -p murphy-cli lsp -- --nocapture
```

- [ ] Close and summarize.

Run:

```bash
git add crates/murphy-cli/Cargo.toml crates/murphy-cli/src/main.rs crates/murphy-cli/src/lsp.rs crates/murphy-cli/tests/lsp_smoke.rs docs/decisions/0022-phase-7-p73-lsp-contract.md && git commit -m "feat(cli): add initial LSP command with diagnostics"
bd close murphy-bn3.3 --reason "Added initial LSP entrypoint and diagnostic publication flow for offense streaming." --json
```

### Integration checkpoint

- Confirm `murphy-bn3` completion condition before merge:
  - phase-7 custom mruby budget feature can be built and is optional by default.
  - LSP path can launch and publish mapped diagnostics for the existing offense contract.
  - both tasks are independently verifiable and leave `murphy lint` behavior unchanged.

- Resolve follow-up `BLOCKS` (`murphy-fmw.2.2`) explicitly after integration.
