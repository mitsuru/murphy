use crate::Severity;
use murphy_plugin_api::{AllCopsContext, RubyVersion};
use std::collections::{BTreeMap, BTreeSet};
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
    /// Option keys whose arrays union across layers, from global or cop-level
    /// `inherit_mode.merge` settings.
    pub inherit_mode_merge: BTreeSet<String>,
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
    /// Option keys whose arrays union across layers, from global or cop-level
    /// `inherit_mode.merge` settings.
    pub inherit_mode_merge: BTreeSet<String>,
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
        use yaml_rust2::Yaml;

        let docs = match load_yaml_preserving_regexp(text) {
            Ok(d) => d,
            Err(_) => return Self::default(),
        };
        let doc = match docs.into_iter().next() {
            Some(Yaml::Hash(h)) => h,
            _ => return Self::default(),
        };

        let global_merge_keys = doc
            .get(&Yaml::String("inherit_mode".to_string()))
            .map(parse_inherit_mode_merge)
            .unwrap_or_default();
        let mut result = Self::default();

        for (key, value) in doc {
            let Yaml::String(section) = key else { continue };

            if section == "inherit_mode" {
                continue;
            }

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
                let mut rule = parse_default_cop_rule(cop_map);
                rule.inherit_mode_merge
                    .extend(global_merge_keys.iter().cloned());
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
            "inherit_mode" => {
                rule.inherit_mode_merge = parse_inherit_mode_merge(&value);
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

/// Parse option keys listed by an `inherit_mode.merge` map.
fn parse_inherit_mode_merge(value: &yaml_rust2::Yaml) -> BTreeSet<String> {
    use yaml_rust2::Yaml;
    let Yaml::Hash(modes) = value else {
        return BTreeSet::new();
    };
    let Some(merge) = modes.get(&Yaml::String("merge".to_string())) else {
        return BTreeSet::new();
    };
    match merge {
        Yaml::Array(keys) => keys
            .iter()
            .filter_map(|key| match key {
                Yaml::String(key) => Some(key.clone()),
                _ => None,
            })
            .collect(),
        Yaml::String(key) => [key.clone()].into_iter().collect(),
        _ => BTreeSet::new(),
    }
}

/// Merge a later pack layer's [`DefaultCopRule`] over an existing base entry.
/// Later layer wins per field; absent fields (`None` / empty `Vec`) keep the
/// base value, so a pack that only sets `Exclude` does not wipe a base
/// `Include` for the same cop. Arrays named by `inherit_mode.merge` union in
/// base-first order.
fn merge_default_cop_rule(base: &mut DefaultCopRule, layer: DefaultCopRule) {
    let merge_keys = base
        .inherit_mode_merge
        .union(&layer.inherit_mode_merge)
        .cloned()
        .collect::<BTreeSet<_>>();

    if layer.enabled.is_some() {
        base.enabled = layer.enabled;
    }
    if layer.severity.is_some() {
        base.severity = layer.severity;
    }
    if !layer.include.is_empty() {
        base.include = if merge_keys.contains("Include") {
            union_string_lists(&base.include, &layer.include)
        } else {
            layer.include
        };
    }
    if !layer.exclude.is_empty() {
        base.exclude = if merge_keys.contains("Exclude") {
            union_string_lists(&base.exclude, &layer.exclude)
        } else {
            layer.exclude
        };
    }
    merge_option_maps(&mut base.options, layer.options, &merge_keys);
    base.inherit_mode_merge = merge_keys;
}

/// Merge option maps with RuboCop-style array union for the named keys.
fn merge_option_maps(
    base: &mut BTreeMap<String, serde_json::Value>,
    top: BTreeMap<String, serde_json::Value>,
    merge_keys: &BTreeSet<String>,
) {
    for (key, top_value) in top {
        if let Some(base_value) = base.get_mut(&key) {
            merge_option_value(base_value, top_value, merge_keys, merge_keys.contains(&key));
        } else {
            base.insert(key, top_value);
        }
    }
}

/// RuboCop recursively merges option hashes; arrays replace unless their key is
/// listed by `inherit_mode.merge` at the cop-option level. Updating existing
/// object entries in place also preserves YAML insertion order for term maps.
fn merge_option_value(
    base: &mut serde_json::Value,
    top: serde_json::Value,
    merge_keys: &BTreeSet<String>,
    merge_array: bool,
) {
    match (base, top) {
        (serde_json::Value::Object(base), serde_json::Value::Object(top)) => {
            for (key, top_value) in top {
                if let Some(base_value) = base.get_mut(&key) {
                    merge_option_value(
                        base_value,
                        top_value,
                        merge_keys,
                        merge_keys.contains(&key),
                    );
                } else {
                    base.insert(key, top_value);
                }
            }
        }
        (serde_json::Value::Array(base_items), serde_json::Value::Array(top_items))
            if merge_array =>
        {
            for item in top_items {
                if !base_items.contains(&item) {
                    base_items.push(item);
                }
            }
        }
        (base, top) => *base = top,
    }
}

/// Union string lists in layer order, dropping duplicates and preserving the
/// first occurrence's position.
fn union_string_lists(base: &[String], top: &[String]) -> Vec<String> {
    dedup_preserving_order(base.iter().chain(top.iter()).cloned())
}

/// Resolve a cop-level path-scope list, unioning layers when its key is merged.
fn resolve_cop_scope_list(
    user: Option<&CopRule>,
    defaults: Option<&DefaultCopRule>,
    key: &str,
) -> Option<Vec<String>> {
    let (user_list, default_list) = match key {
        "Include" => (
            user.map(|rule| &rule.include),
            defaults.map(|rule| &rule.include),
        ),
        "Exclude" => (
            user.map(|rule| &rule.exclude),
            defaults.map(|rule| &rule.exclude),
        ),
        _ => return None,
    };
    let user_list = user_list.filter(|list| !list.is_empty());
    let default_list = default_list.filter(|list| !list.is_empty());
    let merge = user.is_some_and(|rule| rule.inherit_mode_merge.contains(key))
        || defaults.is_some_and(|rule| rule.inherit_mode_merge.contains(key));

    if merge {
        match (default_list, user_list) {
            (Some(defaults), Some(user)) => Some(union_string_lists(defaults, user)),
            (Some(defaults), None) => Some(defaults.clone()),
            (None, Some(user)) => Some(user.clone()),
            (None, None) => None,
        }
    } else {
        user_list.or(default_list).cloned()
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

/// RuboCop's known Ruby versions (`TargetRuby::KNOWN_RUBIES` in RuboCop 1.87).
/// Used to resolve a gemspec `required_ruby_version` requirement to the minimal
/// satisfying version, mirroring `Gem::Requirement#satisfied_by?` against
/// `"X.Y.99"`.
const KNOWN_RUBIES: &[(u16, u16)] = &[
    (2, 0),
    (2, 1),
    (2, 2),
    (2, 3),
    (2, 4),
    (2, 5),
    (2, 6),
    (2, 7),
    (3, 0),
    (3, 1),
    (3, 2),
    (3, 3),
    (3, 4),
    (4, 0),
    (4, 1),
];

/// Detect `TargetRubyVersion` from project files, mirroring RuboCop's
/// `TargetRuby::SOURCES` precedence. Explicit `.murphy.yml`
/// `AllCops.TargetRubyVersion` always wins and is handled by the caller —
/// this only runs when unset.
///
/// Precedence (first hit wins, each source searched upwards from `root`):
///
/// 1. `*.gemspec` `required_ruby_version` (first directory upwards containing
///    exactly one `*.gemspec`, mirroring RuboCop's `traverse_directories_upwards`
///    + `candidates.one?` guard)
/// 2. `.ruby-version`
/// 3. `mise.toml` (`ruby = "X.Y"`)
/// 4. `.tool-versions` (`ruby X.Y` line)
/// 5. `Gemfile.lock` / `gems.locked` (`RUBY VERSION` section)
///
/// Returns `None` when nothing is found (caller keeps the default 3.1).
/// I/O errors are treated as "not found" — detection never fails a run.
pub fn detect_target_ruby_version(root: &Path) -> Option<RubyVersion> {
    if let Some(v) = detect_from_gemspec(root) {
        return Some(v);
    }
    if let Some(v) = detect_from_ruby_version_file(root) {
        return Some(v);
    }
    if let Some(v) = detect_from_mise_toml(root) {
        return Some(v);
    }
    if let Some(v) = detect_from_tool_versions(root) {
        return Some(v);
    }
    if let Some(v) = detect_from_bundler_lock(root) {
        return Some(v);
    }
    None
}

fn absolute_start_dir(root: &Path) -> PathBuf {
    let joined = if root.is_absolute() {
        root.to_path_buf()
    } else if let Ok(cwd) = std::env::current_dir() {
        cwd.join(root)
    } else {
        root.to_path_buf()
    };
    std::fs::canonicalize(&joined).unwrap_or(joined)
}

fn find_file_upwards(root: &Path, filename: &str) -> Option<PathBuf> {
    let start = absolute_start_dir(root);
    for dir in start.ancestors() {
        let candidate = dir.join(filename);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn parse_major_minor(text: &str) -> Option<RubyVersion> {
    let s = text.trim();
    let s = s.strip_prefix("ruby-").unwrap_or(s);
    let mut parts = s.split('.');
    let major_raw = parts.next()?;
    let minor_raw = parts.next()?;
    let major: u16 = leading_digits(major_raw)?.parse().ok()?;
    let minor: u16 = leading_digits(minor_raw)?.parse().ok()?;
    if major_raw.is_empty() || minor_raw.is_empty() {
        return None;
    }
    Some(RubyVersion::new(major, minor))
}

fn leading_digits(s: &str) -> Option<&str> {
    let end = s
        .char_indices()
        .find(|(_, c)| !c.is_ascii_digit())
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    if end == 0 {
        return None;
    }
    Some(&s[..end])
}

fn detect_from_ruby_version_file(root: &Path) -> Option<RubyVersion> {
    let path = find_file_upwards(root, ".ruby-version")?;
    let content = std::fs::read_to_string(path).ok()?;
    // RuboCop: /\A(?:ruby-)?(?<version>\d+\.\d+)/
    parse_major_minor(content.trim())
}

fn detect_from_tool_versions(root: &Path) -> Option<RubyVersion> {
    let path = find_file_upwards(root, ".tool-versions")?;
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        // RuboCop: /^(?:ruby )(?<version>\d+\.\d+)/
        if let Some(rest) = line.strip_prefix("ruby ")
            && let Some(v) = parse_major_minor(rest.trim())
        {
            return Some(v);
        }
    }
    None
}

fn detect_from_mise_toml(root: &Path) -> Option<RubyVersion> {
    let path = find_file_upwards(root, "mise.toml")?;
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        // Tolerate whitespace and single/double quotes; RuboCop is strict
        // (`^ruby = "X.Y`) but mise files vary (`ruby="X.Y"`, `'X.Y'`).
        let trimmed = line.trim();
        if !trimmed.starts_with("ruby") {
            continue;
        }
        let Some(eq) = trimmed.find('=') else {
            continue;
        };
        let rhs = trimmed[eq + 1..].trim();
        let quote = rhs.chars().next()?;
        if quote != '"' && quote != '\'' {
            continue;
        }
        let inner = rhs[1..].split(quote).next()?;
        if let Some(v) = parse_major_minor(inner.trim()) {
            return Some(v);
        }
    }
    None
}

fn detect_from_bundler_lock(root: &Path) -> Option<RubyVersion> {
    for name in ["Gemfile.lock", "gems.locked"] {
        if let Some(path) = find_file_upwards(root, name)
            && let Some(v) = parse_bundler_lock_file(&path)
        {
            return Some(v);
        }
    }
    None
}

fn parse_bundler_lock_file(path: &Path) -> Option<RubyVersion> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut in_ruby_section = false;
    for line in content.lines() {
        if line.trim().eq_ignore_ascii_case("RUBY VERSION") {
            in_ruby_section = true;
            continue;
        }
        if !in_ruby_section {
            continue;
        }
        let trimmed = line.trim();
        // Expect `ruby W.X.Y...` (MRI only; jruby lines carry a trailing
        // parenthetical and are ignored, mirroring RuboCop's regex).
        if let Some(rest) = trimmed.strip_prefix("ruby ") {
            let rest = rest.trim();
            if rest.contains('(') || rest.contains(' ') {
                continue;
            }
            if let Some(v) = parse_major_minor(rest) {
                return Some(v);
            }
        }
    }
    None
}

fn detect_from_gemspec(root: &Path) -> Option<RubyVersion> {
    let path = find_gemspec_path(root)?;
    parse_gemspec_required_ruby_version(&path)
}

fn find_gemspec_path(root: &Path) -> Option<PathBuf> {
    let start = absolute_start_dir(root);
    for dir in start.ancestors() {
        let entries = std::fs::read_dir(dir).ok()?;
        let mut candidates = Vec::new();
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() && p.extension().is_some_and(|e| e == "gemspec") {
                candidates.push(p);
            }
        }
        // RuboCop: `break candidates.first if candidates.one?` — zero or
        // multiple gemspecs mean "keep walking upwards".
        if candidates.len() == 1 {
            return candidates.into_iter().next();
        }
    }
    None
}

fn parse_gemspec_required_ruby_version(path: &Path) -> Option<RubyVersion> {
    let content = std::fs::read_to_string(path).ok()?;
    let requirements = extract_gemspec_requirement_strings(&content)?;
    find_minimal_known_ruby(&requirements)
}

fn extract_gemspec_requirement_strings(file_text: &str) -> Option<Vec<String>> {
    let needle = "required_ruby_version";
    let idx = file_text.find(needle)?;
    let after_needle = &file_text[idx + needle.len()..];
    let eq_rel = after_needle.find('=')?;
    let mut rhs = after_needle[eq_rel + 1..].trim_start();
    // Bound the snippet so following assignments (e.g. `rubygems_version`)
    // are not swallowed: array literal -> to `]`; `Gem::Requirement.new(..)`
    // -> to `)`; otherwise a single line.
    let snippet = if let Some(rest) = rhs.strip_prefix('[') {
        let _ = rest;
        let end = rhs.find(']')?;
        &rhs[..end + 1]
    } else if rhs.starts_with("Gem::Requirement") {
        let open_rel = rhs.find('(')?;
        let after_open = &rhs[open_rel + 1..];
        let close_rel = after_open.find(')')?;
        &rhs[..open_rel + 1 + close_rel + 1]
    } else {
        let end = rhs.find('\n').unwrap_or(rhs.len());
        &rhs[..end]
    };
    let _ = &mut rhs;
    let strings = extract_quoted_strings(snippet);
    if strings.is_empty() {
        None
    } else {
        Some(strings)
    }
}

fn extract_quoted_strings(snippet: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = snippet.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c == '\'' || c == '"' {
            let quote = c;
            i += 1;
            let mut buf = String::new();
            while i < bytes.len() {
                let d = bytes[i] as char;
                if d == '\\' && i + 1 < bytes.len() {
                    buf.push(bytes[i + 1] as char);
                    i += 2;
                    continue;
                }
                if d == quote {
                    i += 1;
                    break;
                }
                buf.push(d);
                i += 1;
            }
            // Skip `.freeze`-style trailing call noise; keep the literal.
            out.push(buf);
        } else {
            i += 1;
        }
    }
    out
}

fn find_minimal_known_ruby(requirements: &[String]) -> Option<RubyVersion> {
    let mut constraints: Vec<(String, Vec<u64>)> = Vec::new();
    for req in requirements {
        for part in req.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let (op, ver) = split_requirement_op(part)?;
            let parsed = parse_gem_version(ver)?;
            constraints.push((op, parsed));
        }
    }
    if constraints.is_empty() {
        return None;
    }
    for (major, minor) in KNOWN_RUBIES.iter().copied() {
        let candidate = vec![u64::from(major), u64::from(minor), 99];
        if constraints
            .iter()
            .all(|(op, req)| satisfies_gem_constraint(&candidate, op, req))
        {
            return Some(RubyVersion::new(major, minor));
        }
    }
    None
}

fn split_requirement_op(part: &str) -> Option<(String, &str)> {
    for op in [">=", "<=", "!=", "~>", ">", "<", "="] {
        if let Some(rest) = part.strip_prefix(op) {
            let ver = rest.trim();
            if ver.is_empty() {
                return None;
            }
            return Some((op.to_string(), ver));
        }
    }
    // Bare version implies `=`.
    Some(("=".to_string(), part.trim()))
}

fn parse_gem_version(s: &str) -> Option<Vec<u64>> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let mut segs = Vec::new();
    for part in s.split('.') {
        let digits = leading_digits(part)?;
        segs.push(digits.parse::<u64>().ok()?);
    }
    if segs.is_empty() {
        return None;
    }
    Some(segs)
}

fn compare_gem_versions(a: &[u64], b: &[u64]) -> std::cmp::Ordering {
    let len = a.len().max(b.len());
    for i in 0..len {
        let x = *a.get(i).unwrap_or(&0);
        let y = *b.get(i).unwrap_or(&0);
        match x.cmp(&y) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}

fn pessimistic_upper_bound(req: &[u64]) -> Vec<u64> {
    if req.len() <= 1 {
        return vec![req.first().copied().unwrap_or(0) + 1];
    }
    let mut upper = req[..req.len() - 1].to_vec();
    if let Some(last) = upper.last_mut() {
        *last += 1;
    }
    upper
}

fn satisfies_gem_constraint(candidate: &[u64], op: &str, req: &[u64]) -> bool {
    use std::cmp::Ordering;
    match op {
        "=" => compare_gem_versions(candidate, req) == Ordering::Equal,
        "!=" => compare_gem_versions(candidate, req) != Ordering::Equal,
        ">" => compare_gem_versions(candidate, req) == Ordering::Greater,
        ">=" => matches!(
            compare_gem_versions(candidate, req),
            Ordering::Greater | Ordering::Equal
        ),
        "<" => compare_gem_versions(candidate, req) == Ordering::Less,
        "<=" => matches!(
            compare_gem_versions(candidate, req),
            Ordering::Less | Ordering::Equal
        ),
        "~>" => {
            compare_gem_versions(candidate, req) != Ordering::Less
                && compare_gem_versions(candidate, &pessimistic_upper_bound(req)) == Ordering::Less
        }
        _ => false,
    }
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
    /// Globally merged cop-option keys from top-level `inherit_mode.merge`.
    inherit_mode_merge: BTreeSet<String>,
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
        let inherit_mode_merge = self
            .inherit_mode_merge
            .union(&base.inherit_mode_merge)
            .cloned()
            .collect::<BTreeSet<_>>();
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
                    merged.insert(
                        name,
                        merge_cop_rule(top_rule, base_rule, &inherit_mode_merge),
                    );
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
            inherit_mode_merge,
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
        let mut rules = self.rules;
        for rule in rules.values_mut() {
            rule.inherit_mode_merge
                .extend(self.inherit_mode_merge.iter().cloned());
        }
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
                rules,
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
/// Per-field, `top` wins unless the key is listed by global or cop-level
/// `inherit_mode.merge`, in which case array values union base-first.
fn merge_cop_rule(top: CopRule, base: CopRule, global_merge_keys: &BTreeSet<String>) -> CopRule {
    let cop_merge_keys = base
        .inherit_mode_merge
        .union(&top.inherit_mode_merge)
        .cloned()
        .collect::<BTreeSet<_>>();
    let merge_keys = cop_merge_keys
        .union(global_merge_keys)
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut options = base.options;
    merge_option_maps(&mut options, top.options, &merge_keys);

    CopRule {
        enabled: top.enabled.or(base.enabled),
        severity: top.severity.or(base.severity),
        include: if merge_keys.contains("Include") {
            union_string_lists(&base.include, &top.include)
        } else if top.include.is_empty() {
            base.include
        } else {
            top.include
        },
        exclude: if merge_keys.contains("Exclude") {
            union_string_lists(&base.exclude, &top.exclude)
        } else if top.exclude.is_empty() {
            base.exclude
        } else {
            top.exclude
        },
        options,
        inherit_mode_merge: cop_merge_keys,
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
            let mut cfg = Self::default();
            // No explicit config: fall back to RuboCop-style detection
            // (.ruby-version / gemspec / .tool-versions / mise.toml / lockfile).
            if let Some(detected) = detect_target_ruby_version(root) {
                cfg.target_ruby_version = detected;
            }
            return Ok(cfg);
        }
        let parsed = load_resolving_inherit(&config_path, &std::collections::HashSet::new())?;
        let explicit = parsed.target_ruby_version.is_some();
        let (mut cfg, _, _) = parsed.into_murphy_config();
        if !explicit && let Some(detected) = detect_target_ruby_version(root) {
            cfg.target_ruby_version = detected;
        }
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
        let explicit = parsed.target_ruby_version.is_some();
        let (mut cfg, saw_include, _saw_exclude) = parsed.into_murphy_config();
        if !explicit && let Some(detected) = detect_target_ruby_version(root) {
            cfg.target_ruby_version = detected;
        }
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

        // Resolve Include and Exclude independently: user setting wins per-field
        // unless that key is listed in the cop's `inherit_mode.merge`.
        let include = resolve_cop_scope_list(rule, default_rule, "Include");
        let exclude = resolve_cop_scope_list(rule, default_rule, "Exclude");

        let matches_include = match include.as_deref() {
            Some(inc) if !inc.is_empty() => globset_matches(inc, file),
            _ => true,
        };
        let matches_exclude = match exclude.as_deref() {
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
            block_forwarding_explicit: self.resolved_block_forwarding_explicit(),
            block_body_empty_lines: self.resolved_block_body_empty_lines(),
            block_braces_space: self.resolved_block_braces_space(),
            max_line_length: self.resolved_max_line_length(),
        }
    }

    /// RuboCop's `config.for_cop('Naming/BlockForwarding')['EnforcedStyle']`
    /// resolved for this config: `true` when the user's (or bundled default's)
    /// `EnforcedStyle` is `explicit`, else `false` (RuboCop's `anonymous`
    /// default). Read unconditionally — `Style/ArgumentsForwarding` consults it
    /// whether or not `Naming/BlockForwarding` is enabled.
    fn resolved_block_forwarding_explicit(&self) -> bool {
        const COP: &str = "Naming/BlockForwarding";
        let style = |opts: Option<&BTreeMap<String, serde_json::Value>>| {
            opts.and_then(|o| o.get("EnforcedStyle"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        };
        style(self.cops.rules.get(COP).map(|r| &r.options))
            .or_else(|| style(self.base_defaults.cop_rules.get(COP).map(|r| &r.options)))
            .as_deref()
            == Some("explicit")
    }

    /// RuboCop's `config.for_enabled_cop('Layout/EmptyLinesAroundBlockBody')`
    /// `['EnforcedStyle']` resolved for this config: `true` when the style is
    /// `empty_lines`, else `false` (RuboCop's `no_empty_lines` default).
    /// Mirrors `for_enabled_cop` (not `for_cop`): when the cop is disabled the
    /// lookup yields no style, so this returns `true` (treated as `empty_lines`,
    /// i.e. the in-block guards are skipped). Read by
    /// `Layout/EmptyLinesAroundAccessModifier` via `Cx::block_body_empty_lines()`
    /// (murphy-xjua).
    fn resolved_block_body_empty_lines(&self) -> bool {
        const COP: &str = "Layout/EmptyLinesAroundBlockBody";
        // `for_enabled_cop`: disabled cop yields no style → `empty_lines`
        // (guards skipped), matching RuboCop's `nil == 'no_empty_lines'` → false.
        if !self.cop_enabled(COP) {
            return true;
        }
        let style = |opts: Option<&BTreeMap<String, serde_json::Value>>| {
            opts.and_then(|o| o.get("EnforcedStyle"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        };
        style(self.cops.rules.get(COP).map(|r| &r.options))
            .or_else(|| style(self.base_defaults.cop_rules.get(COP).map(|r| &r.options)))
            .as_deref()
            == Some("empty_lines")
    }

    /// RuboCop's `config.for_cop('Layout/SpaceInsideBlockBraces')`
    /// `['EnforcedStyle']` resolved for this config: `true` when the style is
    /// `space` (RuboCop's default), `false` when `no_space`.
    /// Mirrors `for_cop` (not `for_enabled_cop`): read unconditionally whether
    /// or not the sibling cop is enabled, with an unset style falling back to
    /// `'space'`. Read by `Layout/SpaceBeforeComma` /
    /// `Layout/SpaceBeforeSemicolon` via `Cx::block_braces_space()`
    /// (murphy-4qhr).
    fn resolved_block_braces_space(&self) -> bool {
        const COP: &str = "Layout/SpaceInsideBlockBraces";
        let style = |opts: Option<&BTreeMap<String, serde_json::Value>>| {
            opts.and_then(|o| o.get("EnforcedStyle"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        };
        style(self.cops.rules.get(COP).map(|r| &r.options))
            .or_else(|| style(self.base_defaults.cop_rules.get(COP).map(|r| &r.options)))
            .as_deref()
            .unwrap_or("space")
            == "space"
    }

    /// RuboCop's `config.for_cop('Layout/SpaceInsideBlockBraces')`
    /// `['EnforcedStyle']` resolved as a wire string for host-baking into
    /// `Layout/SpaceAfterSemicolon`'s `options_json` (murphy-ilrx).
    ///
    /// Mirrors `for_cop` (not `for_enabled_cop`): user wins, else bundled
    /// default, else `'space'` (RuboCop's `cfg['EnforcedStyle'] || 'space'`).
    fn resolved_space_inside_block_braces_style(&self) -> String {
        const COP: &str = "Layout/SpaceInsideBlockBraces";
        let style = |opts: Option<&BTreeMap<String, serde_json::Value>>| {
            opts.and_then(|o| o.get("EnforcedStyle"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        };
        style(self.cops.rules.get(COP).map(|r| &r.options))
            .or_else(|| style(self.base_defaults.cop_rules.get(COP).map(|r| &r.options)))
            .unwrap_or_else(|| "space".to_string())
    }

    /// RuboCop's `config.for_cop('Layout/SpaceInsideHashLiteralBraces')`
    /// `['EnforcedStyle']` resolved as a wire string for host-baking into
    /// `Layout/SpaceAfterComma`'s `options_json` (murphy-ilrx).
    ///
    /// Mirrors `for_cop`: user wins, else bundled default, else `'space'`.
    /// The `compact` style is preserved verbatim; the cop treats any
    /// non-`no_space` style as requiring a space before `}`.
    fn resolved_space_inside_hash_literal_braces_style(&self) -> String {
        const COP: &str = "Layout/SpaceInsideHashLiteralBraces";
        let style = |opts: Option<&BTreeMap<String, serde_json::Value>>| {
            opts.and_then(|o| o.get("EnforcedStyle"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        };
        style(self.cops.rules.get(COP).map(|r| &r.options))
            .or_else(|| style(self.base_defaults.cop_rules.get(COP).map(|r| &r.options)))
            .unwrap_or_else(|| "space".to_string())
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

    /// RuboCop's `config.for_cop('Layout/LineLength')['Max']` resolved
    /// for this config: the user's `Layout/LineLength.Max` if set, else
    /// the bundled default, else [`AllCopsContext::DEFAULT_MAX_LINE_LENGTH`].
    ///
    /// Read unconditionally — RuboCop consults the cop's config whether or not
    /// `Layout/LineLength` is enabled. A configured `Max <= 0` falls back to
    /// the default 120 (mirroring `Layout/LineLength`'s own `max <= 0 → 120`
    /// guard; an explicit `Max: 0` is not a usable budget). Threaded into
    /// `CxRaw` (murphy-bgd8 pattern, murphy-y3h2) so
    /// `Style/IfUnlessModifier`, `Style/WhileUntilModifier`,
    /// `Layout/MultilineBlockLayout` and `Layout/RedundantLineBreak` need not
    /// perform their own cross-cop config lookup. Read via
    /// `Cx::max_line_length()`.
    fn resolved_max_line_length(&self) -> i64 {
        const COP: &str = "Layout/LineLength";
        let max = |opts: Option<&BTreeMap<String, serde_json::Value>>| {
            opts.and_then(|o| o.get("Max"))
                .and_then(serde_json::Value::as_i64)
        };
        let v = max(self.cops.rules.get(COP).map(|r| &r.options))
            .or_else(|| max(self.base_defaults.cop_rules.get(COP).map(|r| &r.options)))
            .unwrap_or(AllCopsContext::DEFAULT_MAX_LINE_LENGTH);
        if v <= 0 {
            AllCopsContext::DEFAULT_MAX_LINE_LENGTH
        } else {
            v
        }
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
        // murphy-ilrx (option 2, no ABI bump): host-bake the sibling
        // `EnforcedStyle` the running cop needs but cannot read via `CxRaw`.
        // The baked key is derived — the sibling's resolved style always wins
        // over any same-named key the running cop's own table may carry.
        let baked: Option<(String, serde_json::Value)> = match name {
            "Layout/SpaceAfterSemicolon" => Some((
                "SpaceInsideBlockBracesEnforcedStyle".to_string(),
                serde_json::Value::String(self.resolved_space_inside_block_braces_style()),
            )),
            "Layout/SpaceAfterComma" => Some((
                "SpaceInsideHashLiteralBracesEnforcedStyle".to_string(),
                serde_json::Value::String(self.resolved_space_inside_hash_literal_braces_style()),
            )),
            // murphy-bjrg.3 (no ABI bump): RuboCop's
            // `allowed_camel_case_file?` exempts a file matching an
            // AllCops:Include pattern containing an uppercase letter
            // (e.g. `**/Gemfile`, `**/Rakefile`). The cop cannot read the
            // merged Include list via `CxRaw`, so the host bakes the
            // matching subset here; the baked key always wins over any
            // same-named user key, mirroring the sibling-style bakings.
            "Naming/FileName" => Some((
                "AllowedCamelCaseIncludePatterns".to_string(),
                serde_json::Value::Array(
                    self.files
                        .include
                        .iter()
                        .filter(|p| p.chars().any(|c| c.is_ascii_uppercase()))
                        .map(|p| serde_json::Value::String(p.clone()))
                        .collect(),
                ),
            )),
            _ => None,
        };
        let default_rule = self.base_defaults.cop_rules.get(name);
        let user_rule = self.cops.rules.get(name);
        let default_opts = default_rule.map(|rule| &rule.options);
        let user_opts = user_rule.map(|rule| &rule.options);

        // Fast path: skip cloning and serializing when both are empty (common case).
        // Baked-sibling cops always need their derived key, so they skip it.
        if baked.is_none()
            && default_opts.is_none_or(|o| o.is_empty())
            && user_opts.is_none_or(|o| o.is_empty())
        {
            return b"{}".to_vec();
        }

        let merge_keys = default_rule
            .into_iter()
            .flat_map(|rule| rule.inherit_mode_merge.iter())
            .chain(
                user_rule
                    .into_iter()
                    .flat_map(|rule| rule.inherit_mode_merge.iter()),
            )
            .cloned()
            .collect();
        let mut merged = default_opts.cloned().unwrap_or_default();
        if let Some(rule) = user_rule {
            merge_option_maps(&mut merged, rule.options.clone(), &merge_keys);
        }
        if let Some((key, value)) = baked {
            merged.insert(key, value);
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
    use yaml_rust2::Yaml;

    let docs =
        load_yaml_preserving_regexp(text).map_err(|e| ConfigError::BadYaml(e.to_string()))?;

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
                // `Exclude` also controls the AllCops discovery-list union; the
                // full key set governs cop options during inheritance.
                parsed.inherit_mode_merge = parse_inherit_mode_merge(&value);
                parsed.exclude_merge = parsed.inherit_mode_merge.contains("Exclude");
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
            "inherit_mode" => {
                rule.inherit_mode_merge = parse_inherit_mode_merge(&value);
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

/// JSON key that preserves YAML `!ruby/regexp` tags through the
/// YAML → JSON option pipeline (murphy-e7bz.41.1).
///
/// A tagged scalar `!ruby/regexp /foo/i` becomes
/// `{"__ruby_regexp__": "/foo/i"}`; plain strings stay strings, so
/// `Regex: '/foo/'` (plain, matches literal slashes) and
/// `Regex: !ruby/regexp '/foo/'` (tagged, matches `foo`) stay
/// distinguishable after `cop_options_json`. Mirrors Ruby Psych, which
/// carries the `!ruby/regexp` tag as `Tag("!", "ruby/regexp")`.
pub(crate) const RUBY_REGEXP_JSON_KEY: &str = "__ruby_regexp__";

fn is_ruby_regexp_tag(handle: &str, suffix: &str) -> bool {
    suffix == "ruby/regexp" || suffix.ends_with("/ruby/regexp") || handle.ends_with("ruby/regexp")
}

fn parse_f64_preserving(v: &str) -> Option<f64> {
    match v {
        ".inf" | ".Inf" | ".INF" | "+.inf" | "+.Inf" | "+.INF" => Some(f64::INFINITY),
        "-.inf" | "-.Inf" | "-.INF" => Some(f64::NEG_INFINITY),
        ".nan" | ".NaN" | ".NAN" => Some(f64::NAN),
        _ if v.as_bytes().iter().any(u8::is_ascii_digit) => v.parse::<f64>().ok(),
        _ => None,
    }
}

/// Load YAML documents while preserving `!ruby/regexp` tags.
///
/// `yaml-rust2::YamlLoader` drops tags (both plain `/foo/` and tagged
/// `!ruby/regexp /foo/` become `Yaml::String("/foo/")`). This loader mirrors
/// `YamlLoader` exactly except tagged scalars become
/// `Yaml::Hash({String("__ruby_regexp__"): String(raw)})`, which
/// `yaml_to_json` then turns into `{"__ruby_regexp__": raw}`. The tag check
/// runs before the `style != Plain` early-return so quoted
/// `!ruby/regexp '/foo/'` (SingleQuoted, as in RuboCop's default.yml) is
/// preserved too. All other scalars, mappings, sequences, aliases, and
/// anchors behave identically to `YamlLoader`.
fn load_yaml_preserving_regexp(
    text: &str,
) -> Result<Vec<yaml_rust2::Yaml>, yaml_rust2::scanner::ScanError> {
    use std::collections::BTreeMap;
    use std::mem;
    use yaml_rust2::Yaml;
    use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser};
    use yaml_rust2::scanner::{Marker, ScanError, TScalarStyle};
    use yaml_rust2::yaml::Hash;

    #[derive(Default)]
    struct Loader {
        docs: Vec<Yaml>,
        doc_stack: Vec<(Yaml, usize)>,
        key_stack: Vec<Yaml>,
        anchor_map: BTreeMap<usize, Yaml>,
        error: Option<ScanError>,
    }

    impl MarkedEventReceiver for Loader {
        fn on_event(&mut self, ev: Event, mark: Marker) {
            if self.error.is_some() {
                return;
            }
            if let Err(e) = self.on_event_impl(ev, mark) {
                self.error = Some(e);
            }
        }
    }

    impl Loader {
        fn on_event_impl(&mut self, ev: Event, mark: Marker) -> Result<(), ScanError> {
            match ev {
                Event::DocumentStart | Event::Nothing | Event::StreamStart | Event::StreamEnd => {}
                Event::DocumentEnd => match self.doc_stack.len() {
                    0 => self.docs.push(Yaml::BadValue),
                    1 => self.docs.push(self.doc_stack.pop().unwrap().0),
                    _ => unreachable!(),
                },
                Event::SequenceStart(aid, _) => {
                    self.doc_stack.push((Yaml::Array(Vec::new()), aid));
                }
                Event::SequenceEnd => {
                    let node = self.doc_stack.pop().unwrap();
                    self.insert_new_node(node, mark)?;
                }
                Event::MappingStart(aid, _) => {
                    self.doc_stack.push((Yaml::Hash(Hash::new()), aid));
                    self.key_stack.push(Yaml::BadValue);
                }
                Event::MappingEnd => {
                    self.key_stack.pop().unwrap();
                    let node = self.doc_stack.pop().unwrap();
                    self.insert_new_node(node, mark)?;
                }
                Event::Scalar(v, style, aid, tag) => {
                    let node = if let Some(tag) = tag {
                        if is_ruby_regexp_tag(&tag.handle, &tag.suffix) {
                            let mut h = Hash::new();
                            h.insert(
                                Yaml::String(RUBY_REGEXP_JSON_KEY.to_string()),
                                Yaml::String(v),
                            );
                            Yaml::Hash(h)
                        } else if style != TScalarStyle::Plain {
                            Yaml::String(v)
                        } else if tag.handle == "tag:yaml.org,2002:" {
                            match tag.suffix.as_ref() {
                                "bool" => match v.as_str() {
                                    "true" | "True" | "TRUE" => Yaml::Boolean(true),
                                    "false" | "False" | "FALSE" => Yaml::Boolean(false),
                                    _ => Yaml::BadValue,
                                },
                                "int" => match v.parse::<i64>() {
                                    Err(_) => Yaml::BadValue,
                                    Ok(v) => Yaml::Integer(v),
                                },
                                "float" => match parse_f64_preserving(&v) {
                                    Some(_) => Yaml::Real(v),
                                    None => Yaml::BadValue,
                                },
                                "null" => match v.as_ref() {
                                    "~" | "null" => Yaml::Null,
                                    _ => Yaml::BadValue,
                                },
                                _ => Yaml::String(v),
                            }
                        } else {
                            Yaml::String(v)
                        }
                    } else if style != TScalarStyle::Plain {
                        Yaml::String(v)
                    } else {
                        Yaml::from_str(&v)
                    };
                    self.insert_new_node((node, aid), mark)?;
                }
                Event::Alias(id) => {
                    let n = match self.anchor_map.get(&id) {
                        Some(v) => v.clone(),
                        None => Yaml::BadValue,
                    };
                    self.insert_new_node((n, 0), mark)?;
                }
            }
            Ok(())
        }

        fn insert_new_node(&mut self, node: (Yaml, usize), mark: Marker) -> Result<(), ScanError> {
            if node.1 > 0 {
                self.anchor_map.insert(node.1, node.0.clone());
            }
            if self.doc_stack.is_empty() {
                self.doc_stack.push(node);
            } else {
                let parent = self.doc_stack.last_mut().unwrap();
                match *parent {
                    (Yaml::Array(ref mut v), _) => v.push(node.0),
                    (Yaml::Hash(ref mut h), _) => {
                        let cur_key = self.key_stack.last_mut().unwrap();
                        if cur_key.is_badvalue() {
                            *cur_key = node.0;
                        } else {
                            let mut newkey = Yaml::BadValue;
                            mem::swap(&mut newkey, cur_key);
                            if h.insert(newkey, node.0).is_some() {
                                let inserted_key = h.back().unwrap().0;
                                return Err(ScanError::new_string(
                                    mark,
                                    format!("{inserted_key:?}: duplicated key in mapping"),
                                ));
                            }
                        }
                    }
                    _ => unreachable!(),
                }
            }
            Ok(())
        }
    }

    let mut parser = Parser::new(text.chars());
    let mut loader = Loader::default();
    parser.load(&mut loader, true)?;
    if let Some(e) = loader.error {
        Err(e)
    } else {
        Ok(loader.docs)
    }
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
    use yaml_rust2::{Yaml, YamlEmitter};

    let docs =
        load_yaml_preserving_regexp(text).map_err(|e| ConfigError::BadYaml(e.to_string()))?;

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
    // Restore `!ruby/regexp` tags preserved as `__ruby_regexp__` sentinel
    // mappings (murphy-e7bz.41.1). `YamlEmitter` has no tag support, so
    // `Regex: {__ruby_regexp__: /foo/}` dumps as `Regex:\n  __ruby_regexp__: /foo/`;
    // rewriting `__ruby_regexp__:` (with colon) to `!ruby/regexp` (no colon)
    // yields `Regex:\n  !ruby/regexp /foo/`, which re-parses to the same
    // tagged scalar (block `key:\n  value` form). Array items
    // `- __ruby_regexp__: /foo/` similarly become `- !ruby/regexp /foo/`.
    let sentinel_with_colon = format!("{RUBY_REGEXP_JSON_KEY}:");
    let restored = yaml_body.replace(&sentinel_with_colon, "!ruby/regexp");
    out.push_str(&restored);

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
    fn allcops_context_resolves_max_line_length() {
        // Configured `Layout/LineLength.Max` flows into the context as
        // the shared resolved line-length budget (murphy-y3h2).
        let cfg =
            MurphyConfig::from_yaml_str("Layout/LineLength:\n  Max: 80\n").expect("config parses");
        assert_eq!(cfg.allcops_context().max_line_length, 80);

        // An explicit `Max: 0` falls back to the default 120 (mirrors
        // `Layout/LineLength`'s own `max <= 0 → 120` guard).
        let cfg =
            MurphyConfig::from_yaml_str("Layout/LineLength:\n  Max: 0\n").expect("config parses");
        assert_eq!(
            cfg.allcops_context().max_line_length,
            AllCopsContext::DEFAULT_MAX_LINE_LENGTH
        );

        // No configuration -> RuboCop's default Max of 120.
        let cfg = MurphyConfig::from_yaml_str("").expect("empty config parses");
        assert_eq!(
            cfg.allcops_context().max_line_length,
            AllCopsContext::DEFAULT_MAX_LINE_LENGTH
        );

        // Bundled default is honoured when the user does not set it.
        let cfg = MurphyConfig::with_defaults("", "Layout/LineLength:\n  Max: 100\n")
            .expect("config parses");
        assert_eq!(cfg.allcops_context().max_line_length, 100);

        // User value overrides a bundled default.
        let cfg = MurphyConfig::with_defaults(
            "Layout/LineLength:\n  Max: 90\n",
            "Layout/LineLength:\n  Max: 100\n",
        )
        .expect("config parses");
        assert_eq!(cfg.allcops_context().max_line_length, 90);
    }

    #[test]
    fn allcops_context_resolves_block_forwarding_explicit() {
        // User `EnforcedStyle: explicit` flows into the context (drives
        // `Style/ArgumentsForwarding`).
        let cfg =
            MurphyConfig::from_yaml_str("Naming/BlockForwarding:\n  EnforcedStyle: explicit\n")
                .expect("config parses");
        assert!(cfg.allcops_context().block_forwarding_explicit);

        // Explicit `anonymous` resolves to false.
        let cfg =
            MurphyConfig::from_yaml_str("Naming/BlockForwarding:\n  EnforcedStyle: anonymous\n")
                .expect("config parses");
        assert!(!cfg.allcops_context().block_forwarding_explicit);

        // Unconfigured -> RuboCop's `anonymous` default (false).
        let cfg = MurphyConfig::from_yaml_str("").expect("empty config parses");
        assert!(!cfg.allcops_context().block_forwarding_explicit);

        // Bundled default `explicit` is honoured when the user does not set it.
        let cfg =
            MurphyConfig::with_defaults("", "Naming/BlockForwarding:\n  EnforcedStyle: explicit\n")
                .expect("config parses");
        assert!(cfg.allcops_context().block_forwarding_explicit);

        // User `anonymous` overrides a bundled `explicit` default.
        let cfg = MurphyConfig::with_defaults(
            "Naming/BlockForwarding:\n  EnforcedStyle: anonymous\n",
            "Naming/BlockForwarding:\n  EnforcedStyle: explicit\n",
        )
        .expect("config parses");
        assert!(!cfg.allcops_context().block_forwarding_explicit);
    }

    #[test]
    fn allcops_context_resolves_block_body_empty_lines() {
        // User `EnforcedStyle: empty_lines` flows into the context (drives
        // `Layout/EmptyLinesAroundAccessModifier` in-block guards, murphy-xjua).
        let cfg = MurphyConfig::from_yaml_str(
            "Layout/EmptyLinesAroundBlockBody:\n  EnforcedStyle: empty_lines\n",
        )
        .expect("config parses");
        assert!(cfg.allcops_context().block_body_empty_lines);

        // Explicit `no_empty_lines` resolves to false.
        let cfg = MurphyConfig::from_yaml_str(
            "Layout/EmptyLinesAroundBlockBody:\n  EnforcedStyle: no_empty_lines\n",
        )
        .expect("config parses");
        assert!(!cfg.allcops_context().block_body_empty_lines);

        // Unconfigured -> RuboCop's `no_empty_lines` default (false).
        // NOTE: empty config has no bundled defaults here; with real
        // `base_defaults` (default.yml) the bundled `no_empty_lines` also
        // resolves to false — covered by the `with_defaults` case below.
        let cfg = MurphyConfig::from_yaml_str("").expect("empty config parses");
        assert!(!cfg.allcops_context().block_body_empty_lines);

        // Bundled default `empty_lines` is honoured when user does not set it.
        let cfg = MurphyConfig::with_defaults(
            "",
            "Layout/EmptyLinesAroundBlockBody:\n  EnforcedStyle: empty_lines\n",
        )
        .expect("config parses");
        assert!(cfg.allcops_context().block_body_empty_lines);

        // User `no_empty_lines` overrides a bundled `empty_lines` default.
        let cfg = MurphyConfig::with_defaults(
            "Layout/EmptyLinesAroundBlockBody:\n  EnforcedStyle: no_empty_lines\n",
            "Layout/EmptyLinesAroundBlockBody:\n  EnforcedStyle: empty_lines\n",
        )
        .expect("config parses");
        assert!(!cfg.allcops_context().block_body_empty_lines);

        // Disabled cop → `for_enabled_cop` yields no style → treated as
        // `empty_lines` (true), matching RuboCop's `nil == 'no_empty_lines'`.
        let cfg =
            MurphyConfig::from_yaml_str("Layout/EmptyLinesAroundBlockBody:\n  Enabled: false\n")
                .expect("config parses");
        assert!(cfg.allcops_context().block_body_empty_lines);
    }

    #[test]
    fn allcops_context_resolves_block_braces_space() {
        // User `EnforcedStyle: space` flows into the context (drives
        // `Layout/SpaceBeforeComma` / `Layout/SpaceBeforeSemicolon`
        // `{`-exemption, murphy-4qhr).
        let cfg =
            MurphyConfig::from_yaml_str("Layout/SpaceInsideBlockBraces:\n  EnforcedStyle: space\n")
                .expect("config parses");
        assert!(cfg.allcops_context().block_braces_space);

        // Explicit `no_space` resolves to false.
        let cfg = MurphyConfig::from_yaml_str(
            "Layout/SpaceInsideBlockBraces:\n  EnforcedStyle: no_space\n",
        )
        .expect("config parses");
        assert!(!cfg.allcops_context().block_braces_space);

        // Unconfigured -> RuboCop's `space` default (true) via `|| 'space'`
        // fallback. NOTE: empty config has no bundled defaults here; with real
        // `base_defaults` (default.yml) the bundled `space` also resolves to
        // true — covered by the `with_defaults` case below.
        let cfg = MurphyConfig::from_yaml_str("").expect("empty config parses");
        assert!(cfg.allcops_context().block_braces_space);

        // Bundled default `no_space` is honoured when user does not set it.
        let cfg = MurphyConfig::with_defaults(
            "",
            "Layout/SpaceInsideBlockBraces:\n  EnforcedStyle: no_space\n",
        )
        .expect("config parses");
        assert!(!cfg.allcops_context().block_braces_space);

        // User `space` overrides a bundled `no_space` default.
        let cfg = MurphyConfig::with_defaults(
            "Layout/SpaceInsideBlockBraces:\n  EnforcedStyle: space\n",
            "Layout/SpaceInsideBlockBraces:\n  EnforcedStyle: no_space\n",
        )
        .expect("config parses");
        assert!(cfg.allcops_context().block_braces_space);

        // Disabled cop → `for_cop` still yields its style (read
        // unconditionally). An explicit `no_space` is honoured even when
        // disabled.
        let cfg = MurphyConfig::from_yaml_str(
            "Layout/SpaceInsideBlockBraces:\n  Enabled: false\n  EnforcedStyle: no_space\n",
        )
        .expect("config parses");
        assert!(!cfg.allcops_context().block_braces_space);

        // Disabled cop with no style → fallback `'space'` (true).
        let cfg = MurphyConfig::from_yaml_str("Layout/SpaceInsideBlockBraces:\n  Enabled: false\n")
            .expect("config parses");
        assert!(cfg.allcops_context().block_braces_space);
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
    fn cop_options_json_bakes_camel_case_includes_for_file_name() {
        // murphy-bjrg.3 (no ABI bump): the baked key carries exactly the
        // effective AllCops:Include patterns containing an uppercase letter.
        let cfg = MurphyConfig::with_defaults(
            "",
            "AllCops:\n  Include:\n    - '**/*.rb'\n    - '**/Gemfile'\n    - '**/lower'\n",
        )
        .expect("config parses");
        let parsed: serde_json::Value =
            serde_json::from_slice(&cfg.cop_options_json("Naming/FileName")).expect("valid JSON");
        let baked = parsed["AllowedCamelCaseIncludePatterns"]
            .as_array()
            .expect("baked key is an array");
        assert_eq!(baked.len(), 1);
        assert_eq!(baked[0], "**/Gemfile");

        // Empty effective list bakes an empty array (strict cop behavior).
        let cfg = MurphyConfig::from_yaml_str("AllCops:\n  Include:\n    - '**/*.rb'\n")
            .expect("config parses");
        let parsed: serde_json::Value =
            serde_json::from_slice(&cfg.cop_options_json("Naming/FileName")).expect("valid JSON");
        assert_eq!(
            parsed["AllowedCamelCaseIncludePatterns"]
                .as_array()
                .unwrap()
                .len(),
            0
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
    fn cop_options_json_bakes_block_braces_style_for_semicolon() {
        // murphy-ilrx (option 2, no ABI bump): empty config bakes the
        // `for_cop` fallback `"space"` so the default behavior is preserved.
        let cfg = MurphyConfig::from_yaml_str("").expect("empty config parses");
        let parsed: serde_json::Value =
            serde_json::from_slice(&cfg.cop_options_json("Layout/SpaceAfterSemicolon"))
                .expect("valid JSON");
        assert_eq!(parsed["SpaceInsideBlockBracesEnforcedStyle"], "space");

        // User `no_space` sibling flows into the running cop's options.
        let cfg = MurphyConfig::from_yaml_str(
            "Layout/SpaceInsideBlockBraces:\n  EnforcedStyle: no_space\n",
        )
        .expect("config parses");
        let parsed: serde_json::Value =
            serde_json::from_slice(&cfg.cop_options_json("Layout/SpaceAfterSemicolon"))
                .expect("valid JSON");
        assert_eq!(parsed["SpaceInsideBlockBracesEnforcedStyle"], "no_space");

        // Bundled default is honoured when the user does not set the sibling.
        let cfg = MurphyConfig::with_defaults(
            "",
            "Layout/SpaceInsideBlockBraces:\n  EnforcedStyle: no_space\n",
        )
        .expect("config parses");
        let parsed: serde_json::Value =
            serde_json::from_slice(&cfg.cop_options_json("Layout/SpaceAfterSemicolon"))
                .expect("valid JSON");
        assert_eq!(parsed["SpaceInsideBlockBracesEnforcedStyle"], "no_space");

        // User wins over the bundled default.
        let cfg = MurphyConfig::with_defaults(
            "Layout/SpaceInsideBlockBraces:\n  EnforcedStyle: space\n",
            "Layout/SpaceInsideBlockBraces:\n  EnforcedStyle: no_space\n",
        )
        .expect("config parses");
        let parsed: serde_json::Value =
            serde_json::from_slice(&cfg.cop_options_json("Layout/SpaceAfterSemicolon"))
                .expect("valid JSON");
        assert_eq!(parsed["SpaceInsideBlockBracesEnforcedStyle"], "space");

        // The baked sibling wins over a same-named key on the running cop's
        // own table (the baked key is derived, not user-configurable).
        let cfg = MurphyConfig::from_yaml_str(
            "Layout/SpaceInsideBlockBraces:\n  EnforcedStyle: no_space\n\
             Layout/SpaceAfterSemicolon:\n  SpaceInsideBlockBracesEnforcedStyle: space\n",
        )
        .expect("config parses");
        let parsed: serde_json::Value =
            serde_json::from_slice(&cfg.cop_options_json("Layout/SpaceAfterSemicolon"))
                .expect("valid JSON");
        assert_eq!(parsed["SpaceInsideBlockBracesEnforcedStyle"], "no_space");
    }

    #[test]
    fn cop_options_json_bakes_hash_braces_style_for_comma() {
        // Empty config bakes `"space"` (RuboCop's `|| 'space'` fallback).
        let cfg = MurphyConfig::from_yaml_str("").expect("empty config parses");
        let parsed: serde_json::Value =
            serde_json::from_slice(&cfg.cop_options_json("Layout/SpaceAfterComma"))
                .expect("valid JSON");
        assert_eq!(parsed["SpaceInsideHashLiteralBracesEnforcedStyle"], "space");

        // `no_space` flows through, including the `compact` third style.
        for style in ["no_space", "compact", "space"] {
            let cfg = MurphyConfig::from_yaml_str(&format!(
                "Layout/SpaceInsideHashLiteralBraces:\n  EnforcedStyle: {style}\n"
            ))
            .expect("config parses");
            let parsed: serde_json::Value =
                serde_json::from_slice(&cfg.cop_options_json("Layout/SpaceAfterComma"))
                    .expect("valid JSON");
            assert_eq!(parsed["SpaceInsideHashLiteralBracesEnforcedStyle"], style);
        }

        // Bundled default + user override behave like the block-braces twin.
        let cfg = MurphyConfig::with_defaults(
            "",
            "Layout/SpaceInsideHashLiteralBraces:\n  EnforcedStyle: no_space\n",
        )
        .expect("config parses");
        let parsed: serde_json::Value =
            serde_json::from_slice(&cfg.cop_options_json("Layout/SpaceAfterComma"))
                .expect("valid JSON");
        assert_eq!(
            parsed["SpaceInsideHashLiteralBracesEnforcedStyle"],
            "no_space"
        );
    }

    #[test]
    fn cop_options_json_leaves_unrelated_cops_unbaked() {
        // No baked keys leak into other cops; the empty fast path is intact.
        let cfg = MurphyConfig::from_yaml_str("").expect("empty config parses");
        let parsed: serde_json::Value =
            serde_json::from_slice(&cfg.cop_options_json("Style/Foo")).expect("valid JSON");
        assert_eq!(parsed, serde_json::json!({}));
        let parsed: serde_json::Value = serde_json::from_slice(
            &cfg.cop_options_json("Layout/SpaceAroundEqualsInParameterDefault"),
        )
        .expect("valid JSON");
        assert!(parsed.get("SpaceInsideBlockBracesEnforcedStyle").is_none());
        assert!(
            parsed
                .get("SpaceInsideHashLiteralBracesEnforcedStyle")
                .is_none()
        );
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

    #[test]
    fn pack_cop_inherit_mode_unions_option_arrays() {
        let mut cfg = MurphyConfig::with_defaults(
            "",
            "Lint/UselessAccessModifier:\n  ContextCreatingMethods:\n    - core_method\n",
        )
        .unwrap();
        cfg.apply_pack_default_layers(&[
            "Lint/UselessAccessModifier:\n  inherit_mode:\n    merge:\n      - ContextCreatingMethods\n  ContextCreatingMethods:\n    - rails_method\n",
        ]);
        let options: serde_json::Value =
            serde_json::from_slice(&cfg.cop_options_json("Lint/UselessAccessModifier")).unwrap();
        assert_eq!(
            options["ContextCreatingMethods"],
            serde_json::json!(["core_method", "rails_method"])
        );
        assert!(
            options.get("inherit_mode").is_none(),
            "inherit_mode is config metadata, not a cop option"
        );
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
    fn inherit_from_recursively_merges_nested_cop_option_hashes() {
        let dir = tempfile::TempDir::new().unwrap();
        write_cfg(
            dir.path(),
            "base.yml",
            "Naming/InclusiveLanguage:\n  FlaggedTerms:\n    shared:\n      Regex: /base/\n      WholeWord: true\n      Suggestions:\n        - base_suggestion\n    inherited:\n      Regex: /inherited/\n",
        );
        write_cfg(
            dir.path(),
            ".murphy.yml",
            "inherit_from: base.yml\nNaming/InclusiveLanguage:\n  FlaggedTerms:\n    shared:\n      Suggestions:\n        - project_suggestion\n    project_only:\n      Regex: /project/\n",
        );
        let cfg = MurphyConfig::load(dir.path()).expect("load succeeds");
        let options: serde_json::Value =
            serde_json::from_slice(&cfg.cop_options_json("Naming/InclusiveLanguage")).unwrap();
        let terms = &options["FlaggedTerms"];
        assert_eq!(terms["shared"]["Regex"], "/base/");
        assert!(terms["shared"]["WholeWord"].as_bool().unwrap());
        assert_eq!(
            terms["shared"]["Suggestions"],
            serde_json::json!(["project_suggestion"])
        );
        assert_eq!(terms["inherited"]["Regex"], "/inherited/");
        assert_eq!(terms["project_only"]["Regex"], "/project/");
        let order: Vec<_> = terms
            .as_object()
            .expect("term map")
            .keys()
            .map(String::as_str)
            .collect();
        let index_of = |term| {
            order
                .iter()
                .position(|candidate| *candidate == term)
                .unwrap()
        };
        assert!(index_of("shared") < index_of("inherited"));
        assert!(index_of("inherited") < index_of("project_only"));
    }

    #[test]
    fn ruby_regexp_tags_survive_yaml_to_json() {
        // murphy-e7bz.41.1: `!ruby/regexp` must not collapse to a plain
        // string; tagged and plain slash-delimited stay distinguishable.
        let cfg = MurphyConfig::from_yaml_str(
            "Naming/InclusiveLanguage:\n  FlaggedTerms:\n    tagged:\n      Regex: !ruby/regexp '/foo/'\n    plain:\n      Regex: '/foo/'\n",
        )
        .expect("config parses");
        let options: serde_json::Value =
            serde_json::from_slice(&cfg.cop_options_json("Naming/InclusiveLanguage")).unwrap();
        assert_eq!(
            options["FlaggedTerms"]["tagged"]["Regex"],
            serde_json::json!({"__ruby_regexp__": "/foo/"}),
            "tagged must become an object"
        );
        assert_eq!(
            options["FlaggedTerms"]["plain"]["Regex"], "/foo/",
            "plain must stay a string"
        );
    }

    #[test]
    fn ruby_regexp_tags_survive_inherit_from_merge() {
        // Base tagged, project plain-overrides one term and adds another;
        // types must survive `merge_option_value` recursion.
        let dir = tempfile::TempDir::new().unwrap();
        write_cfg(
            dir.path(),
            "base.yml",
            "Naming/InclusiveLanguage:\n  FlaggedTerms:\n    shared:\n      Regex: !ruby/regexp '/base/'\n    inherited:\n      Regex: !ruby/regexp '/inherited/'\n",
        );
        write_cfg(
            dir.path(),
            ".murphy.yml",
            "inherit_from: base.yml\nNaming/InclusiveLanguage:\n  FlaggedTerms:\n    shared:\n      Regex: '/plain/'\n    project_only:\n      Regex: !ruby/regexp '/project/'\n",
        );
        let cfg = MurphyConfig::load(dir.path()).expect("load succeeds");
        let options: serde_json::Value =
            serde_json::from_slice(&cfg.cop_options_json("Naming/InclusiveLanguage")).unwrap();
        assert_eq!(
            options["FlaggedTerms"]["shared"]["Regex"], "/plain/",
            "project plain must replace base tagged as a string"
        );
        assert_eq!(
            options["FlaggedTerms"]["inherited"]["Regex"],
            serde_json::json!({"__ruby_regexp__": "/inherited/"}),
            "inherited tagged must stay an object"
        );
        assert_eq!(
            options["FlaggedTerms"]["project_only"]["Regex"],
            serde_json::json!({"__ruby_regexp__": "/project/"}),
            "project tagged must stay an object"
        );
    }

    #[test]
    fn ruby_regexp_arrays_survive_yaml_to_json() {
        let cfg = MurphyConfig::from_yaml_str(
            "Naming/InclusiveLanguage:\n  FlaggedTerms:\n    t:\n      AllowedRegex:\n        - !ruby/regexp /foo/\n        - plain\n",
        )
        .expect("config parses");
        let options: serde_json::Value =
            serde_json::from_slice(&cfg.cop_options_json("Naming/InclusiveLanguage")).unwrap();
        assert_eq!(
            options["FlaggedTerms"]["t"]["AllowedRegex"],
            serde_json::json!([{"__ruby_regexp__": "/foo/"}, "plain"])
        );
    }

    #[test]
    fn migrate_restores_ruby_regexp_tags() {
        let out = migrate_rubocop_yml_to_murphy_yml(
            "Naming/InclusiveLanguage:\n  FlaggedTerms:\n    t:\n      Regex: !ruby/regexp '/foo/'\n",
        )
        .expect("migrate succeeds");
        assert!(
            out.contains("!ruby/regexp"),
            "migrated YAML must keep the tag, got: {out}"
        );
        // Roundtrip: migrated output re-parses to a tagged object.
        let cfg = MurphyConfig::from_yaml_str(&out).expect("migrated output must load");
        let options: serde_json::Value =
            serde_json::from_slice(&cfg.cop_options_json("Naming/InclusiveLanguage")).unwrap();
        assert_eq!(
            options["FlaggedTerms"]["t"]["Regex"],
            serde_json::json!({"__ruby_regexp__": "/foo/"})
        );
    }

    #[test]
    fn inherit_mode_merges_nested_arrays_in_cop_option_hashes() {
        let dir = tempfile::TempDir::new().unwrap();
        write_cfg(
            dir.path(),
            "base.yml",
            "Naming/InclusiveLanguage:\n  inherit_mode:\n    merge:\n      - Suggestions\n  FlaggedTerms:\n    shared:\n      Suggestions:\n        - common\n        - inherited\n",
        );
        write_cfg(
            dir.path(),
            ".murphy.yml",
            "inherit_from: base.yml\nNaming/InclusiveLanguage:\n  FlaggedTerms:\n    shared:\n      Suggestions:\n        - common\n        - project\n",
        );
        let cfg = MurphyConfig::load(dir.path()).expect("load succeeds");
        let options: serde_json::Value =
            serde_json::from_slice(&cfg.cop_options_json("Naming/InclusiveLanguage")).unwrap();
        assert_eq!(
            options["FlaggedTerms"]["shared"]["Suggestions"],
            serde_json::json!(["common", "inherited", "project"])
        );
    }

    #[test]
    fn inherit_mode_globally_merges_cop_option_arrays_across_inherited_files() {
        let dir = tempfile::TempDir::new().unwrap();
        write_cfg(
            dir.path(),
            "base.yml",
            "Style/FormatStringToken:\n  EnforcedStyle: inherited_style\n  AllowedMethods:\n    - inherited\n    - shared\n",
        );
        write_cfg(
            dir.path(),
            ".murphy.yml",
            "inherit_from: base.yml\ninherit_mode:\n  merge:\n    - AllowedMethods\nStyle/FormatStringToken:\n  EnforcedStyle: project_style\n  AllowedMethods:\n    - shared\n    - project\n",
        );
        let cfg = MurphyConfig::load(dir.path()).expect("load succeeds");
        let options: serde_json::Value =
            serde_json::from_slice(&cfg.cop_options_json("Style/FormatStringToken")).unwrap();
        assert_eq!(
            options["AllowedMethods"],
            serde_json::json!(["inherited", "shared", "project"])
        );
        assert_eq!(
            options["EnforcedStyle"], "project_style",
            "non-listed options still use current-layer precedence"
        );
    }

    #[test]
    fn inherit_from_merges_cop_option_arrays_when_listed() {
        let dir = tempfile::TempDir::new().unwrap();
        write_cfg(
            dir.path(),
            "base.yml",
            "Style/FormatStringToken:\n  AllowedMethods:\n    - inherited\n    - shared\n",
        );
        write_cfg(
            dir.path(),
            ".murphy.yml",
            "inherit_from: base.yml\nStyle/FormatStringToken:\n  inherit_mode:\n    merge:\n      - AllowedMethods\n  AllowedMethods:\n    - shared\n    - project\n",
        );
        let cfg = MurphyConfig::load(dir.path()).expect("load succeeds");
        let options: serde_json::Value =
            serde_json::from_slice(&cfg.cop_options_json("Style/FormatStringToken")).unwrap();
        assert_eq!(
            options["AllowedMethods"],
            serde_json::json!(["inherited", "shared", "project"])
        );
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
    fn pack_global_inherit_mode_unions_option_arrays() {
        let mut cfg = MurphyConfig::with_defaults(
            "",
            "Style/FormatStringToken:\n  AllowedMethods:\n    - core_method\n",
        )
        .unwrap();
        cfg.apply_pack_default_layers(&["inherit_mode:\n  merge:\n    - AllowedMethods\nStyle/FormatStringToken:\n  AllowedMethods:\n    - rails_method\n"]);
        let options: serde_json::Value =
            serde_json::from_slice(&cfg.cop_options_json("Style/FormatStringToken")).unwrap();
        assert_eq!(
            options["AllowedMethods"],
            serde_json::json!(["core_method", "rails_method"])
        );
    }

    #[test]
    fn cop_inherit_mode_unions_user_and_pack_option_arrays() {
        let mut cfg = MurphyConfig::from_yaml_str(
            "inherit_mode:\n  merge:\n    - AllowedMethods\nStyle/FormatStringToken:\n  AllowedMethods:\n    - project_method\n",
        )
        .unwrap();
        cfg.apply_pack_default_layers(&[
            "Style/FormatStringToken:\n  AllowedMethods:\n    - redirect\n",
        ]);
        let options: serde_json::Value =
            serde_json::from_slice(&cfg.cop_options_json("Style/FormatStringToken")).unwrap();
        assert_eq!(
            options["AllowedMethods"],
            serde_json::json!(["redirect", "project_method"])
        );
        assert!(
            !cfg.cops.rules["Style/FormatStringToken"]
                .options
                .contains_key("inherit_mode"),
            "cop inherit_mode must not leak into the cop's option JSON"
        );
    }

    #[test]
    fn cop_option_arrays_replace_pack_defaults_without_inherit_mode() {
        let mut cfg = MurphyConfig::from_yaml_str(
            "Style/FormatStringToken:\n  AllowedMethods:\n    - project_method\n",
        )
        .unwrap();
        cfg.apply_pack_default_layers(&[
            "Style/FormatStringToken:\n  AllowedMethods:\n    - redirect\n",
        ]);
        let options: serde_json::Value =
            serde_json::from_slice(&cfg.cop_options_json("Style/FormatStringToken")).unwrap();
        assert_eq!(
            options["AllowedMethods"],
            serde_json::json!(["project_method"])
        );
    }

    #[test]
    fn cop_inherit_mode_unions_user_and_pack_exclude_scopes() {
        let mut cfg = MurphyConfig::from_yaml_str(
            "RSpec/DescribeClass:\n  inherit_mode:\n    merge:\n      - Exclude\n  Exclude:\n    - '**/spec/models/**/*'\n",
        )
        .unwrap();
        cfg.apply_pack_default_layers(&[
            "RSpec/DescribeClass:\n  Exclude:\n    - '**/spec/requests/**/*'\n",
        ]);
        assert!(
            !cfg.cop_applies_to_file("RSpec/DescribeClass", Path::new("spec/models/user_spec.rb")),
            "user Exclude should still apply"
        );
        assert!(
            !cfg.cop_applies_to_file(
                "RSpec/DescribeClass",
                Path::new("spec/requests/user_spec.rb")
            ),
            "pack Exclude should union with the user list"
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

    // --- TargetRubyVersion detection (murphy-os2b) ---

    fn write_file(dir: &Path, name: &str, content: &str) {
        std::fs::write(dir.join(name), content).expect("write fixture");
    }

    #[test]
    fn detects_from_ruby_version() {
        let dir = tempfile::TempDir::new().unwrap();
        write_file(dir.path(), ".ruby-version", "3.3.5\n");
        assert_eq!(
            detect_target_ruby_version(dir.path()),
            Some(RubyVersion::new(3, 3))
        );
        let cfg = MurphyConfig::load(dir.path()).expect("load");
        assert_eq!(cfg.target_ruby_version, RubyVersion::new(3, 3));
    }

    #[test]
    fn detects_ruby_version_with_prefix_and_patch() {
        let dir = tempfile::TempDir::new().unwrap();
        write_file(dir.path(), ".ruby-version", "ruby-3.2.2\n");
        assert_eq!(
            detect_target_ruby_version(dir.path()),
            Some(RubyVersion::new(3, 2))
        );
    }

    #[test]
    fn detects_from_tool_versions() {
        let dir = tempfile::TempDir::new().unwrap();
        write_file(dir.path(), ".tool-versions", "nodejs 20.11.0\nruby 3.4.1\n");
        assert_eq!(
            detect_target_ruby_version(dir.path()),
            Some(RubyVersion::new(3, 4))
        );
    }

    #[test]
    fn detects_from_mise_toml() {
        let dir = tempfile::TempDir::new().unwrap();
        write_file(dir.path(), "mise.toml", "[tools]\nruby = \"3.3.5\"\n");
        assert_eq!(
            detect_target_ruby_version(dir.path()),
            Some(RubyVersion::new(3, 3))
        );
    }

    #[test]
    fn detects_from_gemfile_lock() {
        let dir = tempfile::TempDir::new().unwrap();
        write_file(
            dir.path(),
            "Gemfile.lock",
            "GEM\n  specs:\n\nRUBY VERSION\n   ruby 3.2.2p53\n",
        );
        assert_eq!(
            detect_target_ruby_version(dir.path()),
            Some(RubyVersion::new(3, 2))
        );
    }

    #[test]
    fn detects_from_gemspec_ge() {
        let dir = tempfile::TempDir::new().unwrap();
        write_file(
            dir.path(),
            "foo.gemspec",
            "Gem::Specification.new do |s|\n  s.required_ruby_version = \">= 3.0\"\nend\n",
        );
        assert_eq!(
            detect_target_ruby_version(dir.path()),
            Some(RubyVersion::new(3, 0))
        );
    }

    #[test]
    fn detects_from_gemspec_requirement_new() {
        let dir = tempfile::TempDir::new().unwrap();
        write_file(
            dir.path(),
            "foo.gemspec",
            "Gem::Specification.new do |s|\n  s.required_ruby_version = Gem::Requirement.new(\">= 2.7.0\")\nend\n",
        );
        assert_eq!(
            detect_target_ruby_version(dir.path()),
            Some(RubyVersion::new(2, 7))
        );
    }

    #[test]
    fn detects_from_gemspec_pessimistic() {
        let dir = tempfile::TempDir::new().unwrap();
        write_file(
            dir.path(),
            "foo.gemspec",
            "Gem::Specification.new do |s|\n  s.required_ruby_version = \"~> 3.2.0\"\nend\n",
        );
        assert_eq!(
            detect_target_ruby_version(dir.path()),
            Some(RubyVersion::new(3, 2))
        );
    }

    #[test]
    fn detects_from_gemspec_array() {
        let dir = tempfile::TempDir::new().unwrap();
        write_file(
            dir.path(),
            "foo.gemspec",
            "Gem::Specification.new do |s|\n  s.required_ruby_version = [\">= 2.5\", \"< 3.0\"]\nend\n",
        );
        assert_eq!(
            detect_target_ruby_version(dir.path()),
            Some(RubyVersion::new(2, 5))
        );
    }

    #[test]
    fn gemspec_wins_over_ruby_version() {
        let dir = tempfile::TempDir::new().unwrap();
        write_file(dir.path(), ".ruby-version", "3.3.5\n");
        write_file(
            dir.path(),
            "foo.gemspec",
            "Gem::Specification.new do |s|\n  s.required_ruby_version = \">= 3.0\"\nend\n",
        );
        // RuboCop SOURCES order: gemspec before .ruby-version.
        assert_eq!(
            detect_target_ruby_version(dir.path()),
            Some(RubyVersion::new(3, 0))
        );
    }

    #[test]
    fn ruby_version_wins_over_tool_versions() {
        let dir = tempfile::TempDir::new().unwrap();
        write_file(dir.path(), ".ruby-version", "3.3.5\n");
        write_file(dir.path(), ".tool-versions", "ruby 3.2.0\n");
        // RuboCop SOURCES order: .ruby-version before .tool-versions.
        assert_eq!(
            detect_target_ruby_version(dir.path()),
            Some(RubyVersion::new(3, 3))
        );
    }

    #[test]
    fn explicit_config_wins_over_detection() {
        let dir = tempfile::TempDir::new().unwrap();
        write_file(dir.path(), ".ruby-version", "3.3.5\n");
        write_file(
            dir.path(),
            ".murphy.yml",
            "AllCops:\n  TargetRubyVersion: 3.1\n",
        );
        let cfg = MurphyConfig::load(dir.path()).expect("load");
        assert_eq!(cfg.target_ruby_version, RubyVersion::new(3, 1));
    }

    #[test]
    fn explicit_config_via_inherit_wins_over_detection() {
        let dir = tempfile::TempDir::new().unwrap();
        write_file(dir.path(), ".ruby-version", "3.3.5\n");
        write_file(
            dir.path(),
            "base.yml",
            "AllCops:\n  TargetRubyVersion: 2.7\n",
        );
        write_file(dir.path(), ".murphy.yml", "inherit_from: base.yml\n");
        let cfg = MurphyConfig::load(dir.path()).expect("load");
        assert_eq!(cfg.target_ruby_version, RubyVersion::new(2, 7));
    }

    #[test]
    fn defaults_when_nothing_found() {
        let dir = tempfile::TempDir::new().unwrap();
        assert_eq!(detect_target_ruby_version(dir.path()), None);
        let cfg = MurphyConfig::load(dir.path()).expect("load");
        assert_eq!(cfg.target_ruby_version, RubyVersion::new(3, 1));
    }

    #[test]
    fn invalid_version_files_fall_through() {
        let dir = tempfile::TempDir::new().unwrap();
        write_file(dir.path(), ".ruby-version", "not-a-version\n");
        write_file(dir.path(), ".tool-versions", "nodejs 20\n");
        assert_eq!(detect_target_ruby_version(dir.path()), None);
    }

    #[test]
    fn finds_version_file_in_parent_dir() {
        let dir = tempfile::TempDir::new().unwrap();
        write_file(dir.path(), ".ruby-version", "3.2.0\n");
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        assert_eq!(
            detect_target_ruby_version(&sub),
            Some(RubyVersion::new(3, 2))
        );
    }

    #[test]
    fn load_with_defaults_detects_without_config() {
        let dir = tempfile::TempDir::new().unwrap();
        write_file(dir.path(), ".ruby-version", "3.4.0\n");
        let cfg =
            MurphyConfig::load_with_defaults(dir.path(), "AllCops:\n  Exclude: []\n").unwrap();
        assert_eq!(cfg.target_ruby_version, RubyVersion::new(3, 4));
    }
}
