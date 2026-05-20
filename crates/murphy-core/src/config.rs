use crate::Severity;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::ConfigError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MurphyConfig {
    pub files: FilesConfig,
    pub cops: CopsConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilesConfig {
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopsConfig {
    pub path: PathBuf,
    pub rules: BTreeMap<String, CopRule>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CopRule {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub severity: Option<Severity>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MurphyToml {
    #[serde(default)]
    files: FilesTable,
    #[serde(default)]
    cops: CopsTable,
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
            .unwrap_or(true)
    }

    pub fn severity_override(&self, name: &str) -> Option<Severity> {
        self.cops.rules.get(name).and_then(|rule| rule.severity)
    }
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

    let serde_yaml::Value::Mapping(top) = yaml else {
        return Err(ConfigError::BadYaml(
            "top-level document must be a mapping".to_string(),
        ));
    };

    for (key, value) in top {
        let Some(section) = key.as_str() else {
            continue;
        };
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
}
