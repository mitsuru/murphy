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
    let mut include: Vec<String> = Vec::new();
    let mut exclude: Vec<String> = Vec::new();
    let mut rules: BTreeMap<String, CopRule> = BTreeMap::new();
    let mut section: Option<String> = None;
    let mut list_key: Option<String> = None;

    for raw in text.lines() {
        let line = raw.trim_end();
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed == "---" {
            continue;
        }

        let indent = line.len() - line.trim_start().len();
        if indent == 0 && trimmed.ends_with(':') {
            section = Some(trimmed.trim_end_matches(':').to_string());
            list_key = None;
            continue;
        }

        if indent == 2 && trimmed.ends_with(':') {
            list_key = Some(trimmed.trim_end_matches(':').to_string());
            continue;
        }

        if indent >= 4 && trimmed.starts_with('-') {
            let value = trimmed
                .trim_start_matches('-')
                .trim()
                .trim_matches('"')
                .trim_matches('\'');
            match (section.as_deref(), list_key.as_deref()) {
                (Some("AllCops"), Some("Include")) => include.push(value.to_string()),
                (Some("AllCops"), Some("Exclude")) => exclude.push(value.to_string()),
                _ => {}
            }
            continue;
        }

        if indent == 2 {
            let Some((key, value)) = trimmed.split_once(':') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim().trim_matches('"').trim_matches('\'');
            if let Some(cop_name) = section.as_deref().filter(|name| *name != "AllCops") {
                let rule = rules.entry(cop_name.to_string()).or_default();
                match key {
                    "Enabled" => match value {
                        "true" => rule.enabled = Some(true),
                        "false" => rule.enabled = Some(false),
                        _ => {}
                    },
                    "Severity" => match value {
                        "warning" => rule.severity = Some(Severity::Warning),
                        "error" => rule.severity = Some(Severity::Error),
                        _ => {}
                    },
                    _ => {}
                }
            }
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
