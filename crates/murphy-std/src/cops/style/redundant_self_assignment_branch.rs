//! `Style/RedundantSelfAssignmentBranch` — flags conditional branches that
//! redundantly re-assign the same local variable.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantSelfAssignmentBranch
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Only `lvasgn` (local variables) are checked, matching RuboCop's scope.
//!   Instance, class, and global variables are excluded because they carry
//!   state across method calls and a modifier rewrite could silently nil them.
//!   Heredoc tail handling in the autocorrect is a documented gap — the
//!   correction still fires but will not append the heredoc-end line.
//!   Multi-statement `begin` branches and `elsif` else-branches block the
//!   correction (cannot be safely expressed as modifiers).
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad — ternary form
//! foo = condition ? bar : foo
//!
//! # bad — block form
//! foo = if condition
//!         bar
//!       else
//!         foo
//!       end
//! ```
//!
//! In both cases the cop fires on the self-assigning branch (the `foo`
//! reference, not the whole assignment) and offers an autocorrect that rewrites
//! the RHS `If` node to a modifier statement.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantSelfAssignmentBranch;

const MSG: &str = "Remove the self-assignment branch.";

#[cop(
    name = "Style/RedundantSelfAssignmentBranch",
    description = "Checks for places where conditional branch makes redundant self-assignment.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RedundantSelfAssignmentBranch {
    #[on_node(kind = "lvasgn")]
    fn check_lvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Lvasgn { name, value } = *cx.kind(node) else {
            return;
        };

        let Some(rhs) = value.get() else {
            return;
        };

        // The RHS must be an `If` node with both branches.
        if !matches!(cx.kind(rhs), NodeKind::If { .. }) {
            return;
        }
        if !use_if_and_else_branch(rhs, cx) {
            return;
        }

        let if_branch = cx.if_branch(rhs);
        let else_branch = cx.else_branch(rhs);

        let Some(if_b) = if_branch.get() else {
            return;
        };
        let Some(else_b) = else_branch.get() else {
            return;
        };

        if inconvertible_to_modifier(if_branch, else_branch, cx) {
            return;
        }

        let var_name = cx.symbol_str(name);

        // Case 1: `foo = condition ? foo : bar`
        //          then-branch is self-assign → keyword is `unless`
        if is_self_assign(var_name, if_b, cx) {
            register_offense(rhs, if_b, else_branch, "unless", cx);
        // Case 2: `foo = condition ? bar : foo`
        //          else-branch is self-assign → keyword is `if`
        } else if is_self_assign(var_name, else_b, cx) {
            register_offense(rhs, else_b, if_branch, "if", cx);
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// `use_if_and_else_branch?` from RuboCop:
/// The node must be an If with both branches present. We admit both ternary
/// and block-form if/else.
fn use_if_and_else_branch(node: NodeId, cx: &Cx<'_>) -> bool {
    if !matches!(cx.kind(node), NodeKind::If { .. }) {
        return false;
    }
    // Both branches must be present.
    cx.if_branch(node).get().is_some() && cx.else_branch(node).get().is_some()
}

/// Returns true if `branch` source text equals `var_name`.
fn is_self_assign(var_name: &str, branch: NodeId, cx: &Cx<'_>) -> bool {
    cx.raw_source(cx.range(branch)) == var_name
}

/// `inconvertible_to_modifier?`:
/// - Either branch has multiple statements (a non-empty `begin`)
/// - The else branch is an `elsif`
fn inconvertible_to_modifier(
    if_branch: OptNodeId,
    else_branch: OptNodeId,
    cx: &Cx<'_>,
) -> bool {
    for opt in [if_branch, else_branch] {
        if multiple_statements(opt, cx) {
            return true;
        }
    }
    // elsif: else-branch is an If node with `elsif` keyword
    if let Some(else_b) = else_branch.get()
        && matches!(cx.kind(else_b), NodeKind::If { .. }) && cx.is_elsif(else_b) {
            return true;
        }
    false
}

/// A branch has multiple statements when it is a `begin` with any children.
fn multiple_statements(branch: OptNodeId, cx: &Cx<'_>) -> bool {
    let Some(b) = branch.get() else {
        return false;
    };
    if let NodeKind::Begin(list) = *cx.kind(b) {
        !cx.list(list).is_empty()
    } else {
        false
    }
}

/// Emit the offense on `offense_branch` and register an autocorrect that
/// replaces `if_node` (the whole RHS If) with the modifier form.
fn register_offense(
    if_node: NodeId,
    offense_branch: NodeId,
    opposite_branch: OptNodeId,
    keyword: &str,
    cx: &Cx<'_>,
) {
    let offense_range = cx.range(offense_branch);
    cx.emit_offense(offense_range, MSG, None);

    // Build replacement: `"{opposite} {keyword} {condition}"`
    let condition_source = cx
        .if_condition(if_node)
        .get()
        .map(|c| cx.raw_source(cx.range(c)))
        .unwrap_or_default();

    let opposite_source = opposite_branch
        .get()
        .map(|o| cx.raw_source(cx.range(o)))
        .unwrap_or("nil");

    let replacement = format!("{opposite_source} {keyword} {condition_source}");
    cx.emit_edit(cx.range(if_node), &replacement);
}

#[cfg(test)]
mod tests {
    use super::RedundantSelfAssignmentBranch;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- Ternary form ---

    #[test]
    fn flags_ternary_else_self_assign() {
        // `foo = condition ? bar : foo` — else branch is self-assign
        test::<RedundantSelfAssignmentBranch>().expect_offense(indoc! {"
            foo = condition ? bar : foo
                                    ^^^ Remove the self-assignment branch.
        "});
    }

    #[test]
    fn corrects_ternary_else_self_assign() {
        test::<RedundantSelfAssignmentBranch>().expect_correction(
            indoc! {"
                foo = condition ? bar : foo
                                        ^^^ Remove the self-assignment branch.
            "},
            "foo = bar if condition\n",
        );
    }

    #[test]
    fn flags_ternary_then_self_assign() {
        // `foo = condition ? foo : bar` — then branch is self-assign
        test::<RedundantSelfAssignmentBranch>().expect_offense(indoc! {"
            foo = condition ? foo : bar
                              ^^^ Remove the self-assignment branch.
        "});
    }

    #[test]
    fn corrects_ternary_then_self_assign() {
        test::<RedundantSelfAssignmentBranch>().expect_correction(
            indoc! {"
                foo = condition ? foo : bar
                                  ^^^ Remove the self-assignment branch.
            "},
            "foo = bar unless condition\n",
        );
    }

    // --- Block-form if/else ---

    #[test]
    fn flags_block_form_else_self_assign() {
        test::<RedundantSelfAssignmentBranch>().expect_offense(indoc! {"
            foo = if condition
                    bar
                  else
                    foo
                    ^^^ Remove the self-assignment branch.
                  end
        "});
    }

    #[test]
    fn corrects_block_form_else_self_assign() {
        test::<RedundantSelfAssignmentBranch>().expect_correction(
            indoc! {"
                foo = if condition
                        bar
                      else
                        foo
                        ^^^ Remove the self-assignment branch.
                      end
            "},
            "foo = bar if condition\n",
        );
    }

    #[test]
    fn flags_block_form_then_self_assign() {
        test::<RedundantSelfAssignmentBranch>().expect_offense(indoc! {"
            foo = if condition
                    foo
                    ^^^ Remove the self-assignment branch.
                  else
                    bar
                  end
        "});
    }

    #[test]
    fn corrects_block_form_then_self_assign() {
        test::<RedundantSelfAssignmentBranch>().expect_correction(
            indoc! {"
                foo = if condition
                        foo
                        ^^^ Remove the self-assignment branch.
                      else
                        bar
                      end
            "},
            "foo = bar unless condition\n",
        );
    }

    // --- No-offense cases ---

    #[test]
    fn no_offense_no_self_assign_branch() {
        test::<RedundantSelfAssignmentBranch>()
            .expect_no_offenses("foo = condition ? bar : baz\n");
    }

    #[test]
    fn no_offense_ivar_self_assign() {
        // Only lvasgn is checked — ivar is excluded.
        test::<RedundantSelfAssignmentBranch>()
            .expect_no_offenses("@foo = condition ? bar : @foo\n");
    }

    #[test]
    fn no_offense_missing_else_branch() {
        test::<RedundantSelfAssignmentBranch>().expect_no_offenses(indoc! {"
            foo = if condition
                    bar
                  end
        "});
    }

    #[test]
    fn no_offense_multi_statement_then_branch() {
        test::<RedundantSelfAssignmentBranch>().expect_no_offenses(indoc! {"
            foo = if condition
                    x; y
                  else
                    foo
                  end
        "});
    }

    // --- Nil opposite branch correction ---

    #[test]
    fn corrects_nil_opposite_branch() {
        // When the opposite branch is explicitly nil
        test::<RedundantSelfAssignmentBranch>().expect_correction(
            indoc! {"
                foo = condition ? foo : nil
                                  ^^^ Remove the self-assignment branch.
            "},
            "foo = nil unless condition\n",
        );
    }
}
murphy_plugin_api::submit_cop!(RedundantSelfAssignmentBranch);
