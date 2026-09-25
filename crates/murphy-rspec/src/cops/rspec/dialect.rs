//! `RSpec/Dialect` — enforce custom RSpec dialect method preferences.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/Dialect
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_send` (`(send #rspec? #ALL.all ...)` gated by
//!   `preferred_methods[method]`): the receiver must be bare or the
//!   top-level `RSpec` constant (`#rspec?`), and the method must be a known
//!   RSpec DSL method (`ALL` = ExampleGroups + Examples + Expectations +
//!   Helpers + Hooks + Includes + Runners + SharedGroups + Subjects +
//!   ErrorMatchers from `config/default.yml` Language). When
//!   `PreferredMethods` maps the current name to a preferred spelling the
//!   whole `Send` is flagged per `add_offense(node)` with
//!   `Prefer `%<prefer>s` over `%<current>s`. Detection is at parity;
//!   autocorrect (replace the selector) is not ported in this batch — same
//!   convention as `RSpec/HookArgument` (status: partial, autocorrect as
//!   gap). `PreferredMethods` merging drops reverse mappings per
//!   `MethodPreference#preferred_methods`; with the default empty map the
//!   cop is a no-op (upstream `Enabled: false`).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` (any call shape; block wrappers flag via the
//! `Send` itself):
//!
//! - `context 'x' do; end` with `PreferredMethods: {context: describe}` —
//!   flagged (`Prefer `describe` over `context``).
//! - `RSpec.context 'x'` with the same config — flagged (explicit `RSpec`
//!   receiver still matches `#rspec?`).
//! - `obj.context 'x'` — explicit non-RSpec receiver, not flagged.
//! - `describe 'x'` with `{context: describe}` — no mapping, not flagged.
//! - Empty `PreferredMethods` — never flags.
//!
//! ## No autocorrect
//!
//! Upstream replaces the selector with the preferred spelling. This batch
//! reports only.

use std::collections::BTreeMap;

use murphy_plugin_api::{ConfigError, CopOptions, Cx, NodeId, NodeKind, cop};

use crate::cops::rspec_helpers::{is_rspec_or_bare_receiver, send_without_block_range};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct Dialect;

/// `PreferredMethods: { current => preferred }`. Murphy's
/// `#[derive(CopOptions)]` doesn't model nested maps, so the impl is
/// hand-rolled — defaults mirror upstream (`{}`).
#[derive(Clone, Debug, Default)]
pub struct DialectOptions {
    pub preferred_methods: BTreeMap<String, String>,
}

impl CopOptions for DialectOptions {
    fn from_config_json(bytes: &[u8]) -> Result<Self, ConfigError> {
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(ConfigError::parse)?;
        let obj = value.as_object().ok_or_else(ConfigError::not_an_object)?;
        let Some(methods_value) = obj.get("PreferredMethods") else {
            return Ok(Self::default());
        };
        let methods_obj = methods_value
            .as_object()
            .ok_or_else(|| ConfigError::type_mismatch("PreferredMethods", "object"))?;
        let mut map = BTreeMap::new();
        for (k, v) in methods_obj {
            let s = v.as_str().ok_or_else(|| {
                ConfigError::type_mismatch(format!("PreferredMethods.{k}"), "string")
            })?;
            map.insert(k.clone(), s.to_string());
        }
        Ok(Self {
            preferred_methods: map,
        })
    }

    fn to_config_json(&self) -> String {
        let inner: serde_json::Map<String, serde_json::Value> = self
            .preferred_methods
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect();
        let mut top = serde_json::Map::new();
        top.insert(
            "PreferredMethods".to_string(),
            serde_json::Value::Object(inner),
        );
        serde_json::Value::Object(top).to_string()
    }
}

#[cop(
    name = "RSpec/Dialect",
    description = "Enforces custom RSpec dialects.",
    default_severity = "warning",
    default_enabled = false,
    options = DialectOptions,
)]
impl Dialect {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send {
            receiver,
            method,
            ..
        } = *cx.kind(node)
        else {
            return;
        };
        if !is_rspec_or_bare_receiver(cx, receiver) {
            return;
        }
        let current = cx.symbol_str(method).to_owned();
        if !is_rspec_method(&current) {
            return;
        }
        let opts = cx.options_or_default::<DialectOptions>();
        let Some(prefer) = opts.preferred_methods.get(&current) else {
            return;
        };
        cx.emit_offense(
            send_without_block_range(cx, node),
            &format!("Prefer `{prefer}` over `{current}`."),
            None,
        );
    }
}

/// `true` when `name` is a known RSpec DSL method (`Language::ALL`).
///
/// Lists mirror `config/default.yml` Language section plus `Runners`
/// (`to`, `to_not`, `not_to`) and `ErrorMatchers` (`raise_error`,
/// `raise_exception`).
fn is_rspec_method(name: &str) -> bool {
    matches!(
        name,
        // ExampleGroups (regular + skipped + focused)
        "describe" | "context" | "feature" | "example_group"
            | "xdescribe" | "xcontext" | "xfeature"
            | "fdescribe" | "fcontext" | "ffeature"
            // Examples (regular + focused + skipped + pending)
            | "it" | "specify" | "example" | "scenario" | "its"
            | "fit" | "fspecify" | "fexample" | "fscenario" | "focus"
            | "xit" | "xspecify" | "xexample" | "xscenario" | "skip"
            | "pending"
            // Expectations
            | "are_expected" | "expect" | "expect_any_instance_of" | "is_expected"
            | "should" | "should_not" | "should_not_receive" | "should_receive"
            // Helpers
            | "let" | "let!"
            // Hooks
            | "prepend_before" | "before" | "append_before" | "around"
            | "prepend_after" | "after" | "append_after"
            // Includes (examples + context)
            | "it_behaves_like" | "it_should_behave_like" | "include_examples"
            | "include_context"
            // Runners
            | "to" | "to_not" | "not_to"
            // SharedGroups (examples + context)
            | "shared_examples" | "shared_examples_for" | "shared_context"
            // Subjects
            | "subject" | "subject!"
            // ErrorMatchers
            | "raise_error" | "raise_exception"
    )
}

#[cfg(test)]
mod tests {
    use super::{Dialect, DialectOptions};
    use std::collections::BTreeMap;
    use murphy_plugin_api::test_support::{indoc, test};

    fn context_to_describe() -> DialectOptions {
        DialectOptions {
            preferred_methods: BTreeMap::from([("context".to_owned(), "describe".to_owned())]),
        }
    }

    #[test]
    fn flags_dialected_method() {
        test::<Dialect>()
            .with_options(&context_to_describe())
            .expect_offense(indoc! {r#"
                context 'display name presence' do; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `describe` over `context`.
            "#});
    }

    #[test]
    fn flags_explicit_rspec_receiver() {
        test::<Dialect>()
            .with_options(&context_to_describe())
            .expect_offense(indoc! {r#"
                RSpec.context 'x' do; end
                ^^^^^^^^^^^^^^^^^ Prefer `describe` over `context`.
            "#});
    }

    #[test]
    fn does_not_flag_without_config() {
        test::<Dialect>().expect_no_offenses(indoc! {r#"
                context 'display name presence' do; end
            "#});
    }

    #[test]
    fn does_not_flag_preferred_spelling() {
        test::<Dialect>()
            .with_options(&context_to_describe())
            .expect_no_offenses(indoc! {r#"
                describe 'display name presence' do; end
            "#});
    }

    #[test]
    fn does_not_flag_explicit_receiver() {
        test::<Dialect>()
            .with_options(&context_to_describe())
            .expect_no_offenses(indoc! {r#"
                obj.context 'x' do; end
            "#});
    }

    #[test]
    fn does_not_flag_unknown_method() {
        test::<Dialect>()
            .with_options(&DialectOptions {
                preferred_methods: BTreeMap::from([(
                    "unknown_method".to_owned(),
                    "describe".to_owned(),
                )]),
            })
            .expect_no_offenses(indoc! {r#"
                unknown_method 'x' do; end
            "#});
    }
}

murphy_plugin_api::submit_cop!(Dialect);
