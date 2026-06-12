//! `Style/IfWithBooleanLiteralBranches` — flags redundant `if` expressions
//! whose both branches are boolean literals (`true`/`false`).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/IfWithBooleanLiteralBranches
//! upstream_version_checked: 1.82.1
//! version_added: "1.9"
//! safe: false
//! supports_autocorrect: true
//! status: partial
//! gap_issues: []
//! notes: >
//!   All primary cases implemented: if/unless (block-form and modifier-ternary)
//!   and elsif nodes with true/false branches using comparison operators,
//!   predicate methods (ending in `?`), and double negation (`!!`).
//!   AllowedMethods option supported with default ["infinite?", "nonzero?"].
//!   return_boolean_value? logic: or (both sides), and (rhs only).
//!   multiple_elsif? guard: skips the innermost node in a multi-elsif chain.
//!   Autocorrect: opposite_condition? prefixes `!`; parens added for and/or
//!   keywords or comparison sends. elsif autocorrect: insert `else\n` before,
//!   replace with indented replacement.
//!   Offense range: ternary -> from condition end to node end; block-form ->
//!   the keyword token only (matching RuboCop's `keyword` location).
//!   Autocorrect is unsafe: predicate methods may not return boolean values.
//!   Known v1 gap: begin-wrapped conditions (`(foo?)`) are not detected.
//!   In RuboCop (parser gem), `(foo?)` is a `begin` node; in Murphy (prism),
//!   it becomes an `Unknown` node which cannot be unwrapped. This is a minor
//!   gap -- most real-world code does not parenthesize the condition.
//! ```
//!
//! ## Matched shapes
//!
//! `If` nodes (including `unless`, ternary `a ? b : c`, and `elsif`) where:
//! - Both branches are boolean literals, one `true` and one `false`.
//! - The condition `return_boolean_value?`: is a comparison, predicate, or
//!   double-negation send (recursing through `begin`, `or`, `and`).
//! - Not the innermost node of a multi-elsif chain (`multiple_elsif?` guard).
//! - Not a modifier form without an else branch.
//!
//! ## Autocorrect
//!
//! - Normal: replace the whole node with the condition source (or `!(...)`).
//! - `elsif`: insert `else\n` before the node, then replace the node with the
//!   indented replacement.
//!
//! ## Unsafe autocorrect
//!
//! Predicate methods may not return a true boolean value. Users can suppress
//! specific methods via `AllowedMethods` (default: `["infinite?", "nonzero?"]`).

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};

#[derive(Default)]
pub struct IfWithBooleanLiteralBranches;

#[derive(CopOptions)]
pub struct IfWithBooleanLiteralBranchesOptions {
    #[option(name = "AllowedMethods", 
        default = ["infinite?", "nonzero?"],
        description = "Method names that are always allowed (not flagged)."
    )]
    pub allowed_methods: Vec<String>,
}

#[cop(
    name = "Style/IfWithBooleanLiteralBranches",
    description = "Remove redundant `if` with boolean literal branches.",
    default_severity = "warning",
    default_enabled = true,
    options = IfWithBooleanLiteralBranchesOptions
)]
impl IfWithBooleanLiteralBranches {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<IfWithBooleanLiteralBranchesOptions>();
        check(node, cx, &opts.allowed_methods);
    }
}

/// Returns `true` when `node` is a boolean literal (`true` or `false`).
fn is_boolean_literal(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(cx.kind(node), NodeKind::True_ | NodeKind::False_)
}

/// Returns `true` when `node` is the `true` literal.
fn is_true_literal(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(cx.kind(node), NodeKind::True_)
}

/// Checks whether `condition` can be assumed to return a boolean value.
///
/// Mirrors RuboCop's `return_boolean_value?` method:
/// - `begin` -> unwrap and recurse on first child
/// - `or` -> both sides must qualify
/// - `and` -> only rhs must qualify
/// - otherwise -> `assume_boolean_value?`: must be a send that is a
///   comparison method, predicate method, or double negation, and not
///   in the allowed list.
fn return_boolean_value(node: NodeId, cx: &Cx<'_>, allowed: &[String]) -> bool {
    match cx.kind(node) {
        NodeKind::Begin(list) => {
            let children = cx.list(*list);
            if children.is_empty() {
                false
            } else {
                return_boolean_value(children[0], cx, allowed)
            }
        }
        NodeKind::Or { lhs, rhs } => {
            return_boolean_value(*lhs, cx, allowed) && return_boolean_value(*rhs, cx, allowed)
        }
        NodeKind::And { rhs, .. } => return_boolean_value(*rhs, cx, allowed),
        _ => assume_boolean_value(node, cx, allowed),
    }
}

/// Returns `true` when `node` is a send that can be assumed to return boolean.
///
/// Mirrors RuboCop's `assume_boolean_value?`:
/// - Must be a `Send` or `Csend` node.
/// - Must not be an allowed method.
/// - Must be: comparison method, predicate method (`?`-suffix), or double
///   negation (`!!`).
fn assume_boolean_value(node: NodeId, cx: &Cx<'_>, allowed: &[String]) -> bool {
    if !matches!(
        cx.kind(node),
        NodeKind::Send { .. } | NodeKind::Csend { .. }
    ) {
        return false;
    }
    let Some(name) = cx.method_name(node) else {
        return false;
    };
    if allowed.iter().any(|m| m == name) {
        return false;
    }
    cx.is_comparison_method(node) || cx.is_predicate_method(node) || is_double_negation(node, cx)
}

/// Returns `true` when `node` is a double negation (`!!x`).
fn is_double_negation(node: NodeId, cx: &Cx<'_>) -> bool {
    if !cx.is_negation_method(node) {
        return false;
    }
    let Some(recv) = cx.call_receiver(node).get() else {
        return false;
    };
    cx.is_negation_method(recv)
}

/// Returns `true` when `node` is the inner node of a multi-elsif chain.
///
/// Mirrors RuboCop's `multiple_elsif?`: checks if parent is an `if` node
/// that is itself an `elsif`. This is the guard that prevents flagging the
/// innermost node when there are 2+ `elsif` branches.
fn multiple_elsif(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    matches!(cx.kind(parent), NodeKind::If { .. }) && cx.is_elsif(parent)
}

/// Returns `true` when the condition direction is inverted relative to the
/// branches -- i.e. we need to negate the replacement condition.
///
/// Mirrors RuboCop's `opposite_condition?`:
/// - `(!unless? && if_branch.false?)` -- regular `if`, then-branch is `false`
/// - `(unless? && if_branch.true?)` -- `unless`, then-branch is `true`
///
/// For `unless`, prism swaps the AST branches relative to source order:
/// - `if_then_branch` holds the SOURCE else-body
/// - `if_else_branch` holds the SOURCE then-body
///
/// RuboCop's `if_branch` for `unless` is the SOURCE then-body, which in
/// Murphy is `if_else_branch`. We check `is_true_literal(if_else_branch)`
/// for `unless` to match RuboCop's `if_branch.true_type?` check.
fn opposite_condition(node: NodeId, cx: &Cx<'_>) -> bool {
    if cx.is_unless(node) {
        // Prism swaps unless branches: if_else_branch = source then-body.
        // RuboCop: unless? && if_branch.true_type? (if_branch = source then-body).
        let Some(source_then_body) = cx.if_else_branch(node).get() else {
            return false;
        };
        is_true_literal(source_then_body, cx)
    } else {
        // Regular if: if_then_branch = source then-body.
        // RuboCop: !unless? && if_branch.false_type?
        let Some(then_branch) = cx.if_then_branch(node).get() else {
            return false;
        };
        !is_true_literal(then_branch, cx)
    }
}

/// Returns `true` when negating the condition requires parentheses.
///
/// Mirrors RuboCop's `require_parentheses?`:
/// - `condition.operator_keyword?` (And/Or nodes)
/// - `condition.send_type? && condition.comparison_method?`
fn require_parentheses(cond: NodeId, cx: &Cx<'_>) -> bool {
    cx.is_operator_keyword(cond)
        || (matches!(
            cx.kind(cond),
            NodeKind::Send { .. } | NodeKind::Csend { .. }
        ) && cx.is_comparison_method(cond))
}

/// Build the replacement source string for the whole node.
fn replacement_source(node: NodeId, cond: NodeId, cx: &Cx<'_>) -> String {
    if opposite_condition(node, cx) {
        let cond_src = cx.raw_source(cx.range(cond));
        if require_parentheses(cond, cx) {
            format!("!({cond_src})")
        } else {
            format!("!{cond_src}")
        }
    } else {
        cx.raw_source(cx.range(cond)).to_string()
    }
}

/// Compute the indentation of `node` from the start of its line.
fn node_indentation<'a>(node: NodeId, cx: &Cx<'a>) -> &'a str {
    let source = cx.source();
    let bytes = source.as_bytes();
    let start = cx.range(node).start as usize;
    let line_start = bytes[..start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    let indent_end = bytes[line_start..start]
        .iter()
        .position(|&b| b != b' ' && b != b'\t')
        .map(|i| line_start + i)
        .unwrap_or(start);
    &source[line_start..indent_end]
}

fn check(node: NodeId, cx: &Cx<'_>, allowed: &[String]) {
    // Skip modifier forms -- they can't have both branches.
    if cx.is_modifier_form(node) {
        return;
    }

    // Must have both branches.
    let Some(then_branch) = cx.if_then_branch(node).get() else {
        return;
    };
    let Some(else_branch) = cx.if_else_branch(node).get() else {
        return;
    };

    // One branch must be `true` and the other `false` (order-independent).
    if !(is_boolean_literal(then_branch, cx)
        && is_boolean_literal(else_branch, cx)
        && (is_true_literal(then_branch, cx) != is_true_literal(else_branch, cx)))
    {
        return;
    }

    // The condition must qualify as returning a boolean.
    let Some(cond) = cx.if_condition(node).get() else {
        return;
    };
    if !return_boolean_value(cond, cx, allowed) {
        return;
    }

    // Skip the innermost node in a multi-elsif chain.
    if multiple_elsif(node, cx) {
        return;
    }

    // Build offense range and message keyword.
    let (offense_range, keyword) = if cx.is_ternary(node) {
        // Ternary: from the end of the condition to the end of the node
        // (covers `? true : false`).
        let range = Range {
            start: cx.range(cond).end,
            end: cx.range(node).end,
        };
        (range, "ternary operator".to_string())
    } else {
        // Block-form: the keyword token only (`if`, `unless`, or `elsif`).
        let kw_range = cx.if_keyword_loc(node);
        let kw_text = cx.raw_source(kw_range).to_string();
        (kw_range, format!("`{kw_text}`"))
    };

    let msg = if cx.is_elsif(node) {
        "Use `else` instead of redundant `elsif` with boolean literal branches.".to_string()
    } else {
        format!("Remove redundant {keyword} with boolean literal branches.")
    };

    cx.emit_offense(offense_range, &msg, None);

    // Autocorrect.
    let replacement = replacement_source(node, cond, cx);
    if cx.is_elsif(node) {
        // For elsif: replace the node (which in Murphy includes `end`) with
        // `else\n<indent><replacement>\nend`.
        // The single-edit approach avoids two overlapping edits.
        // The then-branch indentation mirrors RuboCop's `indent(node.if_branch)`.
        let then_indent = cx
            .if_then_branch(node)
            .get()
            .map(|b| node_indentation(b, cx))
            .unwrap_or("");
        let nl = if cx.source().contains("\r\n") { "\r\n" } else { "\n" };
        let edit_src = format!("else{nl}{then_indent}{replacement}{nl}end");
        cx.emit_edit(cx.range(node), &edit_src);
    } else {
        cx.emit_edit(cx.range(node), &replacement);
    }
}

#[cfg(test)]
mod tests {
    use super::{IfWithBooleanLiteralBranches, IfWithBooleanLiteralBranchesOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    // --- Ternary: comparison -> removes ternary ---

    #[test]
    fn flags_ternary_comparison_true_false() {
        test::<IfWithBooleanLiteralBranches>().expect_correction(
            indoc! {"
                foo == bar ? true : false
                          ^^^^^^^^^^^^^^^ Remove redundant ternary operator with boolean literal branches.
            "},
            "foo == bar\n",
        );
    }

    #[test]
    fn flags_ternary_comparison_false_true() {
        test::<IfWithBooleanLiteralBranches>().expect_correction(
            indoc! {"
                foo == bar ? false : true
                          ^^^^^^^^^^^^^^^ Remove redundant ternary operator with boolean literal branches.
            "},
            "!(foo == bar)\n",
        );
    }

    // --- Block-form if/else: comparison ---

    #[test]
    fn flags_if_else_comparison_true_false() {
        test::<IfWithBooleanLiteralBranches>().expect_correction(
            indoc! {"
                if foo == bar
                ^^ Remove redundant `if` with boolean literal branches.
                  true
                else
                  false
                end
            "},
            "foo == bar\n",
        );
    }

    #[test]
    fn flags_if_else_comparison_false_true() {
        test::<IfWithBooleanLiteralBranches>().expect_correction(
            indoc! {"
                if foo == bar
                ^^ Remove redundant `if` with boolean literal branches.
                  false
                else
                  true
                end
            "},
            "!(foo == bar)\n",
        );
    }

    // --- Block-form: predicate method ---

    #[test]
    fn flags_if_predicate_method() {
        test::<IfWithBooleanLiteralBranches>().expect_correction(
            indoc! {"
                if foo.do_something?
                ^^ Remove redundant `if` with boolean literal branches.
                  true
                else
                  false
                end
            "},
            "foo.do_something?\n",
        );
    }

    #[test]
    fn flags_ternary_predicate_method() {
        test::<IfWithBooleanLiteralBranches>().expect_correction(
            indoc! {"
                foo.do_something? ? true : false
                                 ^^^^^^^^^^^^^^^ Remove redundant ternary operator with boolean literal branches.
            "},
            "foo.do_something?\n",
        );
    }

    // --- Double negation ---

    #[test]
    fn flags_if_double_negation() {
        test::<IfWithBooleanLiteralBranches>().expect_correction(
            indoc! {"
                if !!foo
                ^^ Remove redundant `if` with boolean literal branches.
                  true
                else
                  false
                end
            "},
            "!!foo\n",
        );
    }

    // --- unless ---

    #[test]
    fn flags_unless_comparison_true_false() {
        // unless foo == bar; true; else; false; end
        // Prism swaps branches for `unless`: if_then_branch gives body of unless (true here).
        // opposite_condition: unless && if_then_branch is true -> negate.
        test::<IfWithBooleanLiteralBranches>().expect_correction(
            indoc! {"
                unless foo == bar
                ^^^^^^ Remove redundant `unless` with boolean literal branches.
                  true
                else
                  false
                end
            "},
            "!(foo == bar)\n",
        );
    }

    #[test]
    fn flags_unless_comparison_false_true() {
        // unless foo == bar; false; else; true; end
        // if_then_branch => false (body of unless).
        // opposite_condition: unless && if_then_branch is false -> no negate.
        test::<IfWithBooleanLiteralBranches>().expect_correction(
            indoc! {"
                unless foo == bar
                ^^^^^^ Remove redundant `unless` with boolean literal branches.
                  false
                else
                  true
                end
            "},
            "foo == bar\n",
        );
    }

    // --- elsif single: should flag ---

    #[test]
    fn flags_single_elsif() {
        test::<IfWithBooleanLiteralBranches>().expect_correction(
            indoc! {"
                if foo
                  do_something
                elsif bar > baz
                ^^^^^ Use `else` instead of redundant `elsif` with boolean literal branches.
                  true
                else
                  false
                end
            "},
            indoc! {"
                if foo
                  do_something
                else
                  bar > baz
                end
            "},
        );
    }

    // --- multiple elsif: should NOT flag the inner one ---

    #[test]
    fn no_offense_multiple_elsif_inner() {
        // With two `elsif` branches, the inner one is skipped.
        test::<IfWithBooleanLiteralBranches>().expect_no_offenses(indoc! {"
            if foo
              true
            elsif bar > baz
              true
            elsif qux > quux
              true
            else
              false
            end
        "});
    }

    // --- AllowedMethods: infinite? and nonzero? are the defaults ---

    #[test]
    fn allows_nonzero_by_default() {
        test::<IfWithBooleanLiteralBranches>().expect_no_offenses("num.nonzero? ? true : false\n");
    }

    #[test]
    fn flags_nonzero_when_not_in_allowed() {
        test::<IfWithBooleanLiteralBranches>()
            .with_options(&IfWithBooleanLiteralBranchesOptions {
                allowed_methods: vec![],
            })
            .expect_correction(
                indoc! {"
                    num.nonzero? ? true : false
                                ^^^^^^^^^^^^^^^ Remove redundant ternary operator with boolean literal branches.
                "},
                "num.nonzero?\n",
            );
    }

    #[test]
    fn allows_custom_method_in_list() {
        test::<IfWithBooleanLiteralBranches>()
            .with_options(&IfWithBooleanLiteralBranchesOptions {
                allowed_methods: vec!["custom_predicate?".to_string()],
            })
            .expect_no_offenses("custom_predicate? ? true : false\n");
    }

    // --- Negative cases: non-qualifying conditions ---

    #[test]
    fn no_offense_non_predicate_method() {
        // `foo` is not a comparison, predicate, or double negation.
        test::<IfWithBooleanLiteralBranches>().expect_no_offenses("foo ? true : false\n");
    }

    #[test]
    fn no_offense_non_boolean_branches() {
        // Both branches must be boolean literals.
        test::<IfWithBooleanLiteralBranches>().expect_no_offenses("foo == bar ? 1 : 2\n");
    }

    #[test]
    fn no_offense_true_true_branches() {
        // Not one-true-one-false.
        test::<IfWithBooleanLiteralBranches>().expect_no_offenses("foo == bar ? true : true\n");
    }

    #[test]
    fn no_offense_missing_else() {
        test::<IfWithBooleanLiteralBranches>().expect_no_offenses(indoc! {"
            if foo == bar
              true
            end
        "});
    }

    #[test]
    fn no_offense_modifier_form() {
        // `true if foo?` is a modifier form -- no else branch.
        test::<IfWithBooleanLiteralBranches>().expect_no_offenses("true if foo?\n");
    }

    // --- Or/And condition recursion ---

    #[test]
    fn flags_or_condition_both_qualify() {
        test::<IfWithBooleanLiteralBranches>().expect_correction(
            indoc! {"
                if foo? || bar?
                ^^ Remove redundant `if` with boolean literal branches.
                  true
                else
                  false
                end
            "},
            "foo? || bar?\n",
        );
    }

    #[test]
    fn no_offense_or_condition_one_side_not_boolean() {
        // RHS `bar` is not a predicate/comparison/double-negation.
        test::<IfWithBooleanLiteralBranches>().expect_no_offenses(indoc! {"
            if foo? || bar
              true
            else
              false
            end
        "});
    }

    #[test]
    fn flags_and_condition_rhs_qualifies() {
        // `and`: only RHS must qualify.
        test::<IfWithBooleanLiteralBranches>().expect_correction(
            indoc! {"
                if foo and bar?
                ^^ Remove redundant `if` with boolean literal branches.
                  true
                else
                  false
                end
            "},
            "foo and bar?\n",
        );
    }

    #[test]
    fn no_offense_and_condition_rhs_not_boolean() {
        // RHS `bar` is not qualifying.
        test::<IfWithBooleanLiteralBranches>().expect_no_offenses(indoc! {"
            if foo? and bar
              true
            else
              false
            end
        "});
    }

    // --- begin-wrapped condition ---
    // Note: in Murphy's AST (prism), `(foo?)` is an `Unknown` node (parenthesized
    // expression), not a `Begin` node as in RuboCop's parser gem. The begin-unwrap
    // path in `return_boolean_value` therefore does not trigger for parenthesized
    // expressions. This is a known v1 parity gap.

    #[test]
    fn corrects_parenthesized_predicate_condition() {
        // `(foo?)` now lowers to `Begin([Send{foo?}])` via the ParenthesesNode
        // translator fix. `return_boolean_value` unwraps the Begin and detects
        // the predicate, closing the former v1 parity gap.
        test::<IfWithBooleanLiteralBranches>().expect_correction(
            indoc! {"
                if (foo?)
                ^^ Remove redundant `if` with boolean literal branches.
                  true
                else
                  false
                end
            "},
            indoc! {"
                (foo?)
            "},
        );
    }

    // --- Require parentheses for opposite + comparison ---

    #[test]
    fn wraps_in_parens_for_opposite_comparison() {
        test::<IfWithBooleanLiteralBranches>().expect_correction(
            indoc! {"
                if foo > bar
                ^^ Remove redundant `if` with boolean literal branches.
                  false
                else
                  true
                end
            "},
            "!(foo > bar)\n",
        );
    }

    #[test]
    fn no_parens_for_opposite_predicate() {
        // Predicate method: no parens needed.
        test::<IfWithBooleanLiteralBranches>().expect_correction(
            indoc! {"
                if foo?
                ^^ Remove redundant `if` with boolean literal branches.
                  false
                else
                  true
                end
            "},
            "!foo?\n",
        );
    }
}
murphy_plugin_api::submit_cop!(IfWithBooleanLiteralBranches);
