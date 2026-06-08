//! `Lint/RedundantTypeConversion` — detects redundant `to_*` conversions.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/RedundantTypeConversion
//! upstream_version_checked: master
//! status: partial
//! gap_issues: []
//! notes: >
//!   Initial port targets the common v1 shapes: literal receivers,
//!   representative core constructors, same-conversion chains, and typed
//!   `inspect.to_s` / `to_json.to_s` chains. Known v1 limitation: the port
//!   does not yet mirror every RuboCop constructor edge case, `exception: false`
//!   keyword suppression, parenthesized receiver unwrapping, or every csend
//!   autocorrect shape.
//! ```

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind, Range};

#[derive(Default)]
pub struct RedundantTypeConversion;

#[cop(
    name = "Lint/RedundantTypeConversion",
    description = "Checks for redundant type conversion calls.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RedundantTypeConversion {
    #[on_node(kind = "send", methods = ["to_s", "to_sym", "to_i", "to_f", "to_d", "to_r", "to_c", "to_a", "to_h", "to_set"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check_conversion(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        if cx.method_name(node).is_some_and(is_conversion_method) {
            check_conversion(node, cx);
        }
    }
}

fn check_conversion(node: NodeId, cx: &Cx<'_>) {
    if !cx.call_arguments(node).is_empty() || hash_or_set_with_block(node, cx) {
        return;
    }
    let Some(method) = cx.method_name(node) else {
        return;
    };
    let Some(receiver) = cx.call_receiver(node).get() else {
        return;
    };
    let receiver = unwrap_begin(receiver, cx);
    if !(literal_receiver(method, receiver, cx)
        || constructor_receiver(method, receiver, cx)
        || chained_conversion(method, receiver, cx)
        || chained_typed_method(method, receiver, cx))
    {
        return;
    }

    let msg = format!("Redundant `{method}` detected.");
    cx.emit_offense(cx.selector(node), &msg, None);
    if let Some(op) = cx.call_operator_loc(node) {
        cx.emit_edit(
            Range {
                start: op.start,
                end: cx.range(node).end,
            },
            "",
        );
    }
}

fn is_conversion_method(method: &str) -> bool {
    matches!(
        method,
        "to_s" | "to_sym" | "to_i" | "to_f" | "to_d" | "to_r" | "to_c" | "to_a" | "to_h" | "to_set"
    )
}

fn unwrap_begin(mut node: NodeId, cx: &Cx<'_>) -> NodeId {
    loop {
        match *cx.kind(node) {
            NodeKind::Begin(list) | NodeKind::Kwbegin(list) if cx.list(list).len() == 1 => {
                node = cx.list(list)[0];
            }
            _ => return node,
        }
    }
}

fn literal_receiver(method: &str, node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        (method, *cx.kind(node)),
        ("to_s", NodeKind::Str(_) | NodeKind::Dstr(_))
            | ("to_sym", NodeKind::Sym(_) | NodeKind::Dsym(_))
            | ("to_i", NodeKind::Int(_))
            | ("to_f", NodeKind::Float(_))
            | ("to_r", NodeKind::Rational(_))
            | ("to_c", NodeKind::Complex(_))
            | ("to_a", NodeKind::Array(_))
            | ("to_h", NodeKind::Hash(_))
    )
}

fn constructor_receiver(method: &str, node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(name) = cx.method_name(node) else {
        return false;
    };
    match method {
        "to_s" => {
            name == "new" && call_receiver_const(node, "String", cx)
                || kernel_constructor(node, "String", cx)
        }
        "to_i" => kernel_constructor(node, "Integer", cx),
        "to_f" => kernel_constructor(node, "Float", cx),
        "to_d" => kernel_constructor(node, "BigDecimal", cx),
        "to_r" => kernel_constructor(node, "Rational", cx),
        "to_c" => kernel_constructor(node, "Complex", cx),
        "to_a" => {
            name == "new" && call_receiver_const(node, "Array", cx)
                || name == "[]" && call_receiver_const(node, "Array", cx)
                || kernel_constructor(node, "Array", cx)
        }
        "to_h" => {
            name == "new" && call_receiver_const(node, "Hash", cx)
                || name == "[]" && call_receiver_const(node, "Hash", cx)
                || kernel_constructor(node, "Hash", cx)
        }
        "to_set" => (name == "new" || name == "[]") && call_receiver_const(node, "Set", cx),
        _ => false,
    }
}

fn kernel_constructor(node: NodeId, constructor: &str, cx: &Cx<'_>) -> bool {
    if cx.method_name(node) != Some(constructor) {
        return false;
    }
    match cx.call_receiver(node).get() {
        None => true,
        Some(receiver) => cx.is_global_const(receiver, "Kernel"),
    }
}

fn call_receiver_const(node: NodeId, name: &str, cx: &Cx<'_>) -> bool {
    cx.call_receiver(node)
        .get()
        .is_some_and(|receiver| cx.is_global_const(receiver, name))
}

fn chained_conversion(method: &str, receiver: NodeId, cx: &Cx<'_>) -> bool {
    cx.method_name(receiver) == Some(method)
}

fn chained_typed_method(method: &str, receiver: NodeId, cx: &Cx<'_>) -> bool {
    method == "to_s" && matches!(cx.method_name(receiver), Some("inspect" | "to_json"))
}

fn hash_or_set_with_block(node: NodeId, cx: &Cx<'_>) -> bool {
    if !matches!(cx.method_name(node), Some("to_h" | "to_set")) {
        return false;
    }
    cx.block_node(node).get().is_some()
        || cx
            .call_arguments(node)
            .iter()
            .any(|&arg| matches!(cx.kind(arg), NodeKind::BlockPass(_)))
}

murphy_plugin_api::submit_cop!(RedundantTypeConversion);

#[cfg(test)]
mod tests {
    use super::RedundantTypeConversion;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_and_corrects_literal_receivers() {
        let t = test::<RedundantTypeConversion>();
        t.expect_correction(
            indoc! {r#"
                "text".to_s
                       ^^^^ Redundant `to_s` detected.
            "#},
            "\"text\"\n",
        );
        t.expect_correction(
            indoc! {r#"
                :sym.to_sym
                     ^^^^^^ Redundant `to_sym` detected.
            "#},
            ":sym\n",
        );
        t.expect_correction(
            indoc! {r#"
                [1, 2].to_a
                       ^^^^ Redundant `to_a` detected.
            "#},
            "[1, 2]\n",
        );
    }

    #[test]
    fn flags_constructor_and_chain_cases() {
        let t = test::<RedundantTypeConversion>();
        t.expect_correction(
            indoc! {r#"
                Integer(value).to_i
                               ^^^^ Redundant `to_i` detected.
            "#},
            "Integer(value)\n",
        );
        t.expect_correction(
            indoc! {r#"
                foo.to_s.to_s
                         ^^^^ Redundant `to_s` detected.
            "#},
            "foo.to_s\n",
        );
        t.expect_correction(
            indoc! {r#"
                foo.inspect.to_s
                            ^^^^ Redundant `to_s` detected.
            "#},
            "foo.inspect\n",
        );
    }

    #[test]
    fn accepts_non_redundant_conversion() {
        test::<RedundantTypeConversion>()
            .expect_no_offenses("foo.to_s\n")
            .expect_no_offenses("1.to_s\n")
            .expect_no_offenses("String(value).to_i\n");
    }
}
