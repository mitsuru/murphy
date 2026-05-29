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

/// Plugin pack entry from `plugins:` in `.murphy.yml`.
///
/// Two shapes (RuboCop-compatible):
/// - `- murphy-rails` — name-only shorthand. Resolved at load time against
///   the search path (ADR 0042): same-array `Detailed` override →
///   `MURPHY_PLUGIN_PATH` env → project-local `.murphy/plugins/` →
///   user-local `$XDG_DATA_HOME/murphy/plugins/`.
/// - `- name: "..." path: "..."` — explicit path; bypasses the search path.
///
/// Deserialization dispatches manually on input shape (string vs. mapping)
/// instead of `#[serde(untagged)]`: an untagged enum buffers the input, tries
/// each variant, and swallows inner diagnostics into a generic "data did not
/// match any variant". The hand-rolled `Visitor` routes a mapping straight
/// into `PluginDetailed` so its errors propagate verbatim.
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
                f.write_str(r#"a plugin name string or { name: "...", path: "..." } mapping"#)
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

/// Explicit-path plugin entry: `- name: "..." path: "..."`.
///
/// Split out of [`PluginConfig::Detailed`] so that `deny_unknown_fields`
/// and `missing field` diagnostics survive the surrounding custom visitor.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginDetailed {
    pub name: String,
    pub path: PathBuf,
}

/// Per-cop configuration from a top-level cop section in `.murphy.yml`.
///
/// Reserved keys (`Enabled`, `Severity`) are extracted as typed fields;
/// all other keys pass through to `options` for the cop's own parser.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CopRule {
    pub enabled: Option<bool>,
    pub severity: Option<Severity>,
    pub options: BTreeMap<String, serde_yaml::Value>,
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
        let yaml: serde_yaml::Value =
            serde_yaml::from_str(text).map_err(|e| ConfigError::BadYaml(e.to_string()))?;

        // An empty document, a comment-only document, or `---` / `~` / `null`
        // all parse to `Value::Null`; treat them as "no config" (defaults),
        // matching RuboCop's behavior for empty / comment-only config files.
        let top = match yaml {
            serde_yaml::Value::Mapping(m) => m,
            serde_yaml::Value::Null => return Ok(Self::default()),
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
            let Some(section) = key.as_str() else {
                continue;
            };

            match section {
                "AllCops" => {
                    if let serde_yaml::Value::Mapping(all_cops) = value {
                        if let Some(inc) = all_cops.get("Include") {
                            include = yaml_string_list(Some(inc));
                            saw_include = true;
                        }
                        if let Some(exc) = all_cops.get("Exclude") {
                            exclude = yaml_string_list(Some(exc));
                        }
                        if let Some(path) = all_cops.get("CopsPath").and_then(|v| v.as_str()) {
                            cops_path = PathBuf::from(path);
                        }
                    }
                }
                "plugins" => {
                    plugins = serde_yaml::from_value(value)
                        .map_err(|e| ConfigError::BadYaml(e.to_string()))?;
                }
                _ => {
                    // Treat as a cop rule (open-keyed, compatible with RuboCop format).
                    if let serde_yaml::Value::Mapping(cop_map) = value {
                        rules.insert(section.to_string(), parse_cop_rule(cop_map));
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
    /// Used by the `cops list` / lint flow to detect a user trying to opt back
    /// into a cop that is currently in the disabled registry (arena migration),
    /// so the host can emit a warning without breaking the lint run (§12c).
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

fn parse_cop_rule(map: serde_yaml::Mapping) -> CopRule {
    let mut rule = CopRule::default();
    for (key, value) in map {
        let Some(k) = key.as_str() else {
            continue;
        };
        match k {
            "Enabled" => {
                rule.enabled = value.as_bool();
            }
            "Severity" => {
                if let Some(s) = value.as_str() {
                    rule.severity = match s {
                        "warning" => Some(Severity::Warning),
                        "error" => Some(Severity::Error),
                        _ => None,
                    };
                }
            }
            other => {
                rule.options.insert(other.to_string(), value);
            }
        }
    }
    rule
}

fn is_cop_disabled_by_default(name: &str) -> bool {
    // murphy-2ob §14a: until `MurphyConfig::cop_enabled` learns to
    // consult the registry's `DEFAULT_ENABLED` (murphy-bnd), keep the
    // Rails 138-cop arena-migration stub pack's default-off status
    // here as a hardcoded fallback. The list mirrors murphy-rails's
    // `register_cops!` contents one-for-one — adding/removing a cop
    // in murphy-rails requires the same change here until murphy-bnd
    // lands.
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
/// The output is a `.murphy.yml`-compatible YAML document. Since `.murphy.yml`
/// is intentionally compatible with `.rubocop.yml`, migration is minimal:
/// - Adds `CopsPath: cops` under `AllCops` if not present.
/// - Emits a rename-hint comment when `rubocop-` plugin names are detected.
/// - Emits unsupported-plugin comments for non-string/non-mapping plugin entries.
/// - All cop rules pass through verbatim (keys, values, options).
///
/// Lossy: `inherit_from`, `inherit_gem`, and other RuboCop-engine directives
/// have no Murphy equivalent and are silently dropped during load. They are
/// preserved in the output text but will be ignored by `MurphyConfig::load`.
pub fn migrate_rubocop_yml_to_murphy_yml(text: &str) -> Result<String, ConfigError> {
    let yaml: serde_yaml::Value =
        serde_yaml::from_str(text).map_err(|e| ConfigError::BadYaml(e.to_string()))?;

    let serde_yaml::Value::Mapping(mut top) = yaml else {
        return Err(ConfigError::BadYaml(
            "top-level document must be a mapping".to_string(),
        ));
    };

    let mut plugin_names: Vec<String> = Vec::new();
    let mut unsupported_plugins: Vec<String> = Vec::new();

    // Extract plugin info for the rename hint (do not modify the value yet).
    if let Some(plugins_val) = top.get("plugins") {
        let items: Vec<serde_yaml::Value> = match plugins_val {
            serde_yaml::Value::Sequence(seq) => seq.clone(),
            serde_yaml::Value::String(s) => vec![serde_yaml::Value::String(s.clone())],
            other => {
                unsupported_plugins.push(format!("{other:?} (unsupported plugins: form)"));
                vec![]
            }
        };
        for item in items {
            match item {
                serde_yaml::Value::String(s) => plugin_names.push(s),
                serde_yaml::Value::Mapping(m) => {
                    if let Some(name) = m
                        .into_iter()
                        .next()
                        .and_then(|(k, _)| k.as_str().map(|s| s.to_string()))
                    {
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
    }

    // Inject AllCops.CopsPath = "cops" if not already set.
    let all_cops_key = serde_yaml::Value::String("AllCops".to_string());
    match top.get_mut(&all_cops_key) {
        Some(serde_yaml::Value::Mapping(all_cops)) => {
            let cops_path_key = serde_yaml::Value::String("CopsPath".to_string());
            if !all_cops.contains_key(&cops_path_key) {
                all_cops.insert(cops_path_key, serde_yaml::Value::String("cops".to_string()));
            }
        }
        _ => {
            let mut all_cops = serde_yaml::Mapping::new();
            all_cops.insert(
                serde_yaml::Value::String("Include".to_string()),
                serde_yaml::Value::Sequence(vec![serde_yaml::Value::String("**/*.rb".to_string())]),
            );
            all_cops.insert(
                serde_yaml::Value::String("Exclude".to_string()),
                serde_yaml::Value::Sequence(vec![]),
            );
            all_cops.insert(
                serde_yaml::Value::String("CopsPath".to_string()),
                serde_yaml::Value::String("cops".to_string()),
            );
            top.insert(all_cops_key, serde_yaml::Value::Mapping(all_cops));
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
    }
    for unsupported in &unsupported_plugins {
        // `unsupported` comes from user-supplied YAML — sanitize control
        // characters so a malicious plugin name cannot inject extra YAML lines.
        let sanitized: String = unsupported
            .chars()
            .map(|c| if c.is_control() { '?' } else { c })
            .collect();
        out.push_str(&format!("# unsupported plugin entry: {sanitized}\n"));
    }

    let yaml_str = serde_yaml::to_string(&serde_yaml::Value::Mapping(top))
        .map_err(|e| ConfigError::BadYaml(e.to_string()))?;
    out.push_str(&yaml_str);

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
        let cfg = MurphyConfig::from_yaml_str("").expect("empty config parses");
        assert_eq!(cfg.files.include, vec!["**/*.rb"]);
        assert_eq!(cfg.cops.path, PathBuf::from("cops"));
        assert!(cfg.cops.rules.is_empty());
    }

    #[test]
    fn comment_only_file_parses_as_defaults() {
        // RuboCop compatibility: a comment-only or blank config file must not
        // error; it should produce the same result as an absent config file.
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
    fn cop_rule_preserves_rubocop_compatible_options() {
        let cfg = MurphyConfig::from_yaml_str(
            r#"
Style/StringLiterals:
  Enabled: true
  Severity: warning
  EnforcedStyle: single_quotes
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
            Some(&serde_yaml::Value::String("single_quotes".to_string()))
        );
        assert_eq!(
            rule.options.get("Exclude"),
            Some(&serde_yaml::Value::Sequence(vec![
                serde_yaml::Value::String("db/schema.rb".to_string())
            ]))
        );
    }

    #[test]
    fn cop_enabled_is_false_for_rails_cops_disabled_by_default() {
        let cfg = MurphyConfig::from_yaml_str("").expect("empty config parses");

        // murphy-2ob §14a: the 138 Rails cops registered as arena-
        // migration stubs in murphy-rails are all default-off via the
        // `is_cop_disabled_by_default` hardcode fallback.
        const DISABLED_BY_DEFAULT_SAMPLE: [&str; 15] = [
            "Rails/ActionControllerFlashBeforeRender",
            "Rails/ActionControllerTestCase",
            "Rails/AddColumnIndex",
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

        for name in DISABLED_BY_DEFAULT_SAMPLE {
            assert!(
                !cfg.cop_enabled(name),
                "{name} should be disabled by default"
            );
        }

        assert!(cfg.cop_enabled("Unknown/Foo"));
    }

    #[test]
    fn cop_enabled_can_override_default_for_rails_cop() {
        let cfg = MurphyConfig::from_yaml_str(
            r#"
Rails/ActionFilter:
  Enabled: true
"#,
        )
        .expect("config parses");

        assert!(cfg.cop_enabled("Rails/ActionFilter"));
    }

    #[test]
    fn plugins_default_to_empty() {
        let cfg = MurphyConfig::from_yaml_str("").expect("empty config parses");
        assert!(cfg.plugins.is_empty());
    }

    #[test]
    fn parses_plugins_detailed_form() {
        let cfg = MurphyConfig::from_yaml_str(
            r#"
plugins:
  - name: murphy-example-pack
    path: target/debug/libmurphy_example_pack.so
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
        let cfg = MurphyConfig::from_yaml_str("plugins:\n  - murphy-rails\n").unwrap();
        assert_eq!(cfg.plugins.len(), 1);
        match &cfg.plugins[0] {
            PluginConfig::Name(name) => assert_eq!(name, "murphy-rails"),
            other => panic!("expected Name, got {other:?}"),
        }
    }

    #[test]
    fn parses_plugins_heterogeneous_array() {
        let cfg = MurphyConfig::from_yaml_str(
            r#"
plugins:
  - murphy-rails
  - name: local-pack
    path: ./libfoo.so
"#,
        )
        .unwrap();
        assert_eq!(cfg.plugins.len(), 2);
        assert!(matches!(&cfg.plugins[0], PluginConfig::Name(n) if n == "murphy-rails"));
        assert!(matches!(&cfg.plugins[1], PluginConfig::Detailed(d) if d.name == "local-pack"));
    }

    #[test]
    fn migrate_plugins_emits_rubocop_rename_hint_comment() {
        // RuboCop's `plugins: rubocop-foo` migrates to `.murphy.yml` with the
        // plugin name preserved verbatim. The user still has to rename
        // `rubocop-` → `murphy-` themselves (ADR 0041 / 0042: no auto-rename).
        // The migrate output emits a single `# NOTE: ...` line so the user sees
        // the rename requirement immediately instead of getting a cryptic
        // "plugin not found" at first lint run.
        let out =
            migrate_rubocop_yml_to_murphy_yml("plugins:\n  - rubocop-rails\n  - rubocop-rspec\n")
                .unwrap();
        assert!(
            out.contains("rubocop-rails") && out.contains("rubocop-rspec"),
            "plugin names preserved:\n{out}"
        );
        assert!(
            out.contains("# NOTE:") && out.contains("rubocop-") && out.contains("murphy-"),
            "expected rename-hint NOTE line:\n{out}"
        );
    }

    #[test]
    fn migrate_plugins_scalar_form_treated_as_single_element() {
        // RuboCop 互換: `plugins: foo` を `plugins: [foo]` と同義に扱う
        let out = migrate_rubocop_yml_to_murphy_yml("plugins: rubocop-rails\n").unwrap();
        assert!(
            out.contains("rubocop-rails"),
            "scalar plugin should be present:\n{out}"
        );
        assert!(
            out.contains("# NOTE:"),
            "rename hint should be present:\n{out}"
        );
    }

    #[test]
    fn migrate_plugins_non_sequence_non_string_emits_unsupported() {
        // `plugins: 42` のように sequence でも string でもない場合、
        // データを silently drop せず unsupported コメントで明示する
        let out = migrate_rubocop_yml_to_murphy_yml("plugins: 42\n").unwrap();
        assert!(
            out.contains("# unsupported plugin entry:"),
            "non-sequence non-string plugins: value should emit unsupported comment:\n{out}"
        );
    }

    #[test]
    fn migrate_plugins_non_string_item_emits_unsupported() {
        // Sequence 内の非 string / 非 mapping 要素も silently drop しない
        let out =
            migrate_rubocop_yml_to_murphy_yml("plugins:\n  - rubocop-rails\n  - 42\n  - true\n")
                .unwrap();
        assert!(
            out.contains("rubocop-rails"),
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
        // 任意 YAML を inject できないこと。改行 / 制御文字は `?` 置換される。
        let input = "plugins:\n  - \"evil\\n[malicious]\\nfoo: bar\":\n      foo: bar\n";
        let out = migrate_rubocop_yml_to_murphy_yml(input).unwrap();
        // unsupported comment は 1 行に押し込められる
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
        // sanitization マーカ `?` が含まれること
        assert!(
            unsupported_lines[0].contains('?'),
            "control chars in input should be replaced with `?`:\n{}",
            unsupported_lines[0]
        );
    }

    #[test]
    fn migrate_plugins_empty_mapping_item_emits_unsupported() {
        // `- {}` のような空 mapping も silently drop せず unsupported に
        let out = migrate_rubocop_yml_to_murphy_yml("plugins:\n  - {}\n").unwrap();
        assert!(
            out.contains("# unsupported plugin entry: <empty or non-string key>"),
            "empty mapping should emit named unsupported comment:\n{out}"
        );
    }

    #[test]
    fn plugins_detailed_rejects_unknown_field() {
        // PluginDetailed carries its own `deny_unknown_fields`.
        let err = MurphyConfig::from_yaml_str(
            r#"
plugins:
  - name: x
    path: "y"
    version: "0.1"
"#,
        )
        .expect_err("unknown field on Detailed should error");
        let msg = err.to_string();
        assert!(
            msg.contains("unknown field") || msg.contains("version"),
            "expected unknown-field error mentioning `version`, got: {msg}"
        );
    }

    #[test]
    fn plugins_detailed_missing_path_yields_clear_error() {
        let err = MurphyConfig::from_yaml_str(
            r#"
plugins:
  - name: x
"#,
        )
        .expect_err("missing path should error");
        let msg = err.to_string();
        assert!(
            msg.contains("missing field") || msg.contains("path"),
            "expected `missing field 'path'`-style error, got: {msg}"
        );
    }

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
            "CopsPath value 'cops' should be present:\n{out}"
        );
    }

    #[test]
    fn migrate_creates_all_cops_section_if_absent() {
        let out = migrate_rubocop_yml_to_murphy_yml("Style/NoPuts:\n  Enabled: false\n").unwrap();
        assert!(
            out.contains("AllCops"),
            "AllCops section should be created:\n{out}"
        );
        assert!(
            out.contains("CopsPath"),
            "CopsPath should be present:\n{out}"
        );
    }

    #[test]
    fn migrate_cop_rules_pass_through_verbatim() {
        let out = migrate_rubocop_yml_to_murphy_yml(
            "Style/NoPuts:\n  Enabled: false\n  Severity: error\n",
        )
        .unwrap();
        assert!(out.contains("Style/NoPuts"), "cop name present:\n{out}");
        assert!(out.contains("Enabled"), "Enabled present:\n{out}");
        assert!(out.contains("false"), "false present:\n{out}");
        assert!(out.contains("Severity"), "Severity present:\n{out}");
        assert!(out.contains("error"), "error present:\n{out}");
    }
}
