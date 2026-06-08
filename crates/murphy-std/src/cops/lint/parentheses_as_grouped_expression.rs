//! `Lint/ParenthesesAsGroupedExpression` — flags `(...)` used as a grouped
//! expression where the parentheses could be misinterpreted.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/ParenthesesAsGroupedExpression
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Ported from RuboCop Lint/ParenthesesAsGroupedExpression.
//! ```
//!
//! ## Matched shapes
//!
//! - `do_something (foo)` — space before parenthesized argument.
//! - `a.func (x)` — dot-call with space before parens.
//! - `a&.func (x)` — safe-navigation call with space before parens.
//! - `a.concat ((1..1).map { |i| i * 10 })` — double-paren block arg.
//!
//! ## Exclusions
//!
//! - Operator methods: `a % (b + c)`
//! - Setter methods: `a.b = (c == d)`
//! - Chained calls: `func (x).func.func...`
//! - Operator keywords: `func (x) || y`
//! - Hash arguments: `transition (foo - bar) => value`
//! - Ternary expressions: `foo (cond) ? 1 : 2`
//! - Math expressions: `puts (2 + 3) * 4`
//! - Block args without wrapping parens: `a.concat (1..1).map { |i| i * 10 }`
//! - Compound range literals: `rand (a - b)..(c - d)`
//!
//! ## Autocorrect
//!
//! Removes the space between the method name and the opening parenthesis.

use murphy_plugin_api::{Cx, NodeId, NodeKind, NodeList, NoOptions, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct ParenthesesAsGroupedExpression;

#[cop(
    name = "Lint/ParenthesesAsGroupedExpression",
    description = "Flags `(...)` used as a grouped expression.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl ParenthesesAsGroupedExpression {
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

impl ParenthesesAsGroupedExpression {
    fn check(&self, node: NodeId, args: NodeList, cx: &Cx<'_>) {
        let args_list = cx.list(args);

        // Must have exactly one argument.
        if args_list.len() != 1 {
            return;
        }
        let first_arg = args_list[0];

        // First argument must be a parenthesized expression (Begin).
        if !matches!(*cx.kind(first_arg), NodeKind::Begin(_)) {
            return;
        }

        // ── exclusions ──────────────────────────────────────────────────

        if valid_context(node, first_arg, cx) {
            return;
        }

        // ── space detection ─────────────────────────────────────────────

        let space_len = spaces_before_left_paren(node, first_arg, cx);
        if space_len == 0 {
            return;
        }

        // ── offense ─────────────────────────────────────────────────────

        let arg_src = cx.raw_source(cx.range(first_arg));
        let msg = format!("`{arg_src}` interpreted as grouped expression.");
        let range = space_range(first_arg, space_len, cx);
        cx.emit_offense(range, &msg, None);

        // Autocorrect: remove the space.
        cx.emit_edit(range, "");
    }
}

// ── exclusion predicates ─────────────────────────────────────────────────────

/// Returns `true` when the call should NOT be flagged.
fn valid_context(node: NodeId, first_arg: NodeId, cx: &Cx<'_>) -> bool {
    // If the first argument is a block type (Block/Numblock/Itblock), skip.
    if cx.is_any_block_type(first_arg) {
        return true;
    }

    // Operator methods with parenthesized args are valid (e.g. `a % (b + c)`).
    if cx.is_operator_method(node) {
        return true;
    }

    // Setter methods are valid (e.g. `a.b = (c == d)`).
    if cx.is_setter_method(node) {
        return true;
    }

    valid_first_argument(first_arg, cx) || chained_calls(first_arg, cx)
}

/// Returns `true` when the first argument is itself a valid expression
/// (operator keyword, hash, ternary, or compound range).
fn valid_first_argument(first_arg: NodeId, cx: &Cx<'_>) -> bool {
    if cx.is_operator_keyword(first_arg) {
        return true;
    }

    if matches!(*cx.kind(first_arg), NodeKind::Hash(_)) {
        return true;
    }

    if matches!(*cx.kind(first_arg), NodeKind::If { .. }) && cx.is_ternary(first_arg) {
        return true;
    }

    is_compound_range(first_arg, cx)
}

/// Returns `true` when the first argument is a compound range —
/// a range literal whose begin/end nodes are themselves parenthesized.
fn is_compound_range(first_arg: NodeId, cx: &Cx<'_>) -> bool {
    let (begin_, end_) = match *cx.kind(first_arg) {
        NodeKind::RangeExpr { begin_, end_, .. } => (begin_, end_),
        _ => return false,
    };
    let begin_is_begin = matches!(
        begin_.get().map(|b| cx.kind(b)),
        Some(NodeKind::Begin(_) | NodeKind::Kwbegin(_))
    );
    let end_is_begin = matches!(
        end_.get().map(|e| cx.kind(e)),
        Some(NodeKind::Begin(_) | NodeKind::Kwbegin(_))
    );
    begin_is_begin || end_is_begin
}

/// Returns `true` when chained method calls follow the parenthesized expression
/// (e.g. `func (x).chained.call`). Mirrors RuboCop's `chained_calls?`.
fn chained_calls(first_arg: NodeId, cx: &Cx<'_>) -> bool {
    // `first_argument.call_type?`
    if !matches!(*cx.kind(first_arg), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        return false;
    }
    // `(node.children.last&.children&.count || 0) > 1`
    cx.children(first_arg).len() > 1
}

// ── space helpers ────────────────────────────────────────────────────────────

/// Returns the number of whitespace bytes between the method-name selector
/// and the opening parenthesis of the first argument.
fn spaces_before_left_paren(node: NodeId, first_arg: NodeId, cx: &Cx<'_>) -> u32 {
    // If the call itself is parenthesized (e.g. `foo(x)`), there is no
    // spurious space to flag — the parens belong to the call.
    if cx.is_parenthesized(node) {
        return 0;
    }

    let arg_range = cx.range(first_arg);
    // The first argument source must start with `(`.
    if !cx.raw_source(arg_range).starts_with('(') {
        return 0;
    }

    let selector_end = cx.loc(node).name.end;
    if arg_range.start <= selector_end {
        return 0;
    }

    arg_range.start - selector_end
}

/// Returns the source range of the space between the method name and `(`.
fn space_range(first_arg: NodeId, space_len: u32, cx: &Cx<'_>) -> Range {
    let arg_start = cx.range(first_arg).start;
    Range {
        start: arg_start - space_len,
        end: arg_start,
    }
}

// ── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::ParenthesesAsGroupedExpression;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_space_before_parens_in_bare_call() {
        test::<ParenthesesAsGroupedExpression>().expect_offense(indoc! {r#"
            do_something (foo)
                        ^ `(foo)` interpreted as grouped expression.
        "#});
    }

    #[test]
    fn corrects_space_before_parens_in_bare_call() {
        test::<ParenthesesAsGroupedExpression>().expect_correction(
            indoc! {r#"
                do_something (foo)
                            ^ `(foo)` interpreted as grouped expression.
            "#},
            "do_something(foo)\n",
        );
    }

    #[test]
    fn flags_space_before_parens_in_dot_call() {
        test::<ParenthesesAsGroupedExpression>().expect_offense(indoc! {r#"
            a.func (x)
                  ^ `(x)` interpreted as grouped expression.
        "#});
    }

    #[test]
    fn corrects_space_before_parens_in_dot_call() {
        test::<ParenthesesAsGroupedExpression>().expect_correction(
            indoc! {r#"
                a.func (x)
                      ^ `(x)` interpreted as grouped expression.
            "#},
            "a.func(x)\n",
        );
    }

    #[test]
    fn flags_space_before_parens_in_predicate_call() {
        test::<ParenthesesAsGroupedExpression>().expect_offense(indoc! {r#"
            is? (x)
               ^ `(x)` interpreted as grouped expression.
        "#});
    }

    #[test]
    fn corrects_space_before_parens_in_predicate_call() {
        test::<ParenthesesAsGroupedExpression>().expect_correction(
            indoc! {r#"
                is? (x)
                   ^ `(x)` interpreted as grouped expression.
            "#},
            "is?(x)\n",
        );
    }

    #[test]
    fn flags_double_paren_block_arg() {
        test::<ParenthesesAsGroupedExpression>().expect_offense(indoc! {r#"
            a.concat ((1..1).map { |i| i * 10 })
                    ^ `((1..1).map { |i| i * 10 })` interpreted as grouped expression.
        "#});
    }

    #[test]
    fn corrects_double_paren_block_arg() {
        test::<ParenthesesAsGroupedExpression>().expect_correction(
            indoc! {r#"
                a.concat ((1..1).map { |i| i * 10 })
                        ^ `((1..1).map { |i| i * 10 })` interpreted as grouped expression.
            "#},
            "a.concat((1..1).map { |i| i * 10 })\n",
        );
    }

    #[test]
    fn accepts_block_arg_no_parens() {
        test::<ParenthesesAsGroupedExpression>().expect_no_offenses(indoc! {r#"
            a.concat (1..1).map { |i| i * 10 }
        "#});
    }

    #[test]
    fn accepts_operator_with_space() {
        test::<ParenthesesAsGroupedExpression>().expect_no_offenses(indoc! {r#"
            func (x) || y
        "#});
    }

    #[test]
    fn accepts_chained_expression() {
        test::<ParenthesesAsGroupedExpression>().expect_no_offenses(indoc! {r#"
            func (x).func.func.func.func.func
        "#});
    }

    #[test]
    fn accepts_chained_expression_with_safe_nav() {
        test::<ParenthesesAsGroupedExpression>().expect_no_offenses(indoc! {r#"
            func (x).func.func.func.func&.func
        "#});
    }

    #[test]
    fn accepts_math_expression() {
        test::<ParenthesesAsGroupedExpression>().expect_no_offenses(indoc! {r#"
            puts (2 + 3) * 4
        "#});
    }

    #[test]
    fn accepts_math_expression_with_to_i() {
        test::<ParenthesesAsGroupedExpression>().expect_no_offenses(indoc! {r#"
            do_something.eq (foo * bar).to_i
        "#});
    }

    #[test]
    fn accepts_hash_argument() {
        test::<ParenthesesAsGroupedExpression>().expect_no_offenses(indoc! {r#"
            transition (foo - bar) => value
        "#});
    }

    #[test]
    fn accepts_ternary_operator() {
        test::<ParenthesesAsGroupedExpression>().expect_no_offenses(indoc! {r#"
            foo (cond) ? 1 : 2
        "#});
    }

    #[test]
    fn accepts_method_call_without_arguments() {
        test::<ParenthesesAsGroupedExpression>().expect_no_offenses("func\n");
    }

    #[test]
    fn accepts_method_call_with_args_no_parens() {
        test::<ParenthesesAsGroupedExpression>().expect_no_offenses("puts x\n");
    }

    #[test]
    fn accepts_chain_of_method_calls() {
        test::<ParenthesesAsGroupedExpression>().expect_no_offenses(indoc! {r#"
            a.b
            a.b 1
            a.b(1)
        "#});
    }

    #[test]
    fn accepts_method_with_parens_as_arg() {
        test::<ParenthesesAsGroupedExpression>().expect_no_offenses("a b(c)\n");
    }

    #[test]
    fn accepts_operator_call_with_arg_in_parens() {
        test::<ParenthesesAsGroupedExpression>().expect_no_offenses(indoc! {r#"
            a % (b + c)
            a.b = (c == d)
        "#});
    }

    #[test]
    fn accepts_space_inside_opening_paren() {
        test::<ParenthesesAsGroupedExpression>().expect_no_offenses("a( (b) )\n");
    }

    #[test]
    fn accepts_compound_range_literals() {
        test::<ParenthesesAsGroupedExpression>().expect_no_offenses(indoc! {r#"
            rand (a - b)..(c - d)
        "#});
    }

    #[test]
    fn flags_simple_range_literal() {
        test::<ParenthesesAsGroupedExpression>().expect_offense(indoc! {r#"
            rand (1..10)
                ^ `(1..10)` interpreted as grouped expression.
        "#});
    }

    #[test]
    fn corrects_simple_range_literal() {
        test::<ParenthesesAsGroupedExpression>().expect_correction(
            indoc! {r#"
                rand (1..10)
                    ^ `(1..10)` interpreted as grouped expression.
            "#},
            "rand(1..10)\n",
        );
    }

    #[test]
    fn accepts_call_with_multiple_arguments() {
        test::<ParenthesesAsGroupedExpression>().expect_no_offenses(
            "assert_equal (0..1.9), acceleration.domain\n",
        );
    }

    #[test]
    fn flags_safe_navigation_with_space() {
        test::<ParenthesesAsGroupedExpression>().expect_offense(indoc! {r#"
            a&.func (x)
                   ^ `(x)` interpreted as grouped expression.
        "#});
    }

    #[test]
    fn corrects_safe_navigation_with_space() {
        test::<ParenthesesAsGroupedExpression>().expect_correction(
            indoc! {r#"
                a&.func (x)
                       ^ `(x)` interpreted as grouped expression.
            "#},
            "a&.func(x)\n",
        );
    }
}

murphy_plugin_api::submit_cop!(ParenthesesAsGroupedExpression);
