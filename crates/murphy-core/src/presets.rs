//! Official preset catalogue (C3; ADR 0050).
//!
//! Presets are builtin named config layers distributed with the binary.
//! They resolve the Phase 10 §4.3 open question on the side of "builtin,
//! not separate config packs": official presets ship in `murphy-core`
//! (no network, no registry, no gem), third-party presets travel via
//! `inherit_from:` file includes. A preset is a curated delta — user
//! config always wins per-field.
//!
//! Names (both `murphy:<name>` and bare `<name>` are accepted in
//! `extends:` / `--preset`):
//!
//! - `minimal` — Lint + Security + Layout focus; disables Metrics,
//!   Naming, Bundler, Gemspec and documentation cops for large legacy
//!   repos / CI fast path.
//! - `recommended` — Murphy defaults (no overrides; documents intent).
//! - `shopify` — Shopify-style opinions (single quotes, ruby19 hash,
//!   documentation cops off, relaxed Metrics).
//! - `rails-strict` — strict Rails opinions; enables a curated set of
//!   `Rails/*` cops and turns on `ActiveSupportExtensionsEnabled`.
//!   Requires the `murphy-rails` pack for the `Rails/*` cops to run
//!   (`murphy add murphy-rails`); without the pack the `Enabled: true`
//!   entries are inert but harmless.
//!
//! The native plugin ABI is untouched.

/// Canonical preset names (without the `murphy:` prefix).
pub const PRESET_NAMES: &[&str] = &["minimal", "recommended", "shopify", "rails-strict"];

/// Prefix for fully-qualified preset refs (`murphy:minimal`, ...).
pub const PRESET_PREFIX: &str = "murphy:";

/// Normalize a user-supplied preset ref to its canonical bare name.
///
/// Accepts `murphy:<name>` and bare `<name>` (surrounding whitespace
/// trimmed). Returns `None` for unknown names.
pub fn normalize_preset_ref(raw: &str) -> Option<&'static str> {
    let t = raw.trim();
    let bare = t.strip_prefix(PRESET_PREFIX).unwrap_or(t);
    match bare {
        "minimal" => Some("minimal"),
        "recommended" => Some("recommended"),
        "shopify" => Some("shopify"),
        "rails-strict" => Some("rails-strict"),
        _ => None,
    }
}

/// True when `raw` names a known preset (prefixed or bare).
pub fn is_preset_ref(raw: &str) -> bool {
    normalize_preset_ref(raw).is_some()
}

/// The builtin YAML delta for a canonical preset name.
pub fn preset_yaml(name: &str) -> Option<&'static str> {
    match name {
        "minimal" => Some(PRESET_MINIMAL_YAML),
        "recommended" => Some(PRESET_RECOMMENDED_YAML),
        "shopify" => Some(PRESET_SHOPIFY_YAML),
        "rails-strict" => Some(PRESET_RAILS_STRICT_YAML),
        _ => None,
    }
}

/// Short human description for `--help` / docs / errors.
pub fn preset_description(name: &str) -> Option<&'static str> {
    match name {
        "minimal" => Some("Lint + Security focus; mutes Metrics/Naming/packaging/docs cops"),
        "recommended" => Some("Murphy defaults (no overrides)"),
        "shopify" => Some("Shopify-style opinions (single quotes, relaxed Metrics)"),
        "rails-strict" => Some("Strict Rails opinions (enables curated Rails/* cops)"),
        _ => None,
    }
}

/// `minimal`: Lint + Security + Layout focus.
///
/// Disables the Metrics suite (10), the Naming suite (19), packaging
/// suites (Bundler 7 + Gemspec 10) and documentation cops (3). Layout
/// stays on (formatting still matters); Lint and Security stay on.
/// User `Enabled: true` wins per-cop.
const PRESET_MINIMAL_YAML: &str = r#"
Metrics/AbcSize:
  Enabled: false
Metrics/BlockLength:
  Enabled: false
Metrics/BlockNesting:
  Enabled: false
Metrics/ClassLength:
  Enabled: false
Metrics/CollectionLiteralLength:
  Enabled: false
Metrics/CyclomaticComplexity:
  Enabled: false
Metrics/MethodLength:
  Enabled: false
Metrics/ModuleLength:
  Enabled: false
Metrics/ParameterLists:
  Enabled: false
Metrics/PerceivedComplexity:
  Enabled: false
Naming/AccessorMethodName:
  Enabled: false
Naming/AsciiIdentifiers:
  Enabled: false
Naming/BinaryOperatorParameterName:
  Enabled: false
Naming/BlockForwarding:
  Enabled: false
Naming/BlockParameterName:
  Enabled: false
Naming/ClassAndModuleCamelCase:
  Enabled: false
Naming/ConstantName:
  Enabled: false
Naming/FileName:
  Enabled: false
Naming/HeredocDelimiterCase:
  Enabled: false
Naming/HeredocDelimiterNaming:
  Enabled: false
Naming/InclusiveLanguage:
  Enabled: false
Naming/MemoizedInstanceVariableName:
  Enabled: false
Naming/MethodName:
  Enabled: false
Naming/MethodParameterName:
  Enabled: false
Naming/PredicateMethod:
  Enabled: false
Naming/PredicatePrefix:
  Enabled: false
Naming/RescuedExceptionsVariableName:
  Enabled: false
Naming/VariableName:
  Enabled: false
Naming/VariableNumber:
  Enabled: false
Bundler/DuplicatedGem:
  Enabled: false
Bundler/DuplicatedGroup:
  Enabled: false
Bundler/GemComment:
  Enabled: false
Bundler/GemFilename:
  Enabled: false
Bundler/GemVersion:
  Enabled: false
Bundler/InsecureProtocolSource:
  Enabled: false
Bundler/OrderedGems:
  Enabled: false
Gemspec/AddRuntimeDependency:
  Enabled: false
Gemspec/AttributeAssignment:
  Enabled: false
Gemspec/DependencyVersion:
  Enabled: false
Gemspec/DeprecatedAttributeAssignment:
  Enabled: false
Gemspec/DevelopmentDependencies:
  Enabled: false
Gemspec/DuplicatedAssignment:
  Enabled: false
Gemspec/OrderedDependencies:
  Enabled: false
Gemspec/RequiredRubyVersion:
  Enabled: false
Gemspec/RequireMFA:
  Enabled: false
Gemspec/RubyVersionGlobalsUsage:
  Enabled: false
Style/Documentation:
  Enabled: false
Style/DocumentationMethod:
  Enabled: false
Style/FrozenStringLiteralComment:
  Enabled: false
"#;

/// `recommended`: Murphy defaults. Empty delta — documents intent and
/// gives `extends: murphy:recommended` a stable meaning.
const PRESET_RECOMMENDED_YAML: &str = "# recommended preset: Murphy defaults (no overrides).\n";

/// `shopify`: Shopify-style opinions.
///
/// Single quotes, ruby19 hash syntax, documentation cops off, relaxed
/// method/block length. Verified against `murphy-std/config/default.yml`
/// keys (`EnforcedStyle`, `Max`).
const PRESET_SHOPIFY_YAML: &str = r#"
Style/StringLiterals:
  EnforcedStyle: single_quotes
Style/StringLiteralsInInterpolation:
  EnforcedStyle: single_quotes
Style/HashSyntax:
  EnforcedStyle: ruby19
Layout/LineLength:
  Max: 120
Style/Documentation:
  Enabled: false
Style/FrozenStringLiteralComment:
  Enabled: false
Metrics/MethodLength:
  Max: 20
Metrics/BlockLength:
  Max: 30
"#;

/// `rails-strict`: strict Rails opinions.
///
/// Enables a curated set of `Rails/*` cops (all present in
/// `murphy-rails` 138-cop catalogue) and turns on
/// `ActiveSupportExtensionsEnabled` so Rails-aware cops see the
/// ActiveSupport core-ext surface. Needs `murphy-rails` installed
/// (`murphy add murphy-rails`); without the pack the entries are inert.
const PRESET_RAILS_STRICT_YAML: &str = r#"
AllCops:
  ActiveSupportExtensionsEnabled: true
Rails/Blank:
  Enabled: true
Rails/FindBy:
  Enabled: true
Rails/FindEach:
  Enabled: true
Rails/SaveBang:
  Enabled: true
Rails/WhereNot:
  Enabled: true
Rails/CreateTableWithTimestamps:
  Enabled: true
Rails/DefaultScope:
  Enabled: true
Rails/HasManyOrHasOneDependent:
  Enabled: true
Rails/InverseOf:
  Enabled: true
Rails/TimeZone:
  Enabled: true
Rails/Validation:
  Enabled: true
Rails/SkipsModelValidations:
  Enabled: true
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_has_four_presets() {
        assert_eq!(PRESET_NAMES.len(), 4);
        for name in PRESET_NAMES {
            assert!(preset_yaml(name).is_some(), "missing yaml for {name}");
            assert!(
                preset_description(name).is_some(),
                "missing desc for {name}"
            );
        }
    }

    #[test]
    fn normalize_accepts_prefixed_and_bare() {
        assert_eq!(normalize_preset_ref("murphy:minimal"), Some("minimal"));
        assert_eq!(normalize_preset_ref("minimal"), Some("minimal"));
        assert_eq!(
            normalize_preset_ref("  murphy:rails-strict  "),
            Some("rails-strict")
        );
        assert_eq!(normalize_preset_ref("murphy:unknown"), None);
        assert_eq!(normalize_preset_ref(""), None);
    }
}
