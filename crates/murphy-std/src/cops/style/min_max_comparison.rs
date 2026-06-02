//! `Style/MinMaxComparison` — flags ternary/if comparisons that can be
//! replaced with `[a, b].max` or `[a, b].min`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/MinMaxComparison
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Murphy v1 handles the common cases: ternary and standard if/else forms.
//!   Gaps vs RuboCop:
//!   - elsif chain handling is not implemented (the corrector branch that
//!     removes an intermediate elsif keyword). Offenses on elsif nodes
//!     are detected but the autocorrect is skipped.
//!   @safety: this cop is marked unsafe in RuboCop (not necessarily Comparable).
//! ```
//!
//! ## Matched shapes
//!
//! `if` (and ternary) nodes where:
//! - The condition is a `send` with method `>`, `>=`, `<`, or `<=`
//! - The if-branch matches one operand and the else-branch matches the other
//!
//! Example patterns:
//! - `a > b ? a : b` -> `[a, b].max`
//! - `a >= b ? a : b` -> `[a, b].max`
//! - `a < b ? a : b` -> `[a, b].min`
//! - `a <= b ? a : b` -> `[a, b].min`
//!
//! ## Autocorrect
//!
//! Whole-node replacement with `[lhs, rhs].max` or `[lhs, rhs].min`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

const MSG: &str = "Use `%<prefer>s` instead.";

/// Stateless unit struct.
#[derive(Default)]
pub struct MinMaxComparison;

#[cop(
    name = "Style/MinMaxComparison",
    description = "Use `[a, b].max` or `[a, b].min` instead of comparison.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl MinMaxComparison {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// Returns `Some((lhs_id, operator, rhs_id))` if the condition is a
/// comparison send with `>`, `>=`, `<`, or `<=`.
fn match_comparison_condition(
    cond: NodeId,
    cx: &Cx<'_>,
) -> Option<(NodeId, &'static str, NodeId)> {
    let NodeKind::Send { receiver, method, args } = *cx.kind(cond) else {
        return None;
    };
    let lhs = receiver.get()?;
    let op = match cx.symbol_str(method) {
        ">" => ">",
        ">=" => ">=",
        "<" => "<",
        "<=" => "<=",
        _ => return None,
    };
    let arg_list = cx.list(args);
    if arg_list.len() != 1 {
        return None;
    }
    Some((lhs, op, arg_list[0]))
}

/// Returns `Some("max")` or `Some("min")` based on which operand goes
/// where in the branches. Returns `None` if the pattern does not match.
fn preferred_method(
    operator: &str,
    lhs: NodeId,
    rhs: NodeId,
    if_branch: NodeId,
    else_branch: NodeId,
    cx: &Cx<'_>,
) -> Option<&'static str> {
    let lhs_src = cx.raw_source(cx.range(lhs));
    let rhs_src = cx.raw_source(cx.range(rhs));
    let if_src = cx.raw_source(cx.range(if_branch));
    let else_src = cx.raw_source(cx.range(else_branch));

    if lhs_src == if_src && rhs_src == else_src {
        // lhs OP rhs ? lhs : rhs
        match operator {
            ">" | ">=" => Some("max"),
            "<" | "<=" => Some("min"),
            _ => None,
        }
    } else if lhs_src == else_src && rhs_src == if_src {
        // lhs OP rhs ? rhs : lhs  (lhs in else, rhs in if)
        match operator {
            "<" | "<=" => Some("max"),
            ">" | ">=" => Some("min"),
            _ => None,
        }
    } else {
        None
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::If { cond, then_, else_ } = *cx.kind(node) else {
        return;
    };

    // Both branches must be present.
    let Some(if_branch) = then_.get() else {
        return;
    };
    let Some(else_branch) = else_.get() else {
        return;
    };

    // The condition must be a comparison send.
    let Some((lhs, operator, rhs)) = match_comparison_condition(cond, cx) else {
        return;
    };

    // Determine preferred method based on which operand appears in which branch.
    let Some(method) = preferred_method(operator, lhs, rhs, if_branch, else_branch, cx) else {
        return;
    };

    let lhs_src = cx.raw_source(cx.range(lhs));
    let rhs_src = cx.raw_source(cx.range(rhs));
    let replacement = format!("[{lhs_src}, {rhs_src}].{method}");
    let message = MSG.replace("%<prefer>s", &replacement);
    let node_range = cx.range(node);

    cx.emit_offense(node_range, &message, None);

    // Autocorrect: skip the elsif chain case (v1 gap).
    // For regular ternary/if-else, replace the whole node.
    if !cx.is_elsif(node) {
        cx.emit_edit(node_range, &replacement);
    }
}

#[cfg(test)]
mod tests {
    use super::MinMaxComparison;
    use murphy_plugin_api::test_support::{indoc, run_cop, test};

    // ----- Greater-than operators -> max (ternary, single-line) -----

    #[test]
    fn flags_ternary_gt() {
        test::<MinMaxComparison>().expect_correction(
            indoc! {"
                a > b ? a : b
                ^^^^^^^^^^^^^ Use `[a, b].max` instead.
            "},
            "[a, b].max\n",
        );
    }

    #[test]
    fn flags_ternary_gte() {
        test::<MinMaxComparison>().expect_correction(
            indoc! {"
                a >= b ? a : b
                ^^^^^^^^^^^^^^ Use `[a, b].max` instead.
            "},
            "[a, b].max\n",
        );
    }

    // ----- Less-than operators -> min (ternary, single-line) -----

    #[test]
    fn flags_ternary_lt() {
        test::<MinMaxComparison>().expect_correction(
            indoc! {"
                a < b ? a : b
                ^^^^^^^^^^^^^ Use `[a, b].min` instead.
            "},
            "[a, b].min\n",
        );
    }

    #[test]
    fn flags_ternary_lte() {
        test::<MinMaxComparison>().expect_correction(
            indoc! {"
                a <= b ? a : b
                ^^^^^^^^^^^^^^ Use `[a, b].min` instead.
            "},
            "[a, b].min\n",
        );
    }

    // ----- Inverted branch order (lhs in else, rhs in if) -----

    #[test]
    fn flags_ternary_lt_inverted_is_max() {
        // a < b ? b : a -> [a, b].max (b in if-branch, a in else-branch)
        test::<MinMaxComparison>().expect_correction(
            indoc! {"
                a < b ? b : a
                ^^^^^^^^^^^^^ Use `[a, b].max` instead.
            "},
            "[a, b].max\n",
        );
    }

    #[test]
    fn flags_ternary_gt_inverted_is_min() {
        // a > b ? b : a -> [a, b].min
        test::<MinMaxComparison>().expect_correction(
            indoc! {"
                a > b ? b : a
                ^^^^^^^^^^^^^ Use `[a, b].min` instead.
            "},
            "[a, b].min\n",
        );
    }

    // ----- if/else block form -----
    // Block-form if/else offense ranges span multiple lines. These tests use
    // run_cop directly to verify detection without the annotated format.

    #[test]
    fn detects_if_else_gt_block_form() {
        let offenses = run_cop::<MinMaxComparison>("if a > b\n  a\nelse\n  b\nend\n");
        assert_eq!(offenses.len(), 1);
        assert!(
            offenses[0].message.contains("[a, b].max"),
            "expected [a, b].max in message: {}",
            offenses[0].message
        );
    }

    #[test]
    fn detects_if_else_lt_block_form() {
        let offenses = run_cop::<MinMaxComparison>("if a < b\n  a\nelse\n  b\nend\n");
        assert_eq!(offenses.len(), 1);
        assert!(
            offenses[0].message.contains("[a, b].min"),
            "expected [a, b].min in message: {}",
            offenses[0].message
        );
    }

    // Verify the corrected form has no offenses (idempotency).
    #[test]
    fn corrected_max_has_no_offenses() {
        test::<MinMaxComparison>().expect_no_offenses("[a, b].max\n");
    }

    #[test]
    fn corrected_min_has_no_offenses() {
        test::<MinMaxComparison>().expect_no_offenses("[a, b].min\n");
    }

    // ----- Negative cases -----

    #[test]
    fn accepts_branches_do_not_match_operands() {
        test::<MinMaxComparison>().expect_no_offenses("a > b ? c : d\n");
    }

    #[test]
    fn accepts_no_else_branch() {
        test::<MinMaxComparison>().expect_no_offenses(indoc! {"
            if a > b
              a
            end
        "});
    }

    #[test]
    fn accepts_no_comparison_in_condition() {
        test::<MinMaxComparison>().expect_no_offenses("a == b ? a : b\n");
    }

    #[test]
    fn accepts_already_using_max() {
        test::<MinMaxComparison>().expect_no_offenses("[a, b].max\n");
    }

    // ----- Method-call receivers -----

    #[test]
    fn flags_method_call_operands() {
        test::<MinMaxComparison>().expect_correction(
            indoc! {"
                foo.bar > baz.qux ? foo.bar : baz.qux
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `[foo.bar, baz.qux].max` instead.
            "},
            "[foo.bar, baz.qux].max\n",
        );
    }
}

murphy_plugin_api::submit_cop!(MinMaxComparison);
