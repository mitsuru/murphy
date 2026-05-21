# Plugin Cop Config ABI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pass RuboCop-compatible per-cop configuration options to native plugin packs so same-name cops can match RuboCop behavior.

**Architecture:** Extend `murphy.toml` cop rules to retain arbitrary per-cop TOML options while core continues to own `enabled` and `severity`. Bump the native plugin ABI to v2 and add a JSON config slice to file and call callback contexts.

**Tech Stack:** Rust, TOML/serde, serde_json, C-compatible plugin ABI, existing `murphy-example-pack` e2e tests.

---

### Task 1: Preserve Arbitrary Cop Options

**Files:**
- Modify: `crates/murphy-core/src/config.rs`

- [ ] **Step 1: Write failing parser test**

Add a test showing `[cops.rules."Style/StringLiterals"]` keeps `EnforcedStyle` and `Exclude` while still parsing `enabled` and `severity`.

- [ ] **Step 2: Run test and verify RED**

Run: `cargo test -p murphy-core config::tests::cop_rule_preserves_rubocop_compatible_options`

Expected: compile/test failure because `CopRule` has no option storage.

- [ ] **Step 3: Implement minimal option storage**

Add a flattened `BTreeMap<String, toml::Value>` to `CopRule`, expose helpers for serialized JSON and avoid treating `enabled`/`severity` as plugin options.

- [ ] **Step 4: Run test and verify GREEN**

Run: `cargo test -p murphy-core config::tests::cop_rule_preserves_rubocop_compatible_options`

Expected: PASS.

### Task 2: Pass Config Through Plugin ABI v2

**Files:**
- Modify: `crates/murphy-core/src/plugin.rs`
- Modify: `crates/murphy-core/src/registry.rs`
- Modify: `crates/murphy-core/src/lib.rs`

- [ ] **Step 1: Write failing plugin adapter unit test**

Add a test that constructs `PluginFileCop` with config JSON and asserts the file callback sees it through `MurphyFileContext.config`.

- [ ] **Step 2: Run test and verify RED**

Run: `cargo test -p murphy-core plugin::tests::run_file_receives_cop_config_json`

Expected: compile failure because ABI contexts have no `config` field.

- [ ] **Step 3: Implement ABI v2 context fields**

Bump `MURPHY_PLUGIN_ABI_VERSION` to `2`. Add `config: MurphySlice` to `MurphyFileContext` and `MurphyCallContext`. Store config JSON in `PluginFileCop`, pass it from `registry` using the matching cop rule options.

- [ ] **Step 4: Run test and verify GREEN**

Run: `cargo test -p murphy-core plugin::tests::run_file_receives_cop_config_json`

Expected: PASS.

### Task 3: Demonstrate Config-Driven Plugin Behavior

**Files:**
- Modify: `crates/murphy-example-pack/src/lib.rs`
- Modify: `crates/murphy-cli/tests/native_plugin_pack.rs`
- Modify: `docs/decisions/0031-native-plugin-pack-abi.md`

- [ ] **Step 1: Write failing e2e test**

Add a CLI test with `[cops.rules."Example/FileBanner"] message = "configured"` and assert the example plugin emits the configured message.

- [ ] **Step 2: Run test and verify RED**

Run: `cargo test -p murphy-cli --test native_plugin_pack example_native_pack_receives_cop_config_options -- --nocapture`

Expected: FAIL because plugin still emits the default message.

- [ ] **Step 3: Implement example plugin config read**

Parse the JSON config slice minimally in `murphy-example-pack` and use its string `message` option when present.

- [ ] **Step 4: Run focused and full verification**

Run: `cargo test -p murphy-cli --test native_plugin_pack example_native_pack_receives_cop_config_options -- --nocapture`

Run: `cargo test -p murphy-core plugin::tests config::tests`

Run: `cargo test -p murphy-cli --test native_plugin_pack -- --nocapture`

Expected: all PASS.

### Self-Review

- Covers parser option retention, ABI transport, plugin behavior, and docs.
- Leaves full RuboCop option semantics to individual cops, which matches the approved scope.
- Uses ABI v2 because plugin ABI is explicitly still provisional.
