//! `Lint/DuplicateCaseCondition` — flags a `when` condition that repeats an
//! earlier `when` condition within the same `case`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/DuplicateCaseCondition
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Ported from RuboCop Lint/DuplicateCaseCondition. Tracks every `when`
//!   condition across all branches of a `case` in source order and flags any
//!   condition whose key has already been seen (the first occurrence is never
//!   flagged), mirroring RuboCop's `Set#add?` logic. Each `when` may carry
//!   several conditions (`when a, b`) and every duplicate among them is
//!   reported, matching RuboCop (`when a, b; when b, a` flags both `a` and
//!   `b`). Deliberate divergence: RuboCop compares conditions by parser-node
//!   structural equality, while Murphy keys on `raw_source` text. This matches
//!   for all realistic cases but treats syntactically different spellings of
//!   the same value as distinct (`1` vs `0x1`, `'a'` vs `"a"`) and would treat
//!   two whitespace-different spellings of the same expression as distinct.
//!   This is the same `raw_source` keying used by
//!   `Style/IdenticalConditionalBranches`.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! case x
//! when 'first' then do_something
//! when 'first' then do_other   # offense on the second `'first'`
//! end
//! ```
//!
//! ## Why this shape
//!
//! A `when` condition that duplicates an earlier one is dead code: the first
//! matching branch always wins, so the duplicate branch can never run.

use std::collections::HashSet;

use murphy_plugin_api::{Cx, NoOptions, NodeId, cop};

#[derive(Default)]
pub struct DuplicateCaseCondition;

const MSG: &str = "Duplicate `when` condition detected.";

#[cop(
    name = "Lint/DuplicateCaseCondition",
    description = "Do not repeat values in case conditionals.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl DuplicateCaseCondition {
    #[on_node(kind = "case")]
    fn check_case(&self, node: NodeId, cx: &Cx<'_>) {
        // One `seen` set per `case`, accumulated across all `when` branches
        // in source order (RuboCop's `each_with_object(Set.new)`).
        let mut seen: HashSet<&str> = HashSet::new();
        for &when_branch in cx.case_when_branches(node) {
            for &condition in cx.when_conditions(when_branch) {
                let key = cx.raw_source(cx.range(condition));
                if !seen.insert(key) {
                    cx.emit_offense(cx.range(condition), MSG, None);
                }
            }
        }
    }
}

murphy_plugin_api::submit_cop!(DuplicateCaseCondition);

#[cfg(test)]
mod tests {
    use super::DuplicateCaseCondition;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_repeated_condition() {
        test::<DuplicateCaseCondition>().expect_offense(indoc! {r#"
            case x
            when false
              first_method
            when true
              second_method
            when false
                 ^^^^^ Duplicate `when` condition detected.
              third_method
            end
        "#});
    }

    #[test]
    fn flags_immediately_repeated_condition() {
        test::<DuplicateCaseCondition>().expect_offense(indoc! {r#"
            case x
            when false
              first_method
            when false
                 ^^^^^ Duplicate `when` condition detected.
              second_method
            end
        "#});
    }

    #[test]
    fn flags_multiple_duplicates() {
        test::<DuplicateCaseCondition>().expect_offense(indoc! {r#"
            case x
            when false
              first_method
            when true
              second_method
            when false
                 ^^^^^ Duplicate `when` condition detected.
              third_method
            when true
                 ^^^^ Duplicate `when` condition detected.
              fourth_method
            end
        "#});
    }

    #[test]
    fn flags_each_duplicate_in_multi_value_when() {
        // `when b, a` repeats both `a` and `b` from `when a, b`.
        test::<DuplicateCaseCondition>().expect_offense(indoc! {r#"
            case x
            when a, b
              first_method
            when b, a
                 ^ Duplicate `when` condition detected.
                    ^ Duplicate `when` condition detected.
              second_method
            end
        "#});
    }

    #[test]
    fn flags_repeated_logical_condition() {
        test::<DuplicateCaseCondition>().expect_offense(indoc! {r#"
            case x
            when a && b
              first_method
            when a && b
                 ^^^^^^ Duplicate `when` condition detected.
              second_method
            end
        "#});
    }

    // --- no offenses ---

    #[test]
    fn ignores_single_when() {
        test::<DuplicateCaseCondition>()
            .expect_no_offenses("case x\nwhen false\n  first_method\nend\n");
    }

    #[test]
    fn ignores_distinct_conditions() {
        test::<DuplicateCaseCondition>().expect_no_offenses(indoc! {r#"
            case x
            when false
              first_method
            when true
              second_method
            end
        "#});
    }

    #[test]
    fn ignores_distinct_conditions_with_else() {
        test::<DuplicateCaseCondition>().expect_no_offenses(indoc! {r#"
            case x
            when false
              first_method
            when true
              second_method
            else
              third_method
            end
        "#});
    }

    #[test]
    fn ignores_non_equivalent_logical_conditions() {
        test::<DuplicateCaseCondition>().expect_no_offenses(indoc! {r#"
            case x
            when something && another && other
              first_method
            when something && another
              second_method
            end
        "#});
    }

    #[test]
    fn ignores_multibyte_distinct_conditions() {
        test::<DuplicateCaseCondition>()
            .expect_no_offenses("case x\nwhen '名前'\n  a\nwhen '住所'\n  b\nend\n");
    }

    #[test]
    fn whitespace_only_difference_is_not_flagged_documented_divergence() {
        // Pins the `raw_source` keying limitation documented in the
        // murphy-parity block: `a + b` and `a+b` are structurally identical
        // but differ only in whitespace, so they key differently and the
        // duplicate is NOT flagged. RuboCop, comparing parser nodes, WOULD
        // flag the second. Matches the `Style/IdenticalConditionalBranches`
        // approach.
        test::<DuplicateCaseCondition>().expect_no_offenses(indoc! {r#"
            case x
            when a + b
              first_method
            when a+b
              second_method
            end
        "#});
    }
}
