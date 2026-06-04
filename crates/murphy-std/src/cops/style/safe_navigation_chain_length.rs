//! `Style/SafeNavigationChainLength` — limit safe navigation operator chain length.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/SafeNavigationChainLength
//! upstream_version_checked: 1.81.6
//! version_added: "1.68"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Flags safe navigation chains (`&.`) that exceed the configured maximum
//!   length. The offense is reported on the outermost csend ancestor, matching
//!   RuboCop's `add_offense(safe_navigation_chains.last, ...)` behavior.
//!   No autocorrect: RuboCop does not autocorrect this cop either.
//!   Default `Max` is 2, matching RuboCop's default.
//!   Cop is `Enabled: pending` in RuboCop; Murphy ships it enabled by default
//!   because the cop enforces a common readability guideline (limiting chain
//!   length). Users can disable via `.murphy.yml` if needed.
//!   Max must be a positive integer; 0 or negative values are a configuration
//!   error (matching RuboCop's validation behavior).
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad (Max: 2)
//! user&.address&.zip&.upcase
//!
//! # good
//! user&.address&.zip
//! user.address.zip if user
//! ```
//!
//! ## Algorithm
//!
//! `on_csend` fires for every safe navigation send. Starting from the current
//! csend node, we walk up the ancestor chain via `cx.parent()` collecting
//! consecutive csend ancestors. If the count of csend ancestors is >= Max, an
//! offense is emitted on the outermost (last) collected ancestor node.
//!
//! ## No autocorrect
//!
//! RuboCop does not provide autocorrect for this cop; Murphy mirrors that stance.

use murphy_plugin_api::{ConfigError, CopOptions, Cx, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct SafeNavigationChainLength;

/// Options for `Style/SafeNavigationChainLength`.
#[derive(Debug)]
pub struct SafeNavigationChainLengthOptions {
    pub max: i64,
}

impl Default for SafeNavigationChainLengthOptions {
    fn default() -> Self {
        Self { max: 2 }
    }
}

impl CopOptions for SafeNavigationChainLengthOptions {
    fn from_config_json(bytes: &[u8]) -> Result<Self, ConfigError> {
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(ConfigError::parse)?;
        let obj = value.as_object().ok_or_else(ConfigError::not_an_object)?;

        let Some(max_value) = obj.get("Max") else {
            return Ok(Self::default());
        };

        let max = max_value
            .as_i64()
            .ok_or_else(|| ConfigError::type_mismatch("Max", "integer"))?;

        if max <= 0 {
            return Err(ConfigError::type_mismatch("Max", "positive integer"));
        }

        Ok(Self { max })
    }

    fn to_config_json(&self) -> String {
        format!("{{\"Max\": {}}}", self.max)
    }
}

#[cop(
    name = "Style/SafeNavigationChainLength",
    description = "Enforces safe navigation chains length to not exceed the configured maximum.",
    default_severity = "warning",
    default_enabled = true,
    options = SafeNavigationChainLengthOptions,
)]
impl SafeNavigationChainLength {
    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<SafeNavigationChainLengthOptions>();
        let max = opts.max as usize;

        // Walk up the ancestor chain collecting consecutive csend ancestors.
        // Each csend fires this handler and re-walks from itself upward, giving
        // O(n²) work across a chain of n csends. In practice chains are short
        // (2-4 calls), so the difference from RuboCop's single-pass collection
        // is not a practical concern.
        let mut csend_ancestors: Vec<NodeId> = Vec::new();
        let mut current = node;
        while let Some(parent) = cx.parent(current).get() {
            if !matches!(cx.kind(parent), NodeKind::Csend { .. }) {
                break;
            }
            csend_ancestors.push(parent);
            current = parent;
        }

        // Offense fires when the ancestor csend count equals or exceeds Max.
        // Equivalent: total_chain_length (ancestors + 1) > Max, i.e. ancestors >= Max.
        // Mirrors RuboCop: `return if safe_navigation_chains.size < max`.
        if csend_ancestors.len() < max {
            return;
        }

        // Report at the outermost csend ancestor (matching RuboCop's .last).
        let outermost = *csend_ancestors.last().expect("csend_ancestors is non-empty because len >= max >= 1");
        let msg = format!("Avoid safe navigation chains longer than {max} calls.");
        cx.emit_offense(cx.range(outermost), &msg, None);
    }
}

#[cfg(test)]
mod tests {
    use super::{SafeNavigationChainLength, SafeNavigationChainLengthOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    // Default Max: 2

    #[test]
    fn flags_chain_exceeding_default_max() {
        // x&.foo&.bar&.baz — 3 csends, max=2 → offense at outermost
        test::<SafeNavigationChainLength>().expect_offense(indoc! {"
            x&.foo&.bar&.baz
            ^^^^^^^^^^^^^^^^ Avoid safe navigation chains longer than 2 calls.
        "});
    }

    #[test]
    fn flags_chain_with_regular_send_at_start() {
        // x.foo&.bar&.baz&.zoo — 3 csends after regular send, max=2
        test::<SafeNavigationChainLength>().expect_offense(indoc! {"
            x.foo&.bar&.baz&.zoo
            ^^^^^^^^^^^^^^^^^^^^ Avoid safe navigation chains longer than 2 calls.
        "});
    }

    #[test]
    fn flags_chain_in_middle_of_call_chain() {
        // Regular send (.nil?) at end; offense range covers only the csend portion.
        test::<SafeNavigationChainLength>().expect_offense(indoc! {"
            x.foo&.bar&.baz&.zoo.nil?
            ^^^^^^^^^^^^^^^^^^^^ Avoid safe navigation chains longer than 2 calls.
        "});
    }

    #[test]
    fn no_offense_chain_at_max() {
        // x&.foo&.bar — exactly 2 csends, max=2 → no offense
        test::<SafeNavigationChainLength>().expect_no_offenses("x&.foo&.bar\n");
    }

    #[test]
    fn no_offense_regular_method_calls() {
        test::<SafeNavigationChainLength>().expect_no_offenses("x.foo.bar\n");
    }

    // Max: 1

    #[test]
    fn flags_two_csends_when_max_one() {
        test::<SafeNavigationChainLength>()
            .with_options(&SafeNavigationChainLengthOptions { max: 1 })
            .expect_offense(indoc! {"
                x&.foo&.bar
                ^^^^^^^^^^^ Avoid safe navigation chains longer than 1 calls.
            "});
    }

    // Max: 3

    #[test]
    fn flags_four_csends_when_max_three() {
        test::<SafeNavigationChainLength>()
            .with_options(&SafeNavigationChainLengthOptions { max: 3 })
            .expect_offense(indoc! {"
                x&.foo&.bar&.baz&.zoo
                ^^^^^^^^^^^^^^^^^^^^^ Avoid safe navigation chains longer than 3 calls.
            "});
    }

    #[test]
    fn no_offense_chain_at_max_three() {
        test::<SafeNavigationChainLength>()
            .with_options(&SafeNavigationChainLengthOptions { max: 3 })
            .expect_no_offenses("x&.foo&.bar&.baz\n");
    }

    // Edge case: Max: 0 or negative values are a config error.

    #[test]
    fn max_zero_is_a_config_error() {
        use murphy_plugin_api::CopOptions;
        let err = SafeNavigationChainLengthOptions::from_config_json(br#"{"Max": 0}"#)
            .expect_err("Max: 0 should be a config error");
        assert!(
            err.to_string().contains("positive integer"),
            "expected 'positive integer' in error, got: {err}"
        );
    }

    #[test]
    fn max_negative_is_a_config_error() {
        use murphy_plugin_api::CopOptions;
        let err = SafeNavigationChainLengthOptions::from_config_json(br#"{"Max": -1}"#)
            .expect_err("Max: -1 should be a config error");
        assert!(
            err.to_string().contains("positive integer"),
            "expected 'positive integer' in error, got: {err}"
        );
    }

    #[test]
    fn no_offense_single_csend_with_max_one() {
        // Single csend with max=1: 0 ancestors < 1 → no offense.
        test::<SafeNavigationChainLength>()
            .with_options(&SafeNavigationChainLengthOptions { max: 1 })
            .expect_no_offenses("x&.foo\n");
    }

    #[test]
    fn default_options_max_is_two() {
        let opts = SafeNavigationChainLengthOptions::default();
        assert_eq!(opts.max, 2);
    }

    #[test]
    fn options_from_config_json() {
        use murphy_plugin_api::CopOptions;
        let opts = SafeNavigationChainLengthOptions::from_config_json(br#"{"Max": 3}"#)
            .expect("valid config");
        assert_eq!(opts.max, 3);
    }
}

murphy_plugin_api::submit_cop!(SafeNavigationChainLength);
