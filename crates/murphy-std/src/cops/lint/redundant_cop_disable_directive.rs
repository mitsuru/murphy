//! `Lint/RedundantCopDisableDirective` — detect unnecessary `# rubocop:disable`
//! comments.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/RedundantCopDisableDirective
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues:
//!   - murphy-lfpb
//! notes: >
//!   This cop depends on the results of all other cops (`offenses_to_check`
//!   in RuboCop) and on comment directive tracking (`cop_disabled_line_ranges`).
//!   Murphy's plugin API (v1) does not expose cross-cop offense data or
//!   directive-aware comment infrastructure, so this cop cannot detect
//!   redundant disables.
//! ```
//!
//! ## Known v1 limitation: no cross-cop offense data or directive tracking
//!
//! RuboCop's `Lint/RedundantCopDisableDirective` is a meta-cop that runs after
//! all other cops and inspects their offenses to determine whether each
//! `# rubocop:disable` comment is redundant (i.e., no offenses of the disabled
//! cop exist in the disabled range). It also requires `cop_disabled_line_ranges`
//! from `CommentConfig` to know which line ranges are disabled.
//!
//! Murphy's `Cx` only exposes raw comments via `cx.comments()` — there is no
//! mechanism to access offenses from other cops or to introspect the host's
//! disable/enable directive state. Implementing this cop requires extending the
//! plugin ABI with cross-cop reporting and directive tracking.
//!
//! ## Defaults that mirror RuboCop
//!
//! (none — cop emits no offenses in v1)

use murphy_plugin_api::{Cx, NoOptions, cop};

#[derive(Default)]
pub struct RedundantCopDisableDirective;

#[cop(
    name = "Lint/RedundantCopDisableDirective",
    description = "Detect unnecessary rubocop:disable comments.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl RedundantCopDisableDirective {
    #[on_new_investigation]
    fn noop(&self, _cx: &Cx<'_>) {}
}

murphy_plugin_api::submit_cop!(RedundantCopDisableDirective);

#[cfg(test)]
mod tests {
    use super::RedundantCopDisableDirective;
    use murphy_plugin_api::test_support::test;

    #[test]
    fn accepts_any_source() {
        test::<RedundantCopDisableDirective>()
            .expect_no_offenses("# rubocop:disable Metrics/MethodLength\nx += 1\n# rubocop:enable Metrics/MethodLength\n");
    }
}
