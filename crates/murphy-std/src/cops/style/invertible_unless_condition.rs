//! `Style/InvertibleUnlessCondition` — flags `unless` with a condition that
//! can be inverted, suggesting `if` with the inverted condition instead.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/InvertibleUnlessCondition
//! upstream_version_checked: 1.50.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Murphy handles: `!` negation, `InverseMethods` map (hardcoded default
//!   matching RuboCop's yml default; not configurable in v1), `&&`/`||`
//!   logical operators (and/or keyword forms invert to &&/|| in message).
//!   Autocorrect: replaces `unless` keyword with `if`; replaces each
//!   invertible send selector with its inverse; replaces each and/or
//!   operator token with its inverse (`&&`<->`||`); removes `!` selector.
//!   Offense range: first source line of the node (consistent with sibling
//!   cops like Style/NegatedUnless and Style/UnlessElse).
//!   Parity gaps vs RuboCop:
//!   - `InverseMethods` is not configurable; hardcoded to RuboCop's yml default.
//!   - `begin` node (parenthesized condition like `unless (x != y)`) parses
//!     as `NodeKind::Unknown` in Murphy's arena AST; offense silently skipped.
//!   - `and`/`or` keyword operators invert to `&&`/`||` in the message and
//!     autocorrect (matching RuboCop's behavior for semantic ops).
//!   The cop is disabled by default (`Enabled: false` in RuboCop).
//! ```
//!
//! ## Matched shapes
//!
//! `if` nodes (which represent `unless`) where:
//! - Keyword is `unless` (not `if`)
//! - Not a ternary
//! - Condition is invertible:
//!   - `Send { method: "!" }` (bang negation)
//!   - `Send { method: m }` where `m` is in InverseMethods
//!   - `And`/`Or` where both sides are invertible
//!
//! ## Autocorrect
//!
//! - Replace `unless` keyword with `if`
//! - For `!`: remove the `!` selector
//! - For InverseMethods: replace selector with inverse method name
//! - For `&&`/`||` (`and`/`or`): replace operator token with inverse

use murphy_plugin_api::{
    Cx, NodeId, NodeKind, NodeList, NoOptions, OptNodeId, Range, SourceTokenKind, Symbol, cop,
};

/// RuboCop's default InverseMethods map (bidirectional).
static INVERSE_METHODS: &[(&str, &str)] = &[
    ("!=", "=="),
    (">", "<="),
    ("<=", ">"),
    ("<", ">="),
    (">=", "<"),
    ("!~", "=~"),
    ("zero?", "nonzero?"),
    ("nonzero?", "zero?"),
    ("any?", "none?"),
    ("none?", "any?"),
    ("even?", "odd?"),
    ("odd?", "even?"),
];

fn inverse_of(method: &str) -> Option<&'static str> {
    INVERSE_METHODS
        .iter()
        .find(|(k, _)| *k == method)
        .map(|(_, v)| *v)
}

#[derive(Default)]
pub struct InvertibleUnlessCondition;

#[cop(
    name = "Style/InvertibleUnlessCondition",
    description = "Favor `if` with inverted condition over `unless`.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl InvertibleUnlessCondition {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Only `unless`.
    if !cx.is_unless(node) {
        return;
    }
    if cx.is_ternary(node) {
        return;
    }

    let NodeKind::If { cond, .. } = cx.kind(node) else {
        return;
    };
    let cond = *cond;

    if !invertible(cond, cx) {
        return;
    }

    let preferred_cond = preferred_condition(cond, cx);
    let kw_loc = cx.if_keyword_loc(node);
    let current_kw = cx.raw_source(kw_loc);
    let current_cond_src = cx.raw_source(cx.range(cond));

    let msg = format!(
        "Prefer `if {preferred_cond}` over `{current_kw} {current_cond_src}`."
    );

    // Offense range: first source line of the node.
    let node_range = cx.range(node);
    let source = cx.source().as_bytes();
    let node_start = node_range.start as usize;
    let first_line_end = source[node_start..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(node_range.end as usize, |pos| node_start + pos);
    let offense_range = Range {
        start: node_range.start,
        end: first_line_end as u32,
    };

    cx.emit_offense(offense_range, &msg, None);

    // Autocorrect: replace `unless` -> `if`.
    if kw_loc != Range::ZERO {
        cx.emit_edit(kw_loc, "if");
    }

    // Autocorrect: invert the condition.
    autocorrect_condition(cond, cx);
}

/// Returns true if the condition is invertible.
fn invertible(node: NodeId, cx: &Cx<'_>) -> bool {
    match cx.kind(node) {
        NodeKind::Send { method, args, .. } => {
            let m = cx.symbol_str(*method);
            if m == "!" {
                return cx.list(*args).is_empty();
            }
            // Check inheritance: `Foo < Bar` should not be flagged if
            // first arg is a mixed-case constant (class name).
            if m == "<" {
                if let Some(&first_arg) = cx.list(*args).first() {
                    if is_class_inheritance_check(first_arg, cx) {
                        return false;
                    }
                }
            }
            inverse_of(m).is_some()
        }
        NodeKind::And { lhs, rhs } | NodeKind::Or { lhs, rhs } => {
            invertible(*lhs, cx) && invertible(*rhs, cx)
        }
        _ => false,
    }
}

/// Returns true if the argument looks like a class name (mixed-case constant).
/// RuboCop's `inheritance_check?`: first arg is Const whose short_name is not
/// all-uppercase.
fn is_class_inheritance_check(arg: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Const { name, .. } = cx.kind(arg) else {
        return false;
    };
    let name_str = cx.symbol_str(*name);
    !name_str.is_empty() && name_str != name_str.to_uppercase()
}

/// Build the preferred (inverted) condition string for the offense message.
fn preferred_condition(node: NodeId, cx: &Cx<'_>) -> String {
    match cx.kind(node) {
        NodeKind::Send { receiver, method, args } => {
            build_send_condition(node, *receiver, *method, *args, cx)
        }
        NodeKind::And { lhs, rhs } => {
            let lhs_str = preferred_condition(*lhs, cx);
            let rhs_str = preferred_condition(*rhs, cx);
            format!("{lhs_str} || {rhs_str}")
        }
        NodeKind::Or { lhs, rhs } => {
            let lhs_str = preferred_condition(*lhs, cx);
            let rhs_str = preferred_condition(*rhs, cx);
            format!("{lhs_str} && {rhs_str}")
        }
        _ => cx.raw_source(cx.range(node)).to_owned(),
    }
}

fn build_send_condition(
    node: NodeId,
    receiver: OptNodeId,
    method: Symbol,
    args: NodeList,
    cx: &Cx<'_>,
) -> String {
    let m = cx.symbol_str(method);
    let recv_src = receiver.get().map(|r| cx.raw_source(cx.range(r)));
    let arg_list = cx.list(args);

    if m == "!" {
        // !recv -> recv
        return recv_src.unwrap_or("").to_owned();
    }

    let inverse = inverse_of(m).unwrap_or(m);

    if arg_list.is_empty() {
        // Predicate or no-arg method: `recv.method?` -> `recv.inverse?`
        if let Some(recv) = recv_src {
            format!("{recv}.{inverse}")
        } else {
            inverse.to_owned()
        }
    } else if cx.is_operator_method(node) {
        // Operator method: `recv op arg` -> `recv inverse_op arg`
        let recv = recv_src.unwrap_or("");
        let args_src: Vec<&str> = arg_list
            .iter()
            .map(|&a| cx.raw_source(cx.range(a)))
            .collect();
        format!("{recv} {inverse} {}", args_src.join(", "))
    } else if cx.is_parenthesized(node) {
        // Parenthesized call: `recv.method?(arg)` -> `recv.inverse?(arg)`
        let recv = recv_src.map(|r| format!("{r}.")).unwrap_or_default();
        let args_src: Vec<&str> = arg_list
            .iter()
            .map(|&a| cx.raw_source(cx.range(a)))
            .collect();
        format!("{recv}{inverse}({})", args_src.join(", "))
    } else {
        // Space-separated args: `recv.method arg` -> `recv.inverse arg`
        let recv = recv_src.map(|r| format!("{r}.")).unwrap_or_default();
        let args_src: Vec<&str> = arg_list
            .iter()
            .map(|&a| cx.raw_source(cx.range(a)))
            .collect();
        format!("{recv}{inverse} {}", args_src.join(", "))
    }
}

/// Apply autocorrect to the condition node (recursively for And/Or).
fn autocorrect_condition(node: NodeId, cx: &Cx<'_>) {
    match cx.kind(node) {
        NodeKind::Send { method, .. } => {
            let method = *method;
            autocorrect_send(node, method, cx);
        }
        NodeKind::And { lhs, rhs } | NodeKind::Or { lhs, rhs } => {
            let (lhs, rhs) = (*lhs, *rhs);
            // Replace the operator token between lhs and rhs.
            let is_and = matches!(cx.kind(node), NodeKind::And { .. });
            let inverse_op = if is_and { "||" } else { "&&" };
            if let Some(op_range) = find_logical_op_token(lhs, rhs, cx) {
                cx.emit_edit(op_range, inverse_op);
            }
            autocorrect_condition(lhs, cx);
            autocorrect_condition(rhs, cx);
        }
        _ => {}
    }
}

fn autocorrect_send(node: NodeId, method: Symbol, cx: &Cx<'_>) {
    let m = cx.symbol_str(method);
    let selector = cx.selector(node);
    if selector == Range::ZERO {
        return;
    }
    if m == "!" {
        // Remove `!` selector entirely.
        cx.emit_edit(selector, "");
    } else if let Some(inverse) = inverse_of(m) {
        // Replace method name with its inverse.
        cx.emit_edit(selector, inverse);
    }
}

/// Find the `&&`, `||`, `and`, or `or` token between lhs.end and rhs.start.
fn find_logical_op_token(lhs: NodeId, rhs: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let gap = Range {
        start: cx.range(lhs).end,
        end: cx.range(rhs).start,
    };
    if gap.start >= gap.end {
        return None;
    }

    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < gap.start);
    let source = cx.source().as_bytes();

    for tok in &toks[idx..] {
        if tok.range.start >= gap.end {
            break;
        }
        if tok.kind == SourceTokenKind::Other {
            let text = &source[tok.range.start as usize..tok.range.end as usize];
            if text == b"&&" || text == b"||" || text == b"and" || text == b"or" {
                return Some(tok.range);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::InvertibleUnlessCondition;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Simple negation -----

    #[test]
    fn flags_unless_bang_negation() {
        test::<InvertibleUnlessCondition>().expect_correction(
            indoc! {"
                foo unless !bar
                ^^^^^^^^^^^^^^^ Prefer `if bar` over `unless !bar`.
            "},
            "foo if bar\n",
        );
    }

    // ----- InverseMethods -----

    #[test]
    fn flags_unless_not_equal() {
        test::<InvertibleUnlessCondition>().expect_correction(
            indoc! {"
                foo unless x != y
                ^^^^^^^^^^^^^^^^^ Prefer `if x == y` over `unless x != y`.
            "},
            "foo if x == y\n",
        );
    }

    #[test]
    fn flags_unless_greater_equal() {
        test::<InvertibleUnlessCondition>().expect_correction(
            indoc! {"
                foo unless x >= 10
                ^^^^^^^^^^^^^^^^^^ Prefer `if x < 10` over `unless x >= 10`.
            "},
            "foo if x < 10\n",
        );
    }

    #[test]
    fn flags_unless_even_predicate() {
        test::<InvertibleUnlessCondition>().expect_correction(
            indoc! {"
                foo unless x.even?
                ^^^^^^^^^^^^^^^^^^ Prefer `if x.odd?` over `unless x.even?`.
            "},
            "foo if x.odd?\n",
        );
    }

    #[test]
    fn flags_unless_bare_predicate() {
        test::<InvertibleUnlessCondition>().expect_correction(
            indoc! {"
                foo unless odd?
                ^^^^^^^^^^^^^^^ Prefer `if even?` over `unless odd?`.
            "},
            "foo if even?\n",
        );
    }

    // ----- Compound conditions -----

    #[test]
    fn flags_unless_compound_or_inverts_to_and() {
        test::<InvertibleUnlessCondition>().expect_correction(
            indoc! {"
                foo unless x != y || x.even?
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `if x == y && x.odd?` over `unless x != y || x.even?`.
            "},
            "foo if x == y && x.odd?\n",
        );
    }

    #[test]
    fn flags_unless_compound_and_inverts_to_or() {
        test::<InvertibleUnlessCondition>().expect_correction(
            indoc! {"
                foo unless x != y && x.even?
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `if x == y || x.odd?` over `unless x != y && x.even?`.
            "},
            "foo if x == y || x.odd?\n",
        );
    }

    // ----- Block form (prefix) -----

    #[test]
    fn flags_block_unless() {
        test::<InvertibleUnlessCondition>().expect_correction(
            indoc! {"
                unless x != y
                ^^^^^^^^^^^^^ Prefer `if x == y` over `unless x != y`.
                  foo
                end
            "},
            indoc! {"
                if x == y
                  foo
                end
            "},
        );
    }

    // ----- Negative cases -----

    #[test]
    fn accepts_unless_with_non_invertible_condition() {
        test::<InvertibleUnlessCondition>()
            .expect_no_offenses("foo unless some_condition\n");
    }

    #[test]
    fn accepts_unless_where_one_side_not_invertible() {
        test::<InvertibleUnlessCondition>()
            .expect_no_offenses("foo unless x != y || some_method\n");
    }

    #[test]
    fn accepts_if_keyword() {
        test::<InvertibleUnlessCondition>().expect_no_offenses("foo if !bar\n");
    }

    #[test]
    fn accepts_ternary() {
        test::<InvertibleUnlessCondition>().expect_no_offenses("a ? b : c\n");
    }

    #[test]
    fn accepts_inheritance_check() {
        // `Foo < Bar` should not be flagged (class inheritance check).
        test::<InvertibleUnlessCondition>()
            .expect_no_offenses("foo unless Foo < Bar\n");
    }

    #[test]
    fn flags_less_than_with_uppercase_constant() {
        // `Foo < FOO_BAR` — FOO_BAR is all-uppercase so treated as a constant,
        // not a class — this SHOULD be flagged.
        test::<InvertibleUnlessCondition>().expect_offense(indoc! {"
            foo unless Foo < FOO_BAR
            ^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `if Foo >= FOO_BAR` over `unless Foo < FOO_BAR`.
        "});
    }
}

murphy_plugin_api::submit_cop!(InvertibleUnlessCondition);
