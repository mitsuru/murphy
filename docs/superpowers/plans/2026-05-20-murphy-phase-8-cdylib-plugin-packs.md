# Murphy Phase 8 cdylib Plugin Packs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the Phase 8 MVP for native `cdylib` cop packs: config, builtin pack repackaging, plugin loading, one external example pack, and the A6a/C4 distribution ADR.

**Architecture:** Keep the existing `Cop` trait and lint pipeline inside Murphy, but add a pack layer around `CopRegistry`. External dynamic libraries expose a small C ABI; Murphy wraps registered file-level callbacks in in-process `Cop` adapters and keeps loaded libraries alive for the process lifetime.

**Tech Stack:** Rust 2024, `libloading`, `serde`/`toml`, `assert_cmd`, `tempfile`, existing `rayon` file-level parallelism.

---

## File Structure

- Create: `crates/murphy-core/src/plugin.rs` for C ABI structs, dynamic loading, plugin callback adapter cops, duplicate ID validation, and tests that do not need the CLI.
- Modify: `crates/murphy-core/src/config.rs` to parse top-level `[[cop_packs]]` while preserving strict TOML behavior.
- Modify: `crates/murphy-core/src/registry.rs` to represent builtin cops as a logical `builtin` pack and append loaded plugin cops.
- Modify: `crates/murphy-core/src/lib.rs` to expose only the safe pieces needed by the CLI and tests.
- Modify: `crates/murphy-core/Cargo.toml` to make `libloading` available on Unix targets and add no new always-on runtime dependencies.
- Create: `crates/murphy-example-pack/Cargo.toml` and `crates/murphy-example-pack/src/lib.rs` as the local PoC plugin pack fixture.
- Modify: `crates/murphy-cli/src/main.rs` to preserve all existing behavior while reserved-name checks use the final loaded native cop list.
- Create: `crates/murphy-cli/tests/native_plugin_pack.rs` for missing library, missing symbol, and example pack e2e tests.
- Create: `docs/decisions/0031-native-plugin-pack-abi.md` for A6a/C4 distribution and ABI contract.

## Task 1: Parse `[[cop_packs]]` Configuration

**Files:**
- Modify: `crates/murphy-core/src/config.rs`

- [ ] **Step 1: Write failing config tests**

Add this inside `#[cfg(test)] mod tests` in `crates/murphy-core/src/config.rs`:

```rust
#[test]
fn parses_cop_packs() {
    let cfg = MurphyConfig::from_toml_str(
        r#"
[[cop_packs]]
name = "murphy-example-pack"
path = "packs/murphy-example-pack/libmurphy_example_pack.so"
version = "0.1.0"
"#,
    )
    .expect("config parses");

    assert_eq!(cfg.cop_packs.len(), 1);
    assert_eq!(cfg.cop_packs[0].name, "murphy-example-pack");
    assert_eq!(
        cfg.cop_packs[0].path,
        PathBuf::from("packs/murphy-example-pack/libmurphy_example_pack.so")
    );
    assert_eq!(cfg.cop_packs[0].version, "0.1.0");
}

#[test]
fn cop_packs_default_to_empty() {
    let cfg = MurphyConfig::from_toml_str("").expect("empty config parses");
    assert!(cfg.cop_packs.is_empty());
}

#[test]
fn cop_pack_unknown_fields_are_rejected() {
    let err = MurphyConfig::from_toml_str(
        r#"
[[cop_packs]]
name = "murphy-example-pack"
path = "pack.so"
version = "0.1.0"
checksum = "not-supported-yet"
"#,
    )
    .expect_err("unknown fields remain setup errors");

    assert!(matches!(err, ConfigError::BadToml(_)));
}
```

- [ ] **Step 2: Run the targeted tests and verify failure**

Run: `cargo test -p murphy-core config::tests::parses_cop_packs config::tests::cop_packs_default_to_empty config::tests::cop_pack_unknown_fields_are_rejected`

Expected: compile failure mentioning `no field cop_packs on type MurphyConfig`.

- [ ] **Step 3: Add config types and parsing**

Change the top of `crates/murphy-core/src/config.rs` to include `cop_packs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MurphyConfig {
    pub files: FilesConfig,
    pub cops: CopsConfig,
    pub cop_packs: Vec<CopPackConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CopPackConfig {
    pub name: String,
    pub path: PathBuf,
    pub version: String,
}
```

Update `MurphyToml`:

```rust
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MurphyToml {
    #[serde(default)]
    files: FilesTable,
    #[serde(default)]
    cops: CopsTable,
    #[serde(default)]
    cop_packs: Vec<CopPackConfig>,
}
```

Update `Default for MurphyConfig` and `From<MurphyToml>`:

```rust
impl Default for MurphyConfig {
    fn default() -> Self {
        Self {
            files: FilesConfig {
                include: default_include(),
                exclude: Vec::new(),
            },
            cops: CopsConfig {
                path: default_cops_path(),
                rules: BTreeMap::new(),
            },
            cop_packs: Vec::new(),
        }
    }
}

impl From<MurphyToml> for MurphyConfig {
    fn from(value: MurphyToml) -> Self {
        Self {
            files: FilesConfig {
                include: value.files.include,
                exclude: value.files.exclude,
            },
            cops: CopsConfig {
                path: value.cops.path,
                rules: value.cops.rules,
            },
            cop_packs: value.cop_packs,
        }
    }
}
```

- [ ] **Step 4: Run tests and verify pass**

Run: `cargo test -p murphy-core config::tests::`

Expected: all config tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/murphy-core/src/config.rs
git commit -m "feat: parse native cop pack config"
```

## Task 2: Repackage Built-In Cops as `builtin`

**Files:**
- Modify: `crates/murphy-core/src/registry.rs`

- [x] **Step 1: Write pack metadata tests**

Add this test in `crates/murphy-core/src/registry.rs`:

```rust
#[test]
fn registry_exposes_builtin_pack_metadata() {
    let reg = CopRegistry::native_only();
    assert_eq!(reg.native_pack_names(), &["builtin".to_string()]);
    let names: Vec<&str> = reg.native_cops().iter().map(|c| c.name()).collect();
    assert_eq!(names, EXPECTED_NATIVE_COPS);
}
```

- [x] **Step 2: Run the targeted test and verify behavior**

Run: `cargo test -p murphy-core registry::tests::registry_exposes_builtin_pack_metadata`

Expected: test compiles and passes.

- [x] **Step 3: Add minimal pack metadata while keeping native slice unchanged**

In `CopRegistry`, add a pack name field:

```rust
pub struct CopRegistry {
    native: Vec<Box<dyn Cop>>,
    native_pack_names: Vec<String>,
    mruby_cop_paths: Vec<PathBuf>,
}
```

Update constructors to set `native_pack_names: vec!["builtin".to_string()]`.

Add this method:

```rust
pub fn native_pack_names(&self) -> &[String] {
    &self.native_pack_names
}
```

- [x] **Step 4: Run registry tests**

Run: `cargo test -p murphy-core registry::tests::`

Expected: all registry tests pass and existing native cop order remains unchanged.

- [ ] **Step 5: Commit**

Completed note:

- Added `native_pack_names()` metadata with default `builtin`, and added `discover_includes_builtin_pack_then_configured_native_pack` to verify configured external pack load preserves builtin-first order.
- Verified:
  - `cargo test -p murphy-core registry::tests::registry_exposes_builtin_pack_metadata`
  - `cargo test -p murphy-core registry::tests::discover_includes_builtin_pack_then_configured_native_pack`

```bash
git add crates/murphy-core/src/registry.rs
git commit -m "refactor: model builtin cops as a pack"
```

## Task 3: Add Plugin ABI and Loader

**Files:**
- Create: `crates/murphy-core/src/plugin.rs`
- Modify: `crates/murphy-core/src/lib.rs`
- Modify: `crates/murphy-core/src/registry.rs`
- Modify: `crates/murphy-core/Cargo.toml`

- [x] **Step 1: Write failing duplicate-ID unit test**

Create `crates/murphy-core/src/plugin.rs` with this initial test module and the public API names used by the implementation steps below:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::NoReceiverPuts;

    #[test]
    fn rejects_duplicate_plugin_cop_id() {
        let existing: Vec<Box<dyn crate::Cop>> = vec![Box::new(NoReceiverPuts)];
        let err = validate_plugin_cop_ids(&existing, &["Murphy/NoReceiverPuts".to_string()])
            .expect_err("duplicate cop ID must be rejected");
        assert!(err.contains("duplicate cop ID"));
        assert!(err.contains("Murphy/NoReceiverPuts"));
    }
}
```

Add `mod plugin;` to `crates/murphy-core/src/lib.rs`.

- [x] **Step 2: Run the targeted test and verify failure**

Run: `cargo test -p murphy-core plugin::tests::rejects_duplicate_plugin_cop_id`

Expected: test now compiles and validates duplicate IDs.

- [x] **Step 3: Implement ABI structs, adapter, and duplicate validation**

Add this to `crates/murphy-core/src/plugin.rs`:

```rust
use crate::{Cop, CopContext, Offense, Range, Severity};
use std::collections::BTreeSet;
use std::ffi::c_void;

pub const MURPHY_PLUGIN_ABI_VERSION: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MurphySlice {
    pub ptr: *const u8,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MurphyRange {
    pub start_offset: u32,
    pub end_offset: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MurphyPluginOffense {
    pub cop_name: MurphySlice,
    pub message: MurphySlice,
    pub range: MurphyRange,
    pub severity: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MurphyFileContext {
    pub file: MurphySlice,
    pub source: MurphySlice,
}

pub type MurphyEmitOffense = unsafe extern "C" fn(*mut c_void, MurphyPluginOffense) -> i32;
pub type MurphyRunFile = unsafe extern "C" fn(
    ctx: *const MurphyFileContext,
    sink: *mut c_void,
    emit: MurphyEmitOffense,
) -> i32;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MurphyPluginCopV1 {
    pub size: usize,
    pub name: MurphySlice,
    pub run_file: MurphyRunFile,
}

#[repr(C)]
pub struct MurphyPluginV1 {
    pub size: usize,
    pub cops_ptr: *const MurphyPluginCopV1,
    pub cops_len: usize,
}

unsafe impl Sync for MurphySlice {}
unsafe impl Sync for MurphyRange {}
unsafe impl Sync for MurphyPluginOffense {}
unsafe impl Sync for MurphyFileContext {}
unsafe impl Sync for MurphyPluginCopV1 {}

pub fn validate_plugin_cop_ids(
    existing: &[Box<dyn Cop>],
    plugin_names: &[String],
) -> Result<(), String> {
    let mut seen: BTreeSet<String> = existing.iter().map(|cop| cop.name().to_string()).collect();
    for name in plugin_names {
        if name.is_empty() {
            return Err("plugin registered an empty cop ID".to_string());
        }
        if !seen.insert(name.clone()) {
            return Err(format!("duplicate cop ID registered by plugin: {name}"));
        }
    }
    Ok(())
}

pub struct PluginFileCop {
    name: String,
    run_file: MurphyRunFile,
}

struct OffenseSink<'a> {
    file: &'a str,
    offenses: &'a mut Vec<Offense>,
}

unsafe impl Send for PluginFileCop {}
unsafe impl Sync for PluginFileCop {}

impl PluginFileCop {
    pub fn new(name: String, run_file: MurphyRunFile) -> Self {
        Self { name, run_file }
    }
}

impl Cop for PluginFileCop {
    fn name(&self) -> &str {
        &self.name
    }

    fn inspect_file(&self, ctx: &CopContext<'_>, sink: &mut Vec<Offense>) {
        let file = MurphySlice {
            ptr: ctx.file.as_ptr(),
            len: ctx.file.len(),
        };
        let source = MurphySlice {
            ptr: ctx.source.as_ptr(),
            len: ctx.source.len(),
        };
        let ffi_ctx = MurphyFileContext { file, source };
        let mut ffi_sink = OffenseSink {
            file: ctx.file,
            offenses: sink,
        };
        let sink_ptr = &mut ffi_sink as *mut OffenseSink<'_> as *mut c_void;
        let code = unsafe { (self.run_file)(&ffi_ctx, sink_ptr, emit_offense) };
        if code != 0 {
            ffi_sink.offenses.push(Offense::new(
                ctx.file,
                self.name(),
                Range { start_offset: 0, end_offset: 0 },
                Severity::Error,
                "native plugin callback failed",
            ));
        }
    }

    fn on_call_node(
        &self,
        _node: &ruby_prism::CallNode<'_>,
        _ctx: &CopContext<'_>,
        _sink: &mut Vec<Offense>,
    ) {
    }
}

unsafe extern "C" fn emit_offense(sink: *mut c_void, offense: MurphyPluginOffense) -> i32 {
    let Some(sink) = (sink as *mut OffenseSink<'_>).as_mut() else {
        return 1;
    };
    let Some(cop_name) = slice_to_str(offense.cop_name) else {
        return 1;
    };
    let Some(message) = slice_to_str(offense.message) else {
        return 1;
    };
    if offense.range.end_offset < offense.range.start_offset {
        return 1;
    }
    let severity = match offense.severity {
        1 => Severity::Error,
        _ => Severity::Warning,
    };
    sink.offenses.push(Offense::new(
        sink.file,
        cop_name,
        Range {
            start_offset: offense.range.start_offset,
            end_offset: offense.range.end_offset,
        },
        severity,
        message,
    ));
    0
}

fn slice_to_str(slice: MurphySlice) -> Option<&'static str> {
    if slice.ptr.is_null() {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(slice.ptr, slice.len) };
    std::str::from_utf8(bytes).ok()
}
```

- [x] **Step 4: Run targeted unit test**

Run: `cargo test -p murphy-core plugin::tests::rejects_duplicate_plugin_cop_id`

Expected: test passes.

- [x] **Step 5: Add Unix dynamic loader shell**

Append to `plugin.rs`:

```rust
#[cfg(not(target_os = "windows"))]
pub mod dynamic {
    use super::*;
    use libloading::{Library, Symbol};
    use std::path::Path;

    type AbiVersionFn = unsafe extern "C" fn() -> u32;
    type RegisterFn = unsafe extern "C" fn(*mut MurphyPluginV1) -> i32;

    pub struct LoadedPluginPack {
        pub name: String,
        pub cops: Vec<Box<dyn Cop>>,
        _library: Library,
    }

    pub fn load_plugin_pack(name: &str, path: &Path) -> Result<LoadedPluginPack, String> {
        let library = unsafe { Library::new(path) }
            .map_err(|e| format!("cannot load native cop pack {}: {e}", path.display()))?;
        let abi: Symbol<AbiVersionFn> = unsafe { library.get(b"murphy_plugin_abi_version") }
            .map_err(|e| format!("native cop pack {} is missing murphy_plugin_abi_version: {e}", path.display()))?;
        let got = unsafe { abi() };
        if got != MURPHY_PLUGIN_ABI_VERSION {
            return Err(format!(
                "native cop pack {name} uses ABI version {got}, expected {MURPHY_PLUGIN_ABI_VERSION}"
            ));
        }
        let register: Symbol<RegisterFn> = unsafe { library.get(b"murphy_register_plugin") }
            .map_err(|e| format!("native cop pack {} is missing murphy_register_plugin: {e}", path.display()))?;
        let mut plugin = MurphyPluginV1 {
            size: std::mem::size_of::<MurphyPluginV1>(),
            cops_ptr: std::ptr::null(),
            cops_len: 0,
        };
        let code = unsafe { register(&mut plugin) };
        if code != 0 {
            return Err(format!("native cop pack {name} registration failed with code {code}"));
        }
        let cops = unsafe { std::slice::from_raw_parts(plugin.cops_ptr, plugin.cops_len) };
        let mut boxed: Vec<Box<dyn Cop>> = Vec::new();
        for cop in cops {
            let cop_name = slice_to_str(cop.name)
                .ok_or_else(|| format!("native cop pack {name} registered invalid UTF-8 cop name"))?
                .to_string();
            boxed.push(Box::new(PluginFileCop::new(cop_name, cop.run_file)));
        }
        Ok(LoadedPluginPack {
            name: name.to_string(),
            cops: boxed,
            _library: library,
        })
    }
}
```

- [x] **Step 6: Export module pieces**

In `lib.rs`, add:

```rust
pub use plugin::{MURPHY_PLUGIN_ABI_VERSION, PluginFileCop, validate_plugin_cop_ids};
#[cfg(not(target_os = "windows"))]
pub use plugin::dynamic::{LoadedPluginPack, load_plugin_pack};
```

- [x] **Step 7: Run full core tests**

Run: `cargo test -p murphy-core`

Expected: all core tests pass.

- [x] **Step 8: Commit**

Completed note:

- Added `crates/murphy-core/src/plugin.rs` and exposed loader-related types in `crates/murphy-core/src/lib.rs`.
- Verified with:
  - `cargo test -p murphy-core plugin::tests::rejects_duplicate_plugin_cop_id`
  - `cargo test -p murphy-core plugin::tests`
  - `cargo test -p murphy-core`

```bash
git add crates/murphy-core/src/plugin.rs crates/murphy-core/src/lib.rs crates/murphy-core/Cargo.toml
git commit -m "feat: add native plugin ABI loader"
```

## Task 4: Wire Plugin Packs Into Registry and CLI

**Files:**
- Modify: `crates/murphy-core/src/registry.rs`
- Modify: `crates/murphy-cli/src/main.rs`
- Test: `crates/murphy-cli/tests/native_plugin_pack.rs`

- [x] **Step 1: Write missing-library CLI test**

Create `crates/murphy-cli/tests/native_plugin_pack.rs`:

```rust
use assert_cmd::Command;
use std::fs;
use tempfile::tempdir;

#[test]
fn configured_missing_native_pack_exits_2_with_empty_stdout() {
    let dir = tempdir().expect("create tempdir");
    fs::write(
        dir.path().join("murphy.toml"),
        r#"
[[cop_packs]]
name = "missing-pack"
path = "packs/missing/libmissing_pack.so"
version = "0.1.0"
"#,
    )
    .expect("write config");
    fs::write(dir.path().join("clean.rb"), "# frozen_string_literal: true\n\nx = 1\n")
        .expect("write source");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("lint")
        .arg("clean.rb")
        .assert()
        .code(2);

    assert!(assert.get_output().stdout.is_empty());
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("missing-pack"), "stderr was {stderr:?}");
}
```

- [x] **Step 2: Run test and verify failure**

Run: `cargo test -p murphy-cli --test native_plugin_pack configured_missing_native_pack_exits_2_with_empty_stdout`

Expected: test passes with registry load validation, exiting `2`.

- [x] **Step 3: Load configured packs in `CopRegistry`**

In `registry.rs`, import loader APIs:

```rust
#[cfg(not(target_os = "windows"))]
use crate::{load_plugin_pack, validate_plugin_cop_ids};
```

In `discover_with_config`, after building builtin `native`, iterate `config.cop_packs`:

```rust
let mut native = Self::native_cops_list();
let mut native_pack_names = vec!["builtin".to_string()];

#[cfg(not(target_os = "windows"))]
for pack in &config.cop_packs {
    let path = root.join(&pack.path);
    let loaded = load_plugin_pack(&pack.name, &path).map_err(ConfigError::Io)?;
    let plugin_names: Vec<String> = loaded.cops.iter().map(|cop| cop.name().to_string()).collect();
    validate_plugin_cop_ids(&native, &plugin_names).map_err(ConfigError::Io)?;
    native_pack_names.push(loaded.name);
    native.extend(loaded.cops);
}

#[cfg(target_os = "windows")]
if let Some(pack) = config.cop_packs.first() {
    return Err(ConfigError::Io(format!(
        "native cop pack {:?} is not supported on Windows in Phase 8",
        pack.name
    )));
}

let native = native
    .into_iter()
    .filter(|cop| config.cop_enabled(cop.name()))
    .collect();
```

Return `native_pack_names` in the registry struct.

- [x] **Step 4: Run missing-library test**

Run: `cargo test -p murphy-cli --test native_plugin_pack configured_missing_native_pack_exits_2_with_empty_stdout`

Expected: test passes with exit `2` and empty stdout.

- [x] **Step 5: Run snapshot guard**

Run: `cargo test -p murphy-cli --test integration_snapshot multi_file_lint_matches_committed_snapshot`

Expected: pass; native-only output remains unchanged.

- [x] **Step 6: Commit**

Completed note:

- `registry.rs` now loads configured native packs and tracks pack names (`builtin` then configured entries).
- CLI missing pack behavior now returns exit code `2` with empty stdout.
- Verified with:
  - `cargo test -p murphy-cli --test native_plugin_pack configured_missing_native_pack_exits_2_with_empty_stdout`
  - `cargo test -p murphy-cli --test integration_snapshot multi_file_lint_matches_committed_snapshot`

```bash
git add crates/murphy-core/src/registry.rs crates/murphy-cli/tests/native_plugin_pack.rs
git commit -m "feat: load configured native cop packs"
```

## Task 5: Add `murphy-example-pack` E2E PoC

**Files:**
- Create: `crates/murphy-example-pack/Cargo.toml`
- Create: `crates/murphy-example-pack/src/lib.rs`
- Modify: `crates/murphy-cli/tests/native_plugin_pack.rs`

- [x] **Step 1: Create example pack crate**

Create `crates/murphy-example-pack/Cargo.toml`:

```toml
[package]
name = "murphy-example-pack"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
murphy-core = { path = "../murphy-core" }
```

Create `crates/murphy-example-pack/src/lib.rs`:

```rust
use murphy_core::{
    MURPHY_PLUGIN_ABI_VERSION, MurphyFileContext, MurphyPluginCopV1, MurphyPluginOffense,
    MurphyPluginV1, MurphyRange, MurphySlice,
};
use std::ffi::c_void;

static COP_NAME: &[u8] = b"Example/FileBanner";
static MESSAGE: &[u8] = b"example native plugin ran";

static COPS: [MurphyPluginCopV1; 1] = [MurphyPluginCopV1 {
    size: std::mem::size_of::<MurphyPluginCopV1>(),
    name: MurphySlice {
        ptr: COP_NAME.as_ptr(),
        len: COP_NAME.len(),
    },
    run_file,
}];

#[unsafe(no_mangle)]
pub extern "C" fn murphy_plugin_abi_version() -> u32 {
    MURPHY_PLUGIN_ABI_VERSION
}

#[unsafe(no_mangle)]
pub extern "C" fn murphy_register_plugin(plugin: *mut MurphyPluginV1) -> i32 {
    let Some(plugin) = (unsafe { plugin.as_mut() }) else {
        return 1;
    };
    plugin.size = std::mem::size_of::<MurphyPluginV1>();
    plugin.cops_ptr = COPS.as_ptr();
    plugin.cops_len = COPS.len();
    0
}

unsafe extern "C" fn run_file(
    _ctx: *const MurphyFileContext,
    sink: *mut c_void,
    emit: unsafe extern "C" fn(*mut c_void, MurphyPluginOffense) -> i32,
) -> i32 {
    emit(
        sink,
        MurphyPluginOffense {
            cop_name: MurphySlice {
                ptr: COP_NAME.as_ptr(),
                len: COP_NAME.len(),
            },
            message: MurphySlice {
                ptr: MESSAGE.as_ptr(),
                len: MESSAGE.len(),
            },
            range: MurphyRange {
                start_offset: 0,
                end_offset: 0,
            },
            severity: 0,
        },
    )
}
```

- [x] **Step 2: Export FFI types from core**

If the example crate cannot import the ABI types, update `lib.rs`:

```rust
pub use plugin::{
    MURPHY_PLUGIN_ABI_VERSION, MurphyFileContext, MurphyPluginCopV1, MurphyPluginOffense,
    MurphyPluginV1, MurphyRange, MurphySlice, PluginFileCop, validate_plugin_cop_ids,
};
```

- [x] **Step 3: Build example pack**

Run: `cargo build -p murphy-example-pack`

Expected: build succeeds and creates `target/debug/libmurphy_example_pack.so` on Linux.

- [x] **Step 4: Write e2e test**

Append to `crates/murphy-cli/tests/native_plugin_pack.rs`:

```rust
fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cli crate has parent")
        .parent()
        .expect("crates dir has parent")
        .to_path_buf()
}

#[test]
fn example_native_pack_loads_and_emits_offense() {
    let root = workspace_root();
    let status = std::process::Command::new("cargo")
        .current_dir(&root)
        .args(["build", "-p", "murphy-example-pack"])
        .status()
        .expect("run cargo build for example pack");
    assert!(status.success(), "example pack must build before e2e test");

    let dir = tempdir().expect("create tempdir");
    let dylib = root.join("target/debug/libmurphy_example_pack.so");
    fs::write(
        dir.path().join("murphy.toml"),
        format!(
            "[[cop_packs]]\nname = \"murphy-example-pack\"\npath = {:?}\nversion = \"0.1.0\"\n",
            dylib.to_string_lossy()
        ),
    )
    .expect("write config");
    fs::write(dir.path().join("clean.rb"), "# frozen_string_literal: true\n\nx = 1\n")
        .expect("write source");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("lint")
        .arg("clean.rb")
        .assert()
        .code(1);

    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout is JSON");
    assert!(
        parsed.iter().any(|offense| offense["cop_name"] == "Example/FileBanner"),
        "expected example plugin offense, got {parsed:?}"
    );
}
```

- [x] **Step 5: Run e2e test**

Run: `cargo test -p murphy-cli --test native_plugin_pack example_native_pack_loads_and_emits_offense -- --nocapture`

Expected: test passes and JSON contains `Example/FileBanner`.

- [x] **Step 6: Run all CLI tests**

Run: `cargo test -p murphy-cli`

Expected: all CLI tests pass.

- [x] **Step 7: Commit**

Completed note:

- Added `murphy-example-pack` cdylib fixture and `native_plugin_pack` e2e coverage.
- Verified with:
  - `cargo test -p murphy-cli --test native_plugin_pack example_native_pack_loads_and_emits_offense -- --nocapture`
  - `cargo test -p murphy-cli`
  - `cargo test -p murphy-core`

```bash
git add crates/murphy-example-pack crates/murphy-cli/tests/native_plugin_pack.rs crates/murphy-core/src/lib.rs Cargo.lock
git commit -m "test: add example native cop pack"
```

## Task 6: Add A6a/C4 ADR and Close Issues

**Files:**
- Create: `docs/decisions/0031-native-plugin-pack-abi.md`

- [x] **Step 1: Write ADR**

Create `docs/decisions/0031-native-plugin-pack-abi.md`:

```markdown
# ADR 0031 - Native plugin pack ABI and distribution contract

- Date: 2026-05-20
- Status: Accepted
- Issues: `murphy-fmw.1.3`, `murphy-fmw.1.2`, `murphy-fmw.1.5`

## Context

Phase 8 adds native cop packs so third-party Rust cops can be distributed without
rebuilding Murphy. Dynamic loading gives pack authors flexibility, but it also
turns ABI shape, cop identity, and versioning into public contracts.

## Decision

Native packs are loaded as `cdylib` libraries through a C-compatible ABI. The
initial ABI version is `1`. A pack must export `murphy_plugin_abi_version` and
`murphy_register_plugin`. Murphy rejects missing symbols, ABI mismatches,
registration failures, duplicate cop IDs, invalid UTF-8 names, and invalid ranges
as setup errors.

Plugin callbacks may run concurrently on multiple OS threads because Murphy keeps
file-level `rayon` parallelism. Pack authors must synchronize shared mutable
state and must not retain host-owned pointers after a callback returns. Rust
panics must not unwind across the plugin ABI boundary.

## Distribution Contract

- Cop IDs are stable public identifiers.
- Renaming a cop means adding a new ID and deprecating the old one.
- Default severity changes require a major pack version bump.
- Config keys are additive within a major version.
- Removing a config key or changing its meaning requires a major version bump.
- Plugin ABI version and pack semantic version are separate contracts.
- Native plugin packs are trusted code with the privileges of the Murphy process.

## Consequences

The C ABI is more verbose than a Rust trait-object boundary, but avoids relying
on Rust ABI stability. Node-level plugin APIs are deferred until Murphy has a
versioned node-handle ABI; the first plugin ABI is file-level.
```

- [x] **Step 2: Run quality gates**

Run: `cargo test`

Expected: all workspace tests pass.

- [x] **Step 3: Update beads**

Run:

```bash
bd close murphy-fmw.1.3 --reason "Implemented native cdylib plugin ABI and loader MVP"
bd close murphy-fmw.1.2 --reason "Implemented local native cop pack loading and example pack PoC"
bd close murphy-fmw.1.5 --reason "Documented cop pack ABI and versioning contract in ADR 0031"
```

Expected: all three issues close successfully.

- [ ] **Step 4: Commit ADR and any final fixes**

```bash
git add docs/decisions/0031-native-plugin-pack-abi.md .beads/issues.jsonl
git commit -m "docs: define native cop pack contract"
```

- [ ] **Step 5: Push session work**

```bash
git pull --rebase
bd dolt push
git push
git status
```

Expected: branch is up to date with `origin/main` and working tree is clean.

## Self-Review

- Spec coverage: config, builtin pack, `cdylib` loader, thread-safety contract, example pack, error handling, duplicate IDs, and ADR are covered by Tasks 1-6.
- Type consistency: plugin offense emission carries the host file path through `OffenseSink`, so emitted offenses preserve the lint target file field.
- No dependency resolver, registry fetch, Windows support, node-level ABI, or persistent cache integration is included; these are explicitly non-goals for this MVP.
