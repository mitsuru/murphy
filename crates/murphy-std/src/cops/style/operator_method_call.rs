//! `Style/OperatorMethodCall` — flags redundant dot before operator method calls.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/OperatorMethodCall
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detects `Send` nodes where an operator method is called with an explicit dot
//!   (`foo.+ bar` → `foo + bar`). Autocorrects by replacing the dot with a space
//!   and inserting a space after the selector when selector and argument are adjacent.
//!   The `wrap_in_parentheses_if_chained` branch (when the operator call is the
//!   receiver of another send) wraps the call in parentheses in the correction.
//!
//!   Covered:
//!     - All operators in RESTRICT_ON_SEND
//!     - Chaining: `foo.bar.+(baz).quux(2)` → `(foo.bar + baz).quux(2)`
//!     - Division before parenthesized arg: `foo./(bar)` → `foo / (bar)`
//!     - Argument adjacent to selector: `foo.+bar` → `foo + bar`
//!
//!   Guards (no offense):
//!     - Unary `@`-form operators (`+@`, `-@`, `!@`, `~@`): selector source ≠ method name
//!     - Const receiver (`Foo.+(bar)`)
//!     - Argument count ≠ 1
//!     - No explicit dot
//!     - Invalid-syntax args: Splat, Kwsplat (anonymous), BlockPass, ForwardedArgs (Unknown)
//!     - Acceptable chained-parenthesised arg: `foo.+(@bar).to_s` where the arg has
//!       a non-nil first child and the call is parenthesised
//!
//!   Disabled by default (Enabled: pending in RuboCop's default.yml).
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! foo.+ bar
//! foo.& bar
//!
//! # good
//! foo + bar
//! foo & bar
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const MSG: &str = "Redundant dot detected.";

const OPERATOR_METHODS: &[&str] = &[
    "|", "^", "&", "<=>", "==", "===", "=~", ">", ">=", "<", "<=", "<<", ">>", "+", "-", "*",
    "/", "%", "**", "~", "!", "!=", "!~",
];

/// Stateless unit struct.
#[derive(Default)]
pub struct OperatorMethodCall;

#[cop(
    name = "Style/OperatorMethodCall",
    description = "Checks for redundant dot before operator method call.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl OperatorMethodCall {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // Only fire on operator method names.
        let Some(method) = cx.method_name(node) else {
            return;
        };
        if !OPERATOR_METHODS.contains(&method) {
            return;
        }
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Must have an explicit dot.
    let dot = cx.loc(node).dot();
    if dot == Range::ZERO {
        return;
    }

    // Skip unary @-form operators: `foo.+@bar` → selector is `+@` but the method
    // `+@` is NOT in OPERATOR_METHODS so this is already filtered by the on_node
    // methods list. For `~@` and `!@`: the AST normalises these — `foo.~@ bar`
    // parses to method `~` (same name) or `!`. Detect by comparing selector source
    // vs method name: if they differ, skip.
    let selector = cx.selector(node);
    let selector_src = cx.raw_source(selector);
    let Some(method_name) = cx.method_name(node) else {
        return;
    };
    if selector_src != method_name {
        return;
    }

    // Skip const receivers.
    if cx.is_const_receiver(node) {
        return;
    }

    // Must have exactly one argument.
    let args = cx.call_arguments(node);
    if args.len() != 1 {
        return;
    }

    let rhs = args[0];

    // Skip if the single argument causes invalid syntax after dot removal.
    if is_invalid_syntax_argument(rhs, cx) {
        return;
    }

    // Skip the acceptable chained+parenthesised case: `foo.+(@bar).to_s`.
    if is_method_call_with_parenthesized_arg(node, rhs, cx) {
        return;
    }

    // Offense: the dot range.
    let rhs_range = cx.range(rhs);
    let is_chained = is_chained_receiver(node, cx);

    cx.emit_offense(dot, MSG, None);

    // Autocorrect: replace dot with space.
    cx.emit_edit(dot, " ");

    // For the chained case (node is receiver of another send), we need to:
    // 1. Remove the call's own argument-list parens (if present).
    // 2. Add space after selector.
    // 3. Wrap the whole node in outer parens.
    if is_chained {
        let open_paren = cx.loc(node).begin();
        let close_paren = cx.loc(node).end();

        if open_paren != Range::ZERO && close_paren != Range::ZERO {
            // Remove the `(` before the argument.
            cx.emit_edit(open_paren, "");
            // Remove the `)` after the argument.
            cx.emit_edit(close_paren, "");
        }

        // Add space after selector (so `foo.bar + baz` not `foo.bar +baz`).
        cx.emit_edit(
            Range {
                start: selector.end,
                end: selector.end,
            },
            " ",
        );

        // Wrap the entire operator-call node in outer parentheses.
        let node_range = cx.range(node);
        cx.emit_edit(
            Range {
                start: node_range.start,
                end: node_range.start,
            },
            "(",
        );
        cx.emit_edit(
            Range {
                start: node_range.end,
                end: node_range.end,
            },
            ")",
        );
        return;
    }

    // Non-chained case: add space between selector and rhs if adjacent or needed.
    if selector.end == rhs_range.start {
        // Selector and rhs are immediately adjacent — insert a space.
        cx.emit_edit(
            Range {
                start: selector.end,
                end: selector.end,
            },
            " ",
        );
    } else if method_name == "/" {
        // For `/`, if the gap between selector and rhs starts with `(` without
        // a leading space, add a space to avoid a syntax error: `foo./(bar)` → `foo / (bar)`.
        let gap_src = cx.raw_source(Range {
            start: selector.end,
            end: rhs_range.start,
        });
        if !gap_src.starts_with(' ') {
            cx.emit_edit(
                Range {
                    start: selector.end,
                    end: selector.end,
                },
                " ",
            );
        }
    }
}

/// Returns true if removing the dot from this argument would create invalid syntax.
/// Covers: Splat, Kwsplat (anonymous, inside hash), BlockPass, and forwarded args.
fn is_invalid_syntax_argument(arg: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(arg) {
        NodeKind::Splat(_) => true,
        NodeKind::BlockPass(_) => true,
        // Forwarded args appear as Unknown in Murphy's AST.
        NodeKind::Unknown => true,
        // Kwsplat inside a hash literal: `foo.==(**kwargs)` parses as
        // `(hash (kwsplat ...))` — check if the arg is a hash whose only child is kwsplat.
        NodeKind::Hash(list) => {
            let children = cx.list(list);
            children.len() == 1
                && matches!(*cx.kind(children[0]), NodeKind::Kwsplat(_))
        }
        _ => false,
    }
}

/// Returns true if the operator call is the receiver (non-first-arg) of another send.
fn is_chained_receiver(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    if !matches!(*cx.kind(parent), NodeKind::Send { .. }) {
        return false;
    }
    // The node is the receiver of the parent, not an argument.
    // If `node` IS the first argument of parent, it's not the receiver.
    if let Some(first_arg) = cx.first_argument(parent).get() {
        if first_arg == node {
            return false;
        }
    }
    true
}

/// Returns true for the acceptable `foo.+(arg).to_s` chained+parenthesised case
/// where the argument itself is "complex" (has a non-trivial first child in
/// RuboCop's sense). This matches RuboCop's `method_call_with_parenthesized_arg?`.
fn is_method_call_with_parenthesized_arg(node: NodeId, arg: NodeId, cx: &Cx<'_>) -> bool {
    // Grandparent must be a send.
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    if !matches!(*cx.kind(parent), NodeKind::Send { .. }) {
        return false;
    }
    // Operator call must be parenthesised.
    if !cx.is_parenthesized(node) {
        return false;
    }
    // The argument must have a non-nil "first child" in RuboCop's parser sense.
    arg_has_nontrivial_first_child(arg, cx)
}

/// Approximates RuboCop's `argument.children.first` truthiness check.
/// Returns true if the argument node has a non-nil "first child".
fn arg_has_nontrivial_first_child(arg: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(arg) {
        // These nodes have non-nil first children in the parser gem AST.
        NodeKind::Ivar(_)
        | NodeKind::Cvar(_)
        | NodeKind::Gvar(_)
        | NodeKind::Const { .. }
        | NodeKind::Lvar(_)
        | NodeKind::Int(_)
        | NodeKind::Float(_)
        | NodeKind::Str(_)
        | NodeKind::Sym(_)
        | NodeKind::True_
        | NodeKind::False_
        | NodeKind::Nil
        | NodeKind::SelfExpr
        | NodeKind::Array(_)
        | NodeKind::Hash(_) => true,
        // A send node: first child = receiver (may be None/nil for bare `bar` — falsey).
        NodeKind::Send { receiver, .. } => receiver.get().is_some(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::OperatorMethodCall;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Basic offenses -----

    #[test]
    fn flags_dot_plus() {
        test::<OperatorMethodCall>().expect_offense(indoc! {"
            foo.+ bar
               ^ Redundant dot detected.
        "});
    }

    #[test]
    fn flags_dot_ampersand() {
        test::<OperatorMethodCall>().expect_offense(indoc! {"
            foo.& bar
               ^ Redundant dot detected.
        "});
    }

    #[test]
    fn flags_dot_eq_eq() {
        test::<OperatorMethodCall>().expect_offense(indoc! {"
            foo.== bar
               ^ Redundant dot detected.
        "});
    }

    #[test]
    fn flags_dot_minus() {
        test::<OperatorMethodCall>().expect_offense(indoc! {"
            foo.- bar
               ^ Redundant dot detected.
        "});
    }

    #[test]
    fn flags_dot_star() {
        test::<OperatorMethodCall>().expect_offense(indoc! {"
            foo.* bar
               ^ Redundant dot detected.
        "});
    }

    #[test]
    fn flags_nested_receiver() {
        test::<OperatorMethodCall>().expect_offense(indoc! {"
            foo bar.== baz
                   ^ Redundant dot detected.
        "});
    }

    // ----- Autocorrect: basic -----

    #[test]
    fn corrects_dot_plus_space() {
        test::<OperatorMethodCall>().expect_correction(
            indoc! {"
                foo.+ bar
                   ^ Redundant dot detected.
            "},
            "foo + bar\n",
        );
    }

    #[test]
    fn corrects_dot_plus_adjacent() {
        test::<OperatorMethodCall>().expect_correction(
            indoc! {"
                foo.+bar
                   ^ Redundant dot detected.
            "},
            "foo + bar\n",
        );
    }

    #[test]
    fn corrects_dot_eq_eq_space() {
        test::<OperatorMethodCall>().expect_correction(
            indoc! {"
                foo.== bar
                   ^ Redundant dot detected.
            "},
            "foo == bar\n",
        );
    }

    #[test]
    fn corrects_dot_eq_eq_paren_arg() {
        // `foo.==({})` — parenthesised arg, but `{}` is a Hash (nontrivial), skip?
        // Actually: `foo.==({})` is NOT chained so correction just removes dot.
        // `{}` as arg: Hash has no receiver → arg_has_nontrivial_first_child = true
        // But is_chained_receiver = false → so offense IS reported, correction = `foo ==({})`.
        test::<OperatorMethodCall>().expect_correction(
            indoc! {"
                foo.==({})
                   ^ Redundant dot detected.
            "},
            "foo ==({})
",
        );
    }

    #[test]
    fn corrects_dot_slash_paren() {
        // `foo./(bar)` → `foo / (bar)` — space added before `(`
        test::<OperatorMethodCall>().expect_correction(
            indoc! {"
                foo./(bar)
                   ^ Redundant dot detected.
            "},
            "foo / (bar)\n",
        );
    }

    #[test]
    fn corrects_chained_operator_call() {
        // `foo.bar.+(baz).quux(2)` → `(foo.bar + baz).quux(2)`
        test::<OperatorMethodCall>().expect_correction(
            indoc! {"
                foo.bar.+(baz).quux(2)
                       ^ Redundant dot detected.
            "},
            "(foo.bar + baz).quux(2)\n",
        );
    }

    // ----- No offense -----

    #[test]
    fn accepts_infix_plus() {
        test::<OperatorMethodCall>().expect_no_offenses("foo + bar\n");
    }

    #[test]
    fn accepts_const_receiver() {
        test::<OperatorMethodCall>().expect_no_offenses("Foo.+(bar)\n");
    }

    #[test]
    fn accepts_no_arguments() {
        test::<OperatorMethodCall>().expect_no_offenses("obj.!\n");
    }

    #[test]
    fn accepts_multiple_arguments() {
        test::<OperatorMethodCall>().expect_no_offenses("foo.+(bar, baz)\n");
    }

    #[test]
    fn accepts_unary_plus_at() {
        // `foo.+@ bar` — `+@` is NOT in OPERATOR_METHODS so never dispatched.
        test::<OperatorMethodCall>().expect_no_offenses("foo.+@ bar\n");
    }

    #[test]
    fn accepts_unary_bang_at() {
        test::<OperatorMethodCall>().expect_no_offenses("foo.!@ bar\n");
    }

    #[test]
    fn accepts_unary_tilde_at() {
        test::<OperatorMethodCall>().expect_no_offenses("foo.~@ bar\n");
    }

    #[test]
    fn accepts_splat_arg() {
        test::<OperatorMethodCall>().expect_no_offenses("def foo(*args)\n  bar.==(*args)\nend\n");
    }

    #[test]
    fn accepts_block_pass_arg() {
        test::<OperatorMethodCall>().expect_no_offenses("def foo(&blk)\n  bar.==(&blk)\nend\n");
    }

    #[test]
    fn accepts_kwsplat_arg() {
        test::<OperatorMethodCall>()
            .expect_no_offenses("def foo(**kwargs)\n  bar.==(**kwargs)\nend\n");
    }

    #[test]
    fn accepts_parenthesized_chained_ivar_arg() {
        // `foo.+(@bar).to_s` — no offense (parenthesised + chained + ivar arg is nontrivial)
        test::<OperatorMethodCall>().expect_no_offenses("foo.+(@bar).to_s\n");
    }
}

murphy_plugin_api::submit_cop!(OperatorMethodCall);
