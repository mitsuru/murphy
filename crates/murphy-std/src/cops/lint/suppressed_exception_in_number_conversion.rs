//! `Lint/SuppressedExceptionInNumberConversion` — checks numeric constructors
//! whose conversion errors are suppressed with `rescue nil`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/SuppressedExceptionInNumberConversion
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Matches RuboCop's on_rescue coverage: modifier rescue, begin/rescue,
//!   expected ArgumentError/TypeError class filters, Kernel/::Kernel receiver
//!   forms, safe navigation, numeric constructor arity, Ruby >= 2.6 gate, and
//!   unsafe autocorrection to `exception: false`.
//! ```

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind, Range};

const MSG_TEMPLATE_PREFIX: &str = "Use `";
const EXPECTED_EXCEPTION_CLASSES: &[&str] = &["ArgumentError", "TypeError"];

#[derive(Default)]
pub struct SuppressedExceptionInNumberConversion;

#[cop(
    name = "Lint/SuppressedExceptionInNumberConversion",
    description = "Checks numeric constructors whose conversion errors are suppressed with rescue nil.",
    default_severity = "warning",
    default_enabled = true,
    minimum_target_ruby_version = "2.6",
    safe = false,
    options = NoOptions,
)]
impl SuppressedExceptionInNumberConversion {
    #[on_node(kind = "rescue")]
    fn check_rescue(&self, node: NodeId, cx: &Cx<'_>) {
        let Some(offense) = offense(node, cx) else {
            return;
        };
        let message = format!("{MSG_TEMPLATE_PREFIX}{}` instead.", offense.prefer);
        cx.emit_offense(offense.offense_range, &message, None);
        cx.emit_edit(offense.edit_range, &offense.prefer);
    }
}

struct NumericRescueOffense {
    offense_range: Range,
    edit_range: Range,
    prefer: String,
}

fn offense(node: NodeId, cx: &Cx<'_>) -> Option<NumericRescueOffense> {
    let rescue = rescue_match(node, cx)?;
    let range_node = if is_kwbegin_rescue(node, cx) {
        cx.parent(node).get().unwrap_or(node)
    } else {
        node
    };
    let edit_range = cx.range(range_node);
    Some(NumericRescueOffense {
        offense_range: if range_node == node { edit_range } else { first_line_range(edit_range, cx) },
        edit_range,
        prefer: preferred_call(rescue.numeric_call, cx)?,
    })
}

fn first_line_range(range: Range, cx: &Cx<'_>) -> Range {
    let source = cx.source();
    let start = range.start as usize;
    let end = source[start..]
        .find('\n')
        .map_or(range.end, |offset| range.start + offset as u32);
    Range { start: range.start, end }
}

struct NumericRescueMatch {
    numeric_call: NodeId,
}

fn rescue_match(node: NodeId, cx: &Cx<'_>) -> Option<NumericRescueMatch> {
    let NodeKind::Rescue { body, resbodies, else_ } = *cx.kind(node) else {
        return None;
    };
    if else_.get().is_some() {
        return None;
    }
    let numeric_call = body.get()?;
    if !is_numeric_constructor_call(numeric_call, cx) {
        return None;
    }
    let [resbody] = cx.list(resbodies) else {
        return None;
    };
    if !resbody_rescues_to_nil(*resbody, cx) || !expected_exception_classes_only(*resbody, cx) {
        return None;
    }
    Some(NumericRescueMatch { numeric_call })
}

fn is_kwbegin_rescue(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    let is_single_child_begin = match *cx.kind(parent) {
        NodeKind::Kwbegin(list) | NodeKind::Begin(list) => cx.list(list) == [node],
        _ => false,
    };
    is_single_child_begin && cx.raw_source(cx.range(parent)).starts_with("begin")
}

fn resbody_rescues_to_nil(resbody: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Resbody { body, .. } = *cx.kind(resbody) else {
        return false;
    };
    body.get().is_none_or(|body| matches!(*cx.kind(body), NodeKind::Nil))
}

fn expected_exception_classes_only(resbody: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Resbody { exceptions, .. } = *cx.kind(resbody) else {
        return false;
    };
    cx.list(exceptions).iter().all(|&exception| {
        const_name(exception, cx).is_some_and(|name| EXPECTED_EXCEPTION_CLASSES.contains(&name))
    })
}

fn const_name<'a>(node: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    let NodeKind::Const { scope, name } = *cx.kind(node) else {
        return None;
    };
    if scope.get().is_none_or(|scope| matches!(*cx.kind(scope), NodeKind::Cbase)) {
        Some(cx.symbol_str(name))
    } else {
        None
    }
}

fn is_numeric_constructor_call(node: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Send { receiver, method, args } => {
            constructor_receiver(receiver.get(), cx)
                && numeric_constructor_arity(cx.symbol_str(method), cx.list(args), cx)
        }
        NodeKind::Csend { receiver, method, args } => {
            constructor_receiver(Some(receiver), cx)
                && numeric_constructor_arity(cx.symbol_str(method), cx.list(args), cx)
        }
        _ => false,
    }
}

fn constructor_receiver(receiver: Option<NodeId>, cx: &Cx<'_>) -> bool {
    match receiver {
        None => true,
        Some(receiver) => cx.is_global_const(receiver, "Kernel"),
    }
}

fn numeric_constructor_arity(method: &str, args: &[NodeId], cx: &Cx<'_>) -> bool {
    if args.iter().any(|&arg| contains_exception_keyword(arg, cx)) {
        return false;
    }
    let arity = args.len();
    match method {
        "Integer" | "BigDecimal" | "Complex" | "Rational" => matches!(arity, 1 | 2),
        "Float" => arity == 1,
        _ => false,
    }
}

fn contains_exception_keyword(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Hash(pairs) = *cx.kind(node) else {
        return false;
    };
    cx.list(pairs).iter().any(|&pair| {
        let NodeKind::Pair { key, .. } = *cx.kind(pair) else {
            return false;
        };
        matches!(*cx.kind(key), NodeKind::Sym(sym) if cx.symbol_str(sym) == "exception")
    })
}

fn preferred_call(node: NodeId, cx: &Cx<'_>) -> Option<String> {
    match *cx.kind(node) {
        NodeKind::Send { receiver, method, args } => {
            Some(format_preferred_call(receiver.get(), cx.symbol_str(method), cx.list(args), node, cx))
        }
        NodeKind::Csend { receiver, method, args } => {
            Some(format_preferred_call(Some(receiver), cx.symbol_str(method), cx.list(args), node, cx))
        }
        _ => None,
    }
}

fn format_preferred_call(
    receiver: Option<NodeId>,
    method: &str,
    args: &[NodeId],
    node: NodeId,
    cx: &Cx<'_>,
) -> String {
    let mut arguments = args
        .iter()
        .map(|&arg| cx.raw_source(cx.range(arg)).to_string())
        .collect::<Vec<_>>();
    arguments.push("exception: false".to_string());

    let prefix = receiver.map_or_else(String::new, |receiver| {
        let operator = cx.call_operator_loc(node).map_or(".", |range| cx.raw_source(range));
        format!("{}{}", cx.raw_source(cx.range(receiver)), operator)
    });
    format!("{prefix}{method}({})", arguments.join(", "))
}

#[cfg(test)]
mod tests {
    use super::SuppressedExceptionInNumberConversion;
    use murphy_plugin_api::{Range, test_support::{indoc, run_cop_with_edits, test}};

    #[test]
    fn flags_integer_rescue_nil() {
        test::<SuppressedExceptionInNumberConversion>().expect_correction(
            indoc! {r#"
                Integer(arg) rescue nil
                ^^^^^^^^^^^^^^^^^^^^^^^ Use `Integer(arg, exception: false)` instead.
            "#},
            "Integer(arg, exception: false)\n",
        );
    }

    #[test]
    fn flags_numeric_constructors_with_valid_arities() {
        test::<SuppressedExceptionInNumberConversion>()
            .expect_correction(
                indoc! {r#"
                    Integer(arg, base) rescue nil
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Integer(arg, base, exception: false)` instead.
                "#},
                "Integer(arg, base, exception: false)\n",
            )
            .expect_correction(
                indoc! {r#"
                    Float(arg) rescue nil
                    ^^^^^^^^^^^^^^^^^^^^^ Use `Float(arg, exception: false)` instead.
                "#},
                "Float(arg, exception: false)\n",
            )
            .expect_correction(
                indoc! {r#"
                    BigDecimal(s, n) rescue nil
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `BigDecimal(s, n, exception: false)` instead.
                "#},
                "BigDecimal(s, n, exception: false)\n",
            )
            .expect_correction(
                indoc! {r#"
                    Complex(r, i) rescue nil
                    ^^^^^^^^^^^^^^^^^^^^^^^^ Use `Complex(r, i, exception: false)` instead.
                "#},
                "Complex(r, i, exception: false)\n",
            )
            .expect_correction(
                indoc! {r#"
                    Rational(x, y) rescue nil
                    ^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Rational(x, y, exception: false)` instead.
                "#},
                "Rational(x, y, exception: false)\n",
            );
    }

    #[test]
    fn flags_begin_rescue_nil_and_expected_exception_classes() {
        assert_begin_rescue_correction(indoc! {r#"
            begin
              Integer(arg)
            rescue
              nil
            end
        "#});
        assert_begin_rescue_correction(indoc! {r#"
            begin
              Integer(arg)
            rescue ArgumentError, TypeError
              nil
            end
        "#});
        assert_begin_rescue_correction(indoc! {r#"
            begin
              Integer(arg)
            rescue ::ArgumentError, ::TypeError
            end
        "#});
    }

    fn assert_begin_rescue_correction(source: &str) {
        let captured = run_cop_with_edits::<SuppressedExceptionInNumberConversion>(source);
        assert_eq!(captured.offenses.len(), 1);
        assert_eq!(captured.edits.len(), 1);
        assert_eq!(
            captured.offenses[0].message,
            "Use `Integer(arg, exception: false)` instead."
        );
        assert_eq!(captured.offenses[0].range, Range { start: 0, end: 5 });
        assert_eq!(captured.edits[0].range, Range { start: 0, end: source.trim_end_matches('\n').len() as u32 });
        assert_eq!(captured.edits[0].replacement, "Integer(arg, exception: false)");
    }

    #[test]
    fn flags_kernel_receiver_forms() {
        test::<SuppressedExceptionInNumberConversion>()
            .expect_correction(
                indoc! {r#"
                    Kernel::Integer(arg) rescue nil
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Kernel::Integer(arg, exception: false)` instead.
                "#},
                "Kernel::Integer(arg, exception: false)\n",
            )
            .expect_correction(
                indoc! {r#"
                    ::Kernel&.Float(arg) rescue nil
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `::Kernel&.Float(arg, exception: false)` instead.
                "#},
                "::Kernel&.Float(arg, exception: false)\n",
            );
    }

    #[test]
    fn accepts_non_matching_rescues() {
        test::<SuppressedExceptionInNumberConversion>().expect_no_offenses(indoc! {r#"
            Integer(42, exception: false)
            Integer(arg, exception: true) rescue nil
            BigDecimal(arg, exception: true) rescue nil
            Complex(arg, exception: true) rescue nil
            Rational(arg, exception: true) rescue nil
            Integer(arg) rescue 42
            Float(arg, unexpected_arg) rescue nil

            begin
              Integer(arg)
            rescue CustomError
              nil
            end

            begin
              Integer(arg)
            rescue
              42
            end

            begin
              Integer(arg)
            rescue
              nil
            else
              42
            end
        "#});
    }
}

murphy_plugin_api::submit_cop!(SuppressedExceptionInNumberConversion);
