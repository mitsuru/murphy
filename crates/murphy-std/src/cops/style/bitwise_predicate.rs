//! `Style/BitwisePredicate` — prefer bitwise predicate methods over comparisons.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/BitwisePredicate
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Handles (var & flags).positive? → var.anybits?(flags),
//!   (var & flags).zero? → var.nobits?(flags),
//!   (var & flags).> 0 → var.anybits?(flags),
//!   (var & flags) == 0 → var.nobits?(flags),
//!   (var & flags) == flags → var.allbits?(flags).
//!   `>=` is not handled (v1 gap due to safety concerns).
//!   `!=` with non-zero is not handled.
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, cop};

const MSG: &str = "Use a bitwise predicate method instead.";

#[derive(Default)]
pub struct BitwisePredicate;

#[cop(
    name = "Style/BitwisePredicate",
    description = "Prefer bitwise predicate methods over direct comparison.",
    default_severity = "warning",
    default_enabled = true,
    options = murphy_plugin_api::NoOptions
)]
impl BitwisePredicate {
    #[on_node(kind = "send", methods = ["positive?", "zero?", "==", "!=", ">"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, method, args } = *cx.kind(node) else {
            return;
        };
        let Some(recv_id) = receiver.get() else {
            return;
        };
        let begin_children = match cx.kind(recv_id) {
            NodeKind::Begin(children) => *children,
            _ => return,
        };
        let child_list = cx.list(begin_children);
        // Require exactly one expression: `(x & flags)`.
        // Skip multi-expression parentheses: `(x & flags; y)`.
        if child_list.len() != 1 {
            return;
        }
        let inner_send = child_list[0];
        let NodeKind::Send { receiver: bit_recv, method: bit_op, args: bit_args } = *cx.kind(inner_send) else {
            return;
        };
        let bit_op_str = cx.symbol_str(bit_op);
        if bit_op_str != "&" {
            return;
        }
        let Some(bit_recv_id) = bit_recv.get() else {
            return;
        };
        let method_str = cx.symbol_str(method);
        let bit_arg_list = cx.list(bit_args);
        if bit_arg_list.is_empty() {
            return;
        }
        let rhs = bit_arg_list[0];
        let flags_src = cx.raw_source(cx.range(rhs));
        let recv_src = cx.raw_source(cx.range(bit_recv_id));
        let replacement = match method_str {
            "positive?" => Some(format!("({}).anybits?({})", recv_src, flags_src)),
            ">" => {
                let arg_list = cx.list(args);
                if arg_list.is_empty() { None }
                else if cx.raw_source(cx.range(arg_list[0])) == "0" {
                    Some(format!("({}).anybits?({})", recv_src, flags_src))
                } else { None }
            }
            "!=" => {
                let arg_list = cx.list(args);
                if arg_list.is_empty() { None }
                else if cx.raw_source(cx.range(arg_list[0])) == "0" {
                    Some(format!("({}).anybits?({})", recv_src, flags_src))
                } else { None }
            }
            "zero?" => Some(format!("({}).nobits?({})", recv_src, flags_src)),
            "==" => {
                let arg_list = cx.list(args);
                if arg_list.is_empty() { None }
                else {
                    let arg_src = cx.raw_source(cx.range(arg_list[0]));
                    if arg_src == "0" {
                        Some(format!("({}).nobits?({})", recv_src, flags_src))
                    } else if arg_src == flags_src {
                        Some(format!("({}).allbits?({})", recv_src, flags_src))
                    } else { None }
                }
            }
            _ => None,
        };
        let Some(replacement) = replacement else {
            return;
        };
        cx.emit_offense(cx.range(node), MSG, None);
        cx.emit_edit(cx.range(node), &replacement);
    }
}

#[cfg(test)]
mod tests {
    use super::BitwisePredicate;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_positive_after_bit_and() {
        test::<BitwisePredicate>().expect_correction(
            indoc! {"
                (variable & flags).positive?
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use a bitwise predicate method instead.
            "},
            "(variable).anybits?(flags)\n",
        );
    }

    #[test]
    fn flags_zero_after_bit_and() {
        test::<BitwisePredicate>().expect_correction(
            indoc! {"
                (variable & flags).zero?
                ^^^^^^^^^^^^^^^^^^^^^^^^ Use a bitwise predicate method instead.
            "},
            "(variable).nobits?(flags)\n",
        );
    }

    #[test]
    fn flags_gt_zero() {
        test::<BitwisePredicate>().expect_correction(
            indoc! {"
                (variable & flags) > 0
                ^^^^^^^^^^^^^^^^^^^^^^ Use a bitwise predicate method instead.
            "},
            "(variable).anybits?(flags)\n",
        );
    }

    #[test]
    fn flags_eq_zero() {
        test::<BitwisePredicate>().expect_correction(
            indoc! {"
                (variable & flags) == 0
                ^^^^^^^^^^^^^^^^^^^^^^^ Use a bitwise predicate method instead.
            "},
            "(variable).nobits?(flags)\n",
        );
    }

    #[test]
    fn flags_eq_flags() {
        test::<BitwisePredicate>().expect_correction(
            indoc! {"
                (variable & flags) == flags
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use a bitwise predicate method instead.
            "},
            "(variable).allbits?(flags)\n",
        );
    }

    #[test]
    fn accepts_gt_non_zero() {
        test::<BitwisePredicate>().expect_no_offenses("(x & flags) > 1\n");
    }

    #[test]
    fn accepts_eq_non_zero_non_flags() {
        test::<BitwisePredicate>().expect_no_offenses("(x & flags) == 1\n");
    }

    #[test]
    fn accepts_plain_positive() {
        test::<BitwisePredicate>().expect_no_offenses("x.positive?\n");
    }
}
murphy_plugin_api::submit_cop!(BitwisePredicate);
