//! `Lint/CircularArgumentReference` — flags optional argument default values
//! that refer back to the argument's own name.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/CircularArgumentReference
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Checks both optional positional (`optarg`) and optional keyword
//!   (`kwoptarg`) arguments. Two cases, mirroring RuboCop:
//!   (1) direct reference — the default is a local-variable read of the
//!   argument's own name (`def f(a = a)`), offense on the default node;
//!   (2) assignment chain — the default is a chain of `lvasgn` nodes whose
//!   terminal `lvar` read is either the argument name or a name assigned
//!   earlier in the chain (`def f(a = a = a)`, `def f(a = foo = a)`,
//!   `def f(a = foo = b = foo)`), offense on the terminal `lvar` node.
//!   `self.<name>` and method-call defaults are not circular. This syntax was
//!   invalid on Ruby 2.7–3.3 but is allowed again since Ruby 3.4.
//! ```
use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind};

#[derive(Default)]
pub struct CircularArgumentReference;

#[cop(
    name = "Lint/CircularArgumentReference",
    description = "Checks for circular argument references in optional keyword arguments and optional ordinal arguments.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl CircularArgumentReference {
    #[on_node(kind = "optarg")]
    fn check_optarg(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Optarg { name, default } = *cx.kind(node) else {
            return;
        };
        check_for_circular_argument_references(cx.symbol_str(name), default, cx);
    }

    #[on_node(kind = "kwoptarg")]
    fn check_kwoptarg(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Kwoptarg { name, default } = *cx.kind(node) else {
            return;
        };
        check_for_circular_argument_references(cx.symbol_str(name), default, cx);
    }
}

fn check_for_circular_argument_references(arg_name: &str, arg_value: NodeId, cx: &Cx<'_>) {
    // Direct reference: `def f(a = a)` — the default reads the arg's own name.
    if let NodeKind::Lvar(sym) = *cx.kind(arg_value)
        && cx.symbol_str(sym) == arg_name
    {
        cx.emit_offense(cx.range(arg_value), &message(arg_name), None);
        return;
    }

    check_assignment_chain(arg_name, arg_value, cx);
}

/// Mirrors RuboCop's `check_assignment_chain`: walk a chain of `lvasgn`
/// assignments down to its terminal `lvar` read, then report if that read
/// refers back to the argument name or to a name assigned earlier in the chain.
///
/// Two passes over the chain avoid any per-traversal heap allocation: the first
/// finds the terminal `lvar`, the second checks the terminal name against the
/// chain's assignment targets. Chains are bounded by source nesting depth, so
/// the second pass is cheap and there is no fixed-capacity truncation risk.
fn check_assignment_chain(arg_name: &str, node: NodeId, cx: &Cx<'_>) {
    if !matches!(cx.kind(node), NodeKind::Lvasgn { .. }) {
        return;
    }

    // Pass 1: walk to the terminal node.
    let mut current = node;
    while let NodeKind::Lvasgn { value, .. } = *cx.kind(current) {
        let Some(value) = value.get() else {
            return;
        };
        current = value;
    }

    let NodeKind::Lvar(var) = *cx.kind(current) else {
        return;
    };
    let var_name = cx.symbol_str(var);

    // Pass 2: a circular reference if the terminal read names the argument or
    // any variable assigned along the chain.
    let mut is_circular = var_name == arg_name;
    let mut walk = node;
    while let NodeKind::Lvasgn { name, value } = *cx.kind(walk) {
        if cx.symbol_str(name) == var_name {
            is_circular = true;
            break;
        }
        let Some(value) = value.get() else { break };
        walk = value;
    }

    if is_circular {
        cx.emit_offense(cx.range(current), &message(arg_name), None);
    }
}

fn message(arg_name: &str) -> String {
    format!("Circular argument reference - `{arg_name}`.")
}

murphy_plugin_api::submit_cop!(CircularArgumentReference);

#[cfg(test)]
mod tests {
    use super::CircularArgumentReference as Cop;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- optional positional (optarg) ---

    #[test]
    fn flags_simple_circular_optarg() {
        test::<Cop>().expect_offense(indoc! {r#"
            def omg_wow(msg = msg)
                              ^^^ Circular argument reference - `msg`.
              puts msg
            end
        "#});
    }

    #[test]
    fn flags_triple_circular_optarg() {
        test::<Cop>().expect_offense(indoc! {r#"
            def omg_wow(msg = msg = msg)
                                    ^^^ Circular argument reference - `msg`.
              puts msg
            end
        "#});
    }

    #[test]
    fn flags_circular_with_intermediate_argument() {
        test::<Cop>().expect_offense(indoc! {r#"
            def omg_wow(msg = foo = msg)
                                    ^^^ Circular argument reference - `msg`.
              puts msg
            end
        "#});
    }

    #[test]
    fn flags_circular_with_two_intermediate_arguments() {
        test::<Cop>().expect_offense(indoc! {r#"
            def omg_wow(msg = foo = msg2 = foo)
                                           ^^^ Circular argument reference - `msg`.
              puts msg
            end
        "#});
    }

    #[test]
    fn ignores_non_circular_assignment_chain() {
        test::<Cop>().expect_no_offenses(indoc! {r#"
            def omg_wow(msg = foo = self.msg)
              puts msg
            end
        "#});
    }

    #[test]
    fn ignores_simple_parameter() {
        test::<Cop>().expect_no_offenses(indoc! {r#"
            def omg_wow(msg)
              puts msg
            end
        "#});
    }

    #[test]
    fn ignores_self_method_default() {
        test::<Cop>().expect_no_offenses(indoc! {r#"
            def omg_wow(msg = self.msg)
              puts msg
            end
        "#});
    }

    // --- optional keyword (kwoptarg) ---

    #[test]
    fn ignores_non_circular_keyword() {
        test::<Cop>().expect_no_offenses(indoc! {r#"
            def some_method(some_arg: nil)
              puts some_arg
            end
        "#});
    }

    #[test]
    fn ignores_keyword_calling_method() {
        test::<Cop>().expect_no_offenses(indoc! {r#"
            def some_method(some_arg: some_method)
              puts some_arg
            end
        "#});
    }

    #[test]
    fn flags_single_circular_keyword() {
        test::<Cop>().expect_offense(indoc! {r#"
            def some_method(some_arg: some_arg)
                                      ^^^^^^^^ Circular argument reference - `some_arg`.
              puts some_arg
            end
        "#});
    }

    #[test]
    fn ignores_method_on_own_class() {
        test::<Cop>().expect_no_offenses(indoc! {r#"
            def puts_value(value: self.class.value, smile: self.smile)
              puts value
            end
        "#});
    }

    #[test]
    fn ignores_method_on_different_object() {
        test::<Cop>().expect_no_offenses(indoc! {r#"
            def puts_length(length: mystring.length)
              puts length
            end
        "#});
    }

    #[test]
    fn flags_multiple_circular_keywords() {
        test::<Cop>().expect_offense(indoc! {r#"
            def some_method(some_arg: some_arg, other_arg: other_arg)
                                      ^^^^^^^^ Circular argument reference - `some_arg`.
                                                           ^^^^^^^^^ Circular argument reference - `other_arg`.
              puts [some_arg, other_arg]
            end
        "#});
    }
}
