# AllCops.ActiveSupportExtensionsEnabled infra — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Expose `AllCops.ActiveSupportExtensionsEnabled` to cops, default it to true when the rails pack is loaded (the pack delivers its `default.yml` as an embedded data symbol that core collects + merges), and make `Style/SymbolProc` exempt `lambda`/`proc`/`Proc.new` blocks when it is enabled.

**Architecture:** Mirror the existing `AllCops.TargetRailsVersion` end-to-end wiring (config field → `CxRaw` tail field → `cx` method → dispatch populate). For the default-true driver, the rails pack embeds its `config/default.yml` via `include_str!` and exposes it as a `#[no_mangle] static RawSlice MURPHY_PLUGIN_DEFAULT_CONFIG`; the loader reads it when present; core collects every loaded pack's yaml and merges (std `false` < pack `true` < user). No macro, `PluginRegistration`, or ABI-version change.

**Tech Stack:** Rust workspace (`murphy-plugin-api`, `murphy-core`, `murphy-cli`, `murphy-std`, `murphy-rails`), `yaml_rust2`, `libloading`, the single-surface plugin ABI.

**Design:** `docs/plans/2026-06-04-active-support-extensions-enabled-design.md`

**Conventions:**
- TDD: failing test first. Run `eval "$(mise activate bash)"` before cargo if tools are missing.
- The template for core wiring is `target_rails_version` — every wiring task cites its precedent lines.
- Gates: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo +nightly fmt --check`.
- ABI policy: `CxRaw` tail fields do NOT bump `MURPHY_PLUGIN_ABI_VERSION` (lockstep). Keep it 4.

---

### Task 1: `CxRaw` field + `Cx` accessor (murphy-plugin-api)

**Files:** Modify `crates/murphy-plugin-api/src/abi.rs` (CxRaw end ~line 199; assertions ~line 420), `crates/murphy-plugin-api/src/cx.rs` (~line 488).

**Step 1: Failing ABI assertion** — in `abi.rs` test block, after `assert_eq!(offset_of!(CxRaw, target_rails_version), 240);`:
```rust
        assert_eq!(offset_of!(CxRaw, active_support_extensions_enabled), 242);
```
Keep `assert_eq!(size_of::<CxRaw>(), 248);`.

**Step 2: Run** `cargo test -p murphy-plugin-api 2>&1 | tail` → FAIL (no field).

**Step 3: Implement** — append to `CxRaw` after `pub target_rails_version: u16,`:
```rust
    /// `AllCops.ActiveSupportExtensionsEnabled` (default false). Tail-appended
    /// under ABI v4 lockstep (murphy-pfcb); fits existing padding so
    /// `size_of::<CxRaw>()` is unchanged. Per project policy the numeric ABI is
    /// not bumped for tail-appended CxRaw fields.
    pub active_support_extensions_enabled: bool,
```
In `cx.rs` after `rails_version_at_least`:
```rust
    /// Configured `AllCops.ActiveSupportExtensionsEnabled` (default false).
    pub fn active_support_extensions_enabled(&self) -> bool {
        self.raw.active_support_extensions_enabled
    }
```
Fix every `CxRaw { … }` literal (the cx.rs test constructor ~line 2984, and `build_cx_raw` in Task 5) to set the field — `false` in the cx.rs test constructor.

**Step 4: Run** `cargo test -p murphy-plugin-api 2>&1 | tail -5` → PASS (offset 242, size 248).

**Step 5: Commit** `feat(plugin-api): add CxRaw.active_support_extensions_enabled + Cx accessor (murphy-pfcb)`

---

### Task 2: test-support hook (murphy-plugin-api)

**Files:** Modify `crates/murphy-plugin-api/src/test_support.rs` (mirror `with_target_rails_version` ~line 138-216).

**Step 1-3: Implement** — add `active_support_extensions_enabled: bool` field to the tester (default false at construction, beside `target_rails_version: None` ~line 138/152), add:
```rust
    /// Set `AllCops.ActiveSupportExtensionsEnabled` for this cop test.
    pub fn with_active_support_extensions_enabled(mut self, enabled: bool) -> Self {
        self.active_support_extensions_enabled = enabled;
        self
    }
```
Thread it through every `assert_*_inner` call (~177-206) exactly where `self.target_rails_version` is passed, and set it on the inner `CxRaw`. Update the `_inner` fn signatures.

**Step 4: Run** `cargo test -p murphy-plugin-api 2>&1 | tail -5` → compiles/PASS. (No standalone test needed — Task 8's SymbolProc tests exercise it.)

**Step 5: Commit** `test(plugin-api): tester hook with_active_support_extensions_enabled (murphy-pfcb)`

---

### Task 3: config parse (murphy-core)

**Files:** Modify `crates/murphy-core/src/config.rs` — `DefaultCopsData` (~88) + `from_yaml` AllCops arm (~119); `MurphyConfig` field (~11) + `ParsedYaml` field (~201) + `merge_over` (~220) + `into_murphy_config` (~254) + default (~298); user-yaml AllCops parse (~582).

**Step 1: Failing tests** (mirror `parses_target_rails_version_from_all_cops` ~1007):
```rust
#[test]
fn parses_active_support_extensions_enabled_from_all_cops() {
    let cfg = MurphyConfig::from_yaml_str("AllCops:\n  ActiveSupportExtensionsEnabled: true\n").unwrap();
    assert!(cfg.active_support_extensions_enabled);
    let cfg = MurphyConfig::from_yaml_str("").unwrap();
    assert!(!cfg.active_support_extensions_enabled, "default false");
}

#[test]
fn default_cops_data_parses_active_support_flag() {
    assert_eq!(
        DefaultCopsData::from_yaml("AllCops:\n  ActiveSupportExtensionsEnabled: true\n").allcops_active_support_extensions_enabled,
        Some(true),
    );
    assert_eq!(
        DefaultCopsData::from_yaml("AllCops:\n  Include:\n    - '**/*.rb'\n").allcops_active_support_extensions_enabled,
        None,
    );
}
```

**Step 2: Run** `cargo test -p murphy-core active_support 2>&1 | tail` → FAIL.

**Step 3: Implement**
- `DefaultCopsData`: `pub allcops_active_support_extensions_enabled: Option<bool>,`. In `from_yaml` AllCops arm, after `Exclude`:
  ```rust
  if let Some(Yaml::Boolean(b)) = all_cops.get(&Yaml::String("ActiveSupportExtensionsEnabled".to_string())) {
      result.allcops_active_support_extensions_enabled = Some(*b);
  }
  ```
- `MurphyConfig`: `pub active_support_extensions_enabled: bool,`. `ParsedYaml`: `active_support_extensions_enabled: Option<bool>,`. `merge_over`: `self.active_support_extensions_enabled.or(base.active_support_extensions_enabled)`. `into_murphy_config`: `active_support_extensions_enabled: self.active_support_extensions_enabled.unwrap_or(false),`. default: `None`.
- user-yaml parse (~582, next to TargetRailsVersion):
  ```rust
  if let Some(Yaml::Boolean(b)) = all_cops.get(&Yaml::String("ActiveSupportExtensionsEnabled".to_string())) {
      parsed.active_support_extensions_enabled = Some(*b);
  }
  ```

**Step 4: Run** `cargo test -p murphy-core active_support 2>&1 | tail` → PASS.

**Step 5: Commit** `feat(core): parse AllCops.ActiveSupportExtensionsEnabled (murphy-pfcb)`

---

### Task 4: pack-layer merge on MurphyConfig (murphy-core)

**Goal:** A method that takes loaded packs' `default.yml` strings and re-resolves `active_support_extensions_enabled` as `std(false) < pack layers < user`, with the user's explicit value still winning.

**Files:** Modify `crates/murphy-core/src/config.rs`.

**Step 1: Failing tests**
```rust
#[test]
fn pack_layer_flips_active_support_default() {
    // user did not set it; a pack layer says true → resolves true.
    let mut cfg = MurphyConfig::from_yaml_str("").unwrap();
    cfg.apply_pack_default_layers(&["AllCops:\n  ActiveSupportExtensionsEnabled: true\n"]);
    assert!(cfg.active_support_extensions_enabled);
}

#[test]
fn user_value_beats_pack_layer() {
    let mut cfg = MurphyConfig::from_yaml_str("AllCops:\n  ActiveSupportExtensionsEnabled: false\n").unwrap();
    cfg.apply_pack_default_layers(&["AllCops:\n  ActiveSupportExtensionsEnabled: true\n"]);
    assert!(!cfg.active_support_extensions_enabled, "explicit user false wins");
}

#[test]
fn no_pack_layer_leaves_default_false() {
    let mut cfg = MurphyConfig::from_yaml_str("").unwrap();
    cfg.apply_pack_default_layers(&[]);
    assert!(!cfg.active_support_extensions_enabled);
}
```

**Step 2: Run** `cargo test -p murphy-core pack_layer 2>&1 | tail` and `user_value_beats` → FAIL (no method).

**Step 3: Implement** — track whether the user set the value. Add a private `user_set_active_support: bool` to `MurphyConfig` (set true in `into_murphy_config` when the parsed `Option` was `Some`; default false), OR keep the raw `Option` on `MurphyConfig`. Simplest: store the resolved bool plus a `user_set_active_support_extensions_enabled: bool`. Then:
```rust
/// Merge pack-bundled default.yml layers (later overrides earlier) for the
/// ActiveSupport flag. The user's explicit value, if any, still wins.
pub fn apply_pack_default_layers(&mut self, pack_yamls: &[&str]) {
    if self.user_set_active_support_extensions_enabled {
        return; // user wins; pack layers are only defaults
    }
    for yaml in pack_yamls {
        if let Some(v) = DefaultCopsData::from_yaml(yaml).allcops_active_support_extensions_enabled {
            self.active_support_extensions_enabled = v; // later layer overrides
        }
    }
}
```
Wire `user_set_active_support_extensions_enabled` through `ParsedYaml`/`into_murphy_config` (it is `self.active_support_extensions_enabled.is_some()` at resolution time, merged with `||` like the include/exclude `saw_*` flags). Confirm the existing `saw_include`/`saw_exclude` pattern and mirror it.

**Step 4: Run** `cargo test -p murphy-core 2>&1 | tail -5` → PASS.

**Step 5: Commit** `feat(core): apply_pack_default_layers for ActiveSupportExtensionsEnabled (murphy-pfcb)`

---

### Task 5: dispatch threading (murphy-core)

**Files:** Modify `crates/murphy-core/src/dispatch.rs` (`run_cops_with_options_and_target_rails_version` ~266; `build_cx_raw`).

**Step 1-3: Implement** — add parameter `active_support_extensions_enabled: bool` to `run_cops_with_options_and_target_rails_version` (after `target_rails_version`) and `build_cx_raw`; set the `CxRaw` field. Fix the `build_cx_raw` literal. (Leave the fn name; or rename to `…_and_allcops` and update both callers — pick one, be consistent.)

> Per advisor: fix **every** `CxRaw` construction site the compiler flags (mruby/user-cop dispatch builders). For those, set `false` **deliberately** (native SymbolProc is the only consumer this PR; mruby cops seeing the flag is out of scope) and add a one-line comment so it is not an accidental default.

**Step 4: Run** `cargo test -p murphy-core 2>&1 | tail -5` → PASS/compiles.

**Step 5: Commit** `feat(core): thread active_support_extensions_enabled through dispatch (murphy-pfcb)`

---

### Task 6: rails pack embedded default.yml + data symbol (murphy-rails)

**Files:** Create `crates/murphy-rails/config/default.yml`; modify `crates/murphy-rails/src/lib.rs`.

**Step 1: default.yml**
```yaml
# Murphy rails pack bundled defaults — mirrors rubocop-rails: enable
# ActiveSupport extension awareness for cops that consult it.
AllCops:
  ActiveSupportExtensionsEnabled: true
```

**Step 2: Embed + expose** in `lib.rs` (mirror `murphy-std/src/lib.rs:25`; `RawSlice` is re-exported via `murphy_plugin_api`):
```rust
/// default.yml embedded in the .so as a resource.
pub const BUNDLED_DEFAULTS_YAML: &str = include_str!("../config/default.yml");

/// Pure data symbol the host reads after dlopen (not a behavior callback).
#[no_mangle]
pub static MURPHY_PLUGIN_DEFAULT_CONFIG: murphy_plugin_api::RawSlice =
    murphy_plugin_api::RawSlice::from_str(BUNDLED_DEFAULTS_YAML);
```

**Step 3: Verify** `cargo build -p murphy-rails 2>&1 | tail -3` → builds; confirm the symbol is exported:
`nm -D target/debug/libmurphy_rails.so 2>/dev/null | grep MURPHY_PLUGIN_DEFAULT_CONFIG` (Linux) → one entry.

**Step 4: Commit** `feat(rails): embed default.yml + MURPHY_PLUGIN_DEFAULT_CONFIG data symbol (murphy-pfcb)`

---

### Task 7: loader reads the symbol; registry + cli collect & merge (murphy-core, murphy-cli)

**Files:** Modify `crates/murphy-core/src/plugin_loader.rs` (`LoadedPluginPack` ~312; `load_plugin_pack` ~357-392), `crates/murphy-core/src/registry.rs` (surface the contributions), `crates/murphy-cli/src/main.rs` (collect + re-resolve + pass to dispatch ~421/560/1071).

**Step 1: Failing loader test** — add to `plugin_loader.rs` tests a case loading a pack that exports `MURPHY_PLUGIN_DEFAULT_CONFIG` and asserting `pack.default_config_yaml()` returns the yaml. (Reuse the test fixture pack if one exists; otherwise assert the field plumbing compiles and rely on Task 9 e2e for the live read.)

**Step 2: Implement loader read** — in `load_plugin_pack`, after `validate_registration` and **before** moving `library` into the struct:
```rust
let default_config_yaml = {
    // Optional data symbol; absence is normal (not all packs ship config).
    match unsafe { library.get::<*const murphy_plugin_api::RawSlice>(b"MURPHY_PLUGIN_DEFAULT_CONFIG\0") } {
        Ok(sym) => {
            let slice = unsafe { **sym }; // RawSlice (ptr,len), bytes live in the .so
            // SAFETY: bytes are 'static in the loaded library; copy to owned now
            // while `library` is alive.
            let bytes = unsafe { std::slice::from_raw_parts(slice.ptr, slice.len) };
            std::str::from_utf8(bytes).ok().map(|s| s.to_owned())
        }
        Err(_) => None,
    }
};
```
Add `default_config_yaml: Option<String>` to `LoadedPluginPack` and a `pub fn default_config_yaml(&self) -> Option<&str>` accessor; set it in the constructor.

**Step 3: Registry surface** — add `CopRegistry` method `pub fn pack_default_configs(&self) -> Vec<String>` (or `Vec<&str>`) returning each loaded dynamic pack's `default_config_yaml` (skip `None`). Confirm `CopRegistry` retains the `LoadedPluginPack`s (it loads them in `discover_with_config` ~163-167); if it currently drops them after extracting cops, retain the yaml strings instead.

**Step 4: cli wires it** — after `let registry = CopRegistry::discover_with_config(...)` (~1071), before dispatch:
```rust
let pack_layers: Vec<&str> = registry.pack_default_configs_refs(); // or own Strings + collect refs
config.apply_pack_default_layers(&pack_layers);
```
(`config` must be `mut`.) Then pass `config.active_support_extensions_enabled` as the new dispatch argument at both call sites (~421, ~560).

**Step 5: Run** `cargo test -p murphy-core 2>&1 | tail -5 && cargo build -p murphy-cli 2>&1 | tail -3` → PASS/builds.

**Step 6: Commit** `feat(core,cli): collect pack default.yml layers and re-resolve ASE (murphy-pfcb)`

---

### Task 8: SymbolProc exemption (murphy-std)

**Files:** Modify `crates/murphy-std/src/cops/style/symbol_proc.rs` (`check_any_block` ~107; parity block ~28-40).

**Step 1: Failing tests**
```rust
#[test]
fn exempts_lambda_when_active_support_enabled() {
    test::<SymbolProc>().with_active_support_extensions_enabled(true)
        .expect_no_offenses("->(x) { x.method }\n");
}
#[test]
fn exempts_proc_when_active_support_enabled() {
    test::<SymbolProc>().with_active_support_extensions_enabled(true)
        .expect_no_offenses("proc { |x| x.method }\n");
}
#[test]
fn exempts_proc_new_when_active_support_enabled() {
    test::<SymbolProc>().with_active_support_extensions_enabled(true)
        .expect_no_offenses("Proc.new { |x| x.method }\n");
}
#[test]
fn flags_regular_block_even_when_active_support_enabled() {
    test::<SymbolProc>().with_active_support_extensions_enabled(true)
        .expect_offense(indoc! {"
            coll.map { |e| e.upcase }
                     ^^^^^^^^^^^^^^^^ Pass `&:upcase` as an argument to `map` instead of a block.
        "});
}
```
The existing `flags_lambda_arrow` / `flags_proc_block` / `flags_proc_new_block` (no flag → default false) MUST stay green.

**Step 2: Run** `cargo test -p murphy-std symbol_proc 2>&1 | tail` → the three `exempts_*` FAIL.

**Step 3: Implement** — in `check_any_block`, after `block_method` is resolved (~107), before the unsafe-hash check:
```rust
    // ActiveSupport extensions enabled: lambda/proc/Proc.new are exempt
    // (mirrors RuboCop's proc_node? / LAMBDA_OR_PROC guard).
    if cx.active_support_extensions_enabled()
        && is_lambda_or_proc_dispatch(node, call, block_method, cx)
    {
        return;
    }
```
Add the helper near the other exclusion helpers:
```rust
fn is_lambda_or_proc_dispatch(node: NodeId, call: NodeId, block_method: &str, cx: &Cx<'_>) -> bool {
    if cx.is_lambda_literal(node) {
        return true;
    }
    if matches!(block_method, "lambda" | "proc") {
        return true;
    }
    if block_method == "new" {
        if let Some(recv) = cx.call_receiver(call).get() {
            if let NodeKind::Const { name, .. } = *cx.kind(recv) {
                return cx.symbol_str(name) == "Proc";
            }
        }
    }
    false
}
```
> Confirm the `Const { name, .. }` shape with `murphy ast --format sexp -` on `Proc.new {}` if unsure. `call_receiver` / `is_lambda_literal` are already used in this file.

**Step 4: Run** `cargo test -p murphy-std symbol_proc 2>&1 | tail` → PASS (new + existing).

**Step 5: Parity metadata** — move `AllCops::ActiveSupportExtensionsEnabled` from `Gaps:` to `Covered:` (lambda/proc/Proc.new exempted when enabled, driven by the rails pack default). Drop `murphy-pfcb` from `gap_issues`. Keep `status: partial` for the remaining AllowComments-disable nuance.

**Step 6: Commit** `feat(style): SymbolProc exempts lambda/proc under ActiveSupportExtensionsEnabled (murphy-pfcb)`

---

### Task 9: full gate + e2e + Mastodon check

**Step 1: Workspace gates**
```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo +nightly fmt --check
```
All green. `crates/murphy-cli/tests/rails_pack_e2e.rs` must still load the (rebuilt) rails cdylib.

**Step 2: Add an e2e assertion (if cheap)** — extend `rails_pack_e2e.rs` (or a CLI integration test) to lint a fixture containing `->(x) { x.foo }` with `plugins: [murphy-rails]` and assert **no** `Style/SymbolProc` offense (the embedded default.yml flips ASE true via the loader read + `apply_pack_default_layers`). This is the one test that exercises the symbol-read → merge → exemption path end-to-end.

**Step 3: Real-world check (Mastodon)**
```bash
cargo build --release -p murphy-cli
cd /home/ubuntu/mastodon && ~/projects/murphy/target/release/murphy lint 2>/dev/null | grep -c "Style/SymbolProc:"
```
Expected: **0** (was 11). Note: the release `libmurphy_rails.so` must carry the new symbol — it is rebuilt by the same `cargo build --release`.

**Step 4: Commit** any gate fixups.

---

## Done criteria

- `cx.active_support_extensions_enabled()` exists; `CxRaw` offset 242, size 248, ABI stays 4.
- Config parses the key (user); rails pack's embedded `default.yml` is read by the loader and merged by core (std false < pack true < user); user override wins.
- `Style/SymbolProc` exempts lambda/proc/Proc.new when enabled; default-false behavior unchanged (existing tests green).
- Workspace `cargo test` / `clippy` / `fmt` green; rails e2e green; the symbol-read→merge→exemption path has an e2e.
- Mastodon: `Style/SymbolProc` offenses drop 11 → 0.
- `murphy-pfcb` ready to close; the four under-flagging cops remain follow-ups (the flag is now available via `cx`).
