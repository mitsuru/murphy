use crate::Severity;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::ConfigError;

#[derive(Debug, Clone, PartialEq)]
pub struct MurphyConfig {
    pub files: FilesConfig,
    pub cops: CopsConfig,
    pub plugins: Vec<PluginConfig>,
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

/// Plugin pack entry from `[[plugins]]` (or `plugins = [...]`) in
/// `murphy.toml`.
///
/// Heterogeneous array of two shapes:
/// - `plugins = ["murphy-rails"]` — name-only shorthand. RuboCop
///   `.rubocop.yml` plugins: directive compatibility. Resolved at load
///   time against the search path (ADR 0042): same-array `Detailed`
///   override → `MURPHY_PLUGIN_PATH` env → project-local
///   `.murphy/plugins/` → user-local `$XDG_DATA_HOME/murphy/plugins/`.
/// - `[[plugins]] name = "..." path = "..."` — explicit path; bypasses
///   the search path entirely.
///
/// Deserialization dispatches manually on input shape (string vs.
/// table) instead of `#[serde(untagged)]`: an untagged enum buffers
/// the input, tries each variant, and swallows the inner diagnostics
/// (`deny_unknown_fields`, `missing field`) into a generic
/// "data did not match any variant". The hand-rolled `Visitor` routes
/// a table straight into `PluginDetailed` so its errors propagate
/// verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginConfig {
    Name(String),
    Detailed(PluginDetailed),
}

impl<'de> Deserialize<'de> for PluginConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct PluginConfigVisitor;
        impl<'de> serde::de::Visitor<'de> for PluginConfigVisitor {
            type Value = PluginConfig;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str(r#"a plugin name string or { name = "...", path = "..." } table"#)
            }

            fn visit_str<E>(self, v: &str) -> Result<PluginConfig, E>
            where
                E: serde::de::Error,
            {
                Ok(PluginConfig::Name(v.to_owned()))
            }

            fn visit_string<E>(self, v: String) -> Result<PluginConfig, E>
            where
                E: serde::de::Error,
            {
                Ok(PluginConfig::Name(v))
            }

            fn visit_map<M>(self, map: M) -> Result<PluginConfig, M::Error>
            where
                M: serde::de::MapAccess<'de>,
            {
                PluginDetailed::deserialize(serde::de::value::MapAccessDeserializer::new(map))
                    .map(PluginConfig::Detailed)
            }
        }
        deserializer.deserialize_any(PluginConfigVisitor)
    }
}

/// Explicit-path plugin entry: `[[plugins]] name = "..." path = "..."`.
///
/// Split out of [`PluginConfig::Detailed`] so that `deny_unknown_fields`
/// and `missing field` diagnostics survive the surrounding
/// `#[serde(untagged)]` wrapping.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginDetailed {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct CopRule {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub severity: Option<Severity>,
    #[serde(flatten)]
    pub options: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MurphyToml {
    #[serde(default)]
    files: FilesTable,
    #[serde(default)]
    cops: CopsTable,
    #[serde(default)]
    plugins: Vec<PluginConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FilesTable {
    #[serde(default = "default_include")]
    include: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CopsTable {
    #[serde(default = "default_cops_path")]
    path: PathBuf,
    #[serde(default)]
    rules: BTreeMap<String, CopRule>,
}

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
            plugins: Vec::new(),
        }
    }
}

impl Default for FilesTable {
    fn default() -> Self {
        Self {
            include: default_include(),
            exclude: Vec::new(),
        }
    }
}

impl Default for CopsTable {
    fn default() -> Self {
        Self {
            path: default_cops_path(),
            rules: BTreeMap::new(),
        }
    }
}

fn default_include() -> Vec<String> {
    vec!["**/*.rb".to_string()]
}

fn default_cops_path() -> PathBuf {
    PathBuf::from("cops")
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
            plugins: value.plugins,
        }
    }
}

impl MurphyConfig {
    pub fn from_toml_str(text: &str) -> Result<Self, ConfigError> {
        let parsed: MurphyToml =
            toml::from_str(text).map_err(|e| ConfigError::BadToml(e.to_string()))?;
        Ok(parsed.into())
    }

    pub fn load(root: &Path) -> Result<Self, ConfigError> {
        let config_path = root.join("murphy.toml");
        match std::fs::read_to_string(&config_path) {
            Ok(text) => Self::from_toml_str(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(ConfigError::Io(format!(
                "cannot read {}: {e}",
                config_path.display()
            ))),
        }
    }

    pub fn cop_enabled(&self, name: &str) -> bool {
        self.cops
            .rules
            .get(name)
            .and_then(|rule| rule.enabled)
            .unwrap_or(!is_cop_disabled_by_default(name))
    }

    /// True when the user wrote `[cops.rules."Name"] enabled = true`
    /// in `murphy.toml`. Used by the `cops list` / lint flow to detect
    /// a user trying to opt back into a cop that is currently in the
    /// disabled registry (arena migration), so the host can emit a
    /// warning without breaking the lint run (§12c).
    pub fn is_explicitly_enabled(&self, name: &str) -> bool {
        self.cops.rules.get(name).and_then(|rule| rule.enabled) == Some(true)
    }

    pub fn severity_override(&self, name: &str) -> Option<Severity> {
        self.cops.rules.get(name).and_then(|rule| rule.severity)
    }

    pub fn cop_options_json(&self, name: &str) -> Vec<u8> {
        let Some(rule) = self.cops.rules.get(name) else {
            return b"{}".to_vec();
        };
        serde_json::to_vec(&rule.options).unwrap_or_else(|_| b"{}".to_vec())
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

fn is_cop_disabled_by_default(name: &str) -> bool {
    matches!(
        name,
        "Rails/ActionFilter"
            | "Rails/DefaultScope"
            | "Rails/Env"
            | "Rails/EnvironmentVariableAccess"
            | "Rails/OrderById"
            | "Rails/PluckId"
            | "Rails/RequireDependency"
            | "Rails/ReversibleMigrationMethodDefinition"
            | "Rails/SaveBang"
            | "Rails/SchemaComment"
            | "Rails/TableNameAssignment"
            | "Rails/UnusedIgnoredColumns"
    )
}

fn quote_toml_string(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn toml_array(values: &[String]) -> String {
    let parts: Vec<String> = values
        .iter()
        .map(|value| quote_toml_string(value))
        .collect();
    format!("[{}]", parts.join(", "))
}

pub fn migrate_rubocop_yml_to_murphy_toml(text: &str) -> Result<String, ConfigError> {
    let yaml: serde_yaml::Value =
        serde_yaml::from_str(text).map_err(|e| ConfigError::BadYaml(e.to_string()))?;
    let mut include: Vec<String> = Vec::new();
    let mut exclude: Vec<String> = Vec::new();
    let mut rules: BTreeMap<String, CopRule> = BTreeMap::new();
    let mut plugin_names: Vec<String> = Vec::new();
    let mut unsupported_plugins: Vec<String> = Vec::new();

    let serde_yaml::Value::Mapping(top) = yaml else {
        return Err(ConfigError::BadYaml(
            "top-level document must be a mapping".to_string(),
        ));
    };

    for (key, value) in top {
        let Some(section) = key.as_str() else {
            continue;
        };
        if section == "plugins" {
            // RuboCop 互換: `plugins: foo` (scalar) は `plugins: [foo]` と同じく扱う。
            // 非 sequence / 非 string 形は silent drop だと一方向 migrate で
            // データが消えるので、unsupported コメントで明示する。
            let items: Vec<serde_yaml::Value> = match value {
                serde_yaml::Value::Sequence(seq) => seq,
                serde_yaml::Value::String(s) => vec![serde_yaml::Value::String(s)],
                other => {
                    unsupported_plugins.push(format!("{other:?} (unsupported plugins: form)"));
                    continue;
                }
            };
            for item in items {
                match item {
                    serde_yaml::Value::String(s) => plugin_names.push(s),
                    serde_yaml::Value::Mapping(m) => {
                        // `- foo: {...}` 形は MVP では unsupported コメント
                        if let Some(name) = m.into_iter().next().and_then(|(k, _)| match k {
                            serde_yaml::Value::String(s) => Some(s),
                            _ => None,
                        }) {
                            unsupported_plugins.push(name);
                        } else {
                            unsupported_plugins.push("<empty or non-string key>".to_string());
                        }
                    }
                    other => {
                        unsupported_plugins.push(format!("{other:?} (non-string / non-mapping)"));
                    }
                }
            }
            continue;
        }
        let serde_yaml::Value::Mapping(map) = value else {
            continue;
        };
        if section == "AllCops" {
            include = yaml_string_list(map.get(serde_yaml::Value::String("Include".to_string())));
            exclude = yaml_string_list(map.get(serde_yaml::Value::String("Exclude".to_string())));
            continue;
        }
        let mut rule = CopRule::default();
        if let Some(enabled) = map
            .get(serde_yaml::Value::String("Enabled".to_string()))
            .and_then(serde_yaml::Value::as_bool)
        {
            rule.enabled = Some(enabled);
        }
        if let Some(severity) = map
            .get(serde_yaml::Value::String("Severity".to_string()))
            .and_then(serde_yaml::Value::as_str)
        {
            match severity {
                "warning" => rule.severity = Some(Severity::Warning),
                "error" => rule.severity = Some(Severity::Error),
                _ => {}
            }
        }
        if rule.enabled.is_some() || rule.severity.is_some() {
            rules.insert(section.to_string(), rule);
        }
    }

    let mut out = String::new();
    if !plugin_names.is_empty() {
        // RuboCop's `rubocop-X` plugin names are not auto-renamed to
        // `murphy-X` (ADR 0041: explicit > implicit). Surface this once
        // so the user fixes the names before the first lint run instead
        // of debugging a `not found` error from the resolver (ADR 0042).
        out.push_str(
            "# NOTE: RuboCop `rubocop-X` plugin names must be renamed to `murphy-X` \
             manually — Murphy does not auto-translate the prefix.\n",
        );
        out.push_str(&format!("plugins = {}\n", toml_array(&plugin_names)));
    }
    for unsupported in &unsupported_plugins {
        // `unsupported` は .rubocop.yml の mapping-key 由来でユーザー由来文字列。
        // 改行を含むと migrate 出力の `# ...` コメント以降の行が有効な TOML として
        // 解釈されうる (悪意ある YAML が `# foo\n[[plugins]]\nname = "x"\npath = "y"`
        // のような entry 名を持つ場合に config injection)。制御文字 (\r \n \x00 ...)
        // を `?` に置換して 1 行に押し込める。
        let sanitized: String = unsupported
            .chars()
            .map(|c| if c.is_control() { '?' } else { c })
            .collect();
        out.push_str(&format!("# unsupported plugin entry: {sanitized}\n"));
    }
    if !plugin_names.is_empty() || !unsupported_plugins.is_empty() {
        out.push('\n');
    }
    out.push_str("[files]\n");
    let include_values = if include.is_empty() {
        default_include()
    } else {
        include
    };
    out.push_str(&format!("include = {}\n", toml_array(&include_values)));
    out.push_str(&format!("exclude = {}\n\n", toml_array(&exclude)));
    out.push_str("[cops]\npath = \"cops\"\n");
    for (name, rule) in rules {
        out.push('\n');
        out.push_str(&format!("[cops.rules.{}]\n", quote_toml_string(&name)));
        if let Some(enabled) = rule.enabled {
            out.push_str(&format!("enabled = {enabled}\n"));
        }
        if let Some(severity) = rule.severity {
            let value = match severity {
                Severity::Warning => "warning",
                Severity::Error => "error",
            };
            out.push_str(&format!("severity = {value:?}\n"));
        }
    }
    Ok(out)
}

fn yaml_string_list(value: Option<&serde_yaml::Value>) -> Vec<String> {
    match value {
        Some(serde_yaml::Value::Sequence(values)) => values
            .iter()
            .filter_map(serde_yaml::Value::as_str)
            .map(str::to_string)
            .collect(),
        Some(serde_yaml::Value::String(value)) => vec![value.clone()],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_defaults() {
        let cfg = MurphyConfig::from_toml_str("").expect("empty config parses");
        assert_eq!(cfg.files.include, vec!["**/*.rb"]);
        assert_eq!(cfg.cops.path, PathBuf::from("cops"));
        assert!(cfg.cops.rules.is_empty());
    }

    #[test]
    fn parses_cop_rules() {
        let cfg = MurphyConfig::from_toml_str(
            "[cops]\npath = \"custom_cops\"\n[cops.rules.\"Murphy/Foo\"]\nenabled = false\nseverity = \"error\"\n",
        )
        .expect("config parses");
        assert_eq!(cfg.cops.path, PathBuf::from("custom_cops"));
        assert!(!cfg.cop_enabled("Murphy/Foo"));
        assert_eq!(cfg.severity_override("Murphy/Foo"), Some(Severity::Error));
    }

    #[test]
    fn cop_rule_preserves_rubocop_compatible_options() {
        let cfg = MurphyConfig::from_toml_str(
            r#"
[cops.rules."Style/StringLiterals"]
enabled = true
severity = "warning"
EnforcedStyle = "single_quotes"
Exclude = ["db/schema.rb"]
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
            Some(&toml::Value::String("single_quotes".to_string()))
        );
        assert_eq!(
            rule.options.get("Exclude"),
            Some(&toml::Value::Array(vec![toml::Value::String(
                "db/schema.rb".to_string()
            )]))
        );
    }

    #[test]
    fn cop_enabled_is_false_for_rails_cops_disabled_by_default() {
        let cfg = MurphyConfig::from_toml_str("").expect("empty config parses");

        const DISABLED_BY_DEFAULT: [&str; 12] = [
            "Rails/ActionFilter",
            "Rails/DefaultScope",
            "Rails/Env",
            "Rails/EnvironmentVariableAccess",
            "Rails/OrderById",
            "Rails/PluckId",
            "Rails/RequireDependency",
            "Rails/ReversibleMigrationMethodDefinition",
            "Rails/SaveBang",
            "Rails/SchemaComment",
            "Rails/TableNameAssignment",
            "Rails/UnusedIgnoredColumns",
        ];

        for name in DISABLED_BY_DEFAULT {
            assert!(
                !cfg.cop_enabled(name),
                "{name} should be disabled by default"
            );
        }

        assert!(cfg.cop_enabled("Rails/ActionControllerTestCase"));
        assert!(cfg.cop_enabled("Rails/ActionControllerFlashBeforeRender"));
        assert!(cfg.cop_enabled("Rails/AddColumnIndex"));
        assert!(cfg.cop_enabled("Unknown/Foo"));
    }

    #[test]
    fn cop_enabled_can_override_default_for_rails_cop() {
        let cfg = MurphyConfig::from_toml_str(
            r#"
[cops.rules."Rails/ActionFilter"]
enabled = true
"#,
        )
        .expect("config parses");

        assert!(cfg.cop_enabled("Rails/ActionFilter"));
    }

    #[test]
    fn plugins_default_to_empty() {
        let cfg = MurphyConfig::from_toml_str("").expect("empty config parses");
        assert!(cfg.plugins.is_empty());
    }

    #[test]
    fn parses_plugins_detailed_form() {
        let cfg = MurphyConfig::from_toml_str(
            r#"
[[plugins]]
name = "murphy-example-pack"
path = "target/debug/libmurphy_example_pack.so"
"#,
        )
        .unwrap();
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
    fn parses_plugins_name_only_form() {
        let cfg = MurphyConfig::from_toml_str(r#"plugins = ["murphy-rails"]"#).unwrap();
        assert_eq!(cfg.plugins.len(), 1);
        match &cfg.plugins[0] {
            PluginConfig::Name(name) => assert_eq!(name, "murphy-rails"),
            other => panic!("expected Name, got {other:?}"),
        }
    }

    #[test]
    fn parses_plugins_heterogeneous_array() {
        let cfg = MurphyConfig::from_toml_str(
            r#"
plugins = [
  "murphy-rails",
  { name = "local-pack", path = "./libfoo.so" }
]
"#,
        )
        .unwrap();
        assert_eq!(cfg.plugins.len(), 2);
        assert!(matches!(&cfg.plugins[0], PluginConfig::Name(n) if n == "murphy-rails"));
        assert!(matches!(&cfg.plugins[1], PluginConfig::Detailed(d) if d.name == "local-pack"));
    }

    #[test]
    fn migrate_plugins_emits_rubocop_rename_hint_comment() {
        // RuboCop's `plugins: rubocop-foo` migrates to a TOML
        // `plugins = ["rubocop-foo"]`. The user still has to rename
        // `rubocop-` → `murphy-` themselves (ADR 0041 / 0042: no auto-rename).
        // The migrate output emits a single `# NOTE: ...` line above the
        // `plugins = [...]` line so the user sees the rename requirement
        // immediately instead of getting a cryptic "plugin not found" at
        // first lint run.
        let out =
            migrate_rubocop_yml_to_murphy_toml("plugins:\n  - rubocop-rails\n  - rubocop-rspec\n")
                .unwrap();
        assert!(
            out.contains("plugins = [\"rubocop-rails\", \"rubocop-rspec\"]"),
            "plugins line preserved verbatim:\n{out}"
        );
        assert!(
            out.contains("# NOTE:") && out.contains("rubocop-") && out.contains("murphy-"),
            "expected rename-hint NOTE line:\n{out}"
        );
    }

    #[test]
    fn migrate_plugins_scalar_form_treated_as_single_element() {
        // RuboCop 互換: `plugins: foo` を `plugins: [foo]` と同義に扱う
        let out = migrate_rubocop_yml_to_murphy_toml("plugins: rubocop-rails\n").unwrap();
        assert!(
            out.contains("plugins = [\"rubocop-rails\"]"),
            "scalar plugin should be lifted into 1-element array:\n{out}"
        );
    }

    #[test]
    fn migrate_plugins_non_sequence_non_string_emits_unsupported() {
        // `plugins: 42` のように sequence でも string でもない場合、
        // データを silently drop せず unsupported コメントで明示する
        let out = migrate_rubocop_yml_to_murphy_toml("plugins: 42\n").unwrap();
        assert!(
            out.contains("# unsupported plugin entry:"),
            "non-sequence non-string plugins: value should emit unsupported comment:\n{out}"
        );
        assert!(
            !out.contains("plugins ="),
            "should not emit plugins = ... line when input was unsupported:\n{out}"
        );
    }

    #[test]
    fn migrate_plugins_non_string_item_emits_unsupported() {
        // Sequence 内の非 string / 非 mapping 要素も silently drop しない
        let out =
            migrate_rubocop_yml_to_murphy_toml("plugins:\n  - rubocop-rails\n  - 42\n  - true\n")
                .unwrap();
        assert!(
            out.contains("plugins = [\"rubocop-rails\"]"),
            "valid string item should still be present:\n{out}"
        );
        // 42 と true の 2 つ分の unsupported コメント (順序非依存)
        let unsupported_count = out.matches("# unsupported plugin entry:").count();
        assert_eq!(
            unsupported_count, 2,
            "expected 2 unsupported comments for 42 and true, got {unsupported_count}:\n{out}"
        );
    }

    #[test]
    fn migrate_plugins_unsupported_name_with_newline_sanitized_to_single_line() {
        // セキュリティ: 悪意ある YAML mapping-key (改行入り) が migrate 出力に
        // 任意 TOML を inject できないこと。改行 / 制御文字は `?` 置換される。
        let input = "plugins:\n  - \"evil\\n[[plugins]]\\nname = 'x'\":\n      foo: bar\n";
        let out = migrate_rubocop_yml_to_murphy_toml(input).unwrap();
        // LINE-START の "[[plugins]]" (= 有効な TOML section header) が injection
        // されていないこと。comment 内に文字列として残るのは無害なので、行頭判定。
        let injected = out
            .lines()
            .any(|l| l.trim_start().starts_with("[[plugins]]"));
        assert!(
            !injected,
            "unsupported plugin name with newlines must not inject a [[plugins]] TOML section header:\n{out}"
        );
        // 改行は `?` に置換され、unsupported comment は 1 行に押し込められる
        let unsupported_lines: Vec<&str> = out
            .lines()
            .filter(|l| l.starts_with("# unsupported plugin entry:"))
            .collect();
        assert_eq!(
            unsupported_lines.len(),
            1,
            "expected exactly 1 unsupported comment line, got {}:\n{out}",
            unsupported_lines.len()
        );
        assert!(
            !unsupported_lines[0].contains('\n') && !unsupported_lines[0].contains('\r'),
            "sanitized line must not contain control chars:\n{}",
            unsupported_lines[0]
        );
        // sanitization マーカ `?` が含まれること (injection 試行が検出された証拠)
        assert!(
            unsupported_lines[0].contains('?'),
            "control chars in input should be replaced with `?`:\n{}",
            unsupported_lines[0]
        );
    }

    #[test]
    fn migrate_plugins_empty_mapping_item_emits_unsupported() {
        // `- {}` のような空 mapping も silently drop せず unsupported に
        let out = migrate_rubocop_yml_to_murphy_toml("plugins:\n  - {}\n").unwrap();
        assert!(
            out.contains("# unsupported plugin entry: <empty or non-string key>"),
            "empty mapping should emit named unsupported comment:\n{out}"
        );
    }

    #[test]
    fn plugins_detailed_rejects_unknown_field() {
        // PluginDetailed carries its own `deny_unknown_fields`, so a
        // stray key on a Detailed entry surfaces as an unknown-field
        // error instead of being silently accepted (the previous
        // limitation around untagged-enum struct variants — fixed in
        // murphy-9cr.10.3 by extracting PluginDetailed).
        let err = MurphyConfig::from_toml_str(
            r#"
[[plugins]]
name = "x"
path = "y"
version = "0.1"
"#,
        )
        .expect_err("unknown field on Detailed should error");
        let msg = err.to_string();
        assert!(
            msg.contains("unknown field") && msg.contains("version"),
            "expected unknown-field error mentioning `version`, got: {msg}"
        );
    }

    #[test]
    fn plugins_detailed_missing_path_yields_clear_error() {
        // Confirms the user-facing error for `[[plugins]] name = "x"`
        // (missing `path`) names the missing field rather than the
        // cryptic untagged-enum "data did not match any variant"
        // fallback that murphy-9cr.10.1 had to live with.
        let err = MurphyConfig::from_toml_str(
            r#"
[[plugins]]
name = "x"
"#,
        )
        .expect_err("missing path should error");
        let msg = err.to_string();
        assert!(
            msg.contains("missing field") && msg.contains("path"),
            "expected `missing field 'path'`-style error, got: {msg}"
        );
    }
}
