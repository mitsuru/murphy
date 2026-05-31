//! `Style/EmptyCaseCondition` — flags `case` statements with no subject.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/EmptyCaseCondition
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Murphy v1 reports offenses for empty-subject `case` statements only.
//!   Autocorrect (converting to if/elsif chains) is intentionally omitted:
//!   deriving the correct `if` condition from each `when` branch requires
//!   non-trivial source transformation that cannot be done surgically.
//!   RuboCop also guards on parent node type (:return/:break/:next/:send/:csend)
//!   and on when-branch "return type" structure; these guards are omitted in
//!   Murphy v1 since without autocorrect they only suppress noise-free reports.
//! ```
//!
//! ## Matched shapes
//!
//! `Case` nodes whose `subject` is absent (`OptNodeId::NONE`). For example:
//!
//! ```ruby
//! # offense
//! case
//! when condition_a
//!   do_a
//! when condition_b
//!   do_b
//! end
//!
//! # no offense — subject present
//! case x
//! when 1
//!   :one
//! end
//! ```
//!
//! ## Why `case/in` is excluded
//!
//! `case/in` with no subject is a Ruby syntax error, so only `Case`
//! (the `case/when` form) is checked. `CaseMatch` nodes are never examined.
//!
//! ## Autocorrect
//!
//! Not implemented. The transformation requires replacing each `when cond`
//! branch with `if/elsif cond` and preserving any else clause, which is a
//! structural rewrite that cannot be expressed as non-overlapping surgical
//! edits.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

const MSG: &str = "Do not use empty `case` condition, instead use an `if` expression.";

/// Stateless unit struct.
#[derive(Default)]
pub struct EmptyCaseCondition;

#[cop(
    name = "Style/EmptyCaseCondition",
    description = "Avoid empty `case` condition; use `if` instead.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl EmptyCaseCondition {
    #[on_node(kind = "case")]
    fn check_case(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Case { subject, .. } = *cx.kind(node) else {
        return;
    };
    // Only report when the subject is absent.
    if subject.get().is_some() {
        return;
    }
    // Offense range is the `case` keyword only, matching RuboCop's behavior.
    let offense_range = cx.loc(node).keyword();
    cx.emit_offense(offense_range, MSG, None);
}

#[cfg(test)]
mod tests {
    use super::EmptyCaseCondition;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Positive cases (offense) ------------------------------------

    #[test]
    fn flags_case_with_no_subject() {
        test::<EmptyCaseCondition>().expect_offense(indoc! {"
            case
            ^^^^ Do not use empty `case` condition, instead use an `if` expression.
            when 1
              :one
            end
        "});
    }

    #[test]
    fn flags_case_with_multiple_whens() {
        test::<EmptyCaseCondition>().expect_offense(indoc! {"
            case
            ^^^^ Do not use empty `case` condition, instead use an `if` expression.
            when condition_a
              :a
            when condition_b
              :b
            else
              :other
            end
        "});
    }

    // ----- Negative cases (no offense) --------------------------------

    #[test]
    fn accepts_case_with_subject() {
        test::<EmptyCaseCondition>().expect_no_offenses(indoc! {"
            case x
            when 1
              :one
            end
        "});
    }

    #[test]
    fn accepts_case_with_expression_subject() {
        test::<EmptyCaseCondition>().expect_no_offenses(indoc! {"
            case foo.bar
            when :baz
              :result
            end
        "});
    }

    #[test]
    fn accepts_case_in_no_subject_is_syntax_error() {
        // `case/in` with no subject is a Ruby syntax error.
        // This test checks that a valid `case/in` (with subject) is not flagged.
        test::<EmptyCaseCondition>().expect_no_offenses(indoc! {"
            case foo
            in Integer
              :int
            end
        "});
    }
}
