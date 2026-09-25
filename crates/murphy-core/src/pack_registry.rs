//! Thin official pack registry index (C1; ADR 0049).
//!
//! C2 (ADR 0048) proved Gem + Bundler own download, versioning, and
//! install: `plugins = ["murphy-rails"]` resolves from installed gems,
//! `Gemfile` owns versions. A hosted registry service would duplicate
//! that. What Gem cannot express is Murphy-specific compat metadata
//! (required host ABI, minimum core version) plus a stable catalogue of
//! official pack names for `murphy add <pack>` to resolve.
//!
//! This module is that thin overlay:
//!
//! - [`PackRegistryIndex`] parses `registry/index.toml` (bundled at
//!   compile time via [`bundled`], overridable via file/env for
//!   mirrors and tests).
//! - [`check_compat`] rejects an entry whose `murphy-api-version` differs
//!   from the host ABI or whose `min-murphy-version` exceeds the running
//!   core version — before any config edit or `dlopen`.
//! - [`ensure_plugin_in_config_text`] is the minimal `.murphy.yml`
//!   edit `murphy add` applies (append-only, comment-preserving).
//!
//! The native plugin ABI is untouched.

use std::path::Path;

/// Env var overriding the registry index file (mirror / fork / test hook).
pub const REGISTRY_PATH_ENV: &str = "MURPHY_REGISTRY_PATH";

/// Bundled official index (`registry/index.toml` at the workspace root).
pub fn bundled_toml() -> &'static str {
    include_str!("../../../registry/index.toml")
}

/// One catalogue entry: a pack name resolvable to a RubyGem plus the
/// Murphy-specific compat floor.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackEntry {
    /// Distribution / plugin name, e.g. `murphy-rails`.
    pub name: String,
    /// RubyGem to install (usually identical to `name`).
    pub gem: String,
    /// Latest known pack version (informational; Bundler owns resolution).
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub homepage: String,
    /// Required host ABI (must equal `MURPHY_PLUGIN_ABI_VERSION`).
    #[serde(rename = "murphy-api-version")]
    pub murphy_api_version: u32,
    /// Minimum Murphy core version that can host this pack (`"0.1.0"`).
    #[serde(rename = "min-murphy-version", default = "default_min_murphy_version")]
    pub min_murphy_version: String,
}

fn default_min_murphy_version() -> String {
    "0.1.0".to_string()
}

/// Parsed `registry/index.toml`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct PackRegistryIndex {
    #[serde(default)]
    pub registry: RegistryMeta,
    #[serde(default, rename = "packs")]
    pub packs: Vec<PackEntry>,
}

/// `[registry]` header (currently only a format version).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryMeta {
    #[serde(default = "default_registry_version")]
    pub version: u32,
}

fn default_registry_version() -> u32 {
    1
}

/// Compat failure for [`check_compat`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryCompatError {
    /// Pack needs a different host ABI.
    ApiMismatch {
        pack: String,
        expected: u32,
        got: u32,
    },
    /// Pack needs a newer Murphy core.
    CoreTooOld {
        pack: String,
        min: String,
        current: String,
    },
}

impl std::fmt::Display for RegistryCompatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegistryCompatError::ApiMismatch {
                pack,
                expected,
                got,
            } => write!(
                f,
                "pack `{pack}` requires ABI version {got}, host provides {expected} \
                 (rebuild the pack against murphy-plugin-api {expected})"
            ),
            RegistryCompatError::CoreTooOld { pack, min, current } => write!(
                f,
                "pack `{pack}` needs murphy >= {min}, current is {current} (upgrade murphy first)"
            ),
        }
    }
}

impl std::error::Error for RegistryCompatError {}

impl std::str::FromStr for PackRegistryIndex {
    type Err = String;

    /// Parse index TOML text.
    fn from_str(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|e| format!("invalid registry index: {e}"))
    }
}

impl PackRegistryIndex {
    /// Read + parse an index file.
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read registry index `{}`: {e}", path.display()))?;
        text.parse()
    }

    /// The bundled official index.
    pub fn bundled() -> Result<Self, String> {
        bundled_toml().parse()
    }

    /// Load order: explicit `path` > `$MURPHY_REGISTRY_PATH` > bundled.
    pub fn load_with_override(path: Option<&Path>) -> Result<Self, String> {
        if let Some(p) = path {
            return Self::from_file(p);
        }
        if let Some(env) = std::env::var_os(REGISTRY_PATH_ENV) {
            return Self::from_file(Path::new(&env));
        }
        Self::bundled()
    }

    /// Find a pack by `name` (exact match).
    pub fn find(&self, name: &str) -> Option<&PackEntry> {
        self.packs.iter().find(|p| p.name == name)
    }

    /// Sorted pack names (for `murphy add` error hints).
    pub fn pack_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.packs.iter().map(|p| p.name.as_str()).collect();
        names.sort();
        names
    }
}

/// Check `entry` against the running host: ABI equality + core-version floor.
///
/// `host_api_version` is `MURPHY_PLUGIN_ABI_VERSION`; `core_version` is
/// `murphy_core::version()` (`"0.1.0"` semver).
pub fn check_compat(
    entry: &PackEntry,
    host_api_version: u32,
    core_version: &str,
) -> Result<(), RegistryCompatError> {
    if entry.murphy_api_version != host_api_version {
        return Err(RegistryCompatError::ApiMismatch {
            pack: entry.name.clone(),
            expected: host_api_version,
            got: entry.murphy_api_version,
        });
    }
    if compare_versions(core_version, &entry.min_murphy_version) == std::cmp::Ordering::Less {
        return Err(RegistryCompatError::CoreTooOld {
            pack: entry.name.clone(),
            min: entry.min_murphy_version.clone(),
            current: core_version.to_string(),
        });
    }
    Ok(())
}

/// Compare dotted versions (`"0.10.0"` > `"0.9.0"`): numeric segments
/// numerically, non-numeric lexically, missing segments as zero.
pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
    enum Seg {
        Num(u64),
        Str(String),
    }
    let parse = |v: &str| {
        v.split('.')
            .map(|s| match s.parse::<u64>() {
                Ok(n) => Seg::Num(n),
                Err(_) => Seg::Str(s.to_string()),
            })
            .collect::<Vec<_>>()
    };
    parse(a).cmp(&parse(b))
}

/// True when `text` (a `.murphy.yml` document) already declares `name`
/// under `plugins:` — either as `- <name>` or `name: <name>` (including
/// the `- name: <name>` detailed-entry first line).
pub fn config_text_has_plugin(text: &str, name: &str) -> bool {
    for line in text.lines() {
        let t = line.trim();
        // Strip trailing comment for shape checks (`- foo # comment`).
        let code = t.split('#').next().unwrap_or("").trim();
        if code == format!("- {name}")
            || code == format!("- \"{name}\"")
            || code == format!("- '{name}'")
            || code == format!("name: {name}")
            || code == format!("name: \"{name}\"")
            || code == format!("name: '{name}'")
            || code == format!("- name: {name}")
            || code == format!("- name: \"{name}\"")
            || code == format!("- name: '{name}'")
        {
            return true;
        }
        // `plugins: <name>` single-string form.
        if let Some(rest) = code.strip_prefix("plugins:") {
            let v = rest.trim();
            // Skip the empty / block-header form (`plugins:` alone).
            if v.is_empty() || v == "[]" || v == "{}" {
                continue;
            }
            let unquoted = v.trim_matches('"').trim_matches('\'');
            if unquoted == name {
                return true;
            }
        }
    }
    false
}

/// Minimal append-only edit adding `name` to `plugins:` in `.murphy.yml`
/// text. Comment- and key-preserving:
///
/// - missing/empty file → `plugins:\n  - <name>\n`
/// - no `plugins:` key → append `\nplugins:\n  - <name>\n`
/// - `plugins: <other>` inline → rewrite to list form with both entries
/// - block list → insert `  - <name>` at the end of the plugins block
///
/// Returns the new text, or `None` when `name` is already present.
pub fn ensure_plugin_in_config_text(text: &str, name: &str) -> Option<String> {
    if text.trim().is_empty() {
        return Some(format!("plugins:\n  - {name}\n"));
    }
    if config_text_has_plugin(text, name) {
        return None;
    }
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let plugins_idx = lines.iter().position(|l| {
        let t = l.trim();
        t == "plugins:" || t.starts_with("plugins:")
    });
    let Some(idx) = plugins_idx else {
        let mut out = text.to_string();
        if !out.ends_with('\n') {
            out.push('\n');
        }
        // When the file lacks a trailing newline structure, keep exactly
        // one blank line before the appended section.
        if !out.ends_with("\n\n") {
            out.push('\n');
        }
        out.push_str(&format!("plugins:\n  - {name}\n"));
        return Some(out);
    };
    // Inline `plugins: <value>` form.
    let inline = lines[idx]
        .trim()
        .strip_prefix("plugins:")
        .unwrap_or("")
        .trim()
        .to_string();
    // Strip trailing comment for the shape check (`plugins: foo # comment`).
    let inline_value = inline.split('#').next().unwrap_or("").trim();
    if !inline_value.is_empty() && inline_value != "[]" && inline_value != "{}" {
        // `plugins: []` (empty flow list) → replace with block list.
        if inline_value == "[]" {
            lines[idx] = "plugins:".to_string();
            lines.insert(idx + 1, format!("  - {name}"));
            let mut out = lines.join("\n");
            out.push('\n');
            return Some(out);
        }
        let old = inline_value
            .trim_matches('"')
            .trim_matches('\'')
            .to_string();
        lines[idx] = "plugins:".to_string();
        lines.insert(idx + 1, format!("  - {old}"));
        lines.insert(idx + 2, format!("  - {name}"));
        let mut out = lines.join("\n");
        out.push('\n');
        return Some(out);
    }
    // Block form: end of the plugins block is the first line after `idx`
    // starting at column 0 (a new top-level key), else EOF.
    let mut insert_at = lines.len();
    for (i, line) in lines.iter().enumerate().skip(idx + 1) {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        if !line.starts_with(' ') && !line.starts_with('\t') {
            insert_at = i;
            break;
        }
    }
    // Skip trailing blank/comment lines inside the block so the new entry
    // lands after the last real item, not after trailing whitespace.
    let mut anchor = insert_at;
    while anchor > idx + 1 {
        let prev = lines[anchor - 1].trim();
        if prev.is_empty() || prev.starts_with('#') {
            anchor -= 1;
        } else {
            break;
        }
    }
    lines.insert(anchor, format!("  - {name}"));
    let mut out = lines.join("\n");
    out.push('\n');
    Some(out)
}

/// Next-step hint after `murphy add`: Bundler owns the install, so we
/// print — never run — the Gemfile command.
pub fn gem_install_hint(entry: &PackEntry) -> String {
    format!(
        "next: add `gem \"{gem}\"` to your Gemfile, then `bundle install` (or `bundle add {gem}`)",
        gem = entry.gem
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr as _;

    const SAMPLE: &str = r#"
[registry]
version = 1

[[packs]]
name = "murphy-rails"
gem = "murphy-rails"
version = "0.1.0"
description = "Rails cops"
homepage = "https://example.invalid"
murphy-api-version = 4
min-murphy-version = "0.1.0"

[[packs]]
name = "murphy-future"
gem = "murphy-future"
version = "9.9.9"
description = "needs newer host"
homepage = "https://example.invalid"
murphy-api-version = 999
min-murphy-version = "99.0.0"
"#;

    #[test]
    fn parses_and_finds_packs_sorted() {
        let idx = PackRegistryIndex::from_str(SAMPLE).expect("parses");
        assert_eq!(idx.registry.version, 1);
        assert_eq!(idx.pack_names(), vec!["murphy-future", "murphy-rails"]);
        assert_eq!(idx.find("murphy-rails").unwrap().gem, "murphy-rails");
        assert!(idx.find("murphy-missing").is_none());
    }

    #[test]
    fn bundled_index_has_official_packs() {
        let idx = PackRegistryIndex::bundled().expect("bundled parses");
        for name in ["murphy-rails", "murphy-rspec"] {
            assert!(
                idx.find(name).is_some(),
                "bundled index must list {name}: {:?}",
                idx.pack_names()
            );
        }
    }

    #[test]
    fn compat_accepts_current_host() {
        let idx = PackRegistryIndex::from_str(SAMPLE).expect("parses");
        let entry = idx.find("murphy-rails").unwrap();
        check_compat(
            entry,
            murphy_plugin_api::MURPHY_PLUGIN_ABI_VERSION,
            crate::version(),
        )
        .expect("official pack must be compat with this host");
    }

    #[test]
    fn compat_rejects_abi_mismatch_before_core_check() {
        let idx = PackRegistryIndex::from_str(SAMPLE).expect("parses");
        let entry = idx.find("murphy-future").unwrap();
        match check_compat(
            entry,
            murphy_plugin_api::MURPHY_PLUGIN_ABI_VERSION,
            "99.99.99",
        ) {
            Err(RegistryCompatError::ApiMismatch { got, .. }) => assert_eq!(got, 999),
            other => panic!("expected ApiMismatch, got {other:?}"),
        }
    }

    #[test]
    fn compat_rejects_too_old_core() {
        let entry = PackEntry {
            name: "murphy-rails".to_string(),
            gem: "murphy-rails".to_string(),
            version: "0.1.0".to_string(),
            description: String::new(),
            homepage: String::new(),
            murphy_api_version: murphy_plugin_api::MURPHY_PLUGIN_ABI_VERSION,
            min_murphy_version: "99.0.0".to_string(),
        };
        match check_compat(
            &entry,
            murphy_plugin_api::MURPHY_PLUGIN_ABI_VERSION,
            "0.1.0",
        ) {
            Err(RegistryCompatError::CoreTooOld { min, .. }) => assert_eq!(min, "99.0.0"),
            other => panic!("expected CoreTooOld, got {other:?}"),
        }
    }

    #[test]
    fn version_compare_is_numeric_aware() {
        assert_eq!(
            compare_versions("0.10.0", "0.9.0"),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            compare_versions("0.1.0", "0.1.0"),
            std::cmp::Ordering::Equal
        );
        assert_eq!(compare_versions("0.1.0", "0.2.0"), std::cmp::Ordering::Less);
    }

    #[test]
    fn empty_file_creates_plugins_section() {
        assert_eq!(
            ensure_plugin_in_config_text("", "murphy-rails"),
            Some("plugins:\n  - murphy-rails\n".to_string())
        );
    }

    #[test]
    fn file_without_plugins_gets_appended_section() {
        let out =
            ensure_plugin_in_config_text("AllCops:\n  TargetRubyVersion: 3.2\n", "murphy-rails")
                .expect("appends");
        assert!(out.contains("plugins:\n  - murphy-rails"), "got:\n{out}");
        assert!(out.contains("TargetRubyVersion"), "keeps keys:\n{out}");
    }

    #[test]
    fn block_list_inserts_before_next_top_level_key() {
        let text = "plugins:\n  - murphy-rspec\n\nStyle/Foo:\n  Enabled: true\n";
        let out = ensure_plugin_in_config_text(text, "murphy-rails").expect("inserts");
        let plugins_pos = out.find("murphy-rails").expect("new entry");
        let style_pos = out.find("Style/Foo").expect("next key");
        assert!(plugins_pos < style_pos, "insert inside block:\n{out}");
        // Round-trips through the real config parser.
        let cfg = crate::MurphyConfig::from_yaml_str(&out).expect("valid yaml");
        assert!(cfg.plugins.iter().any(|p| match p {
            crate::PluginConfig::Name(n) => n == "murphy-rails",
            crate::PluginConfig::Detailed(d) => d.name == "murphy-rails",
        }));
    }

    #[test]
    fn already_present_returns_none() {
        for text in [
            "plugins:\n  - murphy-rails\n",
            "plugins:\n  - \"murphy-rails\"\n",
            "plugins: murphy-rails\n",
            "plugins:\n  - name: murphy-rails\n    path: ./x.so\n",
        ] {
            assert_eq!(
                ensure_plugin_in_config_text(text, "murphy-rails"),
                None,
                "dup: {text:?}"
            );
        }
    }

    #[test]
    fn inline_single_string_expands_to_list() {
        let out = ensure_plugin_in_config_text("plugins: murphy-rspec\n", "murphy-rails")
            .expect("expands");
        assert!(
            out.contains("- murphy-rspec") && out.contains("- murphy-rails"),
            "got:\n{out}"
        );
    }

    #[test]
    fn load_with_override_prefers_file_and_env() {
        let dir = tempfile::tempdir().expect("tempdir");
        let custom = dir.path().join("custom.toml");
        std::fs::write(
            &custom,
            "[registry]\nversion = 1\n\n[[packs]]\nname = \"custom-pack\"\ngem = \"custom-pack\"\nversion = \"1.0\"\nmurphy-api-version = 4\nmin-murphy-version = \"0.1.0\"\n",
        )
        .expect("write custom");
        let idx = PackRegistryIndex::load_with_override(Some(&custom)).expect("loads file");
        assert!(idx.find("custom-pack").is_some());
    }

    #[test]
    fn invalid_toml_errors() {
        assert!(PackRegistryIndex::from_str("[[packs]\nbroken").is_err());
    }
}
