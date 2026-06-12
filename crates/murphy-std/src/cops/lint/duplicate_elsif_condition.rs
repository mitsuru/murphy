//! `Lint/DuplicateElsifCondition` — flags an `elsif` condition that repeats an
//! earlier condition in the same `if`/`elsif` chain.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/DuplicateElsifCondition
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Ported from RuboCop Lint/DuplicateElsifCondition. Walks the
//!   `if`/`elsif` chain once from its head, accumulating conditions and
//!   flagging any condition already seen, mirroring RuboCop's `on_if`
//!   (`previous.include?(condition)`). Murphy lowers each `elsif` to a
//!   nested `If` node in the else slot, so the `on_node(kind = "if")`
//!   handler fires on every link in the chain; the cop processes only from
//!   the head (it returns early when the node is itself an `elsif`) so the
//!   offense on a given condition is reported exactly once — RuboCop relies
//!   on offense de-duplication to the same effect. Deliberate divergence:
//!   RuboCop compares conditions by parser-node structural equality, while
//!   Murphy keys on `raw_source` text (the same approach as
//!   `Style/IdenticalConditionalBranches`); this matches for realistic code
//!   but treats different spellings of the same value as distinct
//!   (`1` vs `0x1`).
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! if x == 1
//!   do_something
//! elsif x == 1            # offense on the second `x == 1`
//!   do_something_else
//! end
//! ```
//!
//! ## Why this shape
//!
//! An `elsif` condition that duplicates an earlier branch's condition is dead
//! code: the earlier branch always wins, so the duplicate branch can never run.

use murphy_plugin_api::{Cx, NoOptions, NodeId, cop};

#[derive(Default)]
pub struct DuplicateElsifCondition;

const MSG: &str = "Duplicate `elsif` condition detected.";

#[cop(
    name = "Lint/DuplicateElsifCondition",
    description = "Do not repeat conditions used in if `elsif`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl DuplicateElsifCondition {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        // RuboCop's `on_if` walks the chain via `while node.if? || node.elsif?`,
        // so it only ever processes `if` / `elsif` links — never `unless` or a
        // ternary. Murphy lowers `unless` (then/else swapped) and each `elsif`
        // to a `NodeKind::If`, and this handler fires on every such node.
        // Start the walk only at a true `if` head: this skips `elsif` links
        // (the head's walk already covers them, so each duplicate is reported
        // once) and skips `unless` / ternary heads that RuboCop never enters.
        if !cx.is_if(node) {
            return;
        }

        let mut previous: Vec<&str> = Vec::new();
        let mut current = node;
        while let Some(condition) = cx.if_condition(current).get() {
            let key = cx.raw_source(cx.range(condition));
            if previous.contains(&key) {
                cx.emit_offense(cx.range(condition), MSG, None);
            }
            previous.push(key);

            // Descend into the else branch; continue only while it is an
            // `if` / `elsif` link (RuboCop's `break unless node&.if_type?`
            // combined with the `if? || elsif?` loop guard). An `unless`
            // nested in the else is `if_type?` for RuboCop but neither
            // `if?` nor `elsif?`, so it ends the chain — matching that here
            // prevents a false positive on `unless` conditions that merely
            // reuse text from an earlier branch.
            let Some(else_branch) = cx.else_branch(current).get() else {
                break;
            };
            if !(cx.is_if(else_branch) || cx.is_elsif(else_branch)) {
                break;
            }
            current = else_branch;
        }
    }
}

murphy_plugin_api::submit_cop!(DuplicateElsifCondition);

#[cfg(test)]
mod tests {
    use super::DuplicateElsifCondition;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_elsif_duplicating_if_condition() {
        test::<DuplicateElsifCondition>().expect_offense(indoc! {r#"
            if x == 1
            elsif x == 2
            elsif x == 1
                  ^^^^^^ Duplicate `elsif` condition detected.
            end
        "#});
    }

    #[test]
    fn flags_adjacent_duplicate_elsif() {
        test::<DuplicateElsifCondition>().expect_offense(indoc! {r#"
            if x == 1
            elsif x == 2
            elsif x == 2
                  ^^^^^^ Duplicate `elsif` condition detected.
            end
        "#});
    }

    #[test]
    fn flags_multiple_duplicates() {
        test::<DuplicateElsifCondition>().expect_offense(indoc! {r#"
            if x == 1
            elsif x == 2
            elsif x == 1
                  ^^^^^^ Duplicate `elsif` condition detected.
            elsif x == 2
                  ^^^^^^ Duplicate `elsif` condition detected.
            end
        "#});
    }

    #[test]
    fn flags_repeated_elsif_three_times() {
        // `if x; elsif x; elsif x` → the two trailing elsifs both repeat the
        // head condition → 2 offenses (the head itself is never flagged).
        test::<DuplicateElsifCondition>().expect_offense(indoc! {r#"
            if x == 1
            elsif x == 1
                  ^^^^^^ Duplicate `elsif` condition detected.
            elsif x == 1
                  ^^^^^^ Duplicate `elsif` condition detected.
            end
        "#});
    }

    // --- no offenses ---

    #[test]
    fn ignores_unique_elsif_conditions() {
        test::<DuplicateElsifCondition>().expect_no_offenses(indoc! {r#"
            if x == 1
            elsif x == 2
            else
            end
        "#});
    }

    #[test]
    fn ignores_partial_overlap() {
        test::<DuplicateElsifCondition>().expect_no_offenses(indoc! {r#"
            if x == 1
            elsif x == 1 && x == 2
            end
        "#});
    }

    #[test]
    fn ignores_plain_if() {
        test::<DuplicateElsifCondition>().expect_no_offenses("if x == 1\n  foo\nend\n");
    }

    #[test]
    fn ignores_distinct_multibyte_conditions() {
        test::<DuplicateElsifCondition>()
            .expect_no_offenses("if x == '名前'\nelsif x == '住所'\nend\n");
    }

    #[test]
    fn ignores_unless_in_else_reusing_condition_text() {
        // RuboCop's chain walk (`while node.if? || node.elsif?`) never enters
        // an `unless`, so the inner `unless x == 1` is not compared against the
        // outer `if x == 1`. Murphy lowers `unless` to a nested `If`, so the
        // walk must `break` at it to avoid a false positive.
        test::<DuplicateElsifCondition>().expect_no_offenses(indoc! {r#"
            if x == 1
            else
              unless x == 1
                foo
              end
            end
        "#});
    }

    #[test]
    fn ignores_unless_head_with_nested_if_reusing_condition_text() {
        // An `unless` head is neither `if?` nor `elsif?`, so RuboCop's loop
        // body never runs and the nested `if x == 1` is never seeded against
        // the head's condition. The cop must skip `unless` heads entirely.
        test::<DuplicateElsifCondition>().expect_no_offenses(indoc! {r#"
            unless x == 1
              a
            else
              if x == 1
                b
              end
            end
        "#});
    }
}
