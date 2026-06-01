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
//!   RuboCop's `NOT_SUPPORTED_PARENT_TYPES` guard (:return/:break/:next/:send/:csend)
//!   is omitted: Murphy will flag `foo(case; when x; end)` where RuboCop would not,
//!   producing a false-positive divergence. The when-branch "return type" guard is
//!   also omitted for the same reason. These are v1 scope decisions.
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
        let NodeKind::Case { subject, .. } = *cx.kind(node) else {
            return;
        };
        if subject.get().is_some() {
            return;
        }
        cx.emit_offense(cx.loc(node).keyword(), MSG, None);
    }
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

    // ----- on_case_match dispatch PoC (murphy-j1j2 PM-F) ------------------

    /// Minimal cop that fires on every `CaseMatch` node (`case/in` form).
    /// Used as a PoC that `#[on_node(kind = "case_match")]` dispatch works.
    #[derive(Default)]
    struct CaseMatchProbe;

    #[murphy_plugin_api::cop(
        name = "Test/CaseMatchProbe",
        description = "Proof-of-concept: fires on case_match nodes.",
        default_severity = "warning",
        default_enabled = true,
        options = murphy_plugin_api::NoOptions,
    )]
    impl CaseMatchProbe {
        #[on_node(kind = "case_match")]
        fn on_case_match(&self, node: murphy_plugin_api::NodeId, cx: &murphy_plugin_api::Cx<'_>) {
            // Offense range: the `case` keyword token at the node start.
            // `CaseMatch` is not in `keyword_bearing`, so we find the token
            // by searching from the node's expression start.
            use murphy_plugin_api::{Range, SourceTokenKind};
            let node_start = cx.range(node).start;
            let kw_range = cx
                .token_after(node_start)
                .filter(|t| {
                    t.range.start == node_start
                        && t.kind == SourceTokenKind::Other
                        && cx.raw_source(t.range) == "case"
                })
                .map(|t| t.range)
                .unwrap_or(cx.range(node));
            cx.emit_offense(kw_range, "case_match dispatched", None);
        }
    }

    #[test]
    fn on_case_match_dispatch_fires_on_case_in() {
        // Verify that `#[on_node(kind = "case_match")]` dispatches to the
        // handler when the walker visits a `case/in` expression.
        test::<CaseMatchProbe>().expect_offense(indoc! {"
            case foo
            ^^^^ case_match dispatched
            in Integer
              :int
            end
        "});
    }

    #[test]
    fn on_case_match_dispatch_does_not_fire_on_case_when() {
        // `case/when` produces a `Case` node (tag 26), not a `CaseMatch`
        // (tag 86) — the handler must not be triggered.
        test::<CaseMatchProbe>().expect_no_offenses(indoc! {"
            case x
            when 1
              :one
            end
        "});
    }
}
