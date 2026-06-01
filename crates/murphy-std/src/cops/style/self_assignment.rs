//! `Style/SelfAssignment` — flags places where self-assignment shorthand
//! should have been used.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/SelfAssignment
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Handles lvasgn, ivasgn, cvasgn, and gvasgn nodes (casgn is not
//!   supported — constants do not have op-assignment shorthand in Ruby).
//!   Send-rhs ops: +, -, *, **, /, %, ^, <<, >>, |, &.
//!   Boolean-rhs ops: && (And node) and || (Or node).
//!   Autocorrect inserts the operator before `=` and replaces the RHS
//!   with the second operand.
//!   autocorrect_incompatible_with: Layout/SpaceAroundOperators (v1 gap).
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! x = x + 1
//! @x = @x - 1
//! @@x = @@x * 2
//! $x = $x / 2
//! x = x && y
//! x = x || y
//!
//! # good
//! x += 1
//! @x -= 1
//! @@x *= 2
//! $x /= 2
//! x &&= y
//! x ||= y
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, Symbol, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct SelfAssignment;

/// Binary operator methods that have corresponding op-assignment forms.
const OPS: &[&str] = &[
    "+", "-", "*", "**", "/", "%", "^", "<<", ">>", "|", "&",
];

#[cop(
    name = "Style/SelfAssignment",
    description = "Checks for places where self-assignment shorthand should have been used.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl SelfAssignment {
    #[on_node(kind = "lvasgn")]
    fn check_lvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, VarKind::Local, cx);
    }

    #[on_node(kind = "ivasgn")]
    fn check_ivasgn(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, VarKind::Instance, cx);
    }

    #[on_node(kind = "cvasgn")]
    fn check_cvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, VarKind::ClassVar, cx);
    }

    #[on_node(kind = "gvasgn")]
    fn check_gvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, VarKind::Global, cx);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VarKind {
    Local,
    Instance,
    ClassVar,
    Global,
}

fn check(node: NodeId, var_kind: VarKind, cx: &Cx<'_>) {
    // Get the LHS name symbol and RHS value from the assignment node.
    let (lhs_name, value_id) = match *cx.kind(node) {
        NodeKind::Lvasgn { name, value }
        | NodeKind::Ivasgn { name, value }
        | NodeKind::Cvasgn { name, value }
        | NodeKind::Gvasgn { name, value } => match value.get() {
            Some(v) => (name, v),
            None => return,
        },
        _ => return,
    };

    match *cx.kind(value_id) {
        NodeKind::Send { .. } => {
            let args = cx.call_arguments(value_id);
            // Must have exactly one argument (the second operand).
            if args.len() != 1 {
                return;
            }
            let Some(op) = cx.method_name(value_id) else {
                return;
            };
            if !OPS.contains(&op) {
                return;
            }
            // Receiver of the send must be the same variable as the LHS.
            let Some(recv_id) = cx.call_receiver(value_id).get() else {
                return;
            };
            if !var_matches(recv_id, lhs_name, var_kind, cx) {
                return;
            }
            let second_operand = args[0];
            let msg = format!("Use self-assignment shorthand `{op}=`.");
            cx.emit_offense(cx.range(node), &msg, None);
            emit_op_correction(node, value_id, op, second_operand, cx);
        }
        NodeKind::And { lhs, rhs } => {
            if !var_matches(lhs, lhs_name, var_kind, cx) {
                return;
            }
            let msg = format!("Use self-assignment shorthand `&&=`.");
            cx.emit_offense(cx.range(node), &msg, None);
            emit_boolean_correction(node, value_id, "&&", rhs, cx);
        }
        NodeKind::Or { lhs, rhs } => {
            if !var_matches(lhs, lhs_name, var_kind, cx) {
                return;
            }
            let msg = format!("Use self-assignment shorthand `||=`.");
            cx.emit_offense(cx.range(node), &msg, None);
            emit_boolean_correction(node, value_id, "||", rhs, cx);
        }
        _ => {}
    }
}

/// Returns `true` when `node` is the same variable kind/name as `lhs_name`.
fn var_matches(node: NodeId, lhs_name: Symbol, var_kind: VarKind, cx: &Cx<'_>) -> bool {
    match (var_kind, cx.kind(node)) {
        (VarKind::Local, NodeKind::Lvar(sym)) => *sym == lhs_name,
        (VarKind::Instance, NodeKind::Ivar(sym)) => *sym == lhs_name,
        (VarKind::ClassVar, NodeKind::Cvar(sym)) => *sym == lhs_name,
        (VarKind::Global, NodeKind::Gvar(sym)) => *sym == lhs_name,
        _ => false,
    }
}

/// Emit the autocorrect for a send-op pattern: `x = x op rhs` → `x op= rhs`.
///
/// Two edits:
/// 1. Insert `op` before the `=` token (e.g. `=` → `+=`).
/// 2. Replace the whole RHS send node with just the second operand.
fn emit_op_correction(
    asgn_node: NodeId,
    rhs_send: NodeId,
    op: &str,
    second_operand: NodeId,
    cx: &Cx<'_>,
) {
    let eq_range = find_eq_token(asgn_node, rhs_send, cx);
    if eq_range == Range::ZERO {
        return;
    }
    // Edit 1: insert operator before `=`, turning `=` into e.g. `+=`.
    cx.emit_edit(
        Range {
            start: eq_range.start,
            end: eq_range.start,
        },
        op,
    );
    // Edit 2: replace the full RHS with just the second operand source.
    cx.emit_edit(
        cx.range(rhs_send),
        cx.raw_source(cx.range(second_operand)),
    );
}

/// Emit the autocorrect for a boolean pattern: `x = x && rhs` → `x &&= rhs`.
fn emit_boolean_correction(
    asgn_node: NodeId,
    rhs_bool: NodeId,
    op: &str,
    rhs_operand: NodeId,
    cx: &Cx<'_>,
) {
    let eq_range = find_eq_token(asgn_node, rhs_bool, cx);
    if eq_range == Range::ZERO {
        return;
    }
    // Edit 1: insert operator before `=`.
    cx.emit_edit(
        Range {
            start: eq_range.start,
            end: eq_range.start,
        },
        op,
    );
    // Edit 2: replace the full RHS boolean expression with just the RHS operand.
    cx.emit_edit(
        cx.range(rhs_bool),
        cx.raw_source(cx.range(rhs_operand)),
    );
}

/// Find the `=` assignment token in the gap between the LHS name end and the
/// RHS value start.
fn find_eq_token(asgn_node: NodeId, rhs_node: NodeId, cx: &Cx<'_>) -> Range {
    let name_end = cx.node(asgn_node).loc.name.end;
    let rhs_start = cx.range(rhs_node).start;
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < name_end);
    for tok in &toks[idx..] {
        if tok.range.start >= rhs_start {
            break;
        }
        if tok.kind == SourceTokenKind::Other && cx.raw_source(tok.range) == "=" {
            return tok.range;
        }
    }
    Range::ZERO
}

#[cfg(test)]
mod tests {
    use super::SelfAssignment;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Local variables (send ops) -----

    #[test]
    fn flags_local_plus() {
        test::<SelfAssignment>().expect_offense(indoc! {"
            x = x + 1
            ^^^^^^^^^ Use self-assignment shorthand `+=`.
        "});
    }

    #[test]
    fn corrects_local_plus() {
        test::<SelfAssignment>().expect_correction(
            indoc! {"
                x = x + 1
                ^^^^^^^^^ Use self-assignment shorthand `+=`.
            "},
            "x += 1\n",
        );
    }

    #[test]
    fn flags_local_minus() {
        test::<SelfAssignment>().expect_offense(indoc! {"
            x = x - 1
            ^^^^^^^^^ Use self-assignment shorthand `-=`.
        "});
    }

    #[test]
    fn corrects_local_minus() {
        test::<SelfAssignment>().expect_correction(
            indoc! {"
                x = x - 1
                ^^^^^^^^^ Use self-assignment shorthand `-=`.
            "},
            "x -= 1\n",
        );
    }

    #[test]
    fn flags_local_multiply() {
        test::<SelfAssignment>().expect_offense(indoc! {"
            x = x * 2
            ^^^^^^^^^ Use self-assignment shorthand `*=`.
        "});
    }

    #[test]
    fn flags_local_power() {
        test::<SelfAssignment>().expect_offense(indoc! {"
            x = x ** 2
            ^^^^^^^^^^ Use self-assignment shorthand `**=`.
        "});
    }

    #[test]
    fn corrects_local_power() {
        test::<SelfAssignment>().expect_correction(
            indoc! {"
                x = x ** 2
                ^^^^^^^^^^ Use self-assignment shorthand `**=`.
            "},
            "x **= 2\n",
        );
    }

    #[test]
    fn flags_local_divide() {
        test::<SelfAssignment>().expect_offense(indoc! {"
            x = x / 2
            ^^^^^^^^^ Use self-assignment shorthand `/=`.
        "});
    }

    #[test]
    fn flags_local_modulo() {
        test::<SelfAssignment>().expect_offense(indoc! {"
            x = x % 2
            ^^^^^^^^^ Use self-assignment shorthand `%=`.
        "});
    }

    #[test]
    fn flags_local_xor() {
        test::<SelfAssignment>().expect_offense(indoc! {"
            x = x ^ 2
            ^^^^^^^^^ Use self-assignment shorthand `^=`.
        "});
    }

    #[test]
    fn flags_local_left_shift() {
        test::<SelfAssignment>().expect_offense(indoc! {"
            x = x << 1
            ^^^^^^^^^^ Use self-assignment shorthand `<<=`.
        "});
    }

    #[test]
    fn corrects_local_left_shift() {
        test::<SelfAssignment>().expect_correction(
            indoc! {"
                x = x << 1
                ^^^^^^^^^^ Use self-assignment shorthand `<<=`.
            "},
            "x <<= 1\n",
        );
    }

    #[test]
    fn flags_local_right_shift() {
        test::<SelfAssignment>().expect_offense(indoc! {"
            x = x >> 1
            ^^^^^^^^^^ Use self-assignment shorthand `>>=`.
        "});
    }

    #[test]
    fn flags_local_bitwise_or() {
        test::<SelfAssignment>().expect_offense(indoc! {"
            x = x | y
            ^^^^^^^^^ Use self-assignment shorthand `|=`.
        "});
    }

    #[test]
    fn flags_local_bitwise_and() {
        test::<SelfAssignment>().expect_offense(indoc! {"
            x = x & y
            ^^^^^^^^^ Use self-assignment shorthand `&=`.
        "});
    }

    // ----- Instance variables -----

    #[test]
    fn flags_instance_plus() {
        test::<SelfAssignment>().expect_offense(indoc! {"
            @x = @x + 1
            ^^^^^^^^^^^ Use self-assignment shorthand `+=`.
        "});
    }

    #[test]
    fn corrects_instance_plus() {
        test::<SelfAssignment>().expect_correction(
            indoc! {"
                @x = @x + 1
                ^^^^^^^^^^^ Use self-assignment shorthand `+=`.
            "},
            "@x += 1\n",
        );
    }

    // ----- Class variables -----

    #[test]
    fn flags_class_var_plus() {
        test::<SelfAssignment>().expect_offense(indoc! {"
            @@x = @@x + 1
            ^^^^^^^^^^^^^ Use self-assignment shorthand `+=`.
        "});
    }

    #[test]
    fn corrects_class_var_plus() {
        test::<SelfAssignment>().expect_correction(
            indoc! {"
                @@x = @@x + 1
                ^^^^^^^^^^^^^ Use self-assignment shorthand `+=`.
            "},
            "@@x += 1\n",
        );
    }

    // ----- Global variables -----

    #[test]
    fn flags_global_plus() {
        test::<SelfAssignment>().expect_offense(indoc! {"
            $x = $x + 1
            ^^^^^^^^^^^ Use self-assignment shorthand `+=`.
        "});
    }

    #[test]
    fn corrects_global_plus() {
        test::<SelfAssignment>().expect_correction(
            indoc! {"
                $x = $x + 1
                ^^^^^^^^^^^ Use self-assignment shorthand `+=`.
            "},
            "$x += 1\n",
        );
    }

    // ----- Boolean operators -----

    #[test]
    fn flags_logical_and() {
        test::<SelfAssignment>().expect_offense(indoc! {"
            x = x && y
            ^^^^^^^^^^ Use self-assignment shorthand `&&=`.
        "});
    }

    #[test]
    fn corrects_logical_and() {
        test::<SelfAssignment>().expect_correction(
            indoc! {"
                x = x && y
                ^^^^^^^^^^ Use self-assignment shorthand `&&=`.
            "},
            "x &&= y\n",
        );
    }

    #[test]
    fn flags_logical_or() {
        test::<SelfAssignment>().expect_offense(indoc! {"
            x = x || y
            ^^^^^^^^^^ Use self-assignment shorthand `||=`.
        "});
    }

    #[test]
    fn corrects_logical_or() {
        test::<SelfAssignment>().expect_correction(
            indoc! {"
                x = x || y
                ^^^^^^^^^^ Use self-assignment shorthand `||=`.
            "},
            "x ||= y\n",
        );
    }

    // ----- No offense cases -----

    #[test]
    fn accepts_already_shorthand_plus() {
        test::<SelfAssignment>().expect_no_offenses("x += 1\n");
    }

    #[test]
    fn accepts_different_var_on_rhs() {
        test::<SelfAssignment>().expect_no_offenses("x = y + 1\n");
    }

    #[test]
    fn accepts_non_binary_op_rhs() {
        test::<SelfAssignment>().expect_no_offenses("x = -x\n");
    }

    #[test]
    fn accepts_different_instance_var() {
        test::<SelfAssignment>().expect_no_offenses("@x = @y + 1\n");
    }

    #[test]
    fn accepts_logical_and_with_different_var() {
        test::<SelfAssignment>().expect_no_offenses("x = y && z\n");
    }

    #[test]
    fn accepts_multi_arg_send_on_rhs() {
        // x = x.foo(a, b) — more than one arg, not a simple binary op
        test::<SelfAssignment>().expect_no_offenses("x = x.foo(a, b)\n");
    }
}
murphy_plugin_api::submit_cop!(SelfAssignment);
