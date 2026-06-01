//! `Style/StringMethods` — prefer configured method names over non-preferred ones.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/StringMethods
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Disabled by default (matches upstream Enabled: false).
//!   The PreferredMethods map is decoded from config; the host pre-merges
//!   default.yml (which contains `intern: to_sym`) with user overrides,
//!   so from_config_json receives the already-merged map.
//!   RuboCop's MethodPreference reverse-conflict filtering (user `bar => foo`
//!   removes the default `foo => bar` entry) is not implemented; this is a
//!   v1 gap. Users can work around it by explicitly clearing the conflicting
//!   entry or disabling the cop.
//!   Both send and csend are handled (mirrors RuboCop's alias on_csend).
//! ```
//!
//! ## Matched shapes
//!
//! Any `send` / `csend` whose method name appears as a key in
//! `PreferredMethods` in the cop config. Default: `intern` → `to_sym`.
//!
//! ## Autocorrect
//!
//! Surgical rename of the selector (`loc.name`) to the preferred method.
//! Single `cx.emit_edit` call — the receiver and arguments are untouched.

use std::collections::BTreeMap;

use murphy_plugin_api::{ConfigError, CopOptions, Cx, NodeId, cop};

const MSG: &str = "Prefer `%preferred%` over `%current%`.";

/// Stateless unit struct.
#[derive(Default)]
pub struct StringMethods;

/// `PreferredMethods: { undesired => preferred }`.
/// `#[derive(CopOptions)]` doesn't model `String → String` maps, so this
/// is hand-rolled following the error-contract rules from
/// `.claude/rules/cop-options-hand-rolled.md`.
#[derive(Clone, Debug)]
pub struct StringMethodsOptions {
    pub preferred_methods: BTreeMap<String, String>,
}

impl Default for StringMethodsOptions {
    fn default() -> Self {
        // Default mirrors RuboCop's default.yml: intern → to_sym.
        // At runtime, the host pre-merges default.yml + user config before
        // calling from_config_json, so the runtime value already includes
        // these defaults unless the user has overridden them.
        let mut map = BTreeMap::new();
        map.insert("intern".to_string(), "to_sym".to_string());
        Self {
            preferred_methods: map,
        }
    }
}

impl CopOptions for StringMethodsOptions {
    fn from_config_json(bytes: &[u8]) -> Result<Self, ConfigError> {
        let value: serde_json::Value = serde_json::from_slice(bytes).map_err(ConfigError::parse)?;
        let obj = value.as_object().ok_or_else(ConfigError::not_an_object)?;

        // Missing `PreferredMethods` → defaults (matches derive's handling of absent fields).
        let Some(pm_value) = obj.get("PreferredMethods") else {
            return Ok(Self::default());
        };

        let pm_obj = pm_value
            .as_object()
            .ok_or_else(|| ConfigError::type_mismatch("PreferredMethods", "object"))?;

        let mut map = BTreeMap::new();
        for (key, val) in pm_obj {
            let preferred = val.as_str().ok_or_else(|| {
                ConfigError::type_mismatch(format!("PreferredMethods.{key}"), "string")
            })?;
            map.insert(key.clone(), preferred.to_string());
        }

        Ok(Self {
            preferred_methods: map,
        })
    }

    fn to_config_json(&self) -> String {
        let pm: serde_json::Map<String, serde_json::Value> = self
            .preferred_methods
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect();
        let mut top = serde_json::Map::new();
        top.insert(
            "PreferredMethods".to_string(),
            serde_json::Value::Object(pm),
        );
        serde_json::Value::Object(top).to_string()
    }
}

#[cop(
    name = "Style/StringMethods",
    description = "Checks if configured preferred methods are used over non-preferred.",
    default_severity = "warning",
    default_enabled = false,
    options = StringMethodsOptions,
)]
impl StringMethods {
    /// Unfiltered send handler — trigger methods come from config, not compile time.
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let Some(method) = cx.method_name(node) else {
        return;
    };

    let opts = cx.options_or_default::<StringMethodsOptions>();
    let Some(preferred) = opts.preferred_methods.get(method) else {
        return;
    };

    let current = method.to_string();
    let preferred = preferred.clone();
    let message = MSG
        .replace("%preferred%", &preferred)
        .replace("%current%", &current);

    let selector_range = cx.node(node).loc.name;
    cx.emit_offense(selector_range, &message, None);
    // Surgical rename — only the selector bytes change, receiver and args untouched.
    cx.emit_edit(selector_range, &preferred);
}

#[cfg(test)]
mod tests {
    use super::{StringMethods, StringMethodsOptions};
    use murphy_plugin_api::{
        CopOptions,
        test_support::{indoc, test},
    };

    // ----- Default config tests -----

    #[test]
    fn flags_intern_default() {
        test::<StringMethods>().expect_correction(
            indoc! {"
                'name'.intern
                       ^^^^^^ Prefer `to_sym` over `intern`.
            "},
            "'name'.to_sym\n",
        );
    }

    #[test]
    fn accepts_to_sym() {
        test::<StringMethods>().expect_no_offenses("'name'.to_sym\n");
    }

    #[test]
    fn accepts_unknown_method() {
        test::<StringMethods>().expect_no_offenses("x.upcase\n");
    }

    // ----- CopOptions decode tests -----

    #[test]
    fn options_default_contains_intern() {
        let opts = StringMethodsOptions::default();
        assert_eq!(
            opts.preferred_methods.get("intern").map(|s| s.as_str()),
            Some("to_sym")
        );
    }

    #[test]
    fn options_decode_custom_methods() {
        let opts =
            StringMethodsOptions::from_config_json(br#"{"PreferredMethods": {"gsub": "sub"}}"#)
                .expect("valid JSON");
        assert_eq!(
            opts.preferred_methods.get("gsub").map(|s| s.as_str()),
            Some("sub")
        );
    }

    #[test]
    fn options_missing_field_gives_default() {
        let opts = StringMethodsOptions::from_config_json(b"{}")
            .expect("valid JSON — missing field uses default");
        assert_eq!(
            opts.preferred_methods.get("intern").map(|s| s.as_str()),
            Some("to_sym")
        );
    }

    #[test]
    fn options_non_object_root_errors() {
        let err =
            StringMethodsOptions::from_config_json(b"[]").expect_err("non-object root is invalid");
        assert!(matches!(
            err.kind(),
            murphy_plugin_api::ConfigErrorKind::NotAnObject
        ));
    }

    #[test]
    fn options_preferred_methods_not_object_errors() {
        let err = StringMethodsOptions::from_config_json(br#"{"PreferredMethods": "bad"}"#)
            .expect_err("non-object PreferredMethods is invalid");
        let murphy_plugin_api::ConfigErrorKind::TypeMismatch { field, expected } = err.kind()
        else {
            panic!("expected TypeMismatch, got {:?}", err.kind());
        };
        assert_eq!(field, "PreferredMethods");
        assert_eq!(*expected, "object");
    }

    #[test]
    fn options_value_not_string_errors() {
        let err =
            StringMethodsOptions::from_config_json(br#"{"PreferredMethods": {"intern": 42}}"#)
                .expect_err("non-string value is invalid");
        let murphy_plugin_api::ConfigErrorKind::TypeMismatch { field, expected } = err.kind()
        else {
            panic!("expected TypeMismatch, got {:?}", err.kind());
        };
        assert_eq!(field, "PreferredMethods.intern");
        assert_eq!(*expected, "string");
    }

    #[test]
    fn options_roundtrip() {
        let opts = StringMethodsOptions::default();
        let json = opts.to_config_json();
        let opts2 = StringMethodsOptions::from_config_json(json.as_bytes())
            .expect("roundtrip must succeed");
        assert_eq!(opts.preferred_methods, opts2.preferred_methods);
    }
}
murphy_plugin_api::submit_cop!(StringMethods);
