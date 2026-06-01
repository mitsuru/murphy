# TargetRailsVersion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add TargetRailsVersion config parsing and Cx gating for Rails-version-specific cops.

**Architecture:** Reuse the existing major/minor `RubyVersion` wire representation for Rails versions. Store Rails target as optional config, pass it into `CxRaw`, and expose small `Cx` helpers so cops can guard behavior locally.

**Tech Stack:** Rust workspace, `yaml-rust2`, `murphy-plugin-api`, native cop dispatch tests.

---

### Task 1: Config Parsing

**Files:**
- Modify: `crates/murphy-core/src/config.rs`

- [ ] Add failing tests for default `None` and parsing `AllCops.TargetRailsVersion: 5.2` / `7.1.3`.
- [ ] Run `mise exec -- cargo test -p murphy-core config::tests::parses_target_rails_version_from_all_cops config::tests::parses_defaults` and verify RED.
- [ ] Add `target_rails_version: Option<RubyVersion>` to `MurphyConfig`, default it to `None`, and parse from `AllCops`.
- [ ] Re-run the targeted tests and verify GREEN.

### Task 2: Cx API and Dispatch Plumbing

**Files:**
- Modify: `crates/murphy-plugin-api/src/abi.rs`
- Modify: `crates/murphy-plugin-api/src/cx.rs`
- Modify: `crates/murphy-core/src/dispatch.rs`
- Modify: `crates/murphy-plugin-api/src/test_support.rs`
- Modify test CxRaw builders under `crates/murphy-plugin-*` as needed.

- [ ] Add failing tests for `Cx::target_rails_version()` and dispatch propagation.
- [ ] Run targeted tests and verify RED.
- [ ] Tail-append `target_rails_version: u16` to `CxRaw`, add `Cx` helpers, and populate the field from config/test builders.
- [ ] Re-run targeted tests and verify GREEN.

### Task 3: Rails/Pick Guard

**Files:**
- Modify: `crates/murphy-rails/src/cops/rails/pick.rs`
- Modify: `crates/murphy-plugin-api/src/test_support.rs` if test support needs a Rails target setter.

- [ ] Replace the known-limitation test with failing tests showing `TargetRailsVersion: 5.2` suppresses `Rails/Pick` and `6.0` still flags.
- [ ] Run `mise exec -- cargo test -p murphy-rails pick::tests::does_not_fire_below_rails_6 pick::tests::fires_at_rails_6` and verify RED.
- [ ] Add `if !cx.rails_version_at_least(6, 0) { return; }` before pattern matching.
- [ ] Re-run the targeted tests and verify GREEN.

### Task 4: Full Verification and Review

**Files:**
- All modified files.

- [ ] Run `mise exec -- cargo fmt --check`.
- [ ] Run `mise exec -- cargo test --workspace`.
- [ ] Commit the implementation.
- [ ] Run `roborev-refine` on the branch and fix findings.
- [ ] Push the branch and open a PR.

## Self-Review

The plan covers config parsing, Cx API, dispatch plumbing, the `Rails/Pick` regression, and workspace verification. No placeholders remain; each task has concrete files and commands.
