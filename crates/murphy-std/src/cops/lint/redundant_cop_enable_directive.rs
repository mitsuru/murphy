//! `Lint/RedundantCopEnableDirective` — detect unnecessary `# rubocop:enable`
//! comments.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/RedundantCopEnableDirective
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues:
//!   - murphy-k19j
//! notes: >
//!   This cop requires a comment directive tracking system
//!   (`comment_config.extra_enabled_comments` in RuboCop) that Murphy's
//!   plugin API does not yet expose. The cop cannot be implemented until
//!   the host detects `# rubocop:enable` directives and reports which ones
//!   are redundant.
//! ```
//!
//! ## Known v1 limitation: no comment directive tracking
//!
//! RuboCop's `Lint/RedundantCopEnableDirective` works by comparing
//! `# rubocop:disable` / `# rubocop:enable` pairs via its
//! `CommentConfig` class. Murphy's `Cx` only exposes raw comments via
//! `comments_in_range` — there is no notion of enable/disable directive
//! semantics. Implementing this cop requires adding directive-aware
//! comment infrastructure to the host.

use murphy_plugin_api::{Cx, NoOptions, cop};

#[derive(Default)]
pub struct RedundantCopEnableDirective;

#[cop(
    name = "Lint/RedundantCopEnableDirective",
    description = "Detect unnecessary rubocop:enable comments.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl RedundantCopEnableDirective {
    #[on_new_investigation]
    fn noop(&self, _cx: &Cx<'_>) {}
}

murphy_plugin_api::submit_cop!(RedundantCopEnableDirective);
