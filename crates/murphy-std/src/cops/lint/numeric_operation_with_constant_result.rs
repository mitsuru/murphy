//! `Lint/NumericOperationWithConstantResult` — flags numeric operations whose
//! result is always the same constant value.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/NumericOperationWithConstantResult
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   All RuboCop parity items verified: operator-style calls, dot-calls,
//!   abbreviated assignments, safe-navigation exclusion, and non-matching
//!   operator exclusion (`-`, `%`).
//! ```
//!
//! ## Matched shapes
//!
//! - `x * 0` — multiplication by zero, constant 0.
//! - `x ** 0` — exponentiation by zero, constant 1.
//! - `x / x` — division by self, constant 1.
//! - `x.*(0)`, `x.**(0)`, `x./(x)` — dot-call form.
//! - `x *= 0`, `x **= 0`, `x /= x` — abbreviated assignments.
//!
//! ## Autocorrect
//!
//! - `x * 0` → `0`
//! - `x ** 0` → `1`
//! - `x / x` → `1`
//! - `x *= 0` → `x = 0`
//! - `x **= 0` → `x = 1`
//! - `x /= x` → `x = 1`

use murphy_plugin_api::{Cx, NodeId, NodeKind, NoOptions, OptNodeId, cop};

#[derive(Default)]
pub struct NumericOperationWithConstantResult;

#[cop(
    name = "Lint/NumericOperationWithConstantResult",
    description = "Flags numeric operations whose result is always the same constant.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl NumericOperationWithConstantResult {
    #[on_node(kind = "send", methods = ["*", "/", "**"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, method, args } = *cx.kind(node) else {
            return;
        };
        let Some(recv_id) = receiver.get() else {
            return;
        };
        if !is_variable_or_implicit_call(recv_id, cx) {
            return;
        }
        let args_list = cx.list(args);
        if args_list.len() != 1 {
            return;
        }
        let method_str = cx.symbol_str(method);
        let lhs_name = cx.raw_source(cx.range(recv_id));
        if let Some(result) = constant_result(method_str, &lhs_name, args_list[0], cx) {
            let msg = "Numeric operation with a constant result detected.";
            cx.emit_offense(cx.range(node), msg, None);
            let replacement = result.to_string();
            cx.emit_edit(cx.range(node), &replacement);
        }
    }

    #[on_node(kind = "op_asgn")]
    fn check_op_asgn(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::OpAsgn { target, op, value } = *cx.kind(node) else {
            return;
        };
        let op_str = cx.symbol_str(op);
        if !["*", "/", "**"].contains(&op_str) {
            return;
        }
        let NodeKind::Lvasgn { name, value: _ } = *cx.kind(target) else {
            return;
        };
        let lhs_name = cx.symbol_str(name);
        if let Some(result) = constant_result_for_op_asgn(op_str, lhs_name, value, cx) {
            let msg = "Numeric operation with a constant result detected.";
            cx.emit_offense(cx.range(node), msg, None);
            let replacement = format!("{lhs_name} = {result}");
            cx.emit_edit(cx.range(node), &replacement);
        }
    }
}

fn is_variable_or_implicit_call(node: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Lvar(_) => true,
        NodeKind::Send {
            receiver, args, ..
        } if receiver == OptNodeId::NONE => {
            let args_list = cx.list(args);
            args_list.is_empty()
        }
        _ => false,
    }
}

fn constant_result(op: &str, lhs_name: &str, rhs: NodeId, cx: &Cx<'_>) -> Option<i64> {
    match *cx.kind(rhs) {
        NodeKind::Int(0) => match op {
            "*" => Some(0),
            "**" => Some(1),
            _ => None,
        },
        _ => {
            if is_variable_or_implicit_call(rhs, cx) {
                let rhs_name = cx.raw_source(cx.range(rhs));
                if rhs_name == lhs_name && op == "/" {
                    Some(1)
                } else {
                    None
                }
            } else {
                None
            }
        }
    }
}

fn constant_result_for_op_asgn(op: &str, lhs_name: &str, rhs: NodeId, cx: &Cx<'_>) -> Option<i64> {
    match *cx.kind(rhs) {
        NodeKind::Int(0) => match op {
            "*" => Some(0),
            "**" => Some(1),
            _ => None,
        },
        NodeKind::Lvar(name) => {
            let rhs_name = cx.symbol_str(name);
            if rhs_name == lhs_name && op == "/" {
                Some(1)
            } else {
                None
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::NumericOperationWithConstantResult;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_x_times_0() {
        test::<NumericOperationWithConstantResult>().expect_correction(
            indoc! {r#"
                x * 0
                ^^^^^ Numeric operation with a constant result detected.
            "#},
            "0\n",
        );
    }

    #[test]
    fn flags_x_divided_by_x() {
        test::<NumericOperationWithConstantResult>().expect_correction(
            indoc! {r#"
                x / x
                ^^^^^ Numeric operation with a constant result detected.
            "#},
            "1\n",
        );
    }

    #[test]
    fn flags_x_power_0() {
        test::<NumericOperationWithConstantResult>().expect_correction(
            indoc! {r#"
                x ** 0
                ^^^^^^ Numeric operation with a constant result detected.
            "#},
            "1\n",
        );
    }

    #[test]
    fn flags_x_times_0_abbrev() {
        test::<NumericOperationWithConstantResult>().expect_correction(
            indoc! {r#"
                x *= 0
                ^^^^^^ Numeric operation with a constant result detected.
            "#},
            "x = 0\n",
        );
    }

    #[test]
    fn flags_x_divided_by_x_abbrev() {
        test::<NumericOperationWithConstantResult>().expect_correction(
            indoc! {r#"
                x /= x
                ^^^^^^ Numeric operation with a constant result detected.
            "#},
            "x = 1\n",
        );
    }

    #[test]
    fn flags_x_power_0_abbrev() {
        test::<NumericOperationWithConstantResult>().expect_correction(
            indoc! {r#"
                x **= 0
                ^^^^^^^ Numeric operation with a constant result detected.
            "#},
            "x = 1\n",
        );
    }

    #[test]
    fn does_not_flag_other_operators() {
        test::<NumericOperationWithConstantResult>()
            .expect_no_offenses("x - x\n")
            .expect_no_offenses("x -= x\n")
            .expect_no_offenses("x % x\n")
            .expect_no_offenses("x %= x\n")
            .expect_no_offenses("x % 1\n")
            .expect_no_offenses("x %= 1\n");
    }

    #[test]
    fn flags_x_dot_times_0() {
        test::<NumericOperationWithConstantResult>().expect_correction(
            indoc! {r#"
                x.*(0)
                ^^^^^^ Numeric operation with a constant result detected.
            "#},
            "0\n",
        );
    }

    #[test]
    fn flags_x_dot_divide_x() {
        test::<NumericOperationWithConstantResult>().expect_correction(
            indoc! {r#"
                x./(x)
                ^^^^^^ Numeric operation with a constant result detected.
            "#},
            "1\n",
        );
    }

    #[test]
    fn does_not_flag_safe_navigation() {
        test::<NumericOperationWithConstantResult>()
            .expect_no_offenses("x&.*(0)\n")
            .expect_no_offenses("x&.**(0)\n")
            .expect_no_offenses("x&./(x)\n");
    }

    #[test]
    fn does_not_flag_different_variable() {
        test::<NumericOperationWithConstantResult>().expect_no_offenses("x / y\n");
    }

    #[test]
    fn does_not_flag_non_variable_receiver() {
        test::<NumericOperationWithConstantResult>().expect_no_offenses("foo.bar * 0\n");
    }

    #[test]
    fn does_not_flag_literal_receiver() {
        test::<NumericOperationWithConstantResult>().expect_no_offenses("0 * x\n");
    }

    #[test]
    fn does_not_flag_non_constant_operation() {
        test::<NumericOperationWithConstantResult>()
            .expect_no_offenses("x * 2\n")
            .expect_no_offenses("x * y\n");
    }
}

murphy_plugin_api::submit_cop!(NumericOperationWithConstantResult);
