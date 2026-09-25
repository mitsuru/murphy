//! `Style/BitwisePredicate` — prefer bitwise predicate methods over comparisons.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/BitwisePredicate
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop RESTRICT_ON_SEND [!=, ==, >, >=, positive?, zero?] and the
//!   anybits?/allbits?/nobits? node matchers:
//!     anybits?: (x & y).positive?, (x & y) > 0, (x & y) >= 1, (x & y) != 0.
//!     allbits?: (x & y) == y and the swapped (y & x) == y form.
//!     nobits?: (x & y).zero?, (x & y) == 0.
//!   Integer checks use NodeKind::Int so 0x0/0b0/1 spellings also match.
//!   The receiver must be parenthesized (Begin), matching RuboCop's
//!   `node.receiver&.begin_type?` guard; bare `x & y == y` is ignored.
//!   Autocorrect paren-wraps the lhs receiver for safety
//!   (`(variable).anybits?(flags)`), where RuboCop emits `variable.anybits?(flags)`.
//!   The offense message interpolates the preferred replacement, matching RuboCop's
//!   `Replace with ... for comparison with bit flags.` format.
//!   Requires Ruby >= 2.5 (RuboCop minimum_target_ruby_version 2.5). Murphy
//!   does not gate on TargetRubyVersion; documented here only.
//!   Marked unsafe in RuboCop (receiver may not be Integer); Murphy has no
//!   cop-level safety metadata knob in v1.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! (variable & flags).positive?  # -> (variable).anybits?(flags)
//! (variable & flags) > 0        # -> (variable).anybits?(flags)
//! (variable & flags) >= 1       # -> (variable).anybits?(flags)
//! (variable & flags) != 0       # -> (variable).anybits?(flags)
//! (variable & flags).zero?      # -> (variable).nobits?(flags)
//! (variable & flags) == 0       # -> (variable).nobits?(flags)
//! (variable & flags) == flags   # -> (variable).allbits?(flags)
//! (flags & variable) == flags   # -> (variable).allbits?(flags)
//!
//! # good
//! variable.anybits?(flags)
//! variable.allbits?(flags)
//! variable.nobits?(flags)
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, cop};

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
    #[on_node(kind = "send", methods = ["positive?", "zero?", "==", "!=", ">", ">="])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, method, args } = *cx.kind(node) else {
            return;
        };
        let Some(recv_id) = receiver.get() else {
            return;
        };
        let Some(inner_send) = unwrap_parenthesized(recv_id, cx) else {
            return;
        };
        let NodeKind::Send { receiver: bit_recv, method: bit_op, args: bit_args } = *cx.kind(inner_send) else {
            return;
        };
        if cx.symbol_str(bit_op) != "&" {
            return;
        }
        let Some(bit_recv_id) = bit_recv.get() else {
            return;
        };
        let bit_arg_list = cx.list(bit_args);
        if bit_arg_list.len() != 1 {
            return;
        }
        let bit_rhs_id = bit_arg_list[0];
        let method_str = cx.symbol_str(method);
        let arg_list = cx.list(args);

        let lhs_src = cx.raw_source(cx.range(bit_recv_id));
        let rhs_src = cx.raw_source(cx.range(bit_rhs_id));

        // Returns (preferred_method, replacement) or None.
        let replacement: Option<String> = match method_str {
            "positive?" | "zero?" => {
                if !arg_list.is_empty() {
                    return;
                }
                let preferred = if method_str == "positive?" {
                    "anybits?"
                } else {
                    "nobits?"
                };
                Some(format!("({}).{}({})", lhs_src, preferred, rhs_src))
            }
            ">" | ">=" | "!=" => {
                if arg_list.len() != 1 {
                    return;
                }
                let expected: i64 = if method_str == ">=" { 1 } else { 0 };
                if !matches!(cx.kind(arg_list[0]), NodeKind::Int(v) if *v == expected) {
                    return;
                }
                Some(format!("({}).anybits?({})", lhs_src, rhs_src))
            }
            "==" => {
                if arg_list.len() != 1 {
                    return;
                }
                let cmp_id = arg_list[0];
                let cmp_src = cx.raw_source(cx.range(cmp_id));
                // Upstream checks allbits? before nobits?, so `(x & 0) == 0`
                // reports allbits?. Match that order here.
                if cmp_src == rhs_src {
                    Some(format!("({}).allbits?({})", lhs_src, rhs_src))
                } else if cmp_src == lhs_src {
                    Some(format!("({}).allbits?({})", rhs_src, lhs_src))
                } else if matches!(cx.kind(cmp_id), NodeKind::Int(0)) {
                    Some(format!("({}).nobits?({})", lhs_src, rhs_src))
                } else {
                    return;
                }
            }
            _ => return,
        };

        let Some(replacement) = replacement else {
            return;
        };
        let message = format!(
            "Replace with `{}` for comparison with bit flags.",
            replacement
        );
        cx.emit_offense(cx.range(node), &message, None);
        cx.emit_edit(cx.range(node), &replacement);
    }
}

fn unwrap_parenthesized(mut node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let mut unwrapped = false;
    while let NodeKind::Begin(children) = cx.kind(node) {
        let child_list = cx.list(*children);
        if child_list.len() != 1 {
            return None;
        }
        node = child_list[0];
        unwrapped = true;
    }
    unwrapped.then_some(node)
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
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Replace with `(variable).anybits?(flags)` for comparison with bit flags.
            "},
            "(variable).anybits?(flags)\n",
        );
    }

    #[test]
    fn flags_nested_parenthesized_bit_and() {
        test::<BitwisePredicate>().expect_correction(
            indoc! {"
                ((variable & flags)).positive?
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Replace with `(variable).anybits?(flags)` for comparison with bit flags.
            "},
            "(variable).anybits?(flags)\n",
        );
    }

    #[test]
    fn flags_zero_after_bit_and() {
        test::<BitwisePredicate>().expect_correction(
            indoc! {"
                (variable & flags).zero?
                ^^^^^^^^^^^^^^^^^^^^^^^^ Replace with `(variable).nobits?(flags)` for comparison with bit flags.
            "},
            "(variable).nobits?(flags)\n",
        );
    }

    #[test]
    fn flags_gt_zero() {
        test::<BitwisePredicate>().expect_correction(
            indoc! {"
                (variable & flags) > 0
                ^^^^^^^^^^^^^^^^^^^^^^ Replace with `(variable).anybits?(flags)` for comparison with bit flags.
            "},
            "(variable).anybits?(flags)\n",
        );
    }

    #[test]
    fn flags_ge_one() {
        test::<BitwisePredicate>().expect_correction(
            indoc! {"
                (variable & flags) >= 1
                ^^^^^^^^^^^^^^^^^^^^^^^ Replace with `(variable).anybits?(flags)` for comparison with bit flags.
            "},
            "(variable).anybits?(flags)\n",
        );
    }

    #[test]
    fn flags_ne_zero() {
        test::<BitwisePredicate>().expect_correction(
            indoc! {"
                (variable & flags) != 0
                ^^^^^^^^^^^^^^^^^^^^^^^ Replace with `(variable).anybits?(flags)` for comparison with bit flags.
            "},
            "(variable).anybits?(flags)\n",
        );
    }

    #[test]
    fn flags_eq_zero() {
        test::<BitwisePredicate>().expect_correction(
            indoc! {"
                (variable & flags) == 0
                ^^^^^^^^^^^^^^^^^^^^^^^ Replace with `(variable).nobits?(flags)` for comparison with bit flags.
            "},
            "(variable).nobits?(flags)\n",
        );
    }

    #[test]
    fn flags_eq_zero_hex() {
        test::<BitwisePredicate>().expect_correction(
            indoc! {"
                (variable & flags) == 0x0
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Replace with `(variable).nobits?(flags)` for comparison with bit flags.
            "},
            "(variable).nobits?(flags)\n",
        );
    }

    #[test]
    fn flags_eq_flags() {
        test::<BitwisePredicate>().expect_correction(
            indoc! {"
                (variable & flags) == flags
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Replace with `(variable).allbits?(flags)` for comparison with bit flags.
            "},
            "(variable).allbits?(flags)\n",
        );
    }

    #[test]
    fn flags_swapped_eq_flags() {
        test::<BitwisePredicate>().expect_correction(
            indoc! {"
                (flags & variable) == flags
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Replace with `(variable).allbits?(flags)` for comparison with bit flags.
            "},
            "(variable).allbits?(flags)\n",
        );
    }

    #[test]
    fn accepts_gt_non_zero() {
        test::<BitwisePredicate>().expect_no_offenses("(x & flags) > 1\n");
    }

    #[test]
    fn accepts_ge_zero() {
        test::<BitwisePredicate>().expect_no_offenses("(x & flags) >= 0\n");
    }

    #[test]
    fn accepts_ge_two() {
        test::<BitwisePredicate>().expect_no_offenses("(x & flags) >= 2\n");
    }

    #[test]
    fn accepts_ne_non_zero() {
        test::<BitwisePredicate>().expect_no_offenses("(x & flags) != 1\n");
    }

    #[test]
    fn accepts_eq_non_zero_non_flags() {
        test::<BitwisePredicate>().expect_no_offenses("(x & flags) == 1\n");
    }

    #[test]
    fn accepts_unparenthesized_bit_and() {
        test::<BitwisePredicate>().expect_no_offenses("x & flags == flags\n");
    }

    #[test]
    fn accepts_plain_positive() {
        test::<BitwisePredicate>().expect_no_offenses("x.positive?\n");
    }

    #[test]
    fn accepts_non_bit_and() {
        test::<BitwisePredicate>().expect_no_offenses("(x | flags).positive?\n");
    }
}
murphy_plugin_api::submit_cop!(BitwisePredicate);
