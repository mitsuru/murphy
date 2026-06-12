//! `Lint/BinaryOperatorWithIdenticalOperands` — flags binary operators whose
//! operands are identical.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/BinaryOperatorWithIdenticalOperands
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   RESTRICT_ON_SEND is pinned exactly to `== != === <=> =~ > >= < <= | ^`
//!   (note: `&` is deliberately excluded, matching RuboCop; arithmetic/shift
//!   operators `+ - * / ** << >>` are likewise excluded). `&&`/`and` and
//!   `||`/`or` are handled via the `And`/`Or` node hooks rather than `send`.
//!   Operand equality is decided by `raw_source` text comparison (whitespace
//!   sensitive), the same convention used across Murphy's duplicate-detection
//!   cops; RuboCop uses structural AST `==`. The And/Or operator label in the
//!   message is the source operator text (`&&` vs `and`), extracted from the
//!   token between the operands, not hardcoded.
//! ```
use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind, SourceTokenKind};

/// Binary operators RuboCop restricts the `send` check to. `&` is deliberately
/// absent (RuboCop lists only `|` and `^`); all arithmetic/shift operators are
/// excluded so e.g. `x - x` is not flagged.
const RESTRICT_ON_SEND: &[&str] = &[
    "==", "!=", "===", "<=>", "=~", ">", ">=", "<", "<=", "|", "^",
];

#[derive(Default)]
pub struct BinaryOperatorWithIdenticalOperands;

#[cop(
    name = "Lint/BinaryOperatorWithIdenticalOperands",
    description = "Checks for places where binary operator has identical operands.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl BinaryOperatorWithIdenticalOperands {
    #[on_node(
        kind = "send",
        methods = ["==", "!=", "===", "<=>", "=~", ">", ">=", "<", "<=", "|", "^"]
    )]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let Some(op) = cx.method_name(node) else {
            return;
        };
        if !RESTRICT_ON_SEND.contains(&op) {
            return;
        }
        // `binary_operation?`: receiver present and exactly one argument.
        let Some(receiver) = cx.call_receiver(node).get() else {
            return;
        };
        let args = cx.call_arguments(node);
        let [arg] = args else {
            return;
        };
        if cx.raw_source(cx.range(receiver)) != cx.raw_source(cx.range(*arg)) {
            return;
        }
        cx.emit_offense(cx.range(node), &message(op), None);
    }

    #[on_node(kind = "and")]
    fn check_and(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::And { lhs, rhs } = *cx.kind(node) else {
            return;
        };
        self.check_logical(node, lhs, rhs, cx);
    }

    #[on_node(kind = "or")]
    fn check_or(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Or { lhs, rhs } = *cx.kind(node) else {
            return;
        };
        self.check_logical(node, lhs, rhs, cx);
    }
}

impl BinaryOperatorWithIdenticalOperands {
    fn check_logical(&self, node: NodeId, lhs: NodeId, rhs: NodeId, cx: &Cx<'_>) {
        if cx.raw_source(cx.range(lhs)) != cx.raw_source(cx.range(rhs)) {
            return;
        }
        let Some(op) = logical_operator_text(lhs, rhs, cx) else {
            return;
        };
        cx.emit_offense(cx.range(node), &message(op), None);
    }
}

fn message(op: &str) -> String {
    format!("Binary operator `{op}` has identical operands.")
}

/// The source operator text between `lhs` and `rhs` (`&&`/`and`/`||`/`or`).
/// RuboCop reports the literal operator the author wrote, so it is read from
/// the source rather than hardcoded per node kind.
fn logical_operator_text<'a>(lhs: NodeId, rhs: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    let lhs_end = cx.range(lhs).end;
    let rhs_start = cx.range(rhs).start;
    let tok = cx
        .sorted_tokens()
        .iter()
        .find(|t| {
            t.kind == SourceTokenKind::Other
                && t.range.start >= lhs_end
                && t.range.end <= rhs_start
                && matches!(cx.raw_source(t.range), "&&" | "and" | "||" | "or")
        })?;
    Some(cx.raw_source(tok.range))
}

murphy_plugin_api::submit_cop!(BinaryOperatorWithIdenticalOperands);

#[cfg(test)]
mod tests {
    use super::BinaryOperatorWithIdenticalOperands as Cop;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_equality_with_identical_operands() {
        test::<Cop>().expect_offense(indoc! {r#"
            x == x
            ^^^^^^ Binary operator `==` has identical operands.
        "#});
    }

    #[test]
    fn flags_various_comparison_operators() {
        test::<Cop>().expect_offense(indoc! {r#"
            x.top >= x.top
            ^^^^^^^^^^^^^^ Binary operator `>=` has identical operands.
        "#});
    }

    #[test]
    fn flags_spaceship_and_match() {
        test::<Cop>()
            .expect_offense(indoc! {r#"
                a <=> a
                ^^^^^^^ Binary operator `<=>` has identical operands.
            "#})
            .expect_offense(indoc! {r#"
                a =~ a
                ^^^^^^ Binary operator `=~` has identical operands.
            "#});
    }

    #[test]
    fn flags_bitwise_or_and_xor() {
        test::<Cop>()
            .expect_offense(indoc! {r#"
                a | a
                ^^^^^ Binary operator `|` has identical operands.
            "#})
            .expect_offense(indoc! {r#"
                a ^ a
                ^^^^^ Binary operator `^` has identical operands.
            "#});
    }

    #[test]
    fn flags_logical_and_or() {
        test::<Cop>()
            .expect_offense(indoc! {r#"
                a && a
                ^^^^^^ Binary operator `&&` has identical operands.
            "#})
            .expect_offense(indoc! {r#"
                a || a
                ^^^^^^ Binary operator `||` has identical operands.
            "#});
    }

    #[test]
    fn flags_keyword_and_or() {
        test::<Cop>()
            .expect_offense(indoc! {r#"
                a and a
                ^^^^^^^ Binary operator `and` has identical operands.
            "#})
            .expect_offense(indoc! {r#"
                a or a
                ^^^^^^ Binary operator `or` has identical operands.
            "#});
    }

    #[test]
    fn does_not_flag_distinct_operands() {
        test::<Cop>().expect_no_offenses("x == y\na && b\na | b\n");
    }

    #[test]
    fn does_not_flag_arithmetic_operators() {
        // RESTRICT_ON_SEND excludes `+ - * / ** << >> &`.
        test::<Cop>().expect_no_offenses(indoc! {r#"
            x + x
            x - x
            x * x
            x ** x
            x << x
            x & x
        "#});
    }

    #[test]
    fn does_not_flag_unary_minus_difference() {
        test::<Cop>().expect_no_offenses("x == -x\n");
    }
}
