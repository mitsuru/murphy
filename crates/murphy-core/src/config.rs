use crate::Severity;
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
    pub options: BTreeMap<String, serde_json::Value>,
}

fn default_include() -> Vec<String> {
    vec!["**/*.rb".to_string()]
}

fn default_cops_path() -> PathBuf {
    PathBuf::from("cops")
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
        use yaml_rust2::{Yaml, YamlLoader};

        let docs =
            YamlLoader::load_from_str(text).map_err(|e| ConfigError::BadYaml(e.to_string()))?;

        let doc = match docs.into_iter().next() {
            None => return Ok(Self::default()),
            Some(d) => d,
        };

        // An empty document, a comment-only document, or `---` / `~` / `null`
        // all produce Yaml::Null; treat them as defaults (RuboCop-compatible).
        let top = match doc {
            Yaml::Hash(h) => h,
            Yaml::Null => return Ok(Self::default()),
            _ => {
                return Err(ConfigError::BadYaml(
                    "top-level document must be a mapping".to_string(),
                ));
            }
        };

        let mut include = default_include();
        let mut exclude: Vec<String> = Vec::new();
        let mut cops_path = default_cops_path();
        let mut rules: BTreeMap<String, CopRule> = BTreeMap::new();
        let mut plugins: Vec<PluginConfig> = Vec::new();
        let mut saw_include = false;

        for (key, value) in top {
            let Yaml::String(section) = key else {
                continue;
            };

            match section.as_str() {
                "AllCops" => {
                    if let Yaml::Hash(all_cops) = value {
                        if let Some(inc) = all_cops.get(&Yaml::String("Include".to_string())) {
                            include = yaml_string_list(inc);
                            saw_include = true;
                        }
                        if let Some(exc) = all_cops.get(&Yaml::String("Exclude".to_string())) {
                            exclude = yaml_string_list(exc);
                        }
                        if let Some(Yaml::String(p)) =
                            all_cops.get(&Yaml::String("CopsPath".to_string()))
                        {
                            cops_path = PathBuf::from(p);
                        }
                    }
                }
                "plugins" => {
                    plugins =
                        parse_plugins(value).map_err(|e| ConfigError::BadYaml(e.to_string()))?;
                }
                _ => {
                    // Treat as a cop rule (open-keyed, compatible with RuboCop format).
                    if let Yaml::Hash(cop_map) = value {
                        rules.insert(section, parse_cop_rule(cop_map));
                    }
                }
            }
        }

        if !saw_include {
            include = default_include();
        }

        Ok(MurphyConfig {
            files: FilesConfig { include, exclude },
            cops: CopsConfig {
                path: cops_path,
                rules,
            },
            plugins,
        })
    }

    pub fn load(root: &Path) -> Result<Self, ConfigError> {
        let config_path = root.join(".murphy.yml");
        match std::fs::read_to_string(&config_path) {
            Ok(text) => Self::from_yaml_str(&text),
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

    /// True when the user wrote `Enabled: true` for a cop in `.murphy.yml`.
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

/// Parse a `plugins:` value (sequence or scalar string).
fn parse_plugins(value: yaml_rust2::Yaml) -> Result<Vec<PluginConfig>, String> {
    use yaml_rust2::Yaml;
    match value {
        Yaml::String(s) => Ok(vec![PluginConfig::Name(s)]),
        Yaml::Array(arr) => arr.into_iter().map(parse_plugin_entry).collect(),
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
            other => {
                if let Some(json_val) = yaml_to_json(value) {
                    rule.options.insert(other.to_string(), json_val);
                }
            }
        }
    }
    rule
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
                if let Some(key) = key && let Some(val) = yaml_to_json(v) {
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

fn is_cop_disabled_by_default(name: &str) -> bool {
    matches!(
        name,
        "Rails/ActionControllerFlashBeforeRender"
            | "Rails/ActionControllerTestCase"
            | "Rails/ActionFilter"
            | "Rails/ActionOrder"
            | "Rails/ActiveRecordAliases"
            | "Rails/ActiveRecordCallbacksOrder"
            | "Rails/ActiveRecordOverride"
            | "Rails/ActiveSupportAliases"
            | "Rails/ActiveSupportOnLoad"
            | "Rails/AddColumnIndex"
            | "Rails/AfterCommitOverride"
            | "Rails/ApplicationController"
            | "Rails/ApplicationJob"
            | "Rails/ApplicationMailer"
            | "Rails/ApplicationRecord"
            | "Rails/ArelStar"
            | "Rails/AttributeDefaultBlockValue"
            | "Rails/BelongsTo"
            | "Rails/Blank"
            | "Rails/BulkChangeTable"
            | "Rails/CompactBlank"
            | "Rails/ContentTag"
            | "Rails/CreateTableWithTimestamps"
            | "Rails/DangerousColumnNames"
            | "Rails/Date"
            | "Rails/DefaultScope"
            | "Rails/Delegate"
            | "Rails/DelegateAllowBlank"
            | "Rails/DeprecatedActiveModelErrorsMethods"
            | "Rails/DotSeparatedKeys"
            | "Rails/DuplicateAssociation"
            | "Rails/DuplicateScope"
            | "Rails/DurationArithmetic"
            | "Rails/DynamicFindBy"
            | "Rails/EagerEvaluationLogMessage"
            | "Rails/EnumHash"
            | "Rails/EnumSyntax"
            | "Rails/EnumUniqueness"
            | "Rails/Env"
            | "Rails/EnvLocal"
            | "Rails/EnvironmentComparison"
            | "Rails/EnvironmentVariableAccess"
            | "Rails/Exit"
            | "Rails/ExpandedDateRange"
            | "Rails/FilePath"
            | "Rails/FindBy"
            | "Rails/FindById"
            | "Rails/FindByOrAssignmentMemoization"
            | "Rails/FindEach"
            | "Rails/FreezeTime"
            | "Rails/HasAndBelongsToMany"
            | "Rails/HasManyOrHasOneDependent"
            | "Rails/HelperInstanceVariable"
            | "Rails/HttpPositionalArguments"
            | "Rails/HttpStatus"
            | "Rails/HttpStatusNameConsistency"
            | "Rails/I18nLazyLookup"
            | "Rails/I18nLocaleTexts"
            | "Rails/IgnoredColumnsAssignment"
            | "Rails/IgnoredSkipActionFilterOption"
            | "Rails/IndexBy"
            | "Rails/IndexWith"
            | "Rails/Inquiry"
            | "Rails/InverseOf"
            | "Rails/LexicallyScopedActionFilter"
            | "Rails/LinkToBlank"
            | "Rails/MailerName"
            | "Rails/MatchRoute"
            | "Rails/MigrationClassName"
            | "Rails/MultipleRoutePaths"
            | "Rails/NotNullColumn"
            | "Rails/OrderArguments"
            | "Rails/OrderById"
            | "Rails/OutputSafety"
            | "Rails/Pluck"
            | "Rails/PluckId"
            | "Rails/PluckInWhere"
            | "Rails/PluralizationGrammar"
            | "Rails/Presence"
            | "Rails/Present"
            | "Rails/RakeEnvironment"
            | "Rails/ReadWriteAttribute"
            | "Rails/RedirectBackOrTo"
            | "Rails/RedundantActiveRecordAllMethod"
            | "Rails/RedundantAllowNil"
            | "Rails/RedundantForeignKey"
            | "Rails/RedundantPresenceValidationOnBelongsTo"
            | "Rails/RedundantReceiverInWithOptions"
            | "Rails/RedundantTravelBack"
            | "Rails/ReflectionClassName"
            | "Rails/RefuteMethods"
            | "Rails/RelativeDateConstant"
            | "Rails/RenderInline"
            | "Rails/RenderPlainText"
            | "Rails/RequireDependency"
            | "Rails/ResponseParsedBody"
            | "Rails/ReversibleMigration"
            | "Rails/ReversibleMigrationMethodDefinition"
            | "Rails/RootJoinChain"
            | "Rails/RootPathnameMethods"
            | "Rails/RootPublicPath"
            | "Rails/SafeNavigation"
            | "Rails/SafeNavigationWithBlank"
            | "Rails/SaveBang"
            | "Rails/SchemaComment"
            | "Rails/ScopeArgs"
            | "Rails/SelectMap"
            | "Rails/ShortI18n"
            | "Rails/SkipsModelValidations"
            | "Rails/SquishedSQLHeredocs"
            | "Rails/StripHeredoc"
            | "Rails/StrongParametersExpect"
            | "Rails/TableNameAssignment"
            | "Rails/ThreeStateBooleanColumn"
            | "Rails/TimeZone"
            | "Rails/TimeZoneAssignment"
            | "Rails/ToFormattedS"
            | "Rails/ToSWithArgument"
            | "Rails/TopLevelHashWithIndifferentAccess"
            | "Rails/TransactionExitStatement"
            | "Rails/UniqueValidationWithoutIndex"
            | "Rails/UnknownEnv"
            | "Rails/UnusedIgnoredColumns"
            | "Rails/UnusedRenderContent"
            | "Rails/Validation"
            | "Rails/WhereEquals"
            | "Rails/WhereExists"
            | "Rails/WhereMissing"
            | "Rails/WhereNot"
            | "Rails/WhereNotWithMultipleConditions"
            | "Rails/WhereRange"
    )
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
        assert!(cfg.cops.rules.is_empty());
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
        assert_eq!(
            rule.options.get("Exclude"),
            Some(&serde_json::Value::Array(vec![serde_json::Value::String(
                "db/schema.rb".to_string()
            )]))
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
    fn cop_enabled_is_false_for_rails_cops_disabled_by_default() {
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
                !cfg.cop_enabled(name),
                "{name} should be disabled by default"
            );
        }
        assert!(cfg.cop_enabled("Unknown/Foo"));
    }

    #[test]
    fn cop_enabled_can_override_default_for_rails_cop() {
        let cfg = MurphyConfig::from_yaml_str("Rails/ActionFilter:\n  Enabled: true\n")
            .expect("config parses");
        assert!(cfg.cop_enabled("Rails/ActionFilter"));
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
}
