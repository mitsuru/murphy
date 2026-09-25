//! `RSpec/IncludeExamples` — prefer `it_behaves_like` over `include_examples`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/IncludeExamples
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_send` (`RESTRICT_ON_SEND = [:include_examples]`,
//!   no receiver check, so explicit-receiver forms flag too). The offense
//!   range is the selector per `add_offense(selector)` with
//!   `Prefer `it_behaves_like` over `include_examples`.` Detection is at
//!   parity; autocorrect (replace the selector with `it_behaves_like`) is
//!   not ported in this batch — same convention as `RSpec/ItBehavesLike`
//!   (status: partial, autocorrect as gap). Upstream `Enabled: pending`
//!   maps to `default_enabled = false`, mirroring `RSpec/Dialect`.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with `methods = ["include_examples"]` (any
//! receiver):
//!
//! - `include_examples 'a foo'` — flagged (the selector).
//! - `obj.include_examples 'a foo'` — explicit receiver, still flagged.
//! - `it_behaves_like 'a foo'` — different selector, not flagged.
//!
//! ## No autocorrect
//!
//! Upstream replaces the selector with `it_behaves_like`. This batch
//! reports only (the scoping difference makes auto-rewrite unsafe —
//! upstream marks it `SafeAutoCorrect: false`).

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct IncludeExamples;

#[cop(
    name = "RSpec/IncludeExamples",
    description = "Checks for usage of `include_examples`.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl IncludeExamples {
    #[on_node(kind = "send", methods = ["include_examples"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // Upstream `on_send` has no receiver check (`RESTRICT_ON_SEND`
        // only); any receiver flags.
        if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
            return;
        }
        cx.emit_offense(
            cx.node(node).loc.name,
            "Prefer `it_behaves_like` over `include_examples`.",
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::IncludeExamples;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_include_examples() {
        test::<IncludeExamples>().expect_offense(indoc! {r#"
                include_examples 'a foo'
                ^^^^^^^^^^^^^^^^ Prefer `it_behaves_like` over `include_examples`.
            "#});
    }

    #[test]
    fn flags_include_examples_with_explicit_receiver() {
        // Upstream `on_send` constrains only the selector, so an
        // explicit receiver still flags (verified vs 3.7.0).
        test::<IncludeExamples>().expect_offense(indoc! {r#"
                obj.include_examples 'a foo'
                    ^^^^^^^^^^^^^^^^ Prefer `it_behaves_like` over `include_examples`.
            "#});
    }

    #[test]
    fn does_not_flag_behaves_like() {
        test::<IncludeExamples>().expect_no_offenses(indoc! {r#"
                it_behaves_like 'a foo'
            "#});
    }
}

murphy_plugin_api::submit_cop!(IncludeExamples);
