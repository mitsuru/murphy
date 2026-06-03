//! `Style/NestedTernaryOperator` — flags ternary operators nested inside another ternary.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/NestedTernaryOperator
//! upstream_version_checked: 1.86.2
//! version_added: "0.9"
//! safe: true
//! supports_autocorrect: false
//! status: partial
//! gap_issues:
//!   - murphy-yx8w
//! notes: >
//!   Detection: flags any ternary `if` node that has at least one ternary `if`
//!   ancestor. This covers `a ? b ? c : d : e` (then-nested) and
//!   `a ? b : c ? d : e` (else-nested) and multi-level nesting.
//!
//!   Known v1 gap: parenthesized inner ternaries (`a ? (b ? b1 : b2) : a2`)
//!   parse to `Unknown` in Murphy's AST (prism's ParenthesesNode is not yet
//!   translated). These are silently missed — false negatives only.
//!
//!   Autocorrect: not implemented (v1 gap). RuboCop's autocorrect rewrites the
//!   outer ternary to an `if/else` block. Deferred because (a) the restructuring
//!   is non-trivial and (b) the parenthesized-branch gap blocks clean rewriting
//!   for the headline case.
//! ```
//!
//! ## Matched shapes
//!
//! A ternary `if` node (`cond ? a : b`) that is itself contained within another
//! ternary `if` node, at any nesting depth. The offense is reported on the
//! inner (nested) ternary.
//!
//! ```ruby
//! # bad
//! a ? b ? b1 : b2 : a2
//!
//! # bad (else branch)
//! a ? a1 : b ? b1 : b2
//!
//! # good — use if/else instead
//! if a
//!   b ? b1 : b2
//! else
//!   a2
//! end
//! ```
//!
//! ## No autocorrect
//!
//! Autocorrect would require rewriting the outer ternary to an `if/else` block.
//! Deferred because the parenthesized-branch shape (`Unknown` AST node) blocks
//! clean structural rewriting of the RuboCop headline example.

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind};

const MSG: &str = "Ternary operators must not be nested. Prefer `if` or `else` constructs instead.";

#[derive(Default)]
pub struct NestedTernaryOperator;

#[cop(
    name = "Style/NestedTernaryOperator",
    description = "Use one expression per branch in a ternary operator.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl NestedTernaryOperator {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        // Only ternary expressions (`cond ? a : b`).
        if !cx.is_ternary(node) {
            return;
        }

        // Flag this node only if it is nested inside another ternary `if`.
        let is_nested = cx
            .ancestors(node)
            .any(|anc| matches!(cx.kind(anc), NodeKind::If { .. }) && cx.is_ternary(anc));

        if is_nested {
            cx.emit_offense(cx.range(node), MSG, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{NestedTernaryOperator, MSG};
    use murphy_plugin_api::test_support::{indoc, run_cop, test};

    #[test]
    fn flags_nested_ternary_in_then_branch() {
        // `a ? b ? b1 : b2 : a2` — inner ternary is the then-branch.
        // Offense is on the nested ternary node.
        test::<NestedTernaryOperator>().expect_offense(indoc! {"
            a ? b ? b1 : b2 : a2
                ^^^^^^^^^^^ Ternary operators must not be nested. Prefer `if` or `else` constructs instead.
        "});
    }

    #[test]
    fn flags_nested_ternary_in_else_branch() {
        // `a ? a1 : b ? b1 : b2` — inner ternary is the else-branch.
        test::<NestedTernaryOperator>().expect_offense(indoc! {"
            a ? a1 : b ? b1 : b2
                     ^^^^^^^^^^^ Ternary operators must not be nested. Prefer `if` or `else` constructs instead.
        "});
    }

    #[test]
    fn accepts_single_ternary() {
        test::<NestedTernaryOperator>().expect_no_offenses("a ? b : c\n");
    }

    #[test]
    fn accepts_non_ternary_if() {
        test::<NestedTernaryOperator>().expect_no_offenses(indoc! {"
            if a
              b
            else
              c
            end
        "});
    }

    #[test]
    fn accepts_if_containing_ternary() {
        // A regular `if` whose branch contains a ternary — not a nested ternary.
        test::<NestedTernaryOperator>().expect_no_offenses(indoc! {"
            if a
              b ? b1 : b2
            else
              c
            end
        "});
    }

    #[test]
    fn flags_triple_nested_ternary() {
        // `a ? b ? c ? d : e : f : g`
        // The middle ternary `b ? c ? d : e : f` is nested in the outer.
        // The innermost ternary `c ? d : e` is nested in the middle.
        // Both should be flagged (2 offenses).
        let src = "a ? b ? c ? d : e : f : g\n";
        let offenses = run_cop::<NestedTernaryOperator>(src);
        assert_eq!(
            offenses.len(),
            2,
            "expected 2 offenses for triple-nested ternary, got: {:?}",
            offenses
        );
        // Verify the offenses carry the correct message.
        for o in &offenses {
            assert_eq!(o.message, MSG);
        }
    }

    #[test]
    fn no_autocorrect() {
        // The cop is detect-only — no edits should be emitted.
        test::<NestedTernaryOperator>().expect_no_corrections("a ? b ? b1 : b2 : a2\n");
    }
}

murphy_plugin_api::submit_cop!(NestedTernaryOperator);
