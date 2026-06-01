//! `Style/RescueModifier` — flags modifier-form `rescue` usage.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RescueModifier
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detects modifier-form `rescue` (e.g. `foo rescue nil`) and flags the
//!   whole rescue expression. The modifier form is distinguished from block-form
//!   rescue by the absence of an `end` keyword token at the expression end.
//!
//!   Covered:
//!     - Offense on the full rescue modifier expression range.
//!     - No autocorrect in v1 (the RuboCop autocorrect rewrites to a
//!       `begin/rescue/end` block with alignment-aware indentation; this
//!       non-trivial structural rewrite is deferred).
//!
//!   Gaps vs RuboCop:
//!     - No autocorrect.
//!     - Parenthesized form `(foo rescue nil)` behavior not specifically tested.
//! ```
//!
//! ## Matched shapes
//!
//! `Rescue` nodes that have no `end` keyword token at their expression end
//! (i.e. modifier-form `foo rescue nil`, not `begin ... rescue ... end`).
//!
//! ## Distinguishing modifier from block form
//!
//! `cx.loc(node).end_keyword()` scans for an `end` token ending exactly at
//! the node's expression end. Block-form rescue always ends with `end`;
//! modifier-form rescue does not.

use murphy_plugin_api::{Cx, NoOptions, NodeId, Range, cop};

const MSG: &str = "Avoid using `rescue` in its modifier form.";

/// Stateless unit struct.
#[derive(Default)]
pub struct RescueModifier;

#[cop(
    name = "Style/RescueModifier",
    description = "Avoid using `rescue` in its modifier form.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RescueModifier {
    #[on_node(kind = "rescue")]
    fn check_rescue(&self, node: NodeId, cx: &Cx<'_>) {
        // Only flag modifier-form rescue (no `end` keyword at expression end).
        if cx.loc(node).end_keyword() != Range::ZERO {
            return;
        }
        cx.emit_offense(cx.range(node), MSG, None);
    }
}

#[cfg(test)]
mod tests {
    use super::RescueModifier;
    use murphy_plugin_api::test_support::{indoc, test};
    use murphy_plugin_api::NodeCop;

    #[test]
    fn rescue_modifier_cop_has_rescue_kind_tag() {
        // Verify the macro-generated KINDS array contains tag 57 (Rescue).
        // This pins the CLI dispatch contract.
        assert!(
            RescueModifier::KINDS.iter().any(|t| t.0 == 57),
            "RescueModifier::KINDS must contain Rescue tag (57), got: {:?}",
            RescueModifier::KINDS
        );
    }

    // ----- Offense cases -----

    #[test]
    fn flags_simple_rescue_modifier() {
        test::<RescueModifier>().expect_offense(indoc! {"
            some_method rescue handle_error
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid using `rescue` in its modifier form.
        "});
    }

    #[test]
    fn flags_rescue_nil() {
        test::<RescueModifier>().expect_offense(indoc! {"
            foo rescue nil
            ^^^^^^^^^^^^^^ Avoid using `rescue` in its modifier form.
        "});
    }

    #[test]
    fn flags_rescue_with_exception_class() {
        test::<RescueModifier>().expect_offense(indoc! {"
            some_method rescue SomeException
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid using `rescue` in its modifier form.
        "});
    }

    // ----- Negative cases (block form should be accepted) -----

    #[test]
    fn accepts_begin_rescue_end() {
        test::<RescueModifier>().expect_no_offenses(indoc! {"
            begin
              some_method
            rescue
              handle_error
            end
        "});
    }

    #[test]
    fn accepts_def_rescue() {
        test::<RescueModifier>().expect_no_offenses(indoc! {"
            def foo
              some_method
            rescue
              handle_error
            end
        "});
    }

    #[test]
    fn accepts_rescue_with_exception_class_block_form() {
        test::<RescueModifier>().expect_no_offenses(indoc! {"
            begin
              some_method
            rescue SomeException
              handle_error
            end
        "});
    }
}
murphy_plugin_api::submit_cop!(RescueModifier);
