//! `Style/MultipleComparison` — flags repeated equality comparisons of the
//! same variable, suggesting `Array#include?` instead.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/MultipleComparison
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Flags `a == x || a == y || a == z` and suggests `[x, y, z].include?(a)`.
//!   Supports AllowMethodComparison (default true) — when true, comparisons
//!   where the value side is a method call (Send with receiver) are skipped.
//!   ComparisonsThreshold (default 2) sets the minimum number of comparisons
//!   to trigger.
//!   Autocorrect replaces the entire or-chain with `[v1, v2, ...].include?(a)`.
//!   Both `a == b` and `b == a` forms are accepted.
//!   Two-variable comparisons like `a == b` (no literal) are not flagged.
//!   Murphy parity note: in Murphy's AST, a local variable `a` that has been
//!   assigned is `(lvar a)`, while an ambiguous bare name is `(send :a nil)`.
//!   Both are treated as "variable" for this cop's purposes, matching RuboCop's
//!   treatment of both `lvar` and plain-call forms.
//! ```
//!
//! ## Matched shapes
//!
//! `Or` nodes at the root of a chain where:
//! - All branches are equality comparisons (`==`)
//! - All comparisons share the same variable on one side (lvar or bare send with no receiver/no args)
//! - The other side is not a variable-like node
//! - At least `ComparisonsThreshold` comparisons exist
//! - If `AllowMethodComparison: true`, no method-call values (send with receiver) are present
//!
//! ## Autocorrect
//!
//! Replaces the entire or-chain with `[v1, v2, ...].include?(variable)`.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

const MSG: &str = "Avoid comparing a variable with multiple items in a conditional, use `Array#include?` instead.";

#[derive(CopOptions)]
pub struct MultipleComparisonOptions {
    #[option(
        name = "AllowMethodComparison",
        default = true,
        description = "When true, comparisons involving method calls on the value side are ignored."
    )]
    pub allow_method_comparison: bool,

    #[option(
        name = "ComparisonsThreshold",
        default = 2,
        description = "Minimum number of comparisons required to trigger an offense."
    )]
    pub comparisons_threshold: i64,
}

#[derive(Default)]
pub struct MultipleComparison;

#[cop(
    name = "Style/MultipleComparison",
    description = "Avoid comparing a variable with multiple items in a conditional, use `Array#include?` instead.",
    default_severity = "warning",
    default_enabled = true,
    options = MultipleComparisonOptions,
)]
impl MultipleComparison {
    #[on_node(kind = "or")]
    fn check_or(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Only process the root of an or-chain (not sub-nodes).
    if is_or_child(node, cx) {
        return;
    }

    // Must be a chain of nested comparisons.
    if !nested_comparison(node, cx) {
        return;
    }

    let opts = cx.options_or_default::<MultipleComparisonOptions>();

    // Collect the offending variable and compared values.
    let Some((variable, values)) = find_offending_var(node, cx, &opts) else {
        return;
    };

    let threshold = opts.comparisons_threshold.max(1) as usize;
    if values.len() < threshold {
        return;
    }

    // Offense range is the entire root or-node.
    let offense_range = cx.range(node);
    cx.emit_offense(offense_range, MSG, None);

    // Autocorrect: build `[v1, v2, ...].include?(variable)`.
    let elements: Vec<&str> = values
        .iter()
        .map(|&id| cx.raw_source(cx.range(id)))
        .collect();
    let argument = cx.raw_source(cx.range(variable));
    let replacement = format!("[{}].include?({})", elements.join(", "), argument);
    cx.emit_edit(offense_range, &replacement);
}

/// Returns `true` if `node` is a direct child of another `or` node
/// (i.e., it is not the root of the or-chain).
fn is_or_child(node: NodeId, cx: &Cx<'_>) -> bool {
    if let Some(parent) = cx.parent(node).get() {
        matches!(cx.kind(parent), NodeKind::Or { .. })
    } else {
        false
    }
}

/// Returns `true` if every operand (recursively) of an or-chain is
/// either another or-node or a simple comparison.
fn nested_comparison(node: NodeId, cx: &Cx<'_>) -> bool {
    match cx.kind(node) {
        NodeKind::Or { lhs, rhs } => {
            let (lhs, rhs) = (*lhs, *rhs);
            comparison_or_nested(lhs, cx) && comparison_or_nested(rhs, cx)
        }
        _ => false,
    }
}

fn comparison_or_nested(node: NodeId, cx: &Cx<'_>) -> bool {
    simple_comparison(node, cx).is_some() || nested_comparison(node, cx)
}

/// Returns `true` if `node` is "variable-like" for this cop.
///
/// Covers:
/// - `(lvar :name)` — a definite local variable (parsed as lvar after assignment)
/// - `(send :name nil)` — bare name with no receiver and no args (ambiguous local/method)
///
/// Excludes method calls with a receiver (`b.foo`) or with arguments.
fn is_variable_like(node: NodeId, cx: &Cx<'_>) -> bool {
    match cx.kind(node) {
        NodeKind::Lvar(_) => true,
        NodeKind::Send { receiver, args, .. } => {
            // Bare send with no receiver and no args is "variable-like" (ambiguous name).
            receiver.get().is_none() && cx.list(*args).is_empty()
        }
        _ => false,
    }
}

/// Returns `true` if `node` is a method call (`Send` or `Csend` with a receiver,
/// or `Send` with arguments).
fn is_method_call(node: NodeId, cx: &Cx<'_>) -> bool {
    match cx.kind(node) {
        NodeKind::Send { receiver, args, .. } => {
            receiver.get().is_some() || !cx.list(*args).is_empty()
        }
        NodeKind::Csend { .. } => true,
        _ => false,
    }
}

/// Attempt to extract `(variable, compared_value)` from a simple `==`
/// comparison. Returns `None` for anything that is not a clean `x == y`
/// or `y == x` shape.
///
/// - `(variable) == anything_non_variable` → variable = left, value = right
/// - `anything_non_variable == (variable)` → variable = right, value = left
/// - both sides variable-like → `None` (skip, like RuboCop's simple_double_comparison)
/// - neither side variable-like → `None`
fn simple_comparison(node: NodeId, cx: &Cx<'_>) -> Option<(NodeId, NodeId)> {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(node)
    else {
        return None;
    };

    // Must be the `==` method.
    if cx.symbol_str(method) != "==" {
        return None;
    }

    let recv_id = receiver.get()?;
    let arg_list = cx.list(args);
    if arg_list.len() != 1 {
        return None;
    }
    let arg_id = arg_list[0];

    let recv_is_var = is_variable_like(recv_id, cx);
    let arg_is_var = is_variable_like(arg_id, cx);

    // Both sides are variable-like — skip (simple_double_comparison in RuboCop).
    if recv_is_var && arg_is_var {
        return None;
    }

    if recv_is_var {
        Some((recv_id, arg_id))
    } else if arg_is_var {
        Some((arg_id, recv_id))
    } else {
        None
    }
}

/// Walk the or-chain and collect the (variable, values) pair.
/// Returns `None` if the chain has more than one distinct variable,
/// or if any comparison fails `simple_comparison`.
fn find_offending_var(
    node: NodeId,
    cx: &Cx<'_>,
    opts: &MultipleComparisonOptions,
) -> Option<(NodeId, Vec<NodeId>)> {
    let mut variable: Option<NodeId> = None;
    let mut values: Vec<NodeId> = Vec::new();
    if collect_comparisons(node, cx, opts, &mut variable, &mut values) {
        Some((variable?, values))
    } else {
        None
    }
}

/// Returns `true` if successful, `false` if the chain is invalid
/// (multiple variables, a non-comparison, or method call when forbidden).
fn collect_comparisons(
    node: NodeId,
    cx: &Cx<'_>,
    opts: &MultipleComparisonOptions,
    variable: &mut Option<NodeId>,
    values: &mut Vec<NodeId>,
) -> bool {
    match cx.kind(node) {
        NodeKind::Or { lhs, rhs } => {
            let (lhs, rhs) = (*lhs, *rhs);
            collect_comparisons(lhs, cx, opts, variable, values)
                && collect_comparisons(rhs, cx, opts, variable, values)
        }
        _ => {
            let Some((var, val)) = simple_comparison(node, cx) else {
                return false;
            };
            // If AllowMethodComparison is true, skip when value is a method call.
            if opts.allow_method_comparison && is_method_call(val, cx) {
                return false;
            }
            // Check that the variable matches any previously seen variable.
            if let Some(seen_var) = *variable {
                if !same_variable(seen_var, var, cx) {
                    return false;
                }
            } else {
                *variable = Some(var);
            }
            values.push(val);
            true
        }
    }
}

/// Returns `true` if `a` and `b` refer to the same variable-like expression.
/// Compares by source name (both lvar and bare-send share the name as symbol).
fn same_variable(a: NodeId, b: NodeId, cx: &Cx<'_>) -> bool {
    let sym_of = |id: NodeId| -> Option<murphy_plugin_api::Symbol> {
        match *cx.kind(id) {
            NodeKind::Lvar(sym) => Some(sym),
            NodeKind::Send {
                receiver,
                method,
                args,
            } if receiver.get().is_none() && cx.list(args).is_empty() => Some(method),
            _ => None,
        }
    };
    match (sym_of(a), sym_of(b)) {
        (Some(sym_a), Some(sym_b)) => sym_a == sym_b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{MultipleComparison, MultipleComparisonOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_two_comparisons() {
        test::<MultipleComparison>().expect_correction(
            indoc! {r#"
                foo if a == 'a' || a == 'b'
                       ^^^^^^^^^^^^^^^^^^^^ Avoid comparing a variable with multiple items in a conditional, use `Array#include?` instead.
            "#},
            "foo if ['a', 'b'].include?(a)\n",
        );
    }

    #[test]
    fn flags_three_comparisons() {
        test::<MultipleComparison>().expect_correction(
            indoc! {r#"
                foo if a == 'a' || a == 'b' || a == 'c'
                       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid comparing a variable with multiple items in a conditional, use `Array#include?` instead.
            "#},
            "foo if ['a', 'b', 'c'].include?(a)\n",
        );
    }

    #[test]
    fn flags_rhs_variable() {
        // Variable on right-hand side of ==
        test::<MultipleComparison>().expect_correction(
            indoc! {r#"
                foo if 'a' == a || 'b' == a
                       ^^^^^^^^^^^^^^^^^^^^ Avoid comparing a variable with multiple items in a conditional, use `Array#include?` instead.
            "#},
            "foo if ['a', 'b'].include?(a)\n",
        );
    }

    #[test]
    fn accepts_single_comparison() {
        test::<MultipleComparison>().expect_no_offenses("foo if a == 'a'\n");
    }

    #[test]
    fn accepts_different_variables() {
        test::<MultipleComparison>().expect_no_offenses("foo if a == 1 || b == 2\n");
    }

    #[test]
    fn accepts_double_variable_comparison() {
        // both sides are variable-like — skip.
        test::<MultipleComparison>().expect_no_offenses("foo if a == b || a == c\n");
    }

    #[test]
    fn accepts_method_comparison_by_default() {
        // AllowMethodComparison: true (default) — skip method calls on value side.
        test::<MultipleComparison>()
            .expect_no_offenses("foo if a == b.lightweight || a == b.heavyweight\n");
    }

    #[test]
    fn flags_method_comparison_when_disabled() {
        test::<MultipleComparison>()
            .with_options(&MultipleComparisonOptions {
                allow_method_comparison: false,
                comparisons_threshold: 2,
            })
            .expect_offense(indoc! {r#"
                foo if a == b.lightweight || a == b.heavyweight
                       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid comparing a variable with multiple items in a conditional, use `Array#include?` instead.
            "#});
    }

    #[test]
    fn accepts_below_threshold() {
        // ComparisonsThreshold: 3 — two comparisons are ok.
        test::<MultipleComparison>()
            .with_options(&MultipleComparisonOptions {
                allow_method_comparison: true,
                comparisons_threshold: 3,
            })
            .expect_no_offenses("foo if a == 'a' || a == 'b'\n");
    }

    #[test]
    fn flags_at_threshold() {
        test::<MultipleComparison>()
            .with_options(&MultipleComparisonOptions {
                allow_method_comparison: true,
                comparisons_threshold: 3,
            })
            .expect_offense(indoc! {r#"
                foo if a == 'a' || a == 'b' || a == 'c'
                       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid comparing a variable with multiple items in a conditional, use `Array#include?` instead.
            "#});
    }

    #[test]
    fn flags_assigned_lvar() {
        // When `a` is assigned before use, it is a proper `lvar` node.
        test::<MultipleComparison>().expect_correction(
            indoc! {r#"
                a = 1
                foo if a == 1 || a == 2
                       ^^^^^^^^^^^^^^^^ Avoid comparing a variable with multiple items in a conditional, use `Array#include?` instead.
            "#},
            "a = 1\nfoo if [1, 2].include?(a)\n",
        );
    }

    #[test]
    fn negative_threshold_treated_as_one() {
        // A negative ComparisonsThreshold must not disable the cop — it is
        // normalized to 1, so any chain of 1+ comparisons is flagged.
        test::<MultipleComparison>()
            .with_options(&MultipleComparisonOptions {
                allow_method_comparison: true,
                comparisons_threshold: -1,
            })
            .expect_offense(indoc! {r#"
                foo if a == 'a' || a == 'b'
                       ^^^^^^^^^^^^^^^^^^^^ Avoid comparing a variable with multiple items in a conditional, use `Array#include?` instead.
            "#});
    }
}

murphy_plugin_api::submit_cop!(MultipleComparison);
