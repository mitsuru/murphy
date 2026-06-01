//! `Style/YodaExpression` — forbids yoda expressions in binary operations.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/YodaExpression
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Disabled by default (matches RuboCop's Enabled: false).
//!   Checks operators *, +, &, |, ^ (configurable via SupportedOperators).
//!   constant_portion? is Int/Float/Rational/Complex/Const (RuboCop: type?(:numeric, :const)).
//!   Strings/symbols/booleans/nil are NOT constant for this cop (unlike YodaCondition).
//!   offended_ancestor? is implemented by walking cx.ancestors() and checking
//!   if any ancestor Send node was already offended; tracks offended NodeIds in
//!   a local HashSet for the file investigation.
//!   Autocorrect swaps lhs and rhs via whole-node interpolation.
//! ```
//!
//! Flags binary operations (using `*`, `+`, `&`, `|`, `^`) where the order of
//! expression is reversed: a numeric literal or constant appears on the left
//! when a non-constant appears on the right. Example: `1 + x` should be `x + 1`.
//!
//! ## Matched shapes
//!
//! `Send` nodes with one of the configured operator methods, where lhs is
//! a numeric literal or Const, and rhs is not.
//!
//! ## Autocorrect
//!
//! Swaps lhs and rhs via whole-node replacement (`rhs op lhs`).

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};
use std::collections::HashSet;

/// Stateless unit struct.
#[derive(Default)]
pub struct YodaExpression;

#[derive(CopOptions)]
pub struct YodaExpressionOptions {
    #[option(
        name = "SupportedOperators",
        default = ["*", "+", "&", "|", "^"],
        description = "Operators to check for yoda expressions."
    )]
    pub supported_operators: Vec<String>,
}

const MSG_TEMPLATE: &str = "Non-literal operand (`{}`) should be first.";

#[cop(
    name = "Style/YodaExpression",
    description = "Forbid the use of yoda expressions.",
    default_severity = "warning",
    default_enabled = false,
    options = YodaExpressionOptions,
)]
impl YodaExpression {
    #[on_new_investigation]
    fn investigate(&self, cx: &Cx<'_>, opts: &YodaExpressionOptions) {
        let mut offended: HashSet<u32> = HashSet::new();
        // Walk all descendants in source order (pre-order) to mirror
        // RuboCop's on_send dispatch order.
        for node in std::iter::once(cx.root()).chain(cx.descendants(cx.root())) {
            check_node(node, cx, opts, &mut offended);
        }
    }
}

fn check_node(
    node: NodeId,
    cx: &Cx<'_>,
    opts: &YodaExpressionOptions,
    offended: &mut HashSet<u32>,
) {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(node)
    else {
        return;
    };

    let method_name = cx.symbol_str(method);

    // Only check configured operators.
    if !opts.supported_operators.iter().any(|op| op == method_name) {
        return;
    }

    let Some(lhs_id) = receiver.get() else {
        return;
    };

    let arg_list = cx.list(args);
    if arg_list.len() != 1 {
        return;
    }
    let rhs_id = arg_list[0];

    // Yoda expression: lhs is constant, rhs is not.
    if !is_yoda_expression_constant(lhs_id, cx) {
        return;
    }
    if is_yoda_expression_constant(rhs_id, cx) {
        return;
    }

    // Skip if any ancestor Send node was already offended (avoids double-firing
    // on nested arithmetic like `1 + 2 * x` where outer `+` already offended).
    if offended_ancestor(node, cx, offended) {
        return;
    }

    let node_range = cx.range(node);
    let rhs_src = cx.raw_source(cx.range(rhs_id));
    let msg = MSG_TEMPLATE.replacen("{}", rhs_src, 1);
    cx.emit_offense(node_range, &msg, None);

    // Autocorrect: swap lhs and rhs.
    let lhs_src = cx.raw_source(cx.range(lhs_id)).to_owned();
    let rhs_src_owned = rhs_src.to_owned();
    let replacement = format!("{rhs_src_owned} {method_name} {lhs_src}");
    cx.emit_edit(node_range, &replacement);

    offended.insert(node.0);
}

/// `constant_portion?` for YodaExpression: only numerics (Int, Float, Rational,
/// Complex) and Const nodes. Does NOT include string/sym/bool/nil (unlike
/// YodaCondition's broader check).
fn is_yoda_expression_constant(id: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        cx.kind(id),
        NodeKind::Int(_)
            | NodeKind::Float(_)
            | NodeKind::Rational(_)
            | NodeKind::Complex(_)
            | NodeKind::Const { .. }
    )
}

/// Returns true if any ancestor Send node of `node` is in `offended`.
fn offended_ancestor(node: NodeId, cx: &Cx<'_>, offended: &HashSet<u32>) -> bool {
    for ancestor in cx.ancestors(node) {
        if matches!(cx.kind(ancestor), NodeKind::Send { .. }) && offended.contains(&ancestor.0) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{YodaExpression, YodaExpressionOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- basic offenses -----

    #[test]
    fn flags_yoda_mul() {
        test::<YodaExpression>().expect_correction(
            indoc! {"
                10 * y
                ^^^^^^ Non-literal operand (`y`) should be first.
            "},
            "y * 10\n",
        );
    }

    #[test]
    fn flags_yoda_add() {
        test::<YodaExpression>().expect_correction(
            indoc! {"
                1 + x
                ^^^^^ Non-literal operand (`x`) should be first.
            "},
            "x + 1\n",
        );
    }

    #[test]
    fn flags_yoda_bitand() {
        test::<YodaExpression>().expect_correction(
            indoc! {"
                1 & z
                ^^^^^ Non-literal operand (`z`) should be first.
            "},
            "z & 1\n",
        );
    }

    #[test]
    fn flags_yoda_bitor() {
        test::<YodaExpression>().expect_correction(
            indoc! {"
                1 | x
                ^^^^^ Non-literal operand (`x`) should be first.
            "},
            "x | 1\n",
        );
    }

    #[test]
    fn flags_yoda_xor() {
        test::<YodaExpression>().expect_correction(
            indoc! {"
                1 ^ x
                ^^^^^ Non-literal operand (`x`) should be first.
            "},
            "x ^ 1\n",
        );
    }

    #[test]
    fn accepts_const_plus_const() {
        // Both are constant_portion? -> no offense.
        // 1 is numeric, CONST is a const node: both satisfy constant_portion?,
        // so yoda_expression_constant?(1, CONST) = false (rhs IS constant).
        test::<YodaExpression>().expect_no_offenses(
            "1 + CONST
",
        );
    }

    // ----- valid cases -----

    #[test]
    fn accepts_var_plus_literal() {
        test::<YodaExpression>().expect_no_offenses("x + 1\n");
    }

    #[test]
    fn accepts_both_literals() {
        // both constant -> no offense
        test::<YodaExpression>().expect_no_offenses("60 * 24\n");
    }

    #[test]
    fn accepts_both_variables() {
        test::<YodaExpression>().expect_no_offenses("x + y\n");
    }

    #[test]
    fn accepts_string_lhs() {
        // strings are not "constant" for this cop
        test::<YodaExpression>().expect_no_offenses("\"a\" + x\n");
    }

    // ----- nested arithmetic: ancestor check -----

    #[test]
    fn nested_inner_skipped_when_outer_offended() {
        // `1 + 2 * x`: outer `+` fires (lhs=1 const, rhs=`2*x` non-const).
        // inner `*` would also fire (lhs=2 const, rhs=x non-const), but
        // its ancestor `+` was already offended, so it should be skipped.
        test::<YodaExpression>().expect_offense(indoc! {"
            1 + 2 * x
            ^^^^^^^^^ Non-literal operand (`2 * x`) should be first.
        "});
    }

    // ----- SupportedOperators config -----

    #[test]
    fn custom_operators_only_checks_configured() {
        // only checking `*`, so `1 + x` should not fire
        test::<YodaExpression>()
            .with_options(&YodaExpressionOptions {
                supported_operators: vec!["*".to_string()],
            })
            .expect_no_offenses("1 + x\n");
    }

    #[test]
    fn custom_operators_fires_for_configured() {
        test::<YodaExpression>()
            .with_options(&YodaExpressionOptions {
                supported_operators: vec!["*".to_string()],
            })
            .expect_offense(indoc! {"
                10 * y
                ^^^^^^ Non-literal operand (`y`) should be first.
            "});
    }
}
