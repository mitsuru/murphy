//! `Rails/HasAndBelongsToMany` — prefer `has_many :through` over `has_and_belongs_to_many`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/HasAndBelongsToMany
//! upstream_version_checked: 2.35.0
//! version_added: "0.12"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND [:has_and_belongs_to_many]
//!   gating with `command?` (bare call, no receiver) and selector-only offense
//!   range. No autocorrect. Upstream Include (`**/app/models/**/*.rb`) is
//!   enforced via the murphy-rails pack default.yml (engine
//!   `cop_applies_to_file` gate, verified vs rubocop-rails 2.38.0
//!   default.yml, murphy-4gd.1.15).
//! ```
//!
//! Checks for the use of the `has_and_belongs_to_many` macro.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct HasAndBelongsToMany;

#[cop(
    name = "Rails/HasAndBelongsToMany",
    description = "Prefer `has_many :through` to `has_and_belongs_to_many`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl HasAndBelongsToMany {
    // Mirrors upstream `RESTRICT_ON_SEND = %i[has_and_belongs_to_many]`.
    #[on_node(kind = "send", methods = ["has_and_belongs_to_many"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, .. } = *cx.kind(node) else {
            return;
        };
        // Upstream `node.command?` — bare call with no receiver.
        if receiver.get().is_some() {
            return;
        }
        cx.emit_offense(
            cx.selector(node),
            "Prefer `has_many :through` to `has_and_belongs_to_many`.",
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::HasAndBelongsToMany;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_bare_macro() {
        test::<HasAndBelongsToMany>().expect_offense(indoc! {r#"
            has_and_belongs_to_many :ingredients
            ^^^^^^^^^^^^^^^^^^^^^^^ Prefer `has_many :through` to `has_and_belongs_to_many`.
        "#});
    }

    #[test]
    fn does_not_flag_with_receiver() {
        test::<HasAndBelongsToMany>()
            .expect_no_offenses("obj.has_and_belongs_to_many :groups\n");
    }

    #[test]
    fn flags_without_arguments() {
        test::<HasAndBelongsToMany>().expect_offense(indoc! {r#"
            has_and_belongs_to_many
            ^^^^^^^^^^^^^^^^^^^^^^^ Prefer `has_many :through` to `has_and_belongs_to_many`.
        "#});
    }
}
murphy_plugin_api::submit_cop!(HasAndBelongsToMany);
