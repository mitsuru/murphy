//! `Style/NumberedParametersLimit` — avoid excessive numbered params in a
//! single block.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/NumberedParametersLimit
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Murphy's `Numblock { max_n, .. }` field already records the highest
//!   numbered parameter used in the block, so there is no need to walk
//!   descendant lvar nodes as RuboCop does.  The offense is emitted on
//!   the full numblock range, matching RuboCop's `add_offense(node)`.
//!   No autocorrect exists for this cop in RuboCop either.
//!   Default `Max` is 1 (RuboCop `DEFAULT_MAX_VALUE = 1`).
//!   Ruby caps numbered params at 9; values > 9 are clamped to 9.
//! ```
//!
//! ## Detection
//!
//! Subscribes to `NodeKind::Numblock`. When `max_n` exceeds the configured
//! `Max`, an offense is emitted on the whole numblock range.
//!
//! ## Configuration
//!
//! `Max` (integer, default 1) — maximum number of numbered parameters allowed.
//! Values above 9 are treated as 9 (Ruby hard limit).
//!
//! ## No autocorrect
//!
//! RuboCop's autocorrect suggests editing the config file; Murphy does not
//! implement config-file autocorrect.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct NumberedParametersLimit;

#[derive(CopOptions)]
pub struct NumberedParametersLimitOptions {
    #[option(
        name = "Max",
        default = 1,
        description = "Maximum number of numbered parameters allowed in a block."
    )]
    pub max: i64,
}

#[cop(
    name = "Style/NumberedParametersLimit",
    description = "Avoid excessive numbered params in a single block.",
    default_severity = "warning",
    default_enabled = true,
    options = NumberedParametersLimitOptions,
)]
impl NumberedParametersLimit {
    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<NumberedParametersLimitOptions>();
        // Ruby allows at most 9 numbered params; cap the configured max there.
        let max_count = opts.max.clamp(1, 9) as u8;

        let max_n = match *cx.kind(node) {
            NodeKind::Numblock { max_n, .. } => max_n,
            _ => return,
        };

        if max_n <= max_count {
            return;
        }

        let parameter = if max_count == 1 { "parameter" } else { "parameters" };
        let msg = format!(
            "Avoid using more than {max_count} numbered {parameter}; {max_n} detected."
        );
        cx.emit_offense(cx.range(node), &msg, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn accepts_single_numbered_param_default_max() {
        test::<NumberedParametersLimit>().expect_no_offenses("[1, 2].map { _1 * 2 }\n");
    }

    #[test]
    fn accepts_two_params_when_max_is_two() {
        test::<NumberedParametersLimit>()
            .with_options(&NumberedParametersLimitOptions { max: 2 })
            .expect_no_offenses("[1, 2].map { _1 + _2 }\n");
    }

    #[test]
    fn flags_two_numbered_params_with_default_max_one() {
        test::<NumberedParametersLimit>().expect_offense(indoc! {"
            [1, 2].map { _1 + _2 }
            ^^^^^^^^^^^^^^^^^^^^^^ Avoid using more than 1 numbered parameter; 2 detected.
        "});
    }

    #[test]
    fn flags_three_numbered_params_with_max_two() {
        test::<NumberedParametersLimit>()
            .with_options(&NumberedParametersLimitOptions { max: 2 })
            .expect_offense(indoc! {"
                [1, 2, 3].map { _1 + _2 + _3 }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid using more than 2 numbered parameters; 3 detected.
            "});
    }

    #[test]
    fn max_above_nine_is_clamped_to_nine() {
        // Max > 9 should not cause issues — Ruby itself caps at 9.
        test::<NumberedParametersLimit>()
            .with_options(&NumberedParametersLimitOptions { max: 100 })
            .expect_no_offenses("[1, 2].map { _1 + _2 }\n");
    }

    #[test]
    fn max_from_config_json() {
        use murphy_plugin_api::CopOptions;
        let opts = NumberedParametersLimitOptions::from_config_json(br#"{"Max": 2}"#)
            .expect("valid config");
        assert_eq!(opts.max, 2);
    }

    #[test]
    fn default_max_is_one() {
        let opts = NumberedParametersLimitOptions::default();
        assert_eq!(opts.max, 1);
    }
}

murphy_plugin_api::submit_cop!(NumberedParametersLimit);
