# Plugin-Bundled Default.yml Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Each plugin pack bundles its own RuboCop default.yml; murphy-core loads it as a base config layer so `is_cop_disabled_by_default()` can be deleted and defaults become data-driven.

**Architecture:** `murphy-std` exposes `BUNDLED_DEFAULTS_YAML` (from `include_str!`). `murphy-cli` passes it to `MurphyConfig::load_with_defaults(root, yaml)`. `murphy-core` parses it into `DefaultCopsData` stored in `MurphyConfig.base_defaults`. Query methods (`cop_enabled`, `cop_options_json`, `severity_override`, `cop_applies_to_file`) consult base_defaults before falling back. The registry filter also consults `PluginCopV1.default_enabled` (already in ABI) for dynamic packs. ABI version stays at 4 — no struct changes.

**Tech Stack:** Rust, yaml-rust2, serde_json, `tristate_from_wire` from murphy-plugin-api, `include_str!` for the YAML embed.

---

### Task 1: Download and bundle `rubocop/config/default.yml` in murphy-std

**Files:**
- Create: `crates/murphy-std/config/default.yml`
- Modify: `crates/murphy-std/src/lib.rs`

**Step 1: Download the file**

```bash
mkdir -p crates/murphy-std/config
curl -s "https://raw.githubusercontent.com/rubocop/rubocop/master/config/default.yml" \
  -o crates/murphy-std/config/default.yml
```

Verify it downloaded (should be ~6000+ lines):
```bash
wc -l crates/murphy-std/config/default.yml
```
Expected: 6000+ lines.

**Step 2: Add BUNDLED_DEFAULTS_YAML constant to murphy-std/src/lib.rs**

After the existing `pub mod cops;` line, add:

```rust
/// RuboCop's built-in default configuration, embedded at compile time.
/// Passed to `MurphyConfig::load_with_defaults` so defaults are data-driven
/// rather than hardcoded in `is_cop_disabled_by_default`.
pub const BUNDLED_DEFAULTS_YAML: &str = include_str!("../config/default.yml");
```

**Step 3: Verify it compiles**

```bash
cargo build -p murphy-std 2>&1 | grep -E "^error|Compiling murphy-std"
```
Expected: `Compiling murphy-std` followed by no errors.

**Step 4: Commit**

```bash
git add crates/murphy-std/config/default.yml crates/murphy-std/src/lib.rs
git commit -m "feat(murphy-std): bundle rubocop/config/default.yml as BUNDLED_DEFAULTS_YAML"
```

---

### Task 2: Add `DefaultCopsData` struct and YAML parser to murphy-core

**Files:**
- Modify: `crates/murphy-core/src/config.rs` (add structs + `DefaultCopsData::from_yaml`)
- Test: same file (inline `#[test]` module)

**Step 1: Write failing tests first**

In `crates/murphy-core/src/config.rs`, in the `#[cfg(test)]` module at the bottom, add:

```rust
#[test]
fn default_cops_data_parses_enabled_false() {
    let yaml = "Style/Foo:\n  Enabled: false\n  EnforcedStyle: single_quotes\n";
    let data = DefaultCopsData::from_yaml(yaml);
    let rule = data.cop_rules.get("Style/Foo").expect("rule exists");
    assert_eq!(rule.enabled, Some(false));
    assert_eq!(
        rule.options.get("EnforcedStyle"),
        Some(&serde_json::Value::String("single_quotes".to_string()))
    );
}

#[test]
fn default_cops_data_parses_allcops_include_exclude() {
    let yaml = "AllCops:\n  Include:\n    - '**/*.rb'\n    - '**/Gemfile'\n  Exclude:\n    - 'vendor/**'\n";
    let data = DefaultCopsData::from_yaml(yaml);
    assert!(data.allcops_include.contains(&"**/*.rb".to_string()));
    assert!(data.allcops_include.contains(&"**/Gemfile".to_string()));
    assert_eq!(data.allcops_exclude, vec!["vendor/**"]);
}

#[test]
fn default_cops_data_strips_metadata_keys() {
    let yaml = "Style/Foo:\n  Description: 'Some cop'\n  Enabled: true\n  VersionAdded: '1.0'\n  EnforcedStyle: compact\n";
    let data = DefaultCopsData::from_yaml(yaml);
    let rule = data.cop_rules.get("Style/Foo").expect("rule");
    assert!(!rule.options.contains_key("Description"), "Description must not be in options");
    assert!(!rule.options.contains_key("VersionAdded"), "VersionAdded must not be in options");
    assert!(rule.options.contains_key("EnforcedStyle"), "EnforcedStyle must be in options");
}

#[test]
fn default_cops_data_parses_per_cop_include_exclude() {
    let yaml = "Bundler/Foo:\n  Enabled: true\n  Include:\n    - '**/Gemfile'\n  Exclude:\n    - 'vendor/**'\n";
    let data = DefaultCopsData::from_yaml(yaml);
    let rule = data.cop_rules.get("Bundler/Foo").expect("rule");
    assert_eq!(rule.include, vec!["**/Gemfile"]);
    assert_eq!(rule.exclude, vec!["vendor/**"]);
}
```

Run tests (expect failures because structs don't exist yet):
```bash
cargo test -p murphy-core default_cops_data 2>&1 | grep -E "FAILED|error\[E"
```
Expected: compile errors about unknown type `DefaultCopsData`.

**Step 2: Add structs and parser**

In `crates/murphy-core/src/config.rs`, before the `fn default_include()` function, add:

```rust
/// Metadata keys from RuboCop's default.yml that are NOT cop options —
/// documentation or cross-cutting concerns. These are stripped when
/// building the options map so cops don't receive them via JSON.
const METADATA_KEYS: &[&str] = &[
    "Description",
    "VersionAdded",
    "VersionChanged",
    "VersionRemoved",
    "StyleGuide",
    "References",
    "Safe",
    "SafeAutoCorrect",
];

/// Per-cop defaults extracted from a bundled `default.yml`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DefaultCopRule {
    pub enabled: Option<bool>,
    pub severity: Option<Severity>,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub options: BTreeMap<String, serde_json::Value>,
}

/// The full set of defaults parsed from a bundled `default.yml`
/// (e.g. `rubocop/config/default.yml` in murphy-std).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DefaultCopsData {
    /// `AllCops.Include` patterns from the YAML.
    pub allcops_include: Vec<String>,
    /// `AllCops.Exclude` patterns from the YAML.
    pub allcops_exclude: Vec<String>,
    /// Per-cop defaults keyed by cop name.
    pub cop_rules: BTreeMap<String, DefaultCopRule>,
}

impl DefaultCopsData {
    /// Parse a RuboCop-format default.yml string into structured defaults.
    ///
    /// Unrecognised top-level keys (not `AllCops`, not cop names) are
    /// silently ignored — the YAML may have comments or future extensions.
    /// Parse failures are silently skipped; a malformed bundled YAML should
    /// be caught in tests, not at runtime.
    pub fn from_yaml(text: &str) -> Self {
        use yaml_rust2::{Yaml, YamlLoader};

        let docs = match YamlLoader::load_from_str(text) {
            Ok(d) => d,
            Err(_) => return Self::default(),
        };
        let doc = match docs.into_iter().next() {
            Some(Yaml::Hash(h)) => h,
            _ => return Self::default(),
        };

        let mut result = Self::default();

        for (key, value) in doc {
            let Yaml::String(section) = key else { continue };

            if section == "AllCops" {
                if let Yaml::Hash(all_cops) = value {
                    if let Some(inc) = all_cops.get(&Yaml::String("Include".to_string())) {
                        result.allcops_include = yaml_string_list(inc);
                    }
                    if let Some(exc) = all_cops.get(&Yaml::String("Exclude".to_string())) {
                        result.allcops_exclude = yaml_string_list(exc);
                    }
                }
                continue;
            }

            // Treat as a cop section (e.g. "Style/Foo").
            if let Yaml::Hash(cop_map) = value {
                let rule = parse_default_cop_rule(cop_map);
                result.cop_rules.insert(section, rule);
            }
        }

        result
    }
}

fn parse_default_cop_rule(map: yaml_rust2::yaml::Hash) -> DefaultCopRule {
    use yaml_rust2::Yaml;
    let mut rule = DefaultCopRule::default();
    for (key, value) in map {
        let Yaml::String(k) = key else { continue };
        match k.as_str() {
            "Enabled" => {
                if let Yaml::Boolean(b) = value {
                    rule.enabled = Some(b);
                }
            }
            "Severity" => {
                if let Yaml::String(s) = value {
                    rule.severity = match s.as_str() {
                        "warning" => Some(Severity::Warning),
                        "error" => Some(Severity::Error),
                        _ => None,
                    };
                }
            }
            "Include" => {
                rule.include = yaml_string_list(&value);
            }
            "Exclude" => {
                rule.exclude = yaml_string_list(&value);
            }
            other if METADATA_KEYS.contains(&other) => {
                // Skip documentation/metadata keys — not cop options.
            }
            other => {
                if let Some(json_val) = yaml_to_json(value) {
                    rule.options.insert(other.to_string(), json_val);
                }
            }
        }
    }
    rule
}
```

**Step 3: Run tests to verify they pass**

```bash
cargo test -p murphy-core default_cops_data 2>&1 | grep -E "^test result|FAILED"
```
Expected: `test result: ok. 4 passed`.

**Step 4: Commit**

```bash
git add crates/murphy-core/src/config.rs
git commit -m "feat(murphy-core): add DefaultCopsData + parser for bundled default.yml"
```

---

### Task 3: Add `base_defaults` to `MurphyConfig` + `from_yaml_str_raw` + `with_defaults`

**Files:**
- Modify: `crates/murphy-core/src/config.rs`

**Step 1: Write failing tests**

Add to the test module:

```rust
#[test]
fn with_defaults_inherits_allcops_include_when_user_has_none() {
    let defaults = "AllCops:\n  Include:\n    - '**/*.rb'\n    - '**/Gemfile'\n  Exclude:\n    - 'vendor/**'\n";
    let cfg = MurphyConfig::with_defaults("", defaults).expect("parses");
    assert!(cfg.files.include.contains(&"**/Gemfile".to_string()));
    assert!(cfg.files.exclude.contains(&"vendor/**".to_string()));
}

#[test]
fn with_defaults_user_allcops_include_overrides_defaults() {
    let defaults = "AllCops:\n  Include:\n    - '**/*.rb'\n    - '**/Gemfile'\n";
    let user = "AllCops:\n  Include:\n    - 'lib/**/*.rb'\n";
    let cfg = MurphyConfig::with_defaults(user, defaults).expect("parses");
    assert_eq!(cfg.files.include, vec!["lib/**/*.rb"]);
    assert!(!cfg.files.include.contains(&"**/Gemfile".to_string()));
}

#[test]
fn with_defaults_populates_base_defaults() {
    let defaults = "Style/Foo:\n  Enabled: false\n  EnforcedStyle: compact\n";
    let cfg = MurphyConfig::with_defaults("", defaults).expect("parses");
    let rule = cfg.base_defaults.cop_rules.get("Style/Foo").expect("rule");
    assert_eq!(rule.enabled, Some(false));
}
```

Run: expect compile errors about `with_defaults`, `base_defaults`.

**Step 2: Refactor `from_yaml_str` into `from_yaml_str_raw` internal helper**

Rename the existing `from_yaml_str` body to a private helper that also returns the `saw_include` / `saw_exclude` flags. Then restore the public API:

```rust
impl MurphyConfig {
    // Keep from_yaml_str for backward compat (tests, discovery, registry).
    // It returns a config with empty base_defaults and Murphy's own include default.
    pub fn from_yaml_str(text: &str) -> Result<Self, ConfigError> {
        let (cfg, _, _) = Self::from_yaml_str_raw(text)?;
        Ok(cfg)
    }

    fn from_yaml_str_raw(text: &str) -> Result<(Self, bool, bool), ConfigError> {
        // ... (move the existing from_yaml_str body here)
        // return Ok((config, saw_include, saw_exclude))
    }
```

The full existing body of `from_yaml_str` becomes the body of `from_yaml_str_raw`, with these changes:
- Return type: `Result<(Self, bool, bool), ConfigError>`
- The final `Ok(MurphyConfig { ... })` becomes `Ok((MurphyConfig { base_defaults: DefaultCopsData::default(), ... }, saw_include, saw_exclude))`
- `saw_exclude` local variable must be added (mirrors `saw_include`):
  - Initialize `let mut saw_exclude = false;` before the loop
  - Set `saw_exclude = true;` when AllCops.Exclude is parsed

Also add `pub base_defaults: DefaultCopsData` to the `MurphyConfig` struct.

**Step 3: Add `MurphyConfig::with_defaults`**

```rust
/// Parse user YAML, then overlay bundled `defaults_yaml` as a base layer.
/// User settings always win; the defaults fill in missing values.
///
/// Call this from the host (murphy-cli) instead of `from_yaml_str` when
/// a pack has provided a bundled default.yml.
pub fn with_defaults(user_yaml: &str, defaults_yaml: &str) -> Result<Self, ConfigError> {
    let (mut cfg, saw_include, saw_exclude) = Self::from_yaml_str_raw(user_yaml)?;
    let defaults = DefaultCopsData::from_yaml(defaults_yaml);

    if !saw_include && !defaults.allcops_include.is_empty() {
        cfg.files.include = defaults.allcops_include.clone();
    }
    if !saw_exclude && !defaults.allcops_exclude.is_empty() {
        cfg.files.exclude = defaults.allcops_exclude.clone();
    }
    cfg.base_defaults = defaults;
    Ok(cfg)
}
```

**Step 4: Run tests**

```bash
cargo test -p murphy-core 2>&1 | grep -E "^test result|FAILED"
```
Expected: all passing (existing tests still use `from_yaml_str` which has `base_defaults = Default::default()`).

**Step 5: Commit**

```bash
git add crates/murphy-core/src/config.rs
git commit -m "feat(murphy-core): add MurphyConfig::with_defaults + base_defaults field"
```

---

### Task 4: Add `load_with_defaults` to `MurphyConfig` + update CLI callers

**Files:**
- Modify: `crates/murphy-core/src/config.rs` (add `load_with_defaults`)
- Modify: `crates/murphy-cli/src/main.rs` (update two `MurphyConfig::load` calls)
- Modify: `crates/murphy-cli/src/cops.rs` (update one `MurphyConfig::load` call)
- Modify: `crates/murphy-cli/src/lsp.rs` (update one `MurphyConfig::load` call)

**Step 1: Add `load_with_defaults` to config.rs**

```rust
/// Like [`Self::load`] but merges bundled `defaults_yaml` as a base layer
/// before the user's `.murphy.yml`.
///
/// The host (murphy-cli) calls this with the pack's `BUNDLED_DEFAULTS_YAML`
/// so the defaults are data-driven.
pub fn load_with_defaults(root: &Path, defaults_yaml: &str) -> Result<Self, ConfigError> {
    let config_path = root.join(".murphy.yml");
    let user_yaml = match std::fs::read_to_string(&config_path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => {
            return Err(ConfigError::Io(format!(
                "cannot read {}: {e}",
                config_path.display()
            )));
        }
    };
    Self::with_defaults(&user_yaml, defaults_yaml)
}
```

**Step 2: Update `crates/murphy-cli/src/main.rs`**

Find the two `MurphyConfig::load` calls (lines ~897 and ~965). Change both from:
```rust
let config = MurphyConfig::load(Path::new(".")).map_err(|e| AppError::setup(e.to_string()))?;
```
to:
```rust
let config = MurphyConfig::load_with_defaults(Path::new("."), murphy_std::BUNDLED_DEFAULTS_YAML)
    .map_err(|e| AppError::setup(e.to_string()))?;
```

For the lint `root` variant (line ~965):
```rust
let config = MurphyConfig::load(root).map_err(|e| AppError::setup(e.to_string()))?;
```
becomes:
```rust
let config = MurphyConfig::load_with_defaults(root, murphy_std::BUNDLED_DEFAULTS_YAML)
    .map_err(|e| AppError::setup(e.to_string()))?;
```

**Step 3: Update `crates/murphy-cli/src/cops.rs`**

Line ~66:
```rust
let config = MurphyConfig::load(Path::new(".")).map_err(|e| AppError::setup(e.to_string()))?;
```
→
```rust
let config = MurphyConfig::load_with_defaults(Path::new("."), murphy_std::BUNDLED_DEFAULTS_YAML)
    .map_err(|e| AppError::setup(e.to_string()))?;
```

**Step 4: Update `crates/murphy-cli/src/lsp.rs`**

Line ~14:
```rust
MurphyConfig::load(Path::new(".")).map_err(|super::AppError::setup(e.to_string()))?;
```
→
```rust
MurphyConfig::load_with_defaults(Path::new("."), murphy_std::BUNDLED_DEFAULTS_YAML)
    .map_err(|e| super::AppError::setup(e.to_string()))?;
```

**Step 5: Build and run tests**

```bash
cargo build -p murphy-cli 2>&1 | grep -E "^error"
cargo test --workspace 2>&1 | grep -E "^test result|FAILED"
```
Expected: no errors, all tests passing.

**Step 6: Commit**

```bash
git add crates/murphy-core/src/config.rs crates/murphy-cli/src/main.rs \
        crates/murphy-cli/src/cops.rs crates/murphy-cli/src/lsp.rs
git commit -m "feat(murphy-cli): use load_with_defaults with BUNDLED_DEFAULTS_YAML"
```

---

### Task 5: Update `cop_enabled` + add `cop_enabled_with_cop_default` + delete `is_cop_disabled_by_default`

**Files:**
- Modify: `crates/murphy-core/src/config.rs`

**Step 1: Write failing tests**

Add to the test module:

```rust
#[test]
fn cop_enabled_uses_base_defaults_enabled_false() {
    let defaults = "Style/Foo:\n  Enabled: false\n";
    let cfg = MurphyConfig::with_defaults("", defaults).expect("parses");
    assert!(!cfg.cop_enabled("Style/Foo"), "should be disabled by default");
    assert!(cfg.cop_enabled("Style/Bar"), "unknown cop defaults to enabled");
}

#[test]
fn cop_enabled_user_overrides_base_defaults() {
    let defaults = "Style/Foo:\n  Enabled: false\n";
    let user = "Style/Foo:\n  Enabled: true\n";
    let cfg = MurphyConfig::with_defaults(user, defaults).expect("parses");
    assert!(cfg.cop_enabled("Style/Foo"), "user explicit enabled wins");
}

#[test]
fn cop_enabled_with_cop_default_uses_abi_tristate() {
    let cfg = MurphyConfig::with_defaults("", "").expect("parses");
    // cop_default Some(false): dynamic pack cop disabled by default
    assert!(!cfg.cop_enabled_with_cop_default("Rails/Foo", Some(false)));
    // cop_default Some(true): dynamic pack cop enabled by default
    assert!(cfg.cop_enabled_with_cop_default("Rails/Foo", Some(true)));
    // cop_default None: no opinion → base_defaults → true
    assert!(cfg.cop_enabled_with_cop_default("Rails/Unknown", None));
}
```

Run: expect failures.

**Step 2: Update `cop_enabled` and add `cop_enabled_with_cop_default`**

Replace the existing `cop_enabled` body and add the new method:

```rust
pub fn cop_enabled(&self, name: &str) -> bool {
    self.cop_enabled_with_cop_default(name, None)
}

/// Like `cop_enabled` but also accepts the cop's ABI `default_enabled`
/// tristate (from `PluginCopV1.default_enabled`) as a third fallback layer.
///
/// Layer order (first Some wins):
///   1. User `.murphy.yml` explicit `Enabled:`
///   2. Bundled `base_defaults` from pack's default.yml
///   3. `cop_default` from `PluginCopV1.default_enabled` (dynamic pack ABI)
///   4. `true` (enabled by default)
pub fn cop_enabled_with_cop_default(&self, name: &str, cop_default: Option<bool>) -> bool {
    // 1. User explicit
    if let Some(e) = self.cops.rules.get(name).and_then(|r| r.enabled) {
        return e;
    }
    // 2. Bundled base defaults
    if let Some(e) = self.base_defaults.cop_rules.get(name).and_then(|r| r.enabled) {
        return e;
    }
    // 3. PluginCopV1.default_enabled from dynamic pack ABI
    if let Some(e) = cop_default {
        return e;
    }
    // 4. Enabled by default
    true
}
```

**Step 3: Delete `is_cop_disabled_by_default`**

Remove the entire `fn is_cop_disabled_by_default(name: &str) -> bool { ... }` function (lines ~471–606 in the current file).

Also delete these now-stale tests:
- `cop_enabled_is_false_for_rails_cops_disabled_by_default`
- `cop_enabled_can_override_default_for_rails_cop`

**Step 4: Run tests**

```bash
cargo test -p murphy-core 2>&1 | grep -E "^test result|FAILED"
```
Expected: all passing. The Rails cops that were in the hardcoded list will now default to `true` (enabled) unless they appear in the bundled YAML — that's correct since rubocop-rails defaults come from murphy-rails' future bundled YAML (follow-up issue).

**Step 5: Commit**

```bash
git add crates/murphy-core/src/config.rs
git commit -m "feat(murphy-core): layer base_defaults in cop_enabled; delete is_cop_disabled_by_default"
```

---

### Task 6: Update `cop_options_json`, `severity_override`, `cop_applies_to_file` to layer base_defaults

**Files:**
- Modify: `crates/murphy-core/src/config.rs`

**Step 1: Write failing tests**

```rust
#[test]
fn cop_options_json_merges_base_defaults_under_user_options() {
    let defaults = "Style/Foo:\n  EnforcedStyle: compact\n  MaxLength: 120\n";
    let user = "Style/Foo:\n  EnforcedStyle: expanded\n";
    let cfg = MurphyConfig::with_defaults(user, defaults).expect("parses");
    let json = cfg.cop_options_json("Style/Foo");
    let parsed: serde_json::Value = serde_json::from_slice(&json).expect("valid JSON");
    // User wins for EnforcedStyle
    assert_eq!(parsed["EnforcedStyle"], "expanded");
    // Default applies for MaxLength (user didn't set it)
    assert_eq!(parsed["MaxLength"], 120);
}

#[test]
fn severity_override_uses_base_defaults_when_user_absent() {
    let defaults = "Bundler/Foo:\n  Severity: warning\n";
    let cfg = MurphyConfig::with_defaults("", defaults).expect("parses");
    use crate::Severity;
    assert_eq!(cfg.severity_override("Bundler/Foo"), Some(Severity::Warning));
}

#[test]
fn severity_override_user_wins_over_base_defaults() {
    let defaults = "Bundler/Foo:\n  Severity: warning\n";
    let user = "Bundler/Foo:\n  Severity: error\n";
    let cfg = MurphyConfig::with_defaults(user, defaults).expect("parses");
    use crate::Severity;
    assert_eq!(cfg.severity_override("Bundler/Foo"), Some(Severity::Error));
}

#[test]
fn cop_applies_to_file_uses_base_defaults_include() {
    let defaults = "Bundler/Foo:\n  Include:\n    - '**/Gemfile'\n    - '**/*.gemspec'\n";
    let cfg = MurphyConfig::with_defaults("", defaults).expect("parses");
    assert!(cfg.cop_applies_to_file("Bundler/Foo", std::path::Path::new("Gemfile")));
    assert!(!cfg.cop_applies_to_file("Bundler/Foo", std::path::Path::new("app/models/user.rb")));
}

#[test]
fn cop_applies_to_file_user_include_overrides_base_defaults() {
    let defaults = "Bundler/Foo:\n  Include:\n    - '**/Gemfile'\n";
    let user = "Bundler/Foo:\n  Include:\n    - 'custom/**'\n";
    let cfg = MurphyConfig::with_defaults(user, defaults).expect("parses");
    assert!(cfg.cop_applies_to_file("Bundler/Foo", std::path::Path::new("custom/file.rb")));
    assert!(!cfg.cop_applies_to_file("Bundler/Foo", std::path::Path::new("Gemfile")));
}
```

Run: expect failures.

**Step 2: Update `cop_options_json`**

Replace:
```rust
pub fn cop_options_json(&self, name: &str) -> Vec<u8> {
    let Some(rule) = self.cops.rules.get(name) else {
        return b"{}".to_vec();
    };
    serde_json::to_vec(&rule.options).unwrap_or_else(|_| b"{}".to_vec())
}
```
With:
```rust
pub fn cop_options_json(&self, name: &str) -> Vec<u8> {
    // Start from base defaults, then overlay user options (user wins per key).
    let mut merged = self.base_defaults.cop_rules
        .get(name)
        .map(|r| r.options.clone())
        .unwrap_or_default();
    if let Some(rule) = self.cops.rules.get(name) {
        merged.extend(rule.options.clone());
    }
    serde_json::to_vec(&merged).unwrap_or_else(|_| b"{}".to_vec())
}
```

**Step 3: Update `severity_override`**

Replace:
```rust
pub fn severity_override(&self, name: &str) -> Option<Severity> {
    self.cops.rules.get(name).and_then(|rule| rule.severity)
}
```
With:
```rust
pub fn severity_override(&self, name: &str) -> Option<Severity> {
    self.cops.rules.get(name).and_then(|r| r.severity)
        .or_else(|| self.base_defaults.cop_rules.get(name).and_then(|r| r.severity))
}
```

**Step 4: Update `cop_applies_to_file`**

Replace the existing body with:
```rust
pub fn cop_applies_to_file(&self, name: &str, file: &Path) -> bool {
    let file = file.strip_prefix(".").unwrap_or(file);

    // User-level Include/Exclude takes precedence if set.
    if let Some(rule) = self.cops.rules.get(name) {
        if !rule.include.is_empty() || !rule.exclude.is_empty() {
            return (rule.include.is_empty() || globset_matches(&rule.include, file))
                && (rule.exclude.is_empty() || !globset_matches(&rule.exclude, file));
        }
    }

    // Fall through to base_defaults per-cop Include/Exclude
    // (e.g. Bundler cops apply only to Gemfile/gemspec by default).
    if let Some(default_rule) = self.base_defaults.cop_rules.get(name) {
        if !default_rule.include.is_empty() || !default_rule.exclude.is_empty() {
            return (default_rule.include.is_empty()
                || globset_matches(&default_rule.include, file))
                && (default_rule.exclude.is_empty()
                    || !globset_matches(&default_rule.exclude, file));
        }
    }

    true
}
```

**Step 5: Run tests**

```bash
cargo test -p murphy-core 2>&1 | grep -E "^test result|FAILED"
```
Expected: all passing.

**Step 6: Commit**

```bash
git add crates/murphy-core/src/config.rs
git commit -m "feat(murphy-core): layer base_defaults in cop_options_json, severity_override, cop_applies_to_file"
```

---

### Task 7: Update `registry.rs` cop filter to use `cop_enabled_with_cop_default`

**Files:**
- Modify: `crates/murphy-core/src/registry.rs`

The registry filter at line ~218 calls `config.cop_enabled(&name)`. For dynamic pack cops, their `PluginCopV1.default_enabled` byte encodes the tristate (from `#[cop(default_enabled = false)]`). We need this as the third fallback.

**Step 1: Update the filter closure**

The import `murphy_plugin_api::tristate_from_wire` is already pub-exported. Add it to the use list in registry.rs if not present:

```rust
use murphy_plugin_api::tristate_from_wire;  // add if missing
```

Change the filter (lines ~218–228):
```rust
let cops_ptrs: Vec<NonNull<PluginCopV1>> = all_cops_ptrs
    .iter()
    .filter(|cop| {
        let cop = unsafe { cop.as_ref() };
        let name_bytes = unsafe { cop.name.as_bytes() };
        let name = String::from_utf8_lossy(name_bytes);
        config.cop_enabled(&name)
            && cop_supports_target_ruby_version(cop, config.target_ruby_version)
    })
    .copied()
    .collect();
```
to:
```rust
let cops_ptrs: Vec<NonNull<PluginCopV1>> = all_cops_ptrs
    .iter()
    .filter(|cop| {
        let cop = unsafe { cop.as_ref() };
        let name_bytes = unsafe { cop.name.as_bytes() };
        let name = String::from_utf8_lossy(name_bytes);
        let cop_default = murphy_plugin_api::tristate_from_wire(cop.default_enabled);
        config.cop_enabled_with_cop_default(&name, cop_default)
            && cop_supports_target_ruby_version(cop, config.target_ruby_version)
    })
    .copied()
    .collect();
```

**Step 2: Build and test**

```bash
cargo test -p murphy-core 2>&1 | grep -E "^test result|FAILED"
cargo test -p murphy-cli 2>&1 | grep -E "^test result|FAILED"
```
Expected: all passing.

**Step 3: Commit**

```bash
git add crates/murphy-core/src/registry.rs
git commit -m "feat(murphy-core): registry filter uses cop_enabled_with_cop_default for PluginCopV1 tristate"
```

---

### Task 8: Update `cli/cops.rs` Status + `warn_user_enabled_disabled`

**Files:**
- Modify: `crates/murphy-cli/src/cops.rs`

**Step 1: Add `DisabledDefault` Status variant**

In the `Status` enum, add:
```rust
enum Status {
    Enabled,
    DisabledDefault,           // new
    DisabledArenaMigration,
    DisabledUserConfig,
}
```

In `Display`:
```rust
Status::DisabledDefault => f.write_str("disabled: default"),
```

**Step 2: Update the status determination in `list_with_format`**

The current status logic in the `for (cop, pack_name)` loop:
```rust
let status = if config.cop_enabled(&name) {
    Status::Enabled
} else {
    Status::DisabledUserConfig
};
```

Replace with:
```rust
let cop_default = murphy_plugin_api::tristate_from_wire(cop.default_enabled);
let status = if config.cop_enabled_with_cop_default(&name, cop_default) {
    Status::Enabled
} else if config.cops.rules.get(&name).and_then(|r| r.enabled) == Some(false) {
    // User explicitly disabled it
    Status::DisabledUserConfig
} else {
    // Disabled by base_defaults or PluginCopV1.default_enabled
    Status::DisabledDefault
};
```

**Step 3: Update `warn_user_enabled_disabled`**

The existing function iterates `murphy_std::DISABLED_COPS` (currently empty). Update it to use `base_defaults` and `PluginCopV1.default_enabled` from the registry:

```rust
pub fn warn_user_enabled_disabled(config: &MurphyConfig, registry: &CopRegistry) {
    for (cop, _) in registry.all_cops_with_packs() {
        let name = String::from_utf8_lossy(unsafe { cop.name.as_bytes() });
        let cop_default = murphy_plugin_api::tristate_from_wire(cop.default_enabled);
        let disabled_by_default = cop_default == Some(false)
            || config
                .base_defaults
                .cop_rules
                .get(name.as_ref())
                .and_then(|r| r.enabled)
                == Some(false);
        if disabled_by_default && config.is_explicitly_enabled(name.as_ref()) {
            eprintln!(
                "warning: cop `{name}` is disabled by default; \
                 `Enabled: true` in .murphy.yml is honoured but the cop will not run"
            );
        }
    }
}
```

Update all callers of `warn_user_enabled_disabled` in `main.rs` to pass the registry:
```rust
// Before:
cops::warn_user_enabled_disabled(&config);
// After:
cops::warn_user_enabled_disabled(&config, &registry);
```

**Step 4: Update `murphy_std::DISABLED_COPS` references**

The `DISABLED_COPS`-based section in `list_with_format` that adds `DisabledArenaMigration` entries can be removed since `DISABLED_COPS` is already empty (`&[]`). If `DISABLED_COPS` is non-empty in the future, this loop would still work. Leave it in place but it will produce zero entries. No code deletion needed here.

**Step 5: Build and test**

```bash
cargo build -p murphy-cli 2>&1 | grep -E "^error"
cargo test -p murphy-cli 2>&1 | grep -E "^test result|FAILED"
```
Expected: no errors, all passing.

**Step 6: Commit**

```bash
git add crates/murphy-cli/src/cops.rs crates/murphy-cli/src/main.rs
git commit -m "feat(murphy-cli): Status::DisabledDefault, warn_user_enabled_disabled from registry"
```

---

### Task 9: Integration tests + snapshot updates

**Files:**
- Modify: `crates/murphy-cli/tests/cli.rs` and `crates/murphy-cli/tests/rails_pack_e2e.rs` (if snapshots changed)
- Test: `cargo test --workspace`

**Step 1: Run the full suite**

```bash
cargo test --workspace 2>&1 | grep -E "FAILED|^error\["
```

If any snapshot tests fail due to the new `AllCops.Include` patterns (Murphy now discovers Gemfile, Rakefile etc. by default), update the snapshots:

```bash
cargo test --workspace -- --nocapture 2>&1 | grep "snapshot"
```

For `expect-test` inline snapshots, run with `UPDATE_EXPECT=1`:
```bash
UPDATE_EXPECT=1 cargo test --workspace 2>&1 | grep -E "^test result|FAILED"
```

**Step 2: Fix any broken tests**

Common breakage patterns:
- Tests that assert `files.include == ["**/*.rb"]` — update to check that `"**/*.rb"` is in the list (it still is, just alongside others)
- Tests that check `cop_enabled("Rails/...")` returns `false` — Rails cops are now `true` by default (since rubocop-rails default.yml is not bundled yet); delete or update these tests
- `DisabledUserConfig` assertions — verify new `DisabledDefault` variant hasn't broken them

**Step 3: Run quality gates**

```bash
cargo test --workspace 2>&1 | grep -E "^test result|FAILED"
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | grep -E "^error"
cargo +nightly fmt --check 2>&1 | grep -E "^Diff"
```
Expected: all clean.

**Step 4: Commit fixes**

```bash
git add -A
git commit -m "test: update snapshots and integration tests for bundled defaults"
```

---

### Task 10: Verify completion criteria

**Step 1: Verify `is_cop_disabled_by_default` is gone**

```bash
grep -rn "is_cop_disabled_by_default" crates/
```
Expected: no output.

**Step 2: Verify AllCops.Include inherits RuboCop defaults**

```bash
cat > /tmp/test_defaults.rb << 'EOF'
puts "hello"
EOF
# Test that murphy discovers Gemfile-like patterns by default
cargo run -p murphy-cli -- lint --explain Style/StringLiterals 2>&1 | head -5
```

Run a quick smoke test:
```bash
cargo run -p murphy-cli -- lint /tmp/test_defaults.rb 2>&1
```
Expected: exits 0 or 1 with offense output, no crash.

**Step 3: Verify cop_options_json returns defaults**

Write a quick unit test to confirm (can be temporary, run inline):
```bash
cargo test -p murphy-core with_defaults_populates_base_defaults 2>&1 | grep -E "test result|FAILED"
```

**Step 4: Final full test run**

```bash
cargo test --workspace 2>&1 | tail -5
```
Expected: `test result: ok. N passed; 0 failed`.
