//! `Lint/AmbiguousOperator` — flag ambiguous unary operators used as the first
//! argument of a method call written without parentheses, e.g.
//! `do_something *some_array` (is `*` a splat or a multiplication?).
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/AmbiguousOperator
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   RuboCop drives this cop off the parser gem's `:ambiguous_prefix`
//!   diagnostic, which Murphy does not surface (doing so would bypass the
//!   single-surface ABI boundary, ADR 0004). Instead the ambiguity is
//!   reconstructed from Prism's AST shape plus a whitespace check: Prism only
//!   yields a `splat` / `kwsplat` / `block_pass` / unary `-@`/`+@` *first
//!   argument* of an unparenthesized call when the source was the ambiguous
//!   form (`foo *x`), never the disambiguated forms (`foo * x` is a binary
//!   send, `foo(*x)` is parenthesized). The whitespace insurance (space before
//!   the operator, no space after) mirrors what the parser uses to raise the
//!   diagnostic. Signed `Int` / `Float` / `Rational` / `Complex` first
//!   arguments (`foo -1`, `foo +1r`) fold into a signed literal rather than a
//!   unary send, so the sign byte at the argument start is read from source.
//!   Bare binary operator sends (`foo - -1`, `foo * -1`) share the
//!   receiver+operator+arg shape with dot operator calls (`foo.+ -1`), so
//!   operator-method outers without a dot are skipped. Severity is fixed at
//!   the cop default because Murphy has no `diagnostic.level` to forward.
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, NodeList, NoOptions, Range, cop};

#[derive(Default)]
pub struct AmbiguousOperator;

/// `(operator, actual, possible)` — mirrors RuboCop's AMBIGUITIES hash.
const AMBIGUITIES: &[(&str, &str, &str)] = &[
    ("+", "positive number", "an addition"),
    ("-", "negative number", "a subtraction"),
    ("*", "splat", "a multiplication"),
    ("&", "block", "a binary AND"),
    ("**", "keyword splat", "an exponent"),
];

#[cop(
    name = "Lint/AmbiguousOperator",
    description = "Flag ambiguous operators in the first argument of a parenthesis-less call.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl AmbiguousOperator {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { args, .. } = *cx.kind(node) else {
            return;
        };
        self.check(node, args, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Csend { args, .. } = *cx.kind(node) else {
            return;
        };
        self.check(node, args, cx);
    }
}

impl AmbiguousOperator {
    fn check(&self, node: NodeId, args: NodeList, cx: &Cx<'_>) {
        // Parenthesized calls (`foo(*x)`) are never ambiguous.
        if cx.is_parenthesized(node) {
            return;
        }

        // Bare binary operator sends (`foo - -1`, `foo * -1`, `foo - -x`)
        // share the receiver+operator+argument AST shape with dot operator
        // calls (`foo.+ -1`), but only the latter is ambiguous: the parser
        // emits `:ambiguous_prefix` for the dot form and nothing for the
        // binary form. Prism records the dot loc, so skip operator-method
        // outers without a dot. `Csend` (`&.`) is always an explicit call,
        // never a bare binary, so it is never skipped.
        if cx.is_operator_method(node) && !cx.is_dot(node) && !cx.is_safe_navigation(node) {
            return;
        }

        let args_list = cx.list(args);
        let Some(&first_arg) = args_list.first() else {
            return;
        };

        // Resolve `(operator-byte-offset, operator-text)` for the ambiguous
        // first-argument shapes. Anything else is unambiguous.
        let Some((op_start, op_text)) = ambiguous_operator(first_arg, cx) else {
            return;
        };

        // Whitespace insurance — the parser raises `:ambiguous_prefix` only
        // when there is whitespace before the operator and none after it.
        if !has_ambiguous_spacing(op_start, op_text, cx) {
            return;
        }

        let Some((_, actual, possible)) =
            AMBIGUITIES.iter().find(|(op, _, _)| *op == op_text).copied()
        else {
            return;
        };

        let op_range = Range {
            start: op_start,
            end: op_start + op_text.len() as u32,
        };
        let msg = format!(
            "Ambiguous {actual} operator. Parenthesize the method arguments if it's surely a {actual} operator, or add a whitespace to the right of the `{op_text}` if it should be {possible}."
        );
        cx.emit_offense(op_range, &msg, None);

        self.add_parentheses(node, op_start, cx);
    }

    /// `add_parentheses(offense_node, corrector)` — wrap the call's argument
    /// list in parentheses: replace the whitespace between the selector and the
    /// operator with `(`, and insert `)` after the call.
    ///
    /// The opening paren goes right before the operator (`op_start`), not
    /// before the first-argument *node*: for a keyword splat the first argument
    /// is the surrounding `Hash`, whose range may begin after the `**`, so
    /// using the node start would swallow the `**` and silently drop it.
    /// For splat / block-pass / unary shapes `op_start` equals the first-arg
    /// node start, so this is uniformly correct.
    fn add_parentheses(&self, node: NodeId, op_start: u32, cx: &Cx<'_>) {
        let selector_end = cx.loc(node).name.end;
        if op_start >= selector_end {
            cx.emit_edit(
                Range {
                    start: selector_end,
                    end: op_start,
                },
                "(",
            );
        }
        let node_end = cx.range(node).end;
        cx.emit_edit(
            Range {
                start: node_end,
                end: node_end,
            },
            ")",
        );
    }
}

/// Returns `(operator-byte-offset, operator-text)` when `first_arg` is one of
/// the ambiguous prefix shapes, else `None`.
fn ambiguous_operator(first_arg: NodeId, cx: &Cx<'_>) -> Option<(u32, &'static str)> {
    let arg_start = cx.range(first_arg).start;
    match *cx.kind(first_arg) {
        // `foo *x` — splat.
        NodeKind::Splat(_) => Some((arg_start, "*")),
        // `foo &blk` — block pass.
        NodeKind::BlockPass(_) => Some((arg_start, "&")),
        // `foo **opts` — keyword splat. Prism wraps the `**` inside a `Hash`
        // whose first element is the `Kwsplat`; the `**` is at the kwsplat's
        // range start.
        NodeKind::Hash(list) => {
            let first = cx.list(list).first().copied()?;
            if matches!(*cx.kind(first), NodeKind::Kwsplat(_)) {
                Some((cx.range(first).start, "**"))
            } else {
                None
            }
        }
        // `foo -x` / `foo +x` — unary minus/plus on a non-literal operand.
        NodeKind::Send { .. } => match cx.method_name(first_arg) {
            Some("-@") => Some((arg_start, "-")),
            Some("+@") => Some((arg_start, "+")),
            _ => None,
        },
        // `foo -1` / `foo +1` (and `-1.5`, `-1r`, `+1i`, ...) — Prism folds
        // the sign into a signed `Int` / `Float` / `Rational` / `Complex`
        // literal rather than a unary `-@`/`+@` send. The sign byte lives at
        // the argument range start, so an unsigned literal (`foo 1`, whose
        // range starts at the digit) falls through to `None`. Binary forms
        // (`foo - -1`, `foo * -1`) never reach here: the outer binary send
        // is skipped in `check` via the operator-method guard.
        NodeKind::Int(_)
        | NodeKind::Float(_)
        | NodeKind::Rational(_)
        | NodeKind::Complex(_) => {
            let source = cx.source().as_bytes();
            match source.get(arg_start as usize) {
                Some(b'+') => Some((arg_start, "+")),
                Some(b'-') => Some((arg_start, "-")),
                _ => None,
            }
        }
        _ => None,
    }
}

/// True when there is whitespace immediately before the operator and a
/// non-whitespace byte immediately after it — the parser's condition for
/// raising `:ambiguous_prefix`.
fn has_ambiguous_spacing(op_start: u32, op_text: &str, cx: &Cx<'_>) -> bool {
    let source = cx.source().as_bytes();
    let start = op_start as usize;
    let after = start + op_text.len();

    // Need a byte before the operator and a byte after it.
    let Some(&before) = start.checked_sub(1).and_then(|i| source.get(i)) else {
        return false;
    };
    let Some(&next) = source.get(after) else {
        return false;
    };
    before.is_ascii_whitespace() && !next.is_ascii_whitespace()
}

murphy_plugin_api::submit_cop!(AmbiguousOperator);

#[cfg(test)]
mod tests {
    use super::AmbiguousOperator;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_splat() {
        test::<AmbiguousOperator>().expect_offense(indoc! {r#"
            do_something *some_array
                         ^ Ambiguous splat operator. Parenthesize the method arguments if it's surely a splat operator, or add a whitespace to the right of the `*` if it should be a multiplication.
        "#});
    }

    #[test]
    fn corrects_splat() {
        test::<AmbiguousOperator>().expect_correction(
            indoc! {r#"
                do_something *some_array
                             ^ Ambiguous splat operator. Parenthesize the method arguments if it's surely a splat operator, or add a whitespace to the right of the `*` if it should be a multiplication.
            "#},
            "do_something(*some_array)\n",
        );
    }

    #[test]
    fn flags_block_pass() {
        test::<AmbiguousOperator>().expect_offense(indoc! {r#"
            do_something &block
                         ^ Ambiguous block operator. Parenthesize the method arguments if it's surely a block operator, or add a whitespace to the right of the `&` if it should be a binary AND.
        "#});
    }

    #[test]
    fn corrects_block_pass() {
        test::<AmbiguousOperator>().expect_correction(
            indoc! {r#"
                do_something &block
                             ^ Ambiguous block operator. Parenthesize the method arguments if it's surely a block operator, or add a whitespace to the right of the `&` if it should be a binary AND.
            "#},
            "do_something(&block)\n",
        );
    }

    #[test]
    fn flags_keyword_splat() {
        test::<AmbiguousOperator>().expect_offense(indoc! {r#"
            do_something **opts
                         ^^ Ambiguous keyword splat operator. Parenthesize the method arguments if it's surely a keyword splat operator, or add a whitespace to the right of the `**` if it should be an exponent.
        "#});
    }

    #[test]
    fn corrects_keyword_splat() {
        test::<AmbiguousOperator>().expect_correction(
            indoc! {r#"
                do_something **opts
                             ^^ Ambiguous keyword splat operator. Parenthesize the method arguments if it's surely a keyword splat operator, or add a whitespace to the right of the `**` if it should be an exponent.
            "#},
            "do_something(**opts)\n",
        );
    }

    #[test]
    fn flags_unary_minus() {
        test::<AmbiguousOperator>().expect_offense(indoc! {r#"
            do_something -x
                         ^ Ambiguous negative number operator. Parenthesize the method arguments if it's surely a negative number operator, or add a whitespace to the right of the `-` if it should be a subtraction.
        "#});
    }

    #[test]
    fn corrects_unary_minus() {
        test::<AmbiguousOperator>().expect_correction(
            indoc! {r#"
                do_something -x
                             ^ Ambiguous negative number operator. Parenthesize the method arguments if it's surely a negative number operator, or add a whitespace to the right of the `-` if it should be a subtraction.
            "#},
            "do_something(-x)\n",
        );
    }

    #[test]
    fn flags_unary_plus() {
        test::<AmbiguousOperator>().expect_offense(indoc! {r#"
            do_something +x
                         ^ Ambiguous positive number operator. Parenthesize the method arguments if it's surely a positive number operator, or add a whitespace to the right of the `+` if it should be an addition.
        "#});
    }

    #[test]
    fn accepts_parenthesized_splat() {
        test::<AmbiguousOperator>().expect_no_offenses("do_something(*some_array)\n");
    }

    #[test]
    fn accepts_binary_multiplication() {
        test::<AmbiguousOperator>().expect_no_offenses("do_something * some_array\n");
    }

    #[test]
    fn accepts_binary_subtraction() {
        test::<AmbiguousOperator>().expect_no_offenses("do_something - x\n");
    }

    #[test]
    fn accepts_parenthesized_block_pass() {
        test::<AmbiguousOperator>().expect_no_offenses("do_something(&block)\n");
    }

    #[test]
    fn flags_dot_call_splat() {
        test::<AmbiguousOperator>().expect_offense(indoc! {r#"
            x.do_something *some_array
                           ^ Ambiguous splat operator. Parenthesize the method arguments if it's surely a splat operator, or add a whitespace to the right of the `*` if it should be a multiplication.
        "#});
    }

    #[test]
    fn corrects_dot_call_splat() {
        test::<AmbiguousOperator>().expect_correction(
            indoc! {r#"
                x.do_something *some_array
                               ^ Ambiguous splat operator. Parenthesize the method arguments if it's surely a splat operator, or add a whitespace to the right of the `*` if it should be a multiplication.
            "#},
            "x.do_something(*some_array)\n",
        );
    }

    #[test]
    fn accepts_no_arguments() {
        test::<AmbiguousOperator>().expect_no_offenses("do_something\n");
    }

    #[test]
    fn accepts_plain_argument() {
        test::<AmbiguousOperator>().expect_no_offenses("do_something some_array\n");
    }

    #[test]
    fn flags_signed_int_minus() {
        test::<AmbiguousOperator>().expect_offense(indoc! {r#"
            do_something -1
                         ^ Ambiguous negative number operator. Parenthesize the method arguments if it's surely a negative number operator, or add a whitespace to the right of the `-` if it should be a subtraction.
        "#});
    }

    #[test]
    fn flags_signed_int_plus() {
        test::<AmbiguousOperator>().expect_offense(indoc! {r#"
            do_something +1
                         ^ Ambiguous positive number operator. Parenthesize the method arguments if it's surely a positive number operator, or add a whitespace to the right of the `+` if it should be an addition.
        "#});
    }

    #[test]
    fn corrects_signed_int_minus() {
        test::<AmbiguousOperator>().expect_correction(
            indoc! {r#"
                do_something -1
                             ^ Ambiguous negative number operator. Parenthesize the method arguments if it's surely a negative number operator, or add a whitespace to the right of the `-` if it should be a subtraction.
            "#},
            "do_something(-1)\n",
        );
    }

    #[test]
    fn flags_signed_float_minus() {
        test::<AmbiguousOperator>().expect_offense(indoc! {r#"
            do_something -1.5
                         ^ Ambiguous negative number operator. Parenthesize the method arguments if it's surely a negative number operator, or add a whitespace to the right of the `-` if it should be a subtraction.
        "#});
    }

    #[test]
    fn flags_signed_float_plus() {
        test::<AmbiguousOperator>().expect_offense(indoc! {r#"
            do_something +1.5
                         ^ Ambiguous positive number operator. Parenthesize the method arguments if it's surely a positive number operator, or add a whitespace to the right of the `+` if it should be an addition.
        "#});
    }

    #[test]
    fn flags_signed_rational_minus() {
        test::<AmbiguousOperator>().expect_offense(indoc! {r#"
            do_something -1r
                         ^ Ambiguous negative number operator. Parenthesize the method arguments if it's surely a negative number operator, or add a whitespace to the right of the `-` if it should be a subtraction.
        "#});
    }

    #[test]
    fn flags_signed_complex_plus() {
        test::<AmbiguousOperator>().expect_offense(indoc! {r#"
            do_something +1i
                         ^ Ambiguous positive number operator. Parenthesize the method arguments if it's surely a positive number operator, or add a whitespace to the right of the `+` if it should be an addition.
        "#});
    }

    #[test]
    fn flags_dot_call_signed_int() {
        test::<AmbiguousOperator>().expect_offense(indoc! {r#"
            x.do_something -1
                           ^ Ambiguous negative number operator. Parenthesize the method arguments if it's surely a negative number operator, or add a whitespace to the right of the `-` if it should be a subtraction.
        "#});
    }

    #[test]
    fn flags_dot_operator_call_signed_int() {
        test::<AmbiguousOperator>().expect_offense(indoc! {r#"
            foo.+ -1
                  ^ Ambiguous negative number operator. Parenthesize the method arguments if it's surely a negative number operator, or add a whitespace to the right of the `-` if it should be a subtraction.
        "#});
    }

    #[test]
    fn accepts_unsigned_int() {
        test::<AmbiguousOperator>().expect_no_offenses("do_something 1\n");
    }

    #[test]
    fn accepts_unsigned_float() {
        test::<AmbiguousOperator>().expect_no_offenses("do_something 1.5\n");
    }

    #[test]
    fn accepts_unsigned_rational() {
        test::<AmbiguousOperator>().expect_no_offenses("do_something 1r\n");
    }

    #[test]
    fn accepts_unsigned_complex() {
        test::<AmbiguousOperator>().expect_no_offenses("do_something 1i\n");
    }

    #[test]
    fn accepts_binary_minus_with_signed_rhs() {
        test::<AmbiguousOperator>().expect_no_offenses("do_something - -1\n");
    }

    #[test]
    fn accepts_binary_plus_with_signed_rhs() {
        test::<AmbiguousOperator>().expect_no_offenses("do_something + -1\n");
    }

    #[test]
    fn accepts_binary_multiplication_with_signed_rhs() {
        test::<AmbiguousOperator>().expect_no_offenses("do_something * -1\n");
    }

    #[test]
    fn accepts_binary_minus_with_unsigned_rhs() {
        test::<AmbiguousOperator>().expect_no_offenses("do_something - 1\n");
    }

    #[test]
    fn accepts_binary_minus_with_unary_rhs() {
        test::<AmbiguousOperator>().expect_no_offenses("do_something - -x\n");
    }

    #[test]
    fn accepts_parenthesized_signed_int() {
        test::<AmbiguousOperator>().expect_no_offenses("do_something(-1)\n");
    }
}
