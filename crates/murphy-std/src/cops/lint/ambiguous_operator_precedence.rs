//! `Lint/AmbiguousOperatorPrecedence` — flags expressions mixing binary
//! operators of differing precedence without clarifying parentheses.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/AmbiguousOperatorPrecedence
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Two handlers mirror RuboCop's `on_and` and `on_send`. `on_and` flags a
//!   `&&` whose parent is `||` (and not already parenthesized via a `begin`).
//!   `on_send` flags an arithmetic/bitwise operator send nested directly under
//!   a lower-precedence operator (send or `&&`/`||` node). Keyword `and`/`or`
//!   have no precedence entry (`:and`/`:or` ≠ `:"&&"`), so a parent keyword
//!   operator never triggers `on_send`, matching RuboCop. Autocorrect wraps
//!   the higher-precedence subexpression in parentheses.
//! ```
//!
//! ## Matched shapes
//! - `a + b * c` → `a + (b * c)` — `*` nested under `+`
//! - `a || b && c` → `a || (b && c)` — `&&` nested under `||`
//! - `a ** b + c` → `(a ** b) + c` — `**` nested under `+`

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

const MSG: &str = "Wrap expressions with varying precedence with parentheses to avoid ambiguity.";

/// RuboCop's `PRECEDENCE` — outermost index 0 binds tightest.
const PRECEDENCE: &[&[&str]] = &[
    &["**"],
    &["*", "/", "%"],
    &["+", "-"],
    &["<<", ">>"],
    &["&"],
    &["|", "^"],
    &["&&"],
    &["||"],
];

#[derive(Default)]
pub struct AmbiguousOperatorPrecedence;

#[cop(
    name = "Lint/AmbiguousOperatorPrecedence",
    description = "Checks for expressions containing multiple binary operations with ambiguous precedence.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl AmbiguousOperatorPrecedence {
    #[on_node(kind = "and")]
    fn check_and(&self, node: NodeId, cx: &Cx<'_>) {
        let Some(parent) = cx.parent(node).get() else {
            return;
        };
        // `return if parent.begin_type?` — already parenthesized.
        if matches!(*cx.kind(parent), NodeKind::Begin(..)) {
            return;
        }
        // `return unless parent.or_type?`.
        if !matches!(*cx.kind(parent), NodeKind::Or { .. }) {
            return;
        }
        cx.emit_offense(cx.range(node), MSG, None);
        wrap_in_parens(node, cx);
    }

    #[on_node(kind = "send", methods = [
        "**", "*", "/", "%", "+", "-", "<<", ">>", "&", "|", "^"
    ])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // `return if node.parenthesized?`.
        if cx.is_parenthesized(node) {
            return;
        }
        let Some(parent) = cx.parent(node).get() else {
            return;
        };
        // `return unless operator?(parent)`.
        let Some(parent_prec) = operator_precedence(parent, cx) else {
            return;
        };
        let Some(node_prec) = operator_precedence(node, cx) else {
            return;
        };
        // `greater_precedence?`: parent binds looser (higher index) than node.
        if parent_prec <= node_prec {
            return;
        }
        cx.emit_offense(cx.range(node), MSG, None);
        wrap_in_parens(node, cx);
    }
}

/// RuboCop's `precedence(node)` combined with `operator?(node)`: returns the
/// index in `PRECEDENCE` of the node's operator, or `None` when the node is
/// not an operator with a precedence entry.
///
/// Sends use their method name. `&&`/`||` nodes use their operator token
/// (`&&`/`||` are in the table; keyword `and`/`or` are not).
fn operator_precedence(node: NodeId, cx: &Cx<'_>) -> Option<usize> {
    let op = match *cx.kind(node) {
        NodeKind::Send { .. } => cx.method_name(node)?,
        NodeKind::And { .. } | NodeKind::Or { .. } => logical_operator_text(node, cx)?,
        _ => return None,
    };
    PRECEDENCE.iter().position(|ops| ops.contains(&op))
}

/// The operator token for an `and`/`or` node: `&&`/`||` (symbol form) or
/// `and`/`or` (keyword form). Found as the operator token between the two
/// operands.
fn logical_operator_text<'a>(node: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    let (lhs, rhs) = match *cx.kind(node) {
        NodeKind::And { lhs, rhs } | NodeKind::Or { lhs, rhs } => (lhs, rhs),
        _ => return None,
    };
    let search = Range {
        start: cx.range(lhs).end,
        end: cx.range(rhs).start,
    };
    cx.tokens_in(search)
        .iter()
        .filter(|t| t.kind == SourceTokenKind::Other)
        .map(|t| cx.raw_source(t.range))
        .find(|s| matches!(*s, "&&" | "||" | "and" | "or"))
}

/// `corrector.wrap(node, '(', ')')` — two non-overlapping zero-width edits.
fn wrap_in_parens(node: NodeId, cx: &Cx<'_>) {
    let r = cx.range(node);
    cx.emit_edit(Range { start: r.start, end: r.start }, "(");
    cx.emit_edit(Range { start: r.end, end: r.end }, ")");
}

murphy_plugin_api::submit_cop!(AmbiguousOperatorPrecedence);

#[cfg(test)]
mod tests {
    use super::AmbiguousOperatorPrecedence;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_multiply_under_plus() {
        test::<AmbiguousOperatorPrecedence>()
            .expect_offense(indoc! {r#"
                a + b * c
                    ^^^^^ Wrap expressions with varying precedence with parentheses to avoid ambiguity.
            "#})
            .expect_correction(
                indoc! {r#"
                    a + b * c
                        ^^^^^ Wrap expressions with varying precedence with parentheses to avoid ambiguity.
                "#},
                "a + (b * c)\n",
            );
    }

    #[test]
    fn flags_and_under_or() {
        test::<AmbiguousOperatorPrecedence>()
            .expect_offense(indoc! {r#"
                a || b && c
                     ^^^^^^ Wrap expressions with varying precedence with parentheses to avoid ambiguity.
            "#})
            .expect_correction(
                indoc! {r#"
                    a || b && c
                         ^^^^^^ Wrap expressions with varying precedence with parentheses to avoid ambiguity.
                "#},
                "a || (b && c)\n",
            );
    }

    #[test]
    fn flags_exponent_under_plus() {
        test::<AmbiguousOperatorPrecedence>()
            .expect_offense(indoc! {r#"
                a ** b + c
                ^^^^^^ Wrap expressions with varying precedence with parentheses to avoid ambiguity.
            "#})
            .expect_correction(
                indoc! {r#"
                    a ** b + c
                    ^^^^^^ Wrap expressions with varying precedence with parentheses to avoid ambiguity.
                "#},
                "(a ** b) + c\n",
            );
    }

    #[test]
    fn flags_bitand_under_bitor() {
        test::<AmbiguousOperatorPrecedence>()
            .expect_offense(indoc! {r#"
                a | b & c
                    ^^^^^ Wrap expressions with varying precedence with parentheses to avoid ambiguity.
            "#})
            .expect_correction(
                indoc! {r#"
                    a | b & c
                        ^^^^^ Wrap expressions with varying precedence with parentheses to avoid ambiguity.
                "#},
                "a | (b & c)\n",
            );
    }

    #[test]
    fn accepts_same_precedence_chain() {
        test::<AmbiguousOperatorPrecedence>().expect_no_offenses("a + b + c\n");
    }

    #[test]
    fn accepts_already_parenthesized() {
        test::<AmbiguousOperatorPrecedence>().expect_no_offenses("a + (b * c)\n");
    }

    #[test]
    fn accepts_keyword_and_or() {
        // Keyword `and`/`or` have no precedence entry, so no offense.
        test::<AmbiguousOperatorPrecedence>().expect_no_offenses("a or b and c\n");
    }

    #[test]
    fn flags_multiply_first_under_plus() {
        // `a * b + c` parses as `(+ (* a b) c)`, so the `*` nests under the
        // lower-precedence `+` and is flagged just like `a + b * c`.
        test::<AmbiguousOperatorPrecedence>()
            .expect_offense(indoc! {r#"
                a * b + c
                ^^^^^ Wrap expressions with varying precedence with parentheses to avoid ambiguity.
            "#})
            .expect_correction(
                indoc! {r#"
                    a * b + c
                    ^^^^^ Wrap expressions with varying precedence with parentheses to avoid ambiguity.
                "#},
                "(a * b) + c\n",
            );
    }

    #[test]
    fn accepts_comparison_operators() {
        // Comparison operators are not in RESTRICT_ON_SEND / PRECEDENCE.
        test::<AmbiguousOperatorPrecedence>().expect_no_offenses("a == b && c\n");
    }
}
