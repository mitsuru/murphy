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
///   `.rubocop.yml` plugins: directive compatibility. Search-path
///   resolution is deferred to `murphy-9cr.10.2`; in the MVP the
///   registry returns a setup error directing the user to the detailed
///   form.
/// - `[[plugins]] name = "..." path = "..."` — explicit path. The
///   MVP-supported form.
///
/// ## Documented limitation
///
/// `#[serde(deny_unknown_fields)]` is not fully honored on struct
/// variants inside `#[serde(untagged)]` enums — additional fields on
/// the `Detailed` variant (e.g. a stray `version = "..."`) are silently
/// accepted. A future refactor will split `Detailed` into a named
/// struct (`PluginDetailed`) with its own `deny_unknown_fields`. See
/// `plugins_unknown_field_silently_accepted_for_now` in tests.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum PluginConfig {
    Name(String),
    Detailed { name: String, path: PathBuf },
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
                        unsupported_plugins
                            .push(format!("{other:?} (non-string / non-mapping)"));
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
        out.push_str(&format!("plugins = {}\n", toml_array(&plugin_names)));
    }
    for unsupported in &unsupported_plugins {
        out.push_str(&format!("# unsupported plugin entry: {unsupported}\n"));
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
            PluginConfig::Detailed { name, path } => {
                assert_eq!(name, "murphy-example-pack");
                assert_eq!(
                    path.to_str(),
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
        assert!(
            matches!(&cfg.plugins[1], PluginConfig::Detailed { name, .. } if name == "local-pack")
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
        let out = migrate_rubocop_yml_to_murphy_toml("plugins:\n  - rubocop-rails\n  - 42\n  - true\n").unwrap();
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
    fn migrate_plugins_empty_mapping_item_emits_unsupported() {
        // `- {}` のような空 mapping も silently drop せず unsupported に
        let out = migrate_rubocop_yml_to_murphy_toml("plugins:\n  - {}\n").unwrap();
        assert!(
            out.contains("# unsupported plugin entry: <empty or non-string key>"),
            "empty mapping should emit named unsupported comment:\n{out}"
        );
    }

    #[test]
    fn plugins_unknown_field_silently_accepted_for_now() {
        // serde の untagged enum + struct variant は variant 内側で
        // deny_unknown_fields を受け付けない。将来的に PluginDetailed を
        // 別 struct に切り出して deny_unknown_fields を効かせる予定 —
        // それまでは unknown field を silently accept する。
        let cfg = MurphyConfig::from_toml_str(
            r#"
[[plugins]]
name = "x"
path = "y"
version = "0.1"
"#,
        )
        .unwrap();
        assert_eq!(cfg.plugins.len(), 1);
    }
}
