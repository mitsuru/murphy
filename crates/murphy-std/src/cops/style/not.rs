//! `Style/Not` — flags `not` keyword, preferring `!`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/Not
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Covered:
//!     - Flags `not expr` and autocorrects to `!expr`.
//!     - When receiver is a comparison operator (==, !=, <=, >, <, >=),
//!       removes the negation and flips the operator (e.g. `not a == b` → `a != b`).
//!     - When receiver requires parentheses (operator_keyword, binary_operation,
//!       ternary), uses `!(...)` form.
//!     - Otherwise replaces `not ` (with trailing space) with `!`.
//!
//!   Notes on `binary_operation?`:
//!     Murphy port of RuboCop-AST's `binary_operation?`:
//!     `operator_method? && expression.begin_pos != selector.begin_pos`.
//!     This means the operator is not at the start of the expression, i.e.
//!     there is a receiver to the left. Used to decide if parens are needed.
//! ```
//!
//! ## Matched shapes
//!
//! `Send` nodes with method `!` where the selector token is `not` (keyword form).
//!
//! ## Autocorrect
//!
//! Three correction branches (in precedence order):
//! 1. Receiver is an opposite-method comparison (`==`/`!=`/`<=`/`>`/`<`/`>=`):
//!    remove the `not ` prefix and flip the operator on the receiver.
//! 2. Receiver requires parentheses (operator keyword, binary op, ternary):
//!    replace `not ` with `!(` and append `)`.
//! 3. Otherwise: replace `not ` with `!`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, RangeSide, SpaceRangeOptions, cop};
use murphy_plugin_api::method_predicates::is_operator_method;

const MSG: &str = "Use `!` instead of `not`.";

/// Opposite methods for the `not receiver_with_comparison` → flip-the-operator correction.
const OPPOSITE_METHODS: &[(&str, &str)] = &[
    ("==", "!="),
    ("!=", "=="),
    ("<=", ">"),
    (">", "<="),
    ("<", ">="),
    (">=", "<"),
];

/// Stateless unit struct.
#[derive(Default)]
pub struct Not;

#[cop(
    name = "Style/Not",
    description = "Use `!` instead of `not`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl Not {
    #[on_node(kind = "send", methods = ["!"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        if !cx.is_prefix_not(node) {
            return;
        }

        // Offense is on the selector (`not` keyword range).
        let selector = cx.loc(node).name;
        cx.emit_offense(selector, MSG, None);

        // Determine autocorrect strategy.
        // The receiver of `not expr` is `expr`.
        let Some(receiver) = cx.call_receiver(node).get() else {
            return;
        };

        // Expand the `not` selector to include trailing whitespace (the space
        // between `not` and its argument), mirroring RuboCop's
        // `range_with_surrounding_space(node.loc.selector, side: :right)`.
        let not_and_space = cx.range_with_surrounding_space(
            selector,
            SpaceRangeOptions {
                side: RangeSide::Right,
                newlines: true,
                whitespace: false,
                continuations: false,
            },
        );

        if let Some(opposite) = opposite_method(receiver, cx) {
            // Branch 1: flip the comparison operator.
            // Remove `not ` prefix entirely.
            cx.emit_edit(not_and_space, "");
            // Rename the operator on the receiver.
            cx.emit_edit(cx.loc(receiver).name, opposite);
        } else if requires_parens(receiver, cx) {
            // Branch 2: wrap in parens.
            cx.emit_edit(not_and_space, "!(");
            // Insert closing paren at the end of the full `not expr` node.
            let node_end = cx.range(node).end;
            cx.emit_edit(Range { start: node_end, end: node_end }, ")");
        } else {
            // Branch 3: simple replacement.
            cx.emit_edit(not_and_space, "!");
        }
    }
}

/// Returns the opposite method name if `receiver` is a comparison with a known opposite,
/// or `None` otherwise.
fn opposite_method<'a>(receiver: NodeId, cx: &Cx<'_>) -> Option<&'a str> {
    let method = cx.method_name(receiver)?;
    OPPOSITE_METHODS
        .iter()
        .find(|&&(m, _)| m == method)
        .map(|&(_, opp)| opp)
}

/// Returns `true` if the receiver expression requires parentheses when prefixed by `!`.
///
/// Mirrors RuboCop's `requires_parens?`:
/// ```ruby
/// child.operator_keyword? ||
///   (child.send_type? && child.binary_operation?) ||
///   (child.if_type? && child.ternary?)
/// ```
fn requires_parens(receiver: NodeId, cx: &Cx<'_>) -> bool {
    // operator_keyword? — `and` / `or` nodes.
    if cx.is_operator_keyword(receiver) {
        return true;
    }

    // send_type? && binary_operation?
    // binary_operation? = operator_method? && expression.begin != selector.begin
    if matches!(cx.kind(receiver), NodeKind::Send { .. }) {
        if let Some(method) = cx.method_name(receiver) {
            if is_operator_method(method) {
                // Check that the expression does not start at the selector
                // (i.e., there is a receiver to the left of the operator).
                let expr_start = cx.range(receiver).start;
                let selector_start = cx.loc(receiver).name.start;
                if expr_start != selector_start {
                    return true;
                }
            }
        }
    }

    // if_type? && ternary?
    if matches!(cx.kind(receiver), NodeKind::If { .. }) && cx.is_ternary(receiver) {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_not_keyword() {
        test::<Not>().expect_offense(indoc! {"
            x = not something
                ^^^ Use `!` instead of `not`.
        "});
    }

    #[test]
    fn accepts_bang_negation() {
        test::<Not>().expect_no_offenses("x = !something\n");
    }

    // --- Autocorrect: simple (no parens needed) ---

    #[test]
    fn corrects_not_to_bang_simple() {
        test::<Not>().expect_correction(
            "x = not something\n    ^^^ Use `!` instead of `not`.\n",
            "x = !something\n",
        );
    }

    #[test]
    fn corrects_not_to_bang_method_call() {
        test::<Not>().expect_correction(
            "x = not foo.bar\n    ^^^ Use `!` instead of `not`.\n",
            "x = !foo.bar\n",
        );
    }

    // --- Autocorrect: opposite method ---

    #[test]
    fn corrects_not_eq_to_neq() {
        test::<Not>().expect_correction(
            "x = not a == b\n    ^^^ Use `!` instead of `not`.\n",
            "x = a != b\n",
        );
    }

    #[test]
    fn corrects_not_neq_to_eq() {
        test::<Not>().expect_correction(
            "x = not a != b\n    ^^^ Use `!` instead of `not`.\n",
            "x = a == b\n",
        );
    }

    #[test]
    fn corrects_not_gt_to_le() {
        test::<Not>().expect_correction(
            "x = not a > b\n    ^^^ Use `!` instead of `not`.\n",
            "x = a <= b\n",
        );
    }

    #[test]
    fn corrects_not_lt_to_ge() {
        test::<Not>().expect_correction(
            "x = not a < b\n    ^^^ Use `!` instead of `not`.\n",
            "x = a >= b\n",
        );
    }

    #[test]
    fn corrects_not_ge_to_lt() {
        test::<Not>().expect_correction(
            "x = not a >= b\n    ^^^ Use `!` instead of `not`.\n",
            "x = a < b\n",
        );
    }

    #[test]
    fn corrects_not_le_to_gt() {
        test::<Not>().expect_correction(
            "x = not a <= b\n    ^^^ Use `!` instead of `not`.\n",
            "x = a > b\n",
        );
    }

    // --- Autocorrect: with parens (binary operation) ---

    #[test]
    fn corrects_not_binary_add_with_parens() {
        test::<Not>().expect_correction(
            "x = not a + b\n    ^^^ Use `!` instead of `not`.\n",
            "x = !(a + b)\n",
        );
    }

    // --- Autocorrect: with parens (operator keyword) ---

    #[test]
    fn corrects_not_and_with_parens() {
        test::<Not>().expect_correction(
            "x = not a && b\n    ^^^ Use `!` instead of `not`.\n",
            "x = !(a && b)\n",
        );
    }

    // --- Autocorrect: with parens (ternary) ---

    #[test]
    fn corrects_not_ternary_with_parens() {
        test::<Not>().expect_correction(
            "x = not a ? b : c\n    ^^^ Use `!` instead of `not`.\n",
            "x = !(a ? b : c)\n",
        );
    }
}
murphy_plugin_api::submit_cop!(Not);
