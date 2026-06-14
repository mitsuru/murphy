use crate::Severity;
use murphy_plugin_api::{AllCopsContext, RubyVersion};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::ConfigError;

#[derive(Debug, Clone, PartialEq)]
pub struct MurphyConfig {
    pub target_ruby_version: RubyVersion,
    pub target_rails_version: Option<RubyVersion>,
    pub active_support_extensions_enabled: bool,
    /// True when the user explicitly set `ActiveSupportExtensionsEnabled` —
    /// directly in `.murphy.yml` OR via `inherit_from`. Derived from the
    /// *merged* `ParsedYaml` (post-`merge_over`), mirroring `saw_include` /
    /// `saw_exclude`. Read by [`MurphyConfig::apply_pack_default_layers`] so a
    /// user value wins over pack-bundled `default.yml` layers.
    pub(crate) user_set_active_support_extensions_enabled: bool,
    pub files: FilesConfig,
    pub cops: CopsConfig,
    pub plugins: Vec<PluginConfig>,
    /// Defaults parsed from the pack's bundled `default.yml` (e.g. rubocop's).
    /// Populated by `with_defaults`; empty when loaded via `from_yaml_str` / `load`.
    pub base_defaults: DefaultCopsData,
    /// The user's raw `AllCops.Exclude` (`None` when unset). Retained separately
    /// from `files.exclude` (the finalized discovery list) so the base-layer
    /// union can be recomputed each time pack defaults are layered in. See
    /// [`MurphyConfig::finalize_files_exclude`].
    pub(crate) user_exclude: Option<Vec<String>>,
    /// True when `inherit_mode: merge: [Exclude]` was set; the user's
    /// `AllCops.Exclude` then unions with the default base layer rather than
    /// replacing it.
    pub(crate) exclude_merge: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilesConfig {
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CopsConfig {
    pub path: PathBuf,
    pub rules: BTreeMap<String, CopRule>,
}

/// Plugin pack entry from `plugins:` in `.murphy.yml`.
///
/// Two shapes (RuboCop-compatible):
/// - `- murphy-rails` — name-only shorthand.
/// - `- name: "..." path: "..."` — explicit path; bypasses the search path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginConfig {
    Name(String),
    Detailed(PluginDetailed),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginDetailed {
    pub name: String,
    pub path: PathBuf,
}

/// Per-cop configuration from a top-level cop section in `.murphy.yml`.
///
/// `Enabled` and `Severity` are extracted as typed fields; all other keys
/// pass through to `options` as JSON values for the cop's own parser.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CopRule {
    pub enabled: Option<bool>,
    pub severity: Option<Severity>,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub options: BTreeMap<String, serde_json::Value>,
}

/// Metadata keys from RuboCop's default.yml that are NOT cop options.
/// These are stripped when building the options map so cops don't receive them.
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
    /// `AllCops.ActiveSupportExtensionsEnabled` flag from the YAML.
    pub allcops_active_support_extensions_enabled: Option<bool>,
    /// Per-cop defaults keyed by cop name.
    pub cop_rules: BTreeMap<String, DefaultCopRule>,
}

impl DefaultCopsData {
    /// Parse a RuboCop-format default.yml string into structured defaults.
    ///
    /// Unrecognised top-level keys are silently ignored.
    /// Parse failures are silently skipped.
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
                    if let Some(Yaml::Boolean(b)) =
                        all_cops.get(&Yaml::String("ActiveSupportExtensionsEnabled".to_string()))
                    {
                        result.allcops_active_support_extensions_enabled = Some(*b);
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
                // Skip documentation/metadata keys.
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

/// Merge a later pack layer's [`DefaultCopRule`] over an existing base entry.
/// Later layer wins per field; absent fields (`None` / empty `Vec`) keep the
/// base value, so a pack that only sets `Exclude` does not wipe a base
/// `Include` for the same cop.
fn merge_default_cop_rule(base: &mut DefaultCopRule, layer: DefaultCopRule) {
    if layer.enabled.is_some() {
        base.enabled = layer.enabled;
    }
    if layer.severity.is_some() {
        base.severity = layer.severity;
    }
    if !layer.include.is_empty() {
        base.include = layer.include;
    }
    if !layer.exclude.is_empty() {
        base.exclude = layer.exclude;
    }
    for (key, value) in layer.options {
        base.options.insert(key, value);
    }
}

/// Collect strings into a `Vec`, dropping later duplicates and keeping the first
/// occurrence's position. Used to union exclude-pattern layers (defaults first,
/// then user) without repeating a pattern that appears in more than one layer.
fn dedup_preserving_order(items: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for item in items {
        if !seen.contains(&item) {
            seen.insert(item.clone());
            out.push(item);
        }
    }
    out
}

fn default_include() -> Vec<String> {
    vec!["**/*.rb".to_string()]
}

fn default_cops_path() -> PathBuf {
    PathBuf::from("cops")
}

fn default_target_ruby_version() -> RubyVersion {
    RubyVersion::new(3, 1)
}

/// Internal representation of a parsed YAML config file.
///
/// All fields are `Option<T>` so "explicitly set" is distinguished from "not
/// set". Used by [`parse_yaml_str`] and [`load_resolving_inherit`] to merge
/// inherited configs before applying defaults.
#[derive(Default)]
struct ParsedYaml {
    target_ruby_version: Option<RubyVersion>,
    target_rails_version: Option<RubyVersion>,
    active_support_extensions_enabled: Option<bool>,
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    cops_path: Option<PathBuf>,
    rules: BTreeMap<String, CopRule>,
    plugins: Vec<PluginConfig>,
    /// Paths from `inherit_from:` — consumed by `load_resolving_inherit`.
    inherit_from: Vec<String>,
    /// True when `inherit_mode: merge: [Exclude]` is set. Governs whether the
    /// user's `AllCops.Exclude` *unions* with the default base layer (std core ∪
    /// pack defaults) rather than *replacing* it. Mirrors RuboCop's `inherit_mode`
    /// behaviour for the `AllCops.Exclude` key.
    exclude_merge: bool,
}

impl ParsedYaml {
    /// Merge `self` (higher priority) over `base` (lower priority).
    ///
    /// For scalar and `Option` fields, `self`'s `Some` wins; `None` falls back
    /// to `base`. Cop rules are merged per-key (not whole-section replace).
    /// Plugins are concatenated (base first, then self).
    fn merge_over(self, base: ParsedYaml) -> ParsedYaml {
        ParsedYaml {
            target_ruby_version: self.target_ruby_version.or(base.target_ruby_version),
            target_rails_version: self.target_rails_version.or(base.target_rails_version),
            active_support_extensions_enabled: self
                .active_support_extensions_enabled
                .or(base.active_support_extensions_enabled),
            include: self.include.or(base.include),
            exclude: self.exclude.or(base.exclude),
            cops_path: self.cops_path.or(base.cops_path),
            rules: {
                let mut merged = base.rules;
                for (name, top_rule) in self.rules {
                    let base_rule = merged.remove(&name).unwrap_or_default();
                    merged.insert(name, merge_cop_rule(top_rule, base_rule));
                }
                merged
            },
            plugins: {
                let mut all = base.plugins;
                for p in self.plugins {
                    if !all.contains(&p) {
                        all.push(p);
                    }
                }
                all
            },
            inherit_from: vec![],
            // OR semantics: if any file in the inherit chain enables Exclude
            // merge, the merge applies. The root (`self`) is highest priority,
            // but a `true` from either layer is meaningful, so OR is correct.
            exclude_merge: self.exclude_merge || base.exclude_merge,
        }
    }

    /// Convert to `MurphyConfig` by applying defaults to unset fields.
    /// Returns `(config, saw_include, saw_exclude)` for `with_defaults` compat.
    fn into_murphy_config(self) -> (MurphyConfig, bool, bool) {
        let saw_include = self.include.is_some();
        let saw_exclude = self.exclude.is_some();
        let cfg = MurphyConfig {
            target_ruby_version: self
                .target_ruby_version
                .unwrap_or_else(default_target_ruby_version),
            target_rails_version: self.target_rails_version,
            active_support_extensions_enabled: self
                .active_support_extensions_enabled
                .unwrap_or(false),
            // Mirror `saw_include`/`saw_exclude`: snapshot set-ness from the
            // *merged* `ParsedYaml`, so a value arriving via `inherit_from`
            // counts as user-set.
            user_set_active_support_extensions_enabled: self
                .active_support_extensions_enabled
                .is_some(),
            files: FilesConfig {
                include: self.include.unwrap_or_else(default_include),
                // Provisional: the finalized discovery list is recomputed by
                // `finalize_files_exclude` (called from `with_defaults` /
                // `apply_pack_default_layers`) once the default base layer is
                // known. Raw user input is retained in `user_exclude`.
                exclude: self.exclude.clone().unwrap_or_default(),
            },
            cops: CopsConfig {
                path: self.cops_path.unwrap_or_else(default_cops_path),
                rules: self.rules,
            },
            plugins: self.plugins,
            base_defaults: DefaultCopsData::default(),
            user_exclude: self.exclude,
            exclude_merge: self.exclude_merge,
        };
        (cfg, saw_include, saw_exclude)
    }
}

/// Merge `top` cop rule (higher priority) over `base` (lower priority).
/// Per-field: `top` wins when it carries an explicit value; `base` fills gaps.
fn merge_cop_rule(top: CopRule, base: CopRule) -> CopRule {
    CopRule {
        enabled: top.enabled.or(base.enabled),
        severity: top.severity.or(base.severity),
        include: if top.include.is_empty() {
            base.include
        } else {
            top.include
        },
        exclude: if top.exclude.is_empty() {
            base.exclude
        } else {
            top.exclude
        },
        options: {
            let mut opts = base.options;
            opts.extend(top.options);
            opts
        },
    }
}

impl Default for MurphyConfig {
    fn default() -> Self {
        Self {
            target_ruby_version: default_target_ruby_version(),
            target_rails_version: None,
            active_support_extensions_enabled: false,
            user_set_active_support_extensions_enabled: false,
            files: FilesConfig {
                include: default_include(),
                exclude: Vec::new(),
            },
            cops: CopsConfig {
                path: default_cops_path(),
                rules: BTreeMap::new(),
            },
            plugins: Vec::new(),
            base_defaults: DefaultCopsData::default(),
            user_exclude: None,
            exclude_merge: false,
        }
    }
}

impl MurphyConfig {
    /// Parse a `.murphy.yml` document string.
    ///
    /// Schema (RuboCop-compatible):
    /// ```yaml
    /// AllCops:
    ///   Include: ["**/*.rb"]
    ///   Exclude: ["vendor/**"]
    ///   CopsPath: cops          # Murphy-only; no RuboCop equivalent
    ///
    /// plugins:
    ///   - murphy-rails
    ///   - name: local-pack
    ///     path: ./libfoo.so
    ///
    /// Style/StringLiterals:
    ///   Enabled: true
    ///   Severity: warning
    ///   EnforcedStyle: single_quotes
    /// ```
    ///
    /// Top-level keys other than `AllCops` and `plugins` are treated as cop
    /// names (open-keyed, compatible with `.rubocop.yml`).
    pub fn from_yaml_str(text: &str) -> Result<Self, ConfigError> {
        let (cfg, _, _) = Self::from_yaml_str_raw(text)?;
        Ok(cfg)
    }

    /// Internal: parse user YAML and return `(config, saw_include, saw_exclude)`.
    /// `saw_include`/`saw_exclude` tell `with_defaults` whether to apply bundled
    /// AllCops defaults. Does NOT resolve `inherit_from` — use `load` for that.
    fn from_yaml_str_raw(text: &str) -> Result<(Self, bool, bool), ConfigError> {
        Ok(parse_yaml_str(text)?.into_murphy_config())
    }

    /// Parse user YAML and merge bundled `defaults_yaml` as a base layer.
    /// User settings always win; defaults fill in missing values.
    ///
    /// The host (murphy-cli) calls this with `murphy_std::BUNDLED_DEFAULTS_YAML`
    /// so cop defaults are data-driven rather than hardcoded.
    pub fn with_defaults(user_yaml: &str, defaults_yaml: &str) -> Result<Self, ConfigError> {
        let (mut cfg, saw_include, _saw_exclude) = Self::from_yaml_str_raw(user_yaml)?;
        let defaults = DefaultCopsData::from_yaml(defaults_yaml);

        if !saw_include && !defaults.allcops_include.is_empty() {
            cfg.files.include = defaults.allcops_include.clone();
        }
        cfg.base_defaults = defaults;
        // Compute `files.exclude` from the default base layer (std core here;
        // pack layers join later via `apply_pack_default_layers`). Idempotent.
        cfg.finalize_files_exclude();
        Ok(cfg)
    }

    pub fn load(root: &Path) -> Result<Self, ConfigError> {
        let config_path = root.join(".murphy.yml");
        if !config_path.exists() {
            return Ok(Self::default());
        }
        let parsed = load_resolving_inherit(&config_path, &std::collections::HashSet::new())?;
        let (cfg, _, _) = parsed.into_murphy_config();
        Ok(cfg)
    }

    /// Like [`Self::load`] but merges bundled `defaults_yaml` as a base layer.
    /// The host (murphy-cli) calls this with `murphy_std::BUNDLED_DEFAULTS_YAML`.
    pub fn load_with_defaults(root: &Path, defaults_yaml: &str) -> Result<Self, ConfigError> {
        let config_path = root.join(".murphy.yml");
        let parsed = if config_path.exists() {
            load_resolving_inherit(&config_path, &std::collections::HashSet::new())?
        } else {
            ParsedYaml::default()
        };
        let (mut cfg, saw_include, _saw_exclude) = parsed.into_murphy_config();
        let defaults = DefaultCopsData::from_yaml(defaults_yaml);
        if !saw_include && !defaults.allcops_include.is_empty() {
            cfg.files.include = defaults.allcops_include.clone();
        }
        cfg.base_defaults = defaults;
        // See `with_defaults`: recompute `files.exclude` from the base layer.
        cfg.finalize_files_exclude();
        Ok(cfg)
    }

    /// Recompute `files.exclude` — the finalized discovery exclude list — from
    /// the user's raw `AllCops.Exclude` and the default base layer
    /// (`base_defaults.allcops_exclude`, which is std core ∪ any layered pack
    /// defaults).
    ///
    /// Mirrors RuboCop's resolution of `AllCops.Exclude` (verified empirically
    /// against rubocop-rails on Mastodon):
    ///
    /// | user `Exclude` | `inherit_mode: merge: [Exclude]` | `files.exclude`            |
    /// |----------------|----------------------------------|----------------------------|
    /// | unset          | n/a                              | default base layer         |
    /// | set            | no                               | user list (replaces base)  |
    /// | set            | yes                              | base ∪ user (deduped)      |
    ///
    /// Idempotent: derives solely from `user_exclude`, `exclude_merge`, and
    /// `base_defaults.allcops_exclude`, so re-running after pack layers join the
    /// base produces the union including those packs.
    ///
    /// Limitation: a project cannot *un-exclude* a default base entry without
    /// `inherit_mode` (matching RuboCop). Per-file inherit_mode and unioning
    /// `AllCops.Exclude` across `inherit_from` files are not modelled.
    fn finalize_files_exclude(&mut self) {
        let base = &self.base_defaults.allcops_exclude;
        self.files.exclude = match &self.user_exclude {
            None => dedup_preserving_order(base.iter().cloned()),
            Some(user) if self.exclude_merge => {
                dedup_preserving_order(base.iter().chain(user.iter()).cloned())
            }
            Some(user) => user.clone(),
        };
    }

    pub fn cop_enabled(&self, name: &str) -> bool {
        self.cop_enabled_with_cop_default(name, None)
    }

    /// Like `cop_enabled` but also accepts the cop's ABI `default_enabled`
    /// tristate as a third fallback layer (from `PluginCopV1.default_enabled`).
    ///
    /// Layer order (first `Some` wins):
    ///   1. User `.murphy.yml` explicit `Enabled:`
    ///   2. Bundled `base_defaults` from pack's default.yml
    ///   3. `cop_default` from `PluginCopV1.default_enabled` (dynamic pack ABI)
    ///   4. `true` (enabled by default)
    pub fn cop_enabled_with_cop_default(&self, name: &str, cop_default: Option<bool>) -> bool {
        if let Some(e) = self.cops.rules.get(name).and_then(|r| r.enabled) {
            return e;
        }
        if let Some(e) = self
            .base_defaults
            .cop_rules
            .get(name)
            .and_then(|r| r.enabled)
        {
            return e;
        }
        if let Some(e) = cop_default {
            return e;
        }
        true
    }

    /// True when the user wrote `Enabled: true` for a cop in `.murphy.yml`.
    pub fn is_explicitly_enabled(&self, name: &str) -> bool {
        self.cops.rules.get(name).and_then(|rule| rule.enabled) == Some(true)
    }

    /// Merge pack-bundled `default.yml` layers (later overrides earlier) below
    /// user config. The resolution order is `std < pack layers < user`: the
    /// user's explicit value — set directly OR inherited via `inherit_from` —
    /// still wins.
    ///
    /// Two things are layered:
    ///
    /// 1. **Per-cop defaults** (`Enabled`/`Severity`/`Include`/`Exclude`/
    ///    options) merge into `base_defaults.cop_rules` so a pack's file-scope
    ///    defaults — e.g. rubocop-rspec's `RSpec/DescribeClass: Exclude:` for
    ///    `spec/{features,requests,routing,system,views}` — take effect once the
    ///    pack is loaded. `cop_applies_to_file` then resolves them below user
    ///    config. This is **not** gated by the `ActiveSupportExtensionsEnabled`
    ///    early-return below: per-cop file scoping must apply regardless of
    ///    whether the user pinned that flag.
    /// 2. **`AllCops.Exclude`** patterns merge into `base_defaults.allcops_exclude`
    ///    (deduped) so a pack's file-discovery excludes — e.g. rubocop-rails'
    ///    `db/*schema.rb`, `bin/*`, `log/**/*` — join the default base layer.
    ///    `finalize_files_exclude` then recomputes `files.exclude` (see its docs
    ///    for how the user's own `Exclude` interacts). Like (1), this runs
    ///    regardless of the `ActiveSupportExtensionsEnabled` early-return.
    /// 3. The `AllCops.ActiveSupportExtensionsEnabled` flag, which the user can
    ///    override (the early-return below).
    pub fn apply_pack_default_layers(&mut self, pack_yamls: &[&str]) {
        // (1) Per-cop pack defaults — always applied (later layer wins per-field).
        for yaml in pack_yamls {
            for (name, pack_rule) in DefaultCopsData::from_yaml(yaml).cop_rules {
                let entry = self.base_defaults.cop_rules.entry(name).or_default();
                merge_default_cop_rule(entry, pack_rule);
            }
        }

        // (2) Pack AllCops.Exclude joins the default base layer (deduped), then
        // `files.exclude` is recomputed. Must run before the ASE early-return so
        // pack discovery excludes apply even when the user pinned ASE.
        let mut all_excludes = std::mem::take(&mut self.base_defaults.allcops_exclude);
        for yaml in pack_yamls {
            all_excludes.extend(DefaultCopsData::from_yaml(yaml).allcops_exclude);
        }
        self.base_defaults.allcops_exclude = dedup_preserving_order(all_excludes);
        self.finalize_files_exclude();

        // (3) ActiveSupportExtensionsEnabled flag — user wins.
        if self.user_set_active_support_extensions_enabled {
            return; // user wins; pack layers are only defaults
        }
        // Entry invariant: when the user did not set the flag, the std base is
        // false. Reset explicitly so the method is idempotent and does not depend
        // on no prior load path having written the field.
        self.active_support_extensions_enabled = false;
        for yaml in pack_yamls {
            if let Some(v) =
                DefaultCopsData::from_yaml(yaml).allcops_active_support_extensions_enabled
            {
                self.active_support_extensions_enabled = v; // later layer overrides
            }
        }
    }

    pub fn severity_override(&self, name: &str) -> Option<Severity> {
        self.cops
            .rules
            .get(name)
            .and_then(|r| r.severity)
            .or_else(|| {
                self.base_defaults
                    .cop_rules
                    .get(name)
                    .and_then(|r| r.severity)
            })
    }

    pub fn cop_applies_to_file(&self, name: &str, file: &Path) -> bool {
        let file = file.strip_prefix(".").unwrap_or(file);

        let rule = self.cops.rules.get(name);
        let default_rule = self.base_defaults.cop_rules.get(name);

        // Resolve Include and Exclude independently: user setting wins per-field;
        // fall back to base_defaults for each field individually. This prevents
        // a user-level Exclude from accidentally disabling a default Include scope.
        let include = rule
            .map(|r| &r.include)
            .filter(|inc| !inc.is_empty())
            .or_else(|| default_rule.map(|r| &r.include));

        let exclude = rule
            .map(|r| &r.exclude)
            .filter(|exc| !exc.is_empty())
            .or_else(|| default_rule.map(|r| &r.exclude));

        let matches_include = match include {
            Some(inc) if !inc.is_empty() => globset_matches(inc, file),
            _ => true,
        };
        let matches_exclude = match exclude {
            Some(exc) if !exc.is_empty() => globset_matches(exc, file),
            _ => false,
        };

        matches_include && !matches_exclude
    }

    pub fn has_cop_path_scopes(&self) -> bool {
        self.cops
            .rules
            .values()
            .any(|rule| !rule.include.is_empty() || !rule.exclude.is_empty())
    }

    /// The run-wide context scalars resolved from this config, bundled for
    /// threading into dispatch (and thus every cop's `CxRaw`).
    pub fn allcops_context(&self) -> AllCopsContext {
        AllCopsContext {
            target_rails_version: self.target_rails_version,
            target_ruby_version: Some(self.target_ruby_version),
            active_support_extensions_enabled: self.active_support_extensions_enabled,
            indentation_width: self.resolved_indentation_width(),
        }
    }

    /// RuboCop's `config.for_cop('Layout/IndentationWidth')['Width']` resolved
    /// for this config: the user's `Layout/IndentationWidth.Width` if set, else
    /// the bundled default, else [`AllCopsContext::DEFAULT_INDENTATION_WIDTH`].
    ///
    /// Read unconditionally — RuboCop consults the cop's config whether or not
    /// `Layout/IndentationWidth` is enabled. An explicit `Width: 0` is preserved
    /// (user wins over the bundled default, and `0` is a valid width, not
    /// "unset").
    fn resolved_indentation_width(&self) -> i64 {
        const COP: &str = "Layout/IndentationWidth";
        let width = |opts: Option<&BTreeMap<String, serde_json::Value>>| {
            opts.and_then(|o| o.get("Width"))
                .and_then(serde_json::Value::as_i64)
        };
        width(self.cops.rules.get(COP).map(|r| &r.options))
            .or_else(|| width(self.base_defaults.cop_rules.get(COP).map(|r| &r.options)))
            .unwrap_or(AllCopsContext::DEFAULT_INDENTATION_WIDTH)
    }

    /// Cop names the resolved config disables (`Enabled: false`), for seeding
    /// `Cx::extra_enabled_directives()`. Mirrors RuboCop's `registry.disabled(config)`
    /// for the explicitly-configured subset (default-disabled-but-unconfigured cops
    /// are out of scope — see murphy-k19j parity note).
    pub fn disabled_cop_names(&self) -> impl Iterator<Item = &str> {
        self.cops
            .rules
            .iter()
            .filter(|(_, rule)| rule.enabled == Some(false))
            .map(|(name, _)| name.as_str())
    }

    pub fn cop_options_json(&self, name: &str) -> Vec<u8> {
        let default_opts = self.base_defaults.cop_rules.get(name).map(|r| &r.options);
        let user_opts = self.cops.rules.get(name).map(|r| &r.options);

        // Fast path: skip cloning and serializing when both are empty (common case).
        if default_opts.is_none_or(|o| o.is_empty()) && user_opts.is_none_or(|o| o.is_empty()) {
            return b"{}".to_vec();
        }

        // Start from base defaults, then overlay user options (user wins per key).
        let mut merged = default_opts.cloned().unwrap_or_default();
        if let Some(opts) = user_opts {
            merged.extend(opts.clone());
        }
        serde_json::to_vec(&merged).unwrap_or_else(|_| b"{}".to_vec())
    }

    pub fn cop_options_map_json(&self, names: &[String]) -> Vec<u8> {
        let mut options = BTreeMap::new();
        for name in names {
            if let Some(rule) = self.cops.rules.get(name) {
                options.insert(name.clone(), &rule.options);
            }
        }
        serde_json::to_vec(&options).unwrap_or_else(|_| b"{}".to_vec())
    }
}

/// Parse a `.murphy.yml` document string into a [`ParsedYaml`].
///
/// Validates glob patterns at parse time. `inherit_from` paths are stored
/// verbatim and not resolved — only [`load_resolving_inherit`] does that.
fn parse_yaml_str(text: &str) -> Result<ParsedYaml, ConfigError> {
    use yaml_rust2::{Yaml, YamlLoader};

    let docs = YamlLoader::load_from_str(text).map_err(|e| ConfigError::BadYaml(e.to_string()))?;

    let doc = match docs.into_iter().next() {
        None => return Ok(ParsedYaml::default()),
        Some(d) => d,
    };

    let top = match doc {
        Yaml::Hash(h) => h,
        Yaml::Null => return Ok(ParsedYaml::default()),
        _ => {
            return Err(ConfigError::BadYaml(
                "top-level document must be a mapping".to_string(),
            ));
        }
    };

    let mut parsed = ParsedYaml::default();

    for (key, value) in top {
        let Yaml::String(section) = key else {
            continue;
        };

        match section.as_str() {
            "inherit_from" => {
                parsed.inherit_from = match value {
                    Yaml::String(s) => vec![s],
                    Yaml::Array(arr) => arr
                        .into_iter()
                        .filter_map(|v| {
                            if let Yaml::String(s) = v {
                                Some(s)
                            } else {
                                None
                            }
                        })
                        .collect(),
                    _ => vec![],
                };
            }
            "AllCops" => {
                if let Yaml::Hash(all_cops) = value {
                    if let Some(inc) = all_cops.get(&Yaml::String("Include".to_string())) {
                        let list = yaml_string_list(inc);
                        validate_glob_patterns(&list)?;
                        parsed.include = Some(list);
                    }
                    if let Some(exc) = all_cops.get(&Yaml::String("Exclude".to_string())) {
                        let list = yaml_string_list(exc);
                        validate_glob_patterns(&list)?;
                        parsed.exclude = Some(list);
                    }
                    if let Some(Yaml::String(p)) =
                        all_cops.get(&Yaml::String("CopsPath".to_string()))
                    {
                        parsed.cops_path = Some(PathBuf::from(p));
                    }
                    if let Some(v) = all_cops.get(&Yaml::String("TargetRubyVersion".to_string()))
                        && let Some(rv) = parse_ruby_version(v)
                    {
                        parsed.target_ruby_version = Some(rv);
                    }
                    if let Some(v) = all_cops.get(&Yaml::String("TargetRailsVersion".to_string()))
                        && let Some(rv) = parse_ruby_version(v)
                    {
                        parsed.target_rails_version = Some(rv);
                    }
                    if let Some(Yaml::Boolean(b)) =
                        all_cops.get(&Yaml::String("ActiveSupportExtensionsEnabled".to_string()))
                    {
                        parsed.active_support_extensions_enabled = Some(*b);
                    }
                }
            }
            "plugins" => {
                parsed.plugins =
                    parse_plugins(value).map_err(|e| ConfigError::BadYaml(e.to_string()))?;
            }
            "inherit_mode" => {
                // Only `merge:` containing `Exclude` is honoured (thin subset of
                // RuboCop's inherit_mode). It flips `AllCops.Exclude` from
                // replace-the-default to union-with-the-default. Other keys
                // (`override`, per-cop inherit_mode) are not modelled.
                if let Yaml::Hash(modes) = value
                    && let Some(merge) = modes.get(&Yaml::String("merge".to_string()))
                {
                    parsed.exclude_merge = match merge {
                        Yaml::Array(arr) => arr
                            .iter()
                            .any(|v| matches!(v, Yaml::String(s) if s == "Exclude")),
                        Yaml::String(s) => s == "Exclude",
                        _ => false,
                    };
                }
            }
            _ => {
                if let Yaml::Hash(cop_map) = value {
                    let rule = parse_cop_rule(cop_map);
                    validate_glob_patterns(&rule.include)?;
                    validate_glob_patterns(&rule.exclude)?;
                    parsed.rules.insert(section, rule);
                }
            }
        }
    }

    Ok(parsed)
}

/// Recursively load a config file resolving its `inherit_from` chain.
///
/// `seen` is the set of canonicalized paths already on the current call stack;
/// it grows as we recurse and is cloned for each branch so siblings don't
/// interfere with each other's cycle detection.
///
/// Merge priority (highest first):
/// 1. The file at `config_path` itself.
/// 2. Files listed in `inherit_from` (later entries win over earlier ones).
/// 3. Recursively resolved children of each inherited file.
fn load_resolving_inherit(
    config_path: &Path,
    seen: &std::collections::HashSet<PathBuf>,
) -> Result<ParsedYaml, ConfigError> {
    let canonical = std::fs::canonicalize(config_path)
        .map_err(|e| ConfigError::Io(format!("cannot read {}: {e}", config_path.display())))?;

    if seen.contains(&canonical) {
        return Err(ConfigError::BadYaml(format!(
            "inherit_from: cycle detected involving {}",
            config_path.display()
        )));
    }

    let text = std::fs::read_to_string(config_path)
        .map_err(|e| ConfigError::Io(format!("cannot read {}: {e}", config_path.display())))?;

    let mut parsed = parse_yaml_str(&text)?;
    let inherit_paths = std::mem::take(&mut parsed.inherit_from);

    if inherit_paths.is_empty() {
        return Ok(parsed);
    }

    let mut new_seen = seen.clone();
    new_seen.insert(canonical);

    let base_dir = config_path.parent().unwrap_or(Path::new("."));
    let mut inherited = ParsedYaml::default();

    for rel_path in inherit_paths {
        let child_path = base_dir.join(&rel_path);
        let child = load_resolving_inherit(&child_path, &new_seen)?;
        // Later files win: merge child over accumulated inherited.
        inherited = child.merge_over(inherited);
    }

    // Current file is highest priority.
    Ok(parsed.merge_over(inherited))
}

/// Parse a `plugins:` value (sequence or scalar string).
fn parse_plugins(value: yaml_rust2::Yaml) -> Result<Vec<PluginConfig>, String> {
    use yaml_rust2::Yaml;
    match value {
        Yaml::String(s) => Ok(vec![PluginConfig::Name(s)]),
        Yaml::Array(arr) => arr.into_iter().map(parse_plugin_entry).collect(),
        Yaml::Null => Ok(vec![]),
        _ => Err("`plugins:` must be a sequence or string".to_string()),
    }
}

fn parse_plugin_entry(yaml: yaml_rust2::Yaml) -> Result<PluginConfig, String> {
    use yaml_rust2::Yaml;
    match yaml {
        Yaml::String(s) => Ok(PluginConfig::Name(s)),
        Yaml::Hash(mut m) => {
            let name = match m.remove(&Yaml::String("name".to_string())) {
                Some(Yaml::String(s)) => s,
                Some(_) => return Err("plugin `name` must be a string".to_string()),
                None => return Err("plugin entry missing required field `name`".to_string()),
            };
            let path = match m.remove(&Yaml::String("path".to_string())) {
                Some(Yaml::String(s)) => PathBuf::from(s),
                Some(_) => return Err("plugin `path` must be a string".to_string()),
                None => return Err("plugin entry missing required field `path`".to_string()),
            };
            if !m.is_empty() {
                let unknown_key = m
                    .keys()
                    .next()
                    .map(|k| match k {
                        Yaml::String(s) => s.clone(),
                        _ => format!("{k:?}"),
                    })
                    .unwrap_or_default();
                return Err(format!("unknown field `{unknown_key}` in plugin entry"));
            }
            Ok(PluginConfig::Detailed(PluginDetailed { name, path }))
        }
        _ => Err("plugin entry must be a string or mapping".to_string()),
    }
}

fn parse_cop_rule(map: yaml_rust2::yaml::Hash) -> CopRule {
    use yaml_rust2::Yaml;
    let mut rule = CopRule::default();
    for (key, value) in map {
        let Yaml::String(k) = key else {
            continue;
        };
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
            other => {
                if let Some(json_val) = yaml_to_json(value) {
                    rule.options.insert(other.to_string(), json_val);
                }
            }
        }
    }
    rule
}

fn parse_ruby_version(yaml: &yaml_rust2::Yaml) -> Option<RubyVersion> {
    use yaml_rust2::Yaml;
    let raw = match yaml {
        Yaml::String(s) | Yaml::Real(s) => s.as_str(),
        Yaml::Integer(i) => {
            return u16::try_from(*i)
                .ok()
                .map(|major| RubyVersion::new(major, 0));
        }
        _ => return None,
    };
    let mut parts = raw.split('.');
    let major = parts.next()?;
    let minor = parts.next().unwrap_or("0");
    Some(RubyVersion::new(major.parse().ok()?, minor.parse().ok()?))
}

fn globset_matches(patterns: &[String], path: &Path) -> bool {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::sync::Arc;

    thread_local! {
        static GLOBSET_CACHE: RefCell<HashMap<Vec<String>, Arc<globset::GlobSet>>> =
            RefCell::new(HashMap::new());
    }

    GLOBSET_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let set = if let Some(set) = cache.get(patterns) {
            Arc::clone(set)
        } else {
            let mut builder = globset::GlobSetBuilder::new();
            for pattern in patterns {
                let Ok(glob) = globset::Glob::new(pattern) else {
                    continue;
                };
                builder.add(glob);
            }
            let Ok(set) = builder.build() else {
                return false;
            };
            let set = Arc::new(set);
            cache.insert(patterns.to_vec(), Arc::clone(&set));
            set
        };
        set.is_match(path)
    })
}

fn validate_glob_patterns(patterns: &[String]) -> Result<(), ConfigError> {
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in patterns {
        let glob = globset::Glob::new(pattern)
            .map_err(|e| ConfigError::BadGlob(format!("{pattern:?}: {e}")))?;
        builder.add(glob);
    }
    builder
        .build()
        .map(|_| ())
        .map_err(|e| ConfigError::BadGlob(e.to_string()))
}

/// Convert a yaml-rust2 `Yaml` value to a `serde_json::Value`.
///
/// Returns `None` for YAML-specific types with no JSON equivalent (aliases,
/// bad values) and for Infinity/NaN floats (not representable in JSON).
fn yaml_to_json(yaml: yaml_rust2::Yaml) -> Option<serde_json::Value> {
    use yaml_rust2::Yaml;
    match yaml {
        Yaml::String(s) => Some(serde_json::Value::String(s)),
        Yaml::Integer(i) => Some(serde_json::Value::Number(i.into())),
        Yaml::Real(s) => s
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(serde_json::Value::Number),
        Yaml::Boolean(b) => Some(serde_json::Value::Bool(b)),
        Yaml::Null => Some(serde_json::Value::Null),
        Yaml::Array(arr) => {
            let items: Vec<_> = arr.into_iter().filter_map(yaml_to_json).collect();
            Some(serde_json::Value::Array(items))
        }
        Yaml::Hash(h) => {
            let mut map = serde_json::Map::new();
            for (k, v) in h {
                let key = match k {
                    Yaml::String(s) => Some(s),
                    Yaml::Integer(i) => Some(i.to_string()),
                    Yaml::Boolean(b) => Some(b.to_string()),
                    Yaml::Real(s) => Some(s),
                    _ => None,
                };
                if let Some(key) = key
                    && let Some(val) = yaml_to_json(v)
                {
                    map.insert(key, val);
                }
            }
            Some(serde_json::Value::Object(map))
        }
        Yaml::Alias(_) | Yaml::BadValue => None,
    }
}

fn yaml_string_list(yaml: &yaml_rust2::Yaml) -> Vec<String> {
    use yaml_rust2::Yaml;
    match yaml {
        Yaml::Array(arr) => arr
            .iter()
            .filter_map(|v| {
                if let Yaml::String(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            })
            .collect(),
        Yaml::String(s) => vec![s.clone()],
        _ => Vec::new(),
    }
}

/// Migrate a `.rubocop.yml` document to `.murphy.yml` format.
///
/// Near-identity transform: injects `AllCops.CopsPath: cops` and emits plugin
/// rename hints. All cop rules pass through verbatim.
pub fn migrate_rubocop_yml_to_murphy_yml(text: &str) -> Result<String, ConfigError> {
    use yaml_rust2::{Yaml, YamlEmitter, YamlLoader};

    let docs = YamlLoader::load_from_str(text).map_err(|e| ConfigError::BadYaml(e.to_string()))?;

    let doc = match docs.into_iter().next() {
        None => return Ok(String::new()),
        Some(d) => d,
    };

    let mut top = match doc {
        Yaml::Hash(h) => h,
        _ => {
            return Err(ConfigError::BadYaml(
                "top-level document must be a mapping".to_string(),
            ));
        }
    };

    let mut plugin_names: Vec<String> = Vec::new();
    let mut unsupported_plugins: Vec<String> = Vec::new();

    // Extract and normalize `plugins:` to a sequence of string names only.
    // Unsupported entries (mapping-form, non-string items, wrong top-level
    // type) become comments; valid string names stay in the output.
    let plugins_key = Yaml::String("plugins".to_string());
    if let Some(plugins_val) = top.remove(&plugins_key) {
        let items: Vec<Yaml> = match plugins_val {
            Yaml::Array(arr) => arr,
            Yaml::String(s) => vec![Yaml::String(s)],
            Yaml::Null => vec![],
            other => {
                unsupported_plugins.push(format!("{other:?} (unsupported plugins: form)"));
                vec![]
            }
        };
        for item in items {
            match item {
                Yaml::String(s) => plugin_names.push(s),
                Yaml::Hash(m) => {
                    if let Some((Yaml::String(k), _)) = m.into_iter().next() {
                        unsupported_plugins.push(k);
                    } else {
                        unsupported_plugins.push("<empty or non-string key>".to_string());
                    }
                }
                other => {
                    unsupported_plugins.push(format!("{other:?} (non-string / non-mapping)"));
                }
            }
        }
        if !plugin_names.is_empty() {
            top.insert(
                plugins_key,
                Yaml::Array(
                    plugin_names
                        .iter()
                        .map(|n| Yaml::String(n.clone()))
                        .collect(),
                ),
            );
        }
    }

    // Inject AllCops.CopsPath = "cops" if not already set.
    let all_cops_key = Yaml::String("AllCops".to_string());
    let cops_path_key = Yaml::String("CopsPath".to_string());
    match top.get_mut(&all_cops_key) {
        Some(Yaml::Hash(all_cops)) => {
            if !all_cops.contains_key(&cops_path_key) {
                all_cops.insert(cops_path_key, Yaml::String("cops".to_string()));
            }
        }
        _ => {
            let mut all_cops_map = yaml_rust2::yaml::Hash::new();
            all_cops_map.insert(
                Yaml::String("Include".to_string()),
                Yaml::Array(vec![Yaml::String("**/*.rb".to_string())]),
            );
            all_cops_map.insert(Yaml::String("Exclude".to_string()), Yaml::Array(vec![]));
            all_cops_map.insert(cops_path_key, Yaml::String("cops".to_string()));
            top.insert(all_cops_key, Yaml::Hash(all_cops_map));
        }
    }

    let mut out = String::new();

    if !plugin_names.is_empty() {
        // RuboCop's `rubocop-X` plugin names are not auto-renamed to `murphy-X`
        // (ADR 0041). Surface this so the user fixes names before the first run.
        out.push_str(
            "# NOTE: RuboCop `rubocop-X` plugin names must be renamed to `murphy-X` \
             manually — Murphy does not auto-translate the prefix.\n",
        );
    }
    for unsupported in &unsupported_plugins {
        let sanitized: String = unsupported
            .chars()
            .map(|c| if c.is_control() { '?' } else { c })
            .collect();
        out.push_str(&format!("# unsupported plugin entry: {sanitized}\n"));
    }

    let mut yaml_out = String::new();
    YamlEmitter::new(&mut yaml_out)
        .dump(&Yaml::Hash(top))
        .map_err(|e| ConfigError::BadYaml(e.to_string()))?;

    // Strip the leading "---\n" document separator that YamlEmitter always emits.
    let yaml_body = yaml_out.strip_prefix("---\n").unwrap_or(&yaml_out);
    out.push_str(yaml_body);

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_defaults() {
        let cfg = MurphyConfig::from_yaml_str("").expect("empty config parses");
        assert_eq!(cfg.files.include, vec!["**/*.rb"]);
        assert_eq!(cfg.cops.path, PathBuf::from("cops"));
        assert_eq!(cfg.target_ruby_version, RubyVersion::new(3, 1));
        assert_eq!(cfg.target_rails_version, None);
        assert!(cfg.cops.rules.is_empty());
    }

    #[test]
    fn parses_target_ruby_version_from_all_cops() {
        let cfg = MurphyConfig::from_yaml_str("AllCops:\n  TargetRubyVersion: 2.7\n")
            .expect("config parses");
        assert_eq!(cfg.target_ruby_version, RubyVersion::new(2, 7));

        let cfg = MurphyConfig::from_yaml_str("AllCops:\n  TargetRubyVersion: 3.2.2\n")
            .expect("config parses");
        assert_eq!(cfg.target_ruby_version, RubyVersion::new(3, 2));
    }

    #[test]
    fn parses_target_rails_version_from_all_cops() {
        let cfg = MurphyConfig::from_yaml_str("AllCops:\n  TargetRailsVersion: 5.2\n")
            .expect("config parses");
        assert_eq!(cfg.target_rails_version, Some(RubyVersion::new(5, 2)));

        let cfg = MurphyConfig::from_yaml_str("AllCops:\n  TargetRailsVersion: 7.1.3\n")
            .expect("config parses");
        assert_eq!(cfg.target_rails_version, Some(RubyVersion::new(7, 1)));
    }

    #[test]
    fn parses_active_support_extensions_enabled_from_all_cops() {
        let cfg = MurphyConfig::from_yaml_str("AllCops:\n  ActiveSupportExtensionsEnabled: true\n")
            .unwrap();
        assert!(cfg.active_support_extensions_enabled);
        let cfg = MurphyConfig::from_yaml_str("").unwrap();
        assert!(!cfg.active_support_extensions_enabled, "default false");
    }

    #[test]
    fn allcops_context_bundles_resolved_scalars() {
        // The sole aggregation point MurphyConfig -> AllCopsContext. Use
        // non-default values for *both* fields so an accidental swap or a
        // dropped field would change the asserted output. (A literal swap
        // would not compile — the fields differ in type — but an omission
        // that leaves a field at its default would slip through without this.)
        let cfg = MurphyConfig::from_yaml_str(
            "AllCops:\n  TargetRailsVersion: 5.2\n  ActiveSupportExtensionsEnabled: true\n",
        )
        .expect("config parses");

        let ctx = cfg.allcops_context();
        assert_eq!(ctx.target_rails_version, Some(RubyVersion::new(5, 2)));
        assert!(ctx.active_support_extensions_enabled);
        // And the context mirrors the config's own resolved fields exactly.
        assert_eq!(ctx.target_rails_version, cfg.target_rails_version);
        assert_eq!(
            ctx.active_support_extensions_enabled,
            cfg.active_support_extensions_enabled
        );
        // The Ruby target is always threaded as a concrete value.
        assert_eq!(ctx.target_ruby_version, Some(cfg.target_ruby_version));

        // Defaults flow through untouched.
        let default_ctx = MurphyConfig::from_yaml_str("")
            .expect("empty config parses")
            .allcops_context();
        assert_eq!(default_ctx.target_rails_version, None);
        assert!(!default_ctx.active_support_extensions_enabled);
        // Ruby target defaults to murphy's 3.1 floor (never None from config).
        assert_eq!(
            default_ctx.target_ruby_version,
            Some(RubyVersion::new(3, 1))
        );
    }

    #[test]
    fn allcops_context_resolves_indentation_width() {
        // Configured `Layout/IndentationWidth.Width` flows into the context as
        // the shared resolved indentation width (murphy-bgd8).
        let cfg = MurphyConfig::from_yaml_str("Layout/IndentationWidth:\n  Width: 4\n")
            .expect("config parses");
        assert_eq!(cfg.allcops_context().indentation_width, 4);

        // An explicit `Width: 0` is honoured, not coerced to the default 2.
        let cfg = MurphyConfig::from_yaml_str("Layout/IndentationWidth:\n  Width: 0\n")
            .expect("config parses");
        assert_eq!(cfg.allcops_context().indentation_width, 0);

        // No configuration -> RuboCop's default Width of 2.
        let cfg = MurphyConfig::from_yaml_str("").expect("empty config parses");
        assert_eq!(
            cfg.allcops_context().indentation_width,
            AllCopsContext::DEFAULT_INDENTATION_WIDTH
        );
    }

    #[test]
    fn default_cops_data_parses_active_support_flag() {
        assert_eq!(
            DefaultCopsData::from_yaml("AllCops:\n  ActiveSupportExtensionsEnabled: true\n")
                .allcops_active_support_extensions_enabled,
            Some(true),
        );
        assert_eq!(
            DefaultCopsData::from_yaml("AllCops:\n  ActiveSupportExtensionsEnabled: false\n")
                .allcops_active_support_extensions_enabled,
            Some(false),
        );
        assert_eq!(
            DefaultCopsData::from_yaml("AllCops:\n  Include:\n    - '**/*.rb'\n")
                .allcops_active_support_extensions_enabled,
            None,
        );
    }

    #[test]
    fn comment_only_file_parses_as_defaults() {
        for text in ["# just a comment\n", "---\n# comment\n", "~\n", "null\n"] {
            let cfg = MurphyConfig::from_yaml_str(text).unwrap_or_else(|e| {
                panic!("comment-only/null config must not error for {text:?}: {e}")
            });
            assert_eq!(cfg.files.include, vec!["**/*.rb"]);
            assert!(
                cfg.cops.rules.is_empty(),
                "got rules from {text:?}: {:?}",
                cfg.cops.rules
            );
        }
    }

    #[test]
    fn parses_cop_rules() {
        let cfg = MurphyConfig::from_yaml_str("Murphy/Foo:\n  Enabled: false\n  Severity: error\n")
            .expect("config parses");
        assert!(!cfg.cop_enabled("Murphy/Foo"));
        assert_eq!(cfg.severity_override("Murphy/Foo"), Some(Severity::Error));
    }

    #[test]
    fn parses_all_cops_section() {
        let cfg = MurphyConfig::from_yaml_str(
            "AllCops:\n  Include:\n    - 'lib/**/*.rb'\n  Exclude:\n    - 'vendor/**'\n  CopsPath: custom_cops\n",
        )
        .expect("config parses");
        assert_eq!(cfg.files.include, vec!["lib/**/*.rb"]);
        assert_eq!(cfg.files.exclude, vec!["vendor/**"]);
        assert_eq!(cfg.cops.path, PathBuf::from("custom_cops"));
        assert_eq!(cfg.target_ruby_version, RubyVersion::new(3, 1));
        assert_eq!(cfg.target_rails_version, None);
    }

    #[test]
    fn cop_rule_preserves_options_as_json() {
        let cfg = MurphyConfig::from_yaml_str(
            r#"
Style/StringLiterals:
  Enabled: true
  Severity: warning
  EnforcedStyle: single_quotes
  MaxCount: 3
  Exclude:
    - db/schema.rb
"#,
        )
        .expect("config parses");

        let rule = cfg
            .cops
            .rules
            .get("Style/StringLiterals")
            .expect("rule exists");
        assert_eq!(rule.enabled, Some(true));
        assert_eq!(rule.severity, Some(Severity::Warning));
        assert_eq!(
            rule.options.get("EnforcedStyle"),
            Some(&serde_json::Value::String("single_quotes".to_string()))
        );
        assert_eq!(
            rule.options.get("MaxCount"),
            Some(&serde_json::Value::Number(3.into()))
        );
        assert_eq!(rule.exclude, vec!["db/schema.rb"]);
        assert!(!rule.options.contains_key("Exclude"));
    }

    #[test]
    fn parses_cop_rule_include_exclude_as_path_scope() {
        let cfg = MurphyConfig::from_yaml_str(
            r#"
Style/StringLiterals:
  Include:
    - spec/**/*.rb
  Exclude:
    - spec/support/**
  EnforcedStyle: single_quotes
"#,
        )
        .expect("config parses");

        let rule = cfg
            .cops
            .rules
            .get("Style/StringLiterals")
            .expect("rule exists");
        assert_eq!(rule.include, vec!["spec/**/*.rb"]);
        assert_eq!(rule.exclude, vec!["spec/support/**"]);
        assert!(rule.options.contains_key("EnforcedStyle"));
        assert!(!rule.options.contains_key("Include"));
        assert!(!rule.options.contains_key("Exclude"));
    }

    #[test]
    fn cop_applies_to_file_honors_rule_include_exclude() {
        let cfg = MurphyConfig::from_yaml_str(
            r#"
Style/StringLiterals:
  Include:
    - spec/**/*.rb
  Exclude:
    - spec/support/**
"#,
        )
        .expect("config parses");

        assert!(cfg.cop_applies_to_file(
            "Style/StringLiterals",
            Path::new("spec/models/user_spec.rb")
        ));
        assert!(!cfg.cop_applies_to_file("Style/StringLiterals", Path::new("app/models/user.rb")));
        assert!(
            !cfg.cop_applies_to_file("Style/StringLiterals", Path::new("spec/support/factory.rb"))
        );
    }

    #[test]
    fn cop_rule_bad_glob_is_config_error() {
        let err = MurphyConfig::from_yaml_str(
            r#"
Style/StringLiterals:
  Include:
    - '[bad'
"#,
        )
        .expect_err("bad cop-level glob should error");
        assert!(
            matches!(err, ConfigError::BadGlob(_)),
            "expected BadGlob, got {err:?}"
        );
    }

    #[test]
    fn cop_options_json_roundtrip() {
        let cfg =
            MurphyConfig::from_yaml_str("Style/Foo:\n  EnforcedStyle: compact\n  MaxLength: 120\n")
                .expect("config parses");
        let json = cfg.cop_options_json("Style/Foo");
        let parsed: serde_json::Value = serde_json::from_slice(&json).expect("valid JSON");
        assert_eq!(parsed["EnforcedStyle"], "compact");
        assert_eq!(parsed["MaxLength"], 120);
    }

    #[test]
    fn disabled_cop_names_lists_enabled_false_rules() {
        let cfg = MurphyConfig::from_yaml_str(
            "Style/StringLiterals:\n  Enabled: false\nLint/Debugger:\n  Enabled: true\n",
        )
        .expect("config parses");
        let names: Vec<&str> = cfg.disabled_cop_names().collect();
        assert!(names.contains(&"Style/StringLiterals"));
        assert!(!names.contains(&"Lint/Debugger"));
    }

    #[test]
    fn cop_enabled_with_cop_default_disables_rails_stubs() {
        // Rails cop stubs have PluginCopV1.default_enabled = Some(false).
        // cop_enabled_with_cop_default honours this as the 3rd fallback layer.
        let cfg = MurphyConfig::from_yaml_str("").expect("empty config parses");
        const SAMPLE: [&str; 5] = [
            "Rails/ActionControllerFlashBeforeRender",
            "Rails/ActionFilter",
            "Rails/DefaultScope",
            "Rails/SaveBang",
            "Rails/UnusedIgnoredColumns",
        ];
        for name in SAMPLE {
            assert!(
                !cfg.cop_enabled_with_cop_default(name, Some(false)),
                "{name} should be disabled when cop_default is Some(false)"
            );
        }
        // Without a cop_default hint, config layer defaults to enabled.
        assert!(cfg.cop_enabled("Unknown/Foo"));
    }

    #[test]
    fn cop_enabled_user_override_beats_cop_default() {
        let cfg = MurphyConfig::from_yaml_str("Rails/ActionFilter:\n  Enabled: true\n")
            .expect("config parses");
        // User explicit Enabled: true wins even if cop_default says false.
        assert!(cfg.cop_enabled_with_cop_default("Rails/ActionFilter", Some(false)));
    }

    #[test]
    fn plugins_default_to_empty() {
        let cfg = MurphyConfig::from_yaml_str("").expect("empty config parses");
        assert!(cfg.plugins.is_empty());
    }

    #[test]
    fn parses_plugins_name_only_form() {
        let cfg =
            MurphyConfig::from_yaml_str("plugins:\n  - murphy-rails\n").expect("config parses");
        assert_eq!(cfg.plugins.len(), 1);
        assert!(matches!(&cfg.plugins[0], PluginConfig::Name(n) if n == "murphy-rails"));
    }

    #[test]
    fn parses_plugins_detailed_form() {
        let cfg = MurphyConfig::from_yaml_str(
            "plugins:\n  - name: murphy-example-pack\n    path: target/debug/libmurphy_example_pack.so\n",
        )
        .expect("config parses");
        assert_eq!(cfg.plugins.len(), 1);
        match &cfg.plugins[0] {
            PluginConfig::Detailed(d) => {
                assert_eq!(d.name, "murphy-example-pack");
                assert_eq!(
                    d.path.to_str(),
                    Some("target/debug/libmurphy_example_pack.so")
                );
            }
            other => panic!("expected Detailed, got {other:?}"),
        }
    }

    #[test]
    fn parses_plugins_heterogeneous_array() {
        let cfg = MurphyConfig::from_yaml_str(
            "plugins:\n  - murphy-rails\n  - name: local-pack\n    path: ./libfoo.so\n",
        )
        .expect("config parses");
        assert_eq!(cfg.plugins.len(), 2);
        assert!(matches!(&cfg.plugins[0], PluginConfig::Name(n) if n == "murphy-rails"));
        assert!(matches!(&cfg.plugins[1], PluginConfig::Detailed(d) if d.name == "local-pack"));
    }

    #[test]
    fn parses_plugins_scalar_form() {
        // `plugins: murphy-rails` (scalar) — same as a one-element list.
        let cfg = MurphyConfig::from_yaml_str("plugins: murphy-rails\n").expect("config parses");
        assert_eq!(cfg.plugins.len(), 1);
        assert!(matches!(&cfg.plugins[0], PluginConfig::Name(n) if n == "murphy-rails"));
    }

    #[test]
    fn plugins_detailed_rejects_unknown_field() {
        let err = MurphyConfig::from_yaml_str(
            "plugins:\n  - name: x\n    path: \"y\"\n    version: \"0.1\"\n",
        )
        .expect_err("unknown field should error");
        assert!(
            err.to_string().contains("version"),
            "error should mention unknown field: {err}"
        );
    }

    #[test]
    fn plugins_detailed_missing_path_yields_clear_error() {
        let err = MurphyConfig::from_yaml_str("plugins:\n  - name: x\n")
            .expect_err("missing path should error");
        assert!(
            err.to_string().contains("path"),
            "error should mention missing field: {err}"
        );
    }

    // --- migrate tests ---

    #[test]
    fn migrate_injects_cops_path_into_all_cops() {
        let out =
            migrate_rubocop_yml_to_murphy_yml("AllCops:\n  Include:\n    - '**/*.rb'\n").unwrap();
        assert!(
            out.contains("CopsPath"),
            "CopsPath should be injected:\n{out}"
        );
        assert!(
            out.contains("cops"),
            "CopsPath value should be 'cops':\n{out}"
        );
    }

    #[test]
    fn migrate_creates_all_cops_section_if_absent() {
        let out = migrate_rubocop_yml_to_murphy_yml("Style/NoPuts:\n  Enabled: false\n").unwrap();
        assert!(out.contains("AllCops"), "AllCops should be created:\n{out}");
        assert!(
            out.contains("CopsPath"),
            "CopsPath should be present:\n{out}"
        );
    }

    #[test]
    fn migrate_cop_rules_pass_through() {
        let out = migrate_rubocop_yml_to_murphy_yml(
            "Style/NoPuts:\n  Enabled: false\n  Severity: error\n",
        )
        .unwrap();
        assert!(out.contains("Style/NoPuts"), "cop name:\n{out}");
        assert!(out.contains("Enabled"), "Enabled:\n{out}");
        assert!(out.contains("false"), "false:\n{out}");
    }

    #[test]
    fn migrate_plugins_emits_rename_hint() {
        let out =
            migrate_rubocop_yml_to_murphy_yml("plugins:\n  - rubocop-rails\n  - rubocop-rspec\n")
                .unwrap();
        assert!(
            out.contains("rubocop-rails") && out.contains("rubocop-rspec"),
            "plugin names preserved:\n{out}"
        );
        assert!(
            out.contains("# NOTE:") && out.contains("rubocop-") && out.contains("murphy-"),
            "rename hint:\n{out}"
        );
    }

    #[test]
    fn migrate_scalar_plugins_normalizes_to_sequence() {
        let out = migrate_rubocop_yml_to_murphy_yml("plugins: rubocop-rails\n").unwrap();
        assert!(out.contains("rubocop-rails"), "name present:\n{out}");
        assert!(out.contains("# NOTE:"), "hint present:\n{out}");
        // Must roundtrip through from_yaml_str
        let cfg = MurphyConfig::from_yaml_str(&out).expect("migrated output must load");
        assert_eq!(cfg.plugins.len(), 1);
        assert!(matches!(&cfg.plugins[0], PluginConfig::Name(n) if n == "rubocop-rails"));
    }

    #[test]
    fn migrate_unsupported_plugin_emits_comment() {
        let out = migrate_rubocop_yml_to_murphy_yml(
            "plugins:\n  - rubocop-rails\n  - foo:\n      option: x\n",
        )
        .unwrap();
        assert!(
            out.contains("# unsupported plugin entry: foo"),
            "unsupported comment:\n{out}"
        );
        assert!(
            out.contains("rubocop-rails"),
            "valid name still present:\n{out}"
        );
    }

    #[test]
    fn migrate_unsupported_name_sanitized() {
        let input = "plugins:\n  - \"evil\\n[malicious]\":\n      foo: bar\n";
        let out = migrate_rubocop_yml_to_murphy_yml(input).unwrap();
        let unsupported_lines: Vec<&str> = out
            .lines()
            .filter(|l| l.starts_with("# unsupported plugin entry:"))
            .collect();
        assert_eq!(unsupported_lines.len(), 1, "exactly 1 comment:\n{out}");
        assert!(
            unsupported_lines[0].contains('?'),
            "control chars replaced with ?:\n{out}"
        );
    }

    #[test]
    fn migrate_empty_mapping_plugin_emits_unsupported() {
        let out = migrate_rubocop_yml_to_murphy_yml("plugins:\n  - {}\n").unwrap();
        assert!(
            out.contains("# unsupported plugin entry: <empty or non-string key>"),
            "empty mapping:\n{out}"
        );
    }

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
        assert!(
            !rule.options.contains_key("Description"),
            "Description must not be in options"
        );
        assert!(
            !rule.options.contains_key("VersionAdded"),
            "VersionAdded must not be in options"
        );
        assert!(
            rule.options.contains_key("EnforcedStyle"),
            "EnforcedStyle must be in options"
        );
    }

    #[test]
    fn default_cops_data_parses_per_cop_include_exclude() {
        let yaml = "Bundler/Foo:\n  Enabled: true\n  Include:\n    - '**/Gemfile'\n  Exclude:\n    - 'vendor/**'\n";
        let data = DefaultCopsData::from_yaml(yaml);
        let rule = data.cop_rules.get("Bundler/Foo").expect("rule");
        assert_eq!(rule.include, vec!["**/Gemfile"]);
        assert_eq!(rule.exclude, vec!["vendor/**"]);
    }

    #[test]
    fn default_cops_data_parses_severity() {
        let yaml = "Bundler/Foo:\n  Enabled: true\n  Severity: warning\n";
        let data = DefaultCopsData::from_yaml(yaml);
        let rule = data.cop_rules.get("Bundler/Foo").expect("rule");
        assert_eq!(rule.severity, Some(crate::Severity::Warning));
    }

    // --- inherit_from tests ---

    fn write_cfg(dir: &Path, name: &str, content: &str) {
        std::fs::write(dir.join(name), content).unwrap();
    }

    #[test]
    fn inherit_from_single_file_inherits_cop_rule() {
        let dir = tempfile::TempDir::new().unwrap();
        write_cfg(
            dir.path(),
            "base.yml",
            "Style/StringLiterals:\n  Enabled: false\n",
        );
        write_cfg(dir.path(), ".murphy.yml", "inherit_from: base.yml\n");
        let cfg = MurphyConfig::load(dir.path()).expect("load succeeds");
        assert!(
            !cfg.cop_enabled("Style/StringLiterals"),
            "should inherit Enabled: false"
        );
    }

    #[test]
    fn inherit_from_current_file_overrides_inherited() {
        let dir = tempfile::TempDir::new().unwrap();
        write_cfg(
            dir.path(),
            "base.yml",
            "Style/StringLiterals:\n  Enabled: false\n",
        );
        write_cfg(
            dir.path(),
            ".murphy.yml",
            "inherit_from: base.yml\nStyle/StringLiterals:\n  Enabled: true\n",
        );
        let cfg = MurphyConfig::load(dir.path()).expect("load succeeds");
        assert!(
            cfg.cop_enabled("Style/StringLiterals"),
            "current file should win"
        );
    }

    #[test]
    fn inherit_from_multiple_files_later_wins() {
        let dir = tempfile::TempDir::new().unwrap();
        write_cfg(
            dir.path(),
            "a.yml",
            "Style/StringLiterals:\n  Enabled: false\n",
        );
        write_cfg(
            dir.path(),
            "b.yml",
            "Style/StringLiterals:\n  Enabled: true\n",
        );
        write_cfg(
            dir.path(),
            ".murphy.yml",
            "inherit_from:\n  - a.yml\n  - b.yml\n",
        );
        let cfg = MurphyConfig::load(dir.path()).expect("load succeeds");
        assert!(
            cfg.cop_enabled("Style/StringLiterals"),
            "b.yml (later) should win over a.yml"
        );
    }

    #[test]
    fn inherit_from_recursive() {
        let dir = tempfile::TempDir::new().unwrap();
        write_cfg(
            dir.path(),
            "grandparent.yml",
            "Style/StringLiterals:\n  Enabled: false\n",
        );
        write_cfg(dir.path(), "parent.yml", "inherit_from: grandparent.yml\n");
        write_cfg(dir.path(), ".murphy.yml", "inherit_from: parent.yml\n");
        let cfg = MurphyConfig::load(dir.path()).expect("load succeeds");
        assert!(
            !cfg.cop_enabled("Style/StringLiterals"),
            "grandparent value should reach .murphy.yml"
        );
    }

    #[test]
    fn inherit_from_cycle_detection() {
        let dir = tempfile::TempDir::new().unwrap();
        write_cfg(dir.path(), "a.yml", "inherit_from: b.yml\n");
        write_cfg(dir.path(), "b.yml", "inherit_from: a.yml\n");
        write_cfg(dir.path(), ".murphy.yml", "inherit_from: a.yml\n");
        let err = MurphyConfig::load(dir.path()).expect_err("cycle should error");
        assert!(
            matches!(err, ConfigError::BadYaml(_)),
            "cycle must be a BadYaml error, got {err:?}"
        );
        assert!(
            err.to_string().contains("cycle") || err.to_string().contains("inherit"),
            "error message should mention cycle/inherit, got: {err}"
        );
    }

    #[test]
    fn inherit_from_missing_file_errors() {
        let dir = tempfile::TempDir::new().unwrap();
        write_cfg(dir.path(), ".murphy.yml", "inherit_from: nonexistent.yml\n");
        let err = MurphyConfig::load(dir.path()).expect_err("missing file should error");
        assert!(
            matches!(err, ConfigError::Io(_)),
            "missing file must be an Io error, got {err:?}"
        );
    }

    #[test]
    fn inherit_from_merges_cop_options() {
        let dir = tempfile::TempDir::new().unwrap();
        write_cfg(
            dir.path(),
            "base.yml",
            "Style/StringLiterals:\n  Enabled: true\n  EnforcedStyle: single_quotes\n",
        );
        write_cfg(
            dir.path(),
            ".murphy.yml",
            "inherit_from: base.yml\nStyle/StringLiterals:\n  MaxCount: 5\n",
        );
        let cfg = MurphyConfig::load(dir.path()).expect("load succeeds");
        let json = cfg.cop_options_json("Style/StringLiterals");
        let val: serde_json::Value = serde_json::from_slice(&json).unwrap();
        assert_eq!(
            val["EnforcedStyle"], "single_quotes",
            "base option should be inherited"
        );
        assert_eq!(val["MaxCount"], 5, "current file option should be present");
    }

    #[test]
    fn inherit_from_path_relative_to_config_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let sub = dir.path().join("config");
        std::fs::create_dir(&sub).unwrap();
        write_cfg(
            &sub,
            "base.yml",
            "Style/StringLiterals:\n  Enabled: false\n",
        );
        write_cfg(dir.path(), ".murphy.yml", "inherit_from: config/base.yml\n");
        let cfg = MurphyConfig::load(dir.path()).expect("load succeeds");
        assert!(
            !cfg.cop_enabled("Style/StringLiterals"),
            "relative path should resolve correctly"
        );
    }

    #[test]
    fn inherit_from_allcops_include_inherited() {
        let dir = tempfile::TempDir::new().unwrap();
        write_cfg(
            dir.path(),
            "base.yml",
            "AllCops:\n  Include:\n    - lib/**/*.rb\n",
        );
        write_cfg(dir.path(), ".murphy.yml", "inherit_from: base.yml\n");
        let cfg = MurphyConfig::load(dir.path()).expect("load succeeds");
        assert_eq!(cfg.files.include, vec!["lib/**/*.rb"]);
    }

    #[test]
    fn inherit_from_allcops_current_overrides_inherited() {
        let dir = tempfile::TempDir::new().unwrap();
        write_cfg(
            dir.path(),
            "base.yml",
            "AllCops:\n  Include:\n    - lib/**/*.rb\n",
        );
        write_cfg(
            dir.path(),
            ".murphy.yml",
            "inherit_from: base.yml\nAllCops:\n  Include:\n    - app/**/*.rb\n",
        );
        let cfg = MurphyConfig::load(dir.path()).expect("load succeeds");
        assert_eq!(cfg.files.include, vec!["app/**/*.rb"]);
    }

    // --- apply_pack_default_layers tests ---

    #[test]
    fn pack_layer_flips_active_support_default() {
        // user did not set it; a pack layer says true -> resolves true.
        let mut cfg = MurphyConfig::from_yaml_str("").unwrap();
        cfg.apply_pack_default_layers(&["AllCops:\n  ActiveSupportExtensionsEnabled: true\n"]);
        assert!(cfg.active_support_extensions_enabled);
    }

    #[test]
    fn user_value_beats_pack_layer() {
        let mut cfg =
            MurphyConfig::from_yaml_str("AllCops:\n  ActiveSupportExtensionsEnabled: false\n")
                .unwrap();
        cfg.apply_pack_default_layers(&["AllCops:\n  ActiveSupportExtensionsEnabled: true\n"]);
        assert!(
            !cfg.active_support_extensions_enabled,
            "explicit user false wins"
        );
    }

    #[test]
    fn no_pack_layer_leaves_default_false() {
        let mut cfg = MurphyConfig::from_yaml_str("").unwrap();
        cfg.apply_pack_default_layers(&[]);
        assert!(!cfg.active_support_extensions_enabled);
    }

    #[test]
    fn keyless_pack_layer_preserves_earlier_value() {
        let mut cfg = MurphyConfig::from_yaml_str("").unwrap();
        cfg.apply_pack_default_layers(&[
            "AllCops:\n  ActiveSupportExtensionsEnabled: true\n",
            "AllCops:\n  Include:\n    - 'x'\n", // no ASE key
        ]);
        assert!(
            cfg.active_support_extensions_enabled,
            "keyless layer must not clobber earlier value"
        );
    }

    #[test]
    fn later_pack_layer_overrides_earlier() {
        // earlier true, later false -> later wins (when user did not set).
        let mut cfg = MurphyConfig::from_yaml_str("").unwrap();
        cfg.apply_pack_default_layers(&[
            "AllCops:\n  ActiveSupportExtensionsEnabled: true\n",
            "AllCops:\n  ActiveSupportExtensionsEnabled: false\n",
        ]);
        assert!(
            !cfg.active_support_extensions_enabled,
            "later layer overrides earlier"
        );
    }

    #[test]
    fn pack_layer_merges_per_cop_exclude_into_base_defaults() {
        // A pack's per-cop Exclude (e.g. rubocop-rspec's RSpec/DescribeClass)
        // must flow into base_defaults so cop_applies_to_file honours it.
        let mut cfg = MurphyConfig::from_yaml_str("").unwrap();
        cfg.apply_pack_default_layers(&[
            "RSpec/DescribeClass:\n  Exclude:\n    - '**/spec/requests/**/*'\n",
        ]);
        assert!(
            !cfg.cop_applies_to_file(
                "RSpec/DescribeClass",
                Path::new("spec/requests/foo_spec.rb")
            ),
            "pack Exclude must gate the cop in spec/requests"
        );
        assert!(
            cfg.cop_applies_to_file("RSpec/DescribeClass", Path::new("spec/models/foo_spec.rb")),
            "pack Exclude must not over-exclude spec/models"
        );
    }

    #[test]
    fn user_cop_exclude_beats_pack_layer() {
        // User config sets a different Exclude; the user's field wins.
        let mut cfg = MurphyConfig::from_yaml_str(
            "RSpec/DescribeClass:\n  Exclude:\n    - '**/spec/models/**/*'\n",
        )
        .unwrap();
        cfg.apply_pack_default_layers(&[
            "RSpec/DescribeClass:\n  Exclude:\n    - '**/spec/requests/**/*'\n",
        ]);
        assert!(
            cfg.cop_applies_to_file(
                "RSpec/DescribeClass",
                Path::new("spec/requests/foo_spec.rb")
            ),
            "user Exclude wins: requests not excluded"
        );
        assert!(
            !cfg.cop_applies_to_file("RSpec/DescribeClass", Path::new("spec/models/foo_spec.rb")),
            "user Exclude wins: models excluded"
        );
    }

    #[test]
    fn per_cop_merge_runs_even_when_user_pinned_active_support() {
        // The per-cop merge must NOT be gated by the ActiveSupport early-return.
        let mut cfg =
            MurphyConfig::from_yaml_str("AllCops:\n  ActiveSupportExtensionsEnabled: true\n")
                .unwrap();
        cfg.apply_pack_default_layers(&[
            "RSpec/DescribeClass:\n  Exclude:\n    - '**/spec/requests/**/*'\n",
        ]);
        assert!(
            !cfg.cop_applies_to_file(
                "RSpec/DescribeClass",
                Path::new("spec/requests/foo_spec.rb")
            ),
            "per-cop merge applies regardless of ASE pin"
        );
    }

    #[test]
    fn per_cop_merge_is_idempotent() {
        let mut cfg = MurphyConfig::from_yaml_str("").unwrap();
        let layer = "RSpec/DescribeClass:\n  Exclude:\n    - '**/spec/requests/**/*'\n";
        cfg.apply_pack_default_layers(&[layer]);
        cfg.apply_pack_default_layers(&[layer]);
        let rule = cfg
            .base_defaults
            .cop_rules
            .get("RSpec/DescribeClass")
            .expect("rule merged");
        assert_eq!(rule.exclude, vec!["**/spec/requests/**/*".to_string()]);
    }

    #[test]
    fn inherited_user_false_beats_pack_layer_on_disk() {
        // base.yml sets it false; .murphy.yml inherits base + lists the rails pack.
        // The inherited value is still a *user* setting, so the pack layer must
        // NOT override it. Pins that user_set is computed AFTER merge_over.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("base.yml"),
            "AllCops:\n  ActiveSupportExtensionsEnabled: false\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join(".murphy.yml"),
            "inherit_from: base.yml\nplugins:\n  - murphy-rails\n",
        )
        .unwrap();
        let mut cfg = MurphyConfig::load(dir.path()).unwrap();
        cfg.apply_pack_default_layers(&["AllCops:\n  ActiveSupportExtensionsEnabled: true\n"]);
        assert!(
            !cfg.active_support_extensions_enabled,
            "inherited user false wins over pack layer"
        );
    }

    // --- AllCops.Exclude base-layer union model (murphy-ynoq) ---
    //
    // RuboCop (verified on Mastodon w/ rubocop-rails) treats the default
    // `AllCops.Exclude` as `std_core ∪ plugin_default`, an UNCONDITIONAL base
    // layer. The user's explicit `Exclude` then either replaces it (default) or
    // unions with it when `inherit_mode: merge: [Exclude]` is set.

    const STD_EXCLUDE_YAML: &str = "AllCops:\n  Exclude:\n    - 'vendor/**/*'\n    - 'tmp/**/*'\n";
    const PACK_EXCLUDE_YAML: &str = "AllCops:\n  Exclude:\n    - 'db/*schema.rb'\n    - 'bin/*'\n";

    #[test]
    fn pack_exclude_joins_base_layer_when_no_user_exclude() {
        // Row 1: no user Exclude -> discovery excludes = std core UNION pack.
        let mut cfg = MurphyConfig::with_defaults("", STD_EXCLUDE_YAML).unwrap();
        cfg.apply_pack_default_layers(&[PACK_EXCLUDE_YAML]);
        assert!(
            cfg.files.exclude.contains(&"vendor/**/*".to_string()),
            "std core exclude present: {:?}",
            cfg.files.exclude
        );
        assert!(
            cfg.files.exclude.contains(&"db/*schema.rb".to_string()),
            "pack exclude present: {:?}",
            cfg.files.exclude
        );
        assert!(cfg.files.exclude.contains(&"bin/*".to_string()));
    }

    #[test]
    fn user_exclude_without_inherit_mode_replaces_defaults() {
        // Row 3: user Exclude, no inherit_mode merge -> user list replaces all
        // defaults (matches RuboCop without `inherit_mode: merge: [Exclude]`).
        let mut cfg = MurphyConfig::with_defaults(
            "AllCops:\n  Exclude:\n    - Vagrantfile\n",
            STD_EXCLUDE_YAML,
        )
        .unwrap();
        cfg.apply_pack_default_layers(&[PACK_EXCLUDE_YAML]);
        assert_eq!(cfg.files.exclude, vec!["Vagrantfile".to_string()]);
    }

    #[test]
    fn user_exclude_with_inherit_mode_merge_unions_defaults() {
        // Row 4: user Exclude + `inherit_mode: merge: [Exclude]` -> defaults
        // (std core ∪ pack) UNION user.
        let mut cfg = MurphyConfig::with_defaults(
            "AllCops:\n  Exclude:\n    - Vagrantfile\ninherit_mode:\n  merge:\n    - Exclude\n",
            STD_EXCLUDE_YAML,
        )
        .unwrap();
        cfg.apply_pack_default_layers(&[PACK_EXCLUDE_YAML]);
        assert!(
            cfg.files.exclude.contains(&"Vagrantfile".to_string()),
            "user exclude present: {:?}",
            cfg.files.exclude
        );
        assert!(
            cfg.files.exclude.contains(&"vendor/**/*".to_string()),
            "std core present: {:?}",
            cfg.files.exclude
        );
        assert!(
            cfg.files.exclude.contains(&"db/*schema.rb".to_string()),
            "pack present: {:?}",
            cfg.files.exclude
        );
    }

    #[test]
    fn exclude_union_dedups_repeated_patterns() {
        // A pattern present in both std defaults and user (with merge) appears once.
        let mut cfg = MurphyConfig::with_defaults(
            "AllCops:\n  Exclude:\n    - 'vendor/**/*'\ninherit_mode:\n  merge:\n    - Exclude\n",
            STD_EXCLUDE_YAML,
        )
        .unwrap();
        cfg.apply_pack_default_layers(&[]);
        let count = cfg
            .files
            .exclude
            .iter()
            .filter(|e| e.as_str() == "vendor/**/*")
            .count();
        assert_eq!(count, 1, "deduped: {:?}", cfg.files.exclude);
    }

    #[test]
    fn pack_exclude_union_is_idempotent() {
        let mut cfg = MurphyConfig::with_defaults("", STD_EXCLUDE_YAML).unwrap();
        cfg.apply_pack_default_layers(&[PACK_EXCLUDE_YAML]);
        let first = cfg.files.exclude.clone();
        cfg.apply_pack_default_layers(&[PACK_EXCLUDE_YAML]);
        assert_eq!(cfg.files.exclude, first, "idempotent across repeated calls");
    }

    #[test]
    fn pack_exclude_folds_even_when_user_pinned_active_support() {
        // The exclude base-layer fold must run BEFORE the ASE early-return, so a
        // project that pins `ActiveSupportExtensionsEnabled` still gets pack
        // discovery excludes. Mirrors `per_cop_merge_runs_even_when_user_pinned_*`
        // for the AllCops.Exclude path.
        let mut cfg = MurphyConfig::with_defaults(
            "AllCops:\n  ActiveSupportExtensionsEnabled: true\n",
            STD_EXCLUDE_YAML,
        )
        .unwrap();
        cfg.apply_pack_default_layers(&[PACK_EXCLUDE_YAML]);
        assert!(
            cfg.files.exclude.contains(&"db/*schema.rb".to_string()),
            "pack exclude folds despite the ASE early-return: {:?}",
            cfg.files.exclude
        );
    }

    #[test]
    fn parses_inherit_mode_merge_exclude() {
        // The inherit_mode flag must round-trip into the union decision even when
        // it arrives via an inherited file (root wins / OR semantics).
        let mut cfg = MurphyConfig::with_defaults(
            "AllCops:\n  Exclude:\n    - Vagrantfile\ninherit_mode:\n  merge:\n    - Exclude\n",
            STD_EXCLUDE_YAML,
        )
        .unwrap();
        // Before any pack layer, the std base already unions with the user list.
        assert!(cfg.files.exclude.contains(&"vendor/**/*".to_string()));
        assert!(cfg.files.exclude.contains(&"Vagrantfile".to_string()));
        cfg.apply_pack_default_layers(&[]);
        assert!(cfg.files.exclude.contains(&"vendor/**/*".to_string()));
    }
}
