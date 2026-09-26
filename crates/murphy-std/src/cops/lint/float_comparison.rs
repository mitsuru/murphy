//! `Lint/FloatComparison` — flag exact equality comparisons involving floats.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/FloatComparison
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop 1.87 `float?`/`literal_safe?`: float literals,
//!   `to_f`/`Float`/`fdiv`, arithmetic involving floats, float-receiver
//!   `abs`/`magnitude`/`modulo`/`next_float`/`prev_float`/`quo`/`@-`,
//!   negative-float `angle`/`arg`/`phase`, and `ceil`/`floor`/`round`/
//!   `truncate` with positive integer precision, plus safe zero/nil
//!   exemptions, csend, and case/when float shapes.
//!   Intentional refinements: multi-statement `begin` uses the last value
//!   (Ruby semantics) where RuboCop uses the first child, and parenthesized
//!   zero `(0.0)` stays exempt via unwrapping; csend mirrors send.
//!
//! Verbatim port of the call head `(call _ {:== :!= :eql? :equal?} ...)`
//! (murphy-s1yc.33): `call` = `{send csend}` covers safe-navigation
//! (`x&.eql?(0.1)`), mirroring RuboCop `alias on_csend on_send` plus
//! `RESTRICT_ON_SEND EQUALITY_METHODS`; the wildcard receiver binds an
//! absent or present receiver per murphy-if9y; trailing `...` absorbs any
//! argument list. The one-arg and float-shape guards below apply separately
//! (upstream `on_send` returns early unless the comparison shape matches).

use crate::cops::util::unwrap_parenthesized;
use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, NodeList, cop, def_node_matcher};

#[derive(Default)]
pub struct FloatComparison;

// Verbatim port of the call head (murphy-s1yc.33):
// `(call _ {:== :!= :eql? :equal?} ...)` — `call` = `{send csend}` covers
// safe-navigation (`x&.eql?(0.1)`), mirroring RuboCop
// `alias on_csend on_send` plus `RESTRICT_ON_SEND EQUALITY_METHODS`. The `_`
// receiver binds an absent or present receiver per murphy-if9y; trailing
// `...` absorbs any argument list, so the one-arg and float-shape guards
// below apply separately.
def_node_matcher!(
    float_comparison_call,
    "(call _ {:== :!= :eql? :equal?} ...)"
);

#[cop(
    name = "Lint/FloatComparison",
    description = "Flag exact equality comparisons involving floats.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl FloatComparison {
    /// Send path: `x == 0.1` etc.
    /// Triggered on all sends; the verbatim
    /// `(call _ {:== :!= :eql? :equal?} ...)` head filters to the
    /// `RESTRICT_ON_SEND` methods.
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check_comparison(node, cx);
    }

    /// Safe-navigation send path: `x&.eql?(0.1)`.
    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check_comparison(node, cx);
    }

    #[on_node(kind = "case")]
    fn check_case(&self, node: NodeId, cx: &Cx<'_>) {
        for &when_node in cx.case_when_branches(node) {
            for &condition in cx.when_conditions(when_node) {
                let unwrapped = unwrap_parenthesized(condition, cx);
                if is_literal_safe(unwrapped, cx) {
                    continue;
                }
                if is_floatish(unwrapped, cx) {
                    cx.emit_offense(cx.range(condition), "Avoid float literal comparisons in case statements as they are unreliable.", None);
                }
            }
        }
    }
}

fn check_comparison(node: NodeId, cx: &Cx<'_>) {
    // Verbatim `(call _ {:== :!= :eql? :equal?} ...)` head: filters to the
    // `RESTRICT_ON_SEND` methods on either send or csend (safe-navigation),
    // with any receiver (absent or present). Without this, an unrelated call
    // with a float argument (e.g. `x.foo(0.1)`) would run check on every
    // call node instead of being rejected by the method set up front.
    if !float_comparison_call(node, cx) {
        return;
    }
    let (method, lhs, args) = match *cx.kind(node) {
        NodeKind::Send { receiver, method, args } => (method, receiver.get(), cx.list(args)),
        NodeKind::Csend { receiver, method, args } => (method, Some(receiver), cx.list(args)),
        _ => return,
    };
    let Some(lhs) = lhs else { return; };
    let [rhs] = args else { return; };
    let lhs = unwrap_parenthesized(lhs, cx);
    let rhs = unwrap_parenthesized(*rhs, cx);
    if is_literal_safe(lhs, cx) || is_literal_safe(rhs, cx) {
        return;
    }
    if is_floatish(lhs, cx) || is_floatish(rhs, cx) {
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
        NodeKind::Begin(list) => cx.list(list).last().is_some_and(|&child| is_floatish(child, cx)),
        NodeKind::Send { receiver, method, args } => {
            let name = cx.symbol_str(method);
            if matches!(name, "to_f" | "Float" | "fdiv") {
                return true;
            }
            if matches!(name, "+" | "-" | "*" | "/" | "**" | "%") {
                return receiver.get().is_some_and(|recv| is_floatish(recv, cx))
                    || cx.list(args).first().is_some_and(|&arg| is_floatish(arg, cx));
            }
            if let Some(recv) = receiver.get() {
                return is_float_receiver_method(recv, name, args, cx);
            }
            false
        }
        NodeKind::Csend { receiver, method, args } => {
            let name = cx.symbol_str(method);
            if matches!(name, "to_f" | "Float" | "fdiv") {
                return true;
            }
            if matches!(name, "+" | "-" | "*" | "/" | "**" | "%") {
                return is_floatish(receiver, cx)
                    || cx.list(args).first().is_some_and(|&arg| is_floatish(arg, cx));
            }
            is_float_receiver_method(receiver, name, args, cx)
        }
        _ => false,
    }
}

fn is_float_receiver_method(
    receiver: NodeId,
    method: &str,
    args: NodeList,
    cx: &Cx<'_>,
) -> bool {
    let NodeKind::Float(value) = *cx.kind(receiver) else {
        return false;
    };
    // RuboCop `FLOAT_INSTANCE_METHODS`: direct float receiver only, no unwrapping.
    // `@-` is kept for parity even though Murphy lowers unary minus to `-@`
    // (so it never fires, matching RuboCop where `0.1.-@` is not flagged).
    if matches!(
        method,
        "abs" | "magnitude" | "modulo" | "next_float" | "prev_float" | "quo" | "@-"
    ) {
        return true;
    }
    if matches!(method, "angle" | "arg" | "phase") {
        // RuboCop: `Float(receiver.source).negative?`
        return value < 0.0;
    }
    if matches!(method, "ceil" | "floor" | "round" | "truncate") {
        // RuboCop: first arg is int and `Integer(source).positive?`
        return cx
            .list(args)
            .first()
            .is_some_and(|&first| matches!(*cx.kind(first), NodeKind::Int(precision) if precision > 0));
    }
    false
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
    fn case_when_exempts_parenthesized_zero() {
        test::<FloatComparison>().expect_no_offenses(indoc! {r#"
            case value
            when (0.0)
              foo
            end
        "#});
    }

    #[test]
    fn case_when_flags_float_shapes() {
        test::<FloatComparison>()
            .expect_offense(indoc! {r#"
                case value
                when x.to_f
                     ^^^^^^ Avoid float literal comparisons in case statements as they are unreliable.
                  foo
                end
            "#})
            .expect_offense(indoc! {r#"
                case value
                when 0.1.abs
                     ^^^^^^^ Avoid float literal comparisons in case statements as they are unreliable.
                  foo
                end
            "#})
            .expect_offense(indoc! {r#"
                case value
                when 1.1.ceil(1)
                     ^^^^^^^^^^^ Avoid float literal comparisons in case statements as they are unreliable.
                  foo
                end
            "#});
    }

    #[test]
    fn flags_float_receiver_instance_methods() {
        test::<FloatComparison>()
            .expect_offense(indoc! {r#"
                x == 0.1.abs
                ^^^^^^^^^^^^ Avoid equality comparisons of floats as they are unreliable.
            "#})
            .expect_offense(indoc! {r#"
                x == 0.1.magnitude
                ^^^^^^^^^^^^^^^^^^ Avoid equality comparisons of floats as they are unreliable.
            "#})
            .expect_offense(indoc! {r#"
                x == 0.1.modulo(1)
                ^^^^^^^^^^^^^^^^^^ Avoid equality comparisons of floats as they are unreliable.
            "#})
            .expect_offense(indoc! {r#"
                x == 0.1.next_float
                ^^^^^^^^^^^^^^^^^^^ Avoid equality comparisons of floats as they are unreliable.
            "#})
            .expect_offense(indoc! {r#"
                x == 0.1.prev_float
                ^^^^^^^^^^^^^^^^^^^ Avoid equality comparisons of floats as they are unreliable.
            "#})
            .expect_offense(indoc! {r#"
                x == 0.1.quo(2)
                ^^^^^^^^^^^^^^^ Avoid equality comparisons of floats as they are unreliable.
            "#});
    }

    #[test]
    fn does_not_flag_parenthesized_float_receiver_methods() {
        test::<FloatComparison>()
            .expect_no_offenses("x == (-0.1).abs\n")
            .expect_no_offenses("x == (0.1).abs\n")
            .expect_no_offenses("x == (1.1).ceil(1)\n")
            .expect_no_offenses("x == (-1.5).angle\n");
    }

    #[test]
    fn flags_negative_float_angle_arg_phase() {
        test::<FloatComparison>()
            .expect_offense(indoc! {r#"
                x == -1.5.angle
                ^^^^^^^^^^^^^^^ Avoid equality comparisons of floats as they are unreliable.
            "#})
            .expect_offense(indoc! {r#"
                x == -1.5.arg
                ^^^^^^^^^^^^^ Avoid equality comparisons of floats as they are unreliable.
            "#})
            .expect_offense(indoc! {r#"
                x == -1.5.phase
                ^^^^^^^^^^^^^^^ Avoid equality comparisons of floats as they are unreliable.
            "#});
    }

    #[test]
    fn does_not_flag_positive_float_angle_arg_phase() {
        test::<FloatComparison>()
            .expect_no_offenses("x == 1.5.angle\n")
            .expect_no_offenses("x == 1.5.arg\n")
            .expect_no_offenses("x == 1.5.phase\n")
            .expect_no_offenses("x == y.angle\n");
    }

    #[test]
    fn flags_rounding_with_positive_precision() {
        test::<FloatComparison>()
            .expect_offense(indoc! {r#"
                x == 1.1.ceil(1)
                ^^^^^^^^^^^^^^^^ Avoid equality comparisons of floats as they are unreliable.
            "#})
            .expect_offense(indoc! {r#"
                x == 1.1.floor(1)
                ^^^^^^^^^^^^^^^^^ Avoid equality comparisons of floats as they are unreliable.
            "#})
            .expect_offense(indoc! {r#"
                x == 1.1.round(1)
                ^^^^^^^^^^^^^^^^^ Avoid equality comparisons of floats as they are unreliable.
            "#})
            .expect_offense(indoc! {r#"
                x == 1.1.truncate(1)
                ^^^^^^^^^^^^^^^^^^^^ Avoid equality comparisons of floats as they are unreliable.
            "#});
    }

    #[test]
    fn does_not_flag_rounding_without_positive_precision() {
        test::<FloatComparison>()
            .expect_no_offenses("x == 1.1.ceil\n")
            .expect_no_offenses("x == 1.1.floor\n")
            .expect_no_offenses("x == 1.1.round\n")
            .expect_no_offenses("x == 1.1.truncate\n")
            .expect_no_offenses("x == 1.1.ceil(0)\n")
            .expect_no_offenses("x == 1.1.ceil(-1)\n")
            .expect_no_offenses("x == 1.1.round(n)\n")
            .expect_no_offenses("x == 1.1.round(1.5)\n")
            .expect_no_offenses("x == y.ceil(1)\n")
            .expect_no_offenses("x == y.abs\n");
    }

    #[test]
    fn accepts_zero_nil_and_epsilon_style_comparisons() {
        test::<FloatComparison>()
            .expect_no_offenses("x == 0.0\n")
            .expect_no_offenses("x == (0.0)\n")
            .expect_no_offenses("x == ((0.0))\n")
            .expect_no_offenses("Float(x, exception: false) == nil\n")
            .expect_no_offenses("(0.1; value) == x\n")
            .expect_no_offenses("(x - 0.1).abs < Float::EPSILON\n");
    }

    #[test]
    fn uses_last_expression_value_for_begin_float_detection() {
        test::<FloatComparison>().expect_offense(indoc! {r#"
            (side_effect; 0.1) == x
            ^^^^^^^^^^^^^^^^^^^^^^^ Avoid equality comparisons of floats as they are unreliable.
        "#});
    }

    // --- Characterization (murphy-s1yc.33): pin the exact node set the
    // dual send(methods=[== != eql? equal?]) + manual-csend-filter dispatch
    // matches, so the verbatim `(call _ {:== :!= :eql? :equal?} ...)` port can
    // be proven byte-identical. `call` covers safe-navigation (mirroring
    // upstream `alias on_csend on_send` plus `RESTRICT_ON_SEND
    // EQUALITY_METHODS`); trailing `...` absorbs any argument list, so the
    // one-arg and float-shape guards below apply separately.

    #[test]
    fn s1yc33_flags_csend() {
        // Safe navigation: `call` covers `csend` per murphy-if9y, mirroring
        // upstream `alias on_csend on_send`. Pre-port the `csend` handler
        // filters `==`/`!=`/`eql?`/`equal?` manually because
        // `methods = [...]` is only valid for `kind = "send"`; the verbatim
        // head collapses the workaround.
        test::<FloatComparison>().expect_offense(indoc! {r#"
            x&.eql?(0.1)
            ^^^^^^^^^^^^ Avoid equality comparisons of floats as they are unreliable.
        "#});
    }

    #[test]
    fn s1yc33_accepts_bare_receiver() {
        // Bare `eql?`: the `_` receiver binds an absent receiver per
        // murphy-if9y so the head matches, but this cop requires a receiver
        // (`let Some(lhs) ... else return`) so the offense is still rejected
        // by the complementary guard.
        test::<FloatComparison>().expect_no_offenses("eql?(0.1)\n");
    }

    #[test]
    fn s1yc33_accepts_with_extra_args() {
        // Arguments: trailing `...` absorbs the argument list so the head
        // matches, but this cop requires exactly one argument so the offense
        // is still rejected by the complementary guard.
        test::<FloatComparison>().expect_no_offenses("x.eql?(0.1, extra)\n");
    }

    #[test]
    fn s1yc33_accepts_unrelated_method() {
        // `foo` is outside the verbatim method set, so the head rejects.
        test::<FloatComparison>().expect_no_offenses("x.foo(0.1)\n");
    }
}
