//! `Lint/FloatComparison` — flag exact equality comparisons involving floats.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/FloatComparison
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues:
//!   - murphy-h3a9
//! notes: >
//!   Covers float literals, common float-returning sends, arithmetic involving
//!   floats, safe zero/nil comparisons, csend, and case/when float literals.
//!   Some RuboCop float-instance method refinements are intentionally omitted
//!   in v1 because Murphy has no type inference.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct FloatComparison;

#[cop(
    name = "Lint/FloatComparison",
    description = "Flag exact equality comparisons involving floats.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl FloatComparison {
    #[on_node(kind = "send", methods = ["==", "!=", "eql?", "equal?"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check_comparison(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Csend { method, .. } = *cx.kind(node) else { return; };
        if matches!(cx.symbol_str(method), "==" | "!=" | "eql?" | "equal?") {
            check_comparison(node, cx);
        }
    }

    #[on_node(kind = "case")]
    fn check_case(&self, node: NodeId, cx: &Cx<'_>) {
        for &when_node in cx.case_when_branches(node) {
            for &condition in cx.when_conditions(when_node) {
                if is_floatish(condition, cx) && !is_literal_safe(condition, cx) {
                    cx.emit_offense(cx.range(condition), "Avoid float literal comparisons in case statements as they are unreliable.", None);
                }
            }
        }
    }
}

fn check_comparison(node: NodeId, cx: &Cx<'_>) {
    let (method, lhs, args) = match *cx.kind(node) {
        NodeKind::Send { receiver, method, args } => (method, receiver.get(), cx.list(args)),
        NodeKind::Csend { receiver, method, args } => (method, Some(receiver), cx.list(args)),
        _ => return,
    };
    let Some(lhs) = lhs else { return; };
    let [rhs] = args else { return; };
    if is_literal_safe(lhs, cx) || is_literal_safe(*rhs, cx) {
        return;
    }
    if is_floatish(lhs, cx) || is_floatish(*rhs, cx) {
        let message = if cx.symbol_str(method) == "!=" {
            "Avoid inequality comparisons of floats as they are unreliable."
        } else {
            "Avoid equality comparisons of floats as they are unreliable."
        };
        cx.emit_offense(cx.range(node), message, None);
    }
}

fn is_literal_safe(node: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Nil => true,
        NodeKind::Int(0) => true,
        NodeKind::Float(value) => value == 0.0,
        _ => false,
    }
}

fn is_floatish(node: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Float(_) => true,
        NodeKind::Begin(list) => cx.list(list).first().is_some_and(|&child| is_floatish(child, cx)),
        NodeKind::Send { receiver, method, args } => {
            let name = cx.symbol_str(method);
            if matches!(name, "to_f" | "Float" | "fdiv") {
                return true;
            }
            if matches!(name, "+" | "-" | "*" | "/" | "**" | "%") {
                return receiver.get().is_some_and(|recv| is_floatish(recv, cx))
                    || cx.list(args).first().is_some_and(|&arg| is_floatish(arg, cx));
            }
            false
        }
        NodeKind::Csend { receiver, method, args } => {
            let name = cx.symbol_str(method);
            matches!(name, "to_f" | "fdiv")
                || (matches!(name, "+" | "-" | "*" | "/" | "**" | "%")
                    && (is_floatish(receiver, cx) || cx.list(args).first().is_some_and(|&arg| is_floatish(arg, cx))))
        }
        _ => false,
    }
}

murphy_plugin_api::submit_cop!(FloatComparison);

#[cfg(test)]
mod tests {
    use super::FloatComparison;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_float_equality_and_inequality() {
        test::<FloatComparison>()
            .expect_offense(indoc! {r#"
                x == 0.1
                ^^^^^^^^ Avoid equality comparisons of floats as they are unreliable.
            "#})
            .expect_offense(indoc! {r#"
                x != 0.1
                ^^^^^^^^ Avoid inequality comparisons of floats as they are unreliable.
            "#});
    }

    #[test]
    fn flags_case_when_float_literals() {
        test::<FloatComparison>().expect_offense(indoc! {r#"
            case value
            when 1.0
                 ^^^ Avoid float literal comparisons in case statements as they are unreliable.
              foo
            end
        "#});
    }

    #[test]
    fn accepts_zero_nil_and_epsilon_style_comparisons() {
        test::<FloatComparison>()
            .expect_no_offenses("x == 0.0\n")
            .expect_no_offenses("Float(x, exception: false) == nil\n")
            .expect_no_offenses("(x - 0.1).abs < Float::EPSILON\n");
    }
}
