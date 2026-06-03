//! `Style/CollectionMethods` — enforce consistent collection method names.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/CollectionMethods
//! upstream_version_checked: 1.86.2
//! version_added: "0.0"
//! safe: false
//! supports_autocorrect: true
//! status: partial
//! gap_issues: []
//! notes: >
//!   Ports the core PreferredMethods mapping with autocorrect (selector rename).
//!   Safe: false — the cop detects by method name without verifying the receiver
//!   is Enumerable, so false positives are possible.
//!   Enabled: false by default (matches RuboCop's default.yml).
//!
//!   Implemented:
//!     - Block form: `items.collect { }` → `items.map { }`.
//!       Detected via on_block; flags the send_node's selector.
//!     - Block-pass form: `items.collect(&:foo)` → `items.map(&:foo)`.
//!       Detected via on_send/on_csend with implicit_block? guard.
//!     - Symbol argument form: `items.inject(:+)` → `items.reduce(:+)`.
//!       Only for methods listed in MethodsAcceptingSymbol (default: inject,
//!       reduce). Detected via on_send/on_csend with implicit_block? guard.
//!     - Both send and csend (safe-navigation `&.`) are handled.
//!     - Autocorrect: surgical rename of selector (loc.name only).
//!
//!   Gaps / v1 limitations:
//!     - numblock and itblock forms (`items.collect { _1 }`,
//!       `items.collect { it }`) are not handled — Murphy does not expose
//!       these node kinds. Users can disable this cop via
//!       `[cops.rules."Style/CollectionMethods"] enabled = false`.
//!     - The `MethodPreference#preferred_methods` reconciliation logic (which
//!       removes default entries that have been overridden in reverse) is not
//!       wired: custom `PreferredMethods` maps may surface a false-positive
//!       if a user creates a cycle (e.g., `find: detect` when the default has
//!       `detect: find`). This case is rare and the cop is disabled by
//!       default; document if it becomes a reported issue.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! items.collect { |x| x }
//! items.collect!
//! items.collect_concat { |x| [x] }
//! items.inject(:+)
//! items.detect { |x| x.even? }
//! items.find_all { |x| x.odd? }
//! items.member?(:foo)
//!
//! # good
//! items.map { |x| x }
//! items.map!
//! items.flat_map { |x| [x] }
//! items.reduce(:+)
//! items.find { |x| x.even? }
//! items.select { |x| x.odd? }
//! items.include?(:foo)
//! ```
//!
//! ## Autocorrect
//!
//! Surgical rename: `cx.emit_edit(cx.node(send_node).loc.name, preferred)`.
//! The receiver and arguments are untouched.

use std::collections::BTreeMap;

use murphy_plugin_api::{ConfigError, CopOptions, Cx, NodeId, NodeKind, cop};

// ---------------------------------------------------------------------------
// Default preferred methods map (matches RuboCop's config/default.yml)
// ---------------------------------------------------------------------------

fn default_preferred_methods() -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert("collect".to_string(), "map".to_string());
    m.insert("collect!".to_string(), "map!".to_string());
    m.insert("collect_concat".to_string(), "flat_map".to_string());
    m.insert("inject".to_string(), "reduce".to_string());
    m.insert("detect".to_string(), "find".to_string());
    m.insert("find_all".to_string(), "select".to_string());
    m.insert("member?".to_string(), "include?".to_string());
    m
}

fn default_methods_accepting_symbol() -> Vec<String> {
    vec!["inject".to_string(), "reduce".to_string()]
}

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

/// Options for [`CollectionMethods`].
///
/// `PreferredMethods` is a `String → String` map which requires a hand-rolled
/// impl because `#[derive(CopOptions)]` does not support nested maps.
#[derive(Clone, Debug)]
pub struct CollectionMethodsOptions {
    /// Maps undesired method name to preferred method name.
    pub preferred_methods: BTreeMap<String, String>,
    /// Methods that accept a final bare symbol as an implicit block
    /// (e.g. `inject(:+)` — not a Symbol#to_proc `&:+`).
    pub methods_accepting_symbol: Vec<String>,
}

impl Default for CollectionMethodsOptions {
    fn default() -> Self {
        Self {
            preferred_methods: default_preferred_methods(),
            methods_accepting_symbol: default_methods_accepting_symbol(),
        }
    }
}

impl CopOptions for CollectionMethodsOptions {
    fn from_config_json(bytes: &[u8]) -> Result<Self, ConfigError> {
        let value: serde_json::Value = serde_json::from_slice(bytes).map_err(ConfigError::parse)?;
        let obj = value.as_object().ok_or_else(ConfigError::not_an_object)?;

        // --- PreferredMethods ---
        let preferred_methods = if let Some(pm_value) = obj.get("PreferredMethods") {
            let pm_obj = pm_value
                .as_object()
                .ok_or_else(|| ConfigError::type_mismatch("PreferredMethods", "object"))?;
            let mut map = default_preferred_methods();
            for (key, val) in pm_obj {
                let s = val.as_str().ok_or_else(|| {
                    ConfigError::type_mismatch(format!("PreferredMethods.{key}"), "string")
                })?;
                map.insert(key.clone(), s.to_string());
            }
            map
        } else {
            default_preferred_methods()
        };

        // --- MethodsAcceptingSymbol ---
        let methods_accepting_symbol = if let Some(mas_value) = obj.get("MethodsAcceptingSymbol") {
            let arr = mas_value.as_array().ok_or_else(|| {
                ConfigError::type_mismatch("MethodsAcceptingSymbol", "string_list")
            })?;
            let mut vec = Vec::with_capacity(arr.len());
            for (i, item) in arr.iter().enumerate() {
                let s = item.as_str().ok_or_else(|| {
                    ConfigError::type_mismatch(format!("MethodsAcceptingSymbol[{i}]"), "string")
                })?;
                vec.push(s.to_string());
            }
            vec
        } else {
            default_methods_accepting_symbol()
        };

        Ok(Self {
            preferred_methods,
            methods_accepting_symbol,
        })
    }

    fn to_config_json(&self) -> String {
        let pm_pairs: serde_json::Map<String, serde_json::Value> = self
            .preferred_methods
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect();
        let mas_arr: Vec<serde_json::Value> = self
            .methods_accepting_symbol
            .iter()
            .map(|s| serde_json::Value::String(s.clone()))
            .collect();

        let mut top = serde_json::Map::new();
        top.insert(
            "PreferredMethods".to_string(),
            serde_json::Value::Object(pm_pairs),
        );
        top.insert(
            "MethodsAcceptingSymbol".to_string(),
            serde_json::Value::Array(mas_arr),
        );
        serde_json::Value::Object(top).to_string()
    }
}

// ---------------------------------------------------------------------------
// Cop
// ---------------------------------------------------------------------------

/// Stateless unit struct.
#[derive(Default)]
pub struct CollectionMethods;

const MSG: &str = "Prefer `%preferred%` over `%current%`.";

#[cop(
    name = "Style/CollectionMethods",
    description = "Enforces the use of consistent method names from the Enumerable module.",
    default_severity = "warning",
    default_enabled = false,
    options = CollectionMethodsOptions,
)]
impl CollectionMethods {
    /// Block form: `items.collect { }`, `items.inject(:+) { }`.
    /// We dispatch on `block` and check the inner send/csend call's selector.
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, .. } = *cx.kind(node) else {
            return;
        };
        check_method_node(call, cx);
    }

    /// Send form: fires only when `implicit_block?` holds.
    /// Covers `items.collect(&:foo)` and `items.inject(:+)`.
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        if !implicit_block(node, cx) {
            return;
        }
        check_method_node(node, cx);
    }

    /// Safe-navigation send: `items&.collect(&:foo)`.
    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        if !implicit_block(node, cx) {
            return;
        }
        check_method_node(node, cx);
    }
}

/// Returns `true` when the send node has an implicit block equivalent.
///
/// Mirrors RuboCop's `implicit_block?`:
///   - Node must have at least one argument.
///   - The last argument is a block-pass (`&:foo`), OR
///   - The last argument is a symbol AND the method is in `MethodsAcceptingSymbol`.
fn implicit_block(node: NodeId, cx: &Cx<'_>) -> bool {
    let args = cx.call_arguments(node);
    let Some(&last_arg) = args.last() else {
        return false; // no arguments → not implicit block
    };

    // Block-pass argument: `&:foo`
    if matches!(cx.kind(last_arg), NodeKind::BlockPass(_)) {
        return true;
    }

    // Bare symbol argument with method accepting symbol: `inject(:+)`
    if matches!(cx.kind(last_arg), NodeKind::Sym(_)) {
        let opts = cx.options_or_default::<CollectionMethodsOptions>();
        let method = cx.method_name(node).unwrap_or("");
        if opts.methods_accepting_symbol.iter().any(|m| m == method) {
            return true;
        }
    }

    false
}

/// Core check: if the method name is in the preferred map, emit offense + edit.
fn check_method_node(node: NodeId, cx: &Cx<'_>) {
    let current = match cx.method_name(node) {
        Some(m) => m,
        None => return,
    };

    let opts = cx.options_or_default::<CollectionMethodsOptions>();
    let Some(preferred) = opts.preferred_methods.get(current) else {
        return;
    };
    let preferred = preferred.clone();

    let message = MSG
        .replace("%preferred%", &preferred)
        .replace("%current%", current);

    let selector_range = cx.node(node).loc.name;
    cx.emit_offense(selector_range, &message, None);
    cx.emit_edit(selector_range, &preferred);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- Block form ---

    #[test]
    fn flags_collect_with_block() {
        test::<CollectionMethods>().expect_offense(indoc! {r#"
            items.collect { |x| x }
                  ^^^^^^^ Prefer `map` over `collect`.
        "#});
    }

    #[test]
    fn corrects_collect_with_block() {
        test::<CollectionMethods>().expect_correction(
            indoc! {r#"
                items.collect { |x| x }
                      ^^^^^^^ Prefer `map` over `collect`.
            "#},
            "items.map { |x| x }\n",
        );
    }

    #[test]
    fn flags_detect_with_block() {
        test::<CollectionMethods>().expect_offense(indoc! {r#"
            items.detect { |x| x.even? }
                  ^^^^^^ Prefer `find` over `detect`.
        "#});
    }

    #[test]
    fn corrects_detect_with_block() {
        test::<CollectionMethods>().expect_correction(
            indoc! {r#"
                items.detect { |x| x.even? }
                      ^^^^^^ Prefer `find` over `detect`.
            "#},
            "items.find { |x| x.even? }\n",
        );
    }

    #[test]
    fn flags_inject_with_block() {
        test::<CollectionMethods>().expect_offense(indoc! {r#"
            items.inject { |a, b| a + b }
                  ^^^^^^ Prefer `reduce` over `inject`.
        "#});
    }

    #[test]
    fn flags_find_all_with_block() {
        test::<CollectionMethods>().expect_offense(indoc! {r#"
            items.find_all { |x| x.odd? }
                  ^^^^^^^^ Prefer `select` over `find_all`.
        "#});
    }

    #[test]
    fn flags_collect_concat_with_block() {
        test::<CollectionMethods>().expect_offense(indoc! {r#"
            items.collect_concat { |x| [x] }
                  ^^^^^^^^^^^^^^ Prefer `flat_map` over `collect_concat`.
        "#});
    }

    #[test]
    fn corrects_collect_concat_with_block() {
        test::<CollectionMethods>().expect_correction(
            indoc! {r#"
                items.collect_concat { |x| [x] }
                      ^^^^^^^^^^^^^^ Prefer `flat_map` over `collect_concat`.
            "#},
            "items.flat_map { |x| [x] }\n",
        );
    }

    #[test]
    fn flags_member_with_block() {
        test::<CollectionMethods>().expect_offense(indoc! {r#"
            items.member? { |x| x == :foo }
                  ^^^^^^^ Prefer `include?` over `member?`.
        "#});
    }

    /// `member?(:foo)` is NOT flagged because `member?` is not in
    /// `MethodsAcceptingSymbol` and `:foo` is not a block-pass.
    #[test]
    fn no_offense_member_with_symbol_arg() {
        test::<CollectionMethods>().expect_no_offenses(
            "items.member?(:foo)
",
        );
    }

    // --- Bang method ---

    #[test]
    fn flags_collect_bang_with_block() {
        test::<CollectionMethods>().expect_offense(indoc! {r#"
            items.collect! { |x| x }
                  ^^^^^^^^ Prefer `map!` over `collect!`.
        "#});
    }

    #[test]
    fn corrects_collect_bang_with_block() {
        test::<CollectionMethods>().expect_correction(
            indoc! {r#"
                items.collect! { |x| x }
                      ^^^^^^^^ Prefer `map!` over `collect!`.
            "#},
            "items.map! { |x| x }\n",
        );
    }

    // --- Block-pass form (on_send) ---

    #[test]
    fn flags_collect_with_block_pass() {
        test::<CollectionMethods>().expect_offense(indoc! {r#"
            items.collect(&:to_s)
                  ^^^^^^^ Prefer `map` over `collect`.
        "#});
    }

    #[test]
    fn corrects_collect_with_block_pass() {
        test::<CollectionMethods>().expect_correction(
            indoc! {r#"
                items.collect(&:to_s)
                      ^^^^^^^ Prefer `map` over `collect`.
            "#},
            "items.map(&:to_s)\n",
        );
    }

    // --- Symbol argument form (MethodsAcceptingSymbol) ---

    #[test]
    fn flags_inject_with_symbol() {
        test::<CollectionMethods>().expect_offense(indoc! {r#"
            items.inject(:+)
                  ^^^^^^ Prefer `reduce` over `inject`.
        "#});
    }

    #[test]
    fn corrects_inject_with_symbol() {
        test::<CollectionMethods>().expect_correction(
            indoc! {r#"
                items.inject(:+)
                      ^^^^^^ Prefer `reduce` over `inject`.
            "#},
            "items.reduce(:+)\n",
        );
    }

    /// `detect(:foo)` is NOT in MethodsAcceptingSymbol — no offense.
    #[test]
    fn no_offense_detect_with_symbol() {
        test::<CollectionMethods>().expect_no_offenses("items.detect(:foo)\n");
    }

    // --- Bare call (no block, no block-pass, no symbol) → no offense ---

    #[test]
    fn no_offense_bare_collect() {
        test::<CollectionMethods>().expect_no_offenses("items.collect\n");
    }

    #[test]
    fn no_offense_bare_detect() {
        test::<CollectionMethods>().expect_no_offenses("items.detect\n");
    }

    #[test]
    fn no_offense_bare_inject() {
        test::<CollectionMethods>().expect_no_offenses("items.inject\n");
    }

    // --- Good methods — no offense ---

    #[test]
    fn no_offense_map_with_block() {
        test::<CollectionMethods>().expect_no_offenses("items.map { |x| x }\n");
    }

    #[test]
    fn no_offense_select_with_block() {
        test::<CollectionMethods>().expect_no_offenses("items.select { |x| x.odd? }\n");
    }

    #[test]
    fn no_offense_find_with_block() {
        test::<CollectionMethods>().expect_no_offenses("items.find { |x| x.even? }\n");
    }

    #[test]
    fn no_offense_flat_map_with_block() {
        test::<CollectionMethods>().expect_no_offenses("items.flat_map { |x| [x] }\n");
    }

    #[test]
    fn no_offense_reduce_with_symbol() {
        test::<CollectionMethods>().expect_no_offenses("items.reduce(:+)\n");
    }

    #[test]
    fn no_offense_include_predicate() {
        test::<CollectionMethods>().expect_no_offenses("items.include?(:foo)\n");
    }

    // --- csend ---

    #[test]
    fn flags_csend_collect_with_block_pass() {
        test::<CollectionMethods>().expect_offense(indoc! {r#"
            items&.collect(&:to_s)
                   ^^^^^^^ Prefer `map` over `collect`.
        "#});
    }

    #[test]
    fn corrects_csend_collect_with_block_pass() {
        test::<CollectionMethods>().expect_correction(
            indoc! {r#"
                items&.collect(&:to_s)
                       ^^^^^^^ Prefer `map` over `collect`.
            "#},
            "items&.map(&:to_s)\n",
        );
    }

    #[test]
    fn flags_csend_inject_with_symbol() {
        test::<CollectionMethods>().expect_offense(indoc! {r#"
            items&.inject(:+)
                   ^^^^^^ Prefer `reduce` over `inject`.
        "#});
    }

    // --- No double-fire: block form should not also fire via send ---

    #[test]
    fn no_double_fire_for_block_form() {
        // `items.collect { |x| x }` — exactly one offense (from on_block),
        // not two (block and the inner send). The send_node inside a block
        // has no arguments, so implicit_block? returns false.
        let offenses = murphy_plugin_api::test_support::run_cop::<CollectionMethods>(
            "items.collect { |x| x }\n",
        );
        assert_eq!(
            offenses.len(),
            1,
            "expected exactly 1 offense, got: {offenses:?}"
        );
    }

    // --- Custom PreferredMethods config ---

    #[test]
    fn custom_preferred_methods_find_to_detect() {
        // If user maps `find` -> `detect`, then `items.find { }` is bad.
        let mut pm = BTreeMap::new();
        pm.insert("find".to_string(), "detect".to_string());
        // Remove the default detect -> find mapping to avoid cycle.
        test::<CollectionMethods>()
            .with_options(&CollectionMethodsOptions {
                preferred_methods: pm,
                methods_accepting_symbol: default_methods_accepting_symbol(),
            })
            .expect_offense(indoc! {r#"
                items.find { |x| x }
                      ^^^^ Prefer `detect` over `find`.
            "#});
    }

    // --- from_config_json error contract ---

    #[test]
    fn config_not_an_object() {
        let err = <CollectionMethodsOptions as CopOptions>::from_config_json(b"[]")
            .expect_err("not an object is invalid");
        assert_eq!(*err.kind(), murphy_plugin_api::ConfigErrorKind::NotAnObject,);
    }

    #[test]
    fn config_preferred_methods_not_object() {
        let err = <CollectionMethodsOptions as CopOptions>::from_config_json(
            br#"{"PreferredMethods": "bad"}"#,
        )
        .expect_err("string is not a valid PreferredMethods");
        let murphy_plugin_api::ConfigErrorKind::TypeMismatch { field, expected } = err.kind()
        else {
            panic!("expected TypeMismatch, got {:?}", err.kind());
        };
        assert_eq!(field, "PreferredMethods");
        assert_eq!(*expected, "object");
    }

    #[test]
    fn config_preferred_methods_value_not_string() {
        let err = <CollectionMethodsOptions as CopOptions>::from_config_json(
            br#"{"PreferredMethods": {"collect": 42}}"#,
        )
        .expect_err("integer is not a valid value");
        let murphy_plugin_api::ConfigErrorKind::TypeMismatch { field, expected } = err.kind()
        else {
            panic!("expected TypeMismatch, got {:?}", err.kind());
        };
        assert_eq!(field, "PreferredMethods.collect");
        assert_eq!(*expected, "string");
    }

    #[test]
    fn config_methods_accepting_symbol_not_array() {
        let err = <CollectionMethodsOptions as CopOptions>::from_config_json(
            br#"{"MethodsAcceptingSymbol": "inject"}"#,
        )
        .expect_err("string is not a valid MethodsAcceptingSymbol");
        let murphy_plugin_api::ConfigErrorKind::TypeMismatch { field, expected } = err.kind()
        else {
            panic!("expected TypeMismatch, got {:?}", err.kind());
        };
        assert_eq!(field, "MethodsAcceptingSymbol");
        assert_eq!(*expected, "string_list");
    }

    #[test]
    fn config_methods_accepting_symbol_element_not_string() {
        let err = <CollectionMethodsOptions as CopOptions>::from_config_json(
            br#"{"MethodsAcceptingSymbol": [1, 2]}"#,
        )
        .expect_err("integer element is not a valid string");
        let murphy_plugin_api::ConfigErrorKind::TypeMismatch { field, expected } = err.kind()
        else {
            panic!("expected TypeMismatch, got {:?}", err.kind());
        };
        assert_eq!(field, "MethodsAcceptingSymbol[0]");
        assert_eq!(*expected, "string");
    }

    #[test]
    fn config_roundtrip() {
        let opts = CollectionMethodsOptions::default();
        let json = opts.to_config_json();
        let opts2 = <CollectionMethodsOptions as CopOptions>::from_config_json(json.as_bytes())
            .expect("roundtrip");
        assert_eq!(opts.preferred_methods, opts2.preferred_methods);
        assert_eq!(
            opts.methods_accepting_symbol,
            opts2.methods_accepting_symbol
        );
    }
}

murphy_plugin_api::submit_cop!(CollectionMethods);
