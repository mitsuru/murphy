//! `Lint/AmbiguousBlockAssociation` — flag ambiguous block association when a
//! block is passed to a method whose argument is itself a method call without
//! parentheses, e.g. `some_method a { |val| puts val }`.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/AmbiguousBlockAssociation
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's Lint/AmbiguousBlockAssociation. The cop fires on a
//!   `send`/`csend` with arguments whose last argument is a block whose inner
//!   send has no arguments, and skips when the call is parenthesized, the last
//!   argument is a lambda/proc, the call is an assignment / operator method /
//!   `[]` accessor, or the inner method name matches `AllowedMethods` /
//!   `AllowedPatterns`. Autocorrect removes the whitespace between the
//!   selector and the first argument, inserts `(` before the first argument,
//!   and `)` after the call.
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, NodeList, OptNodeId, Range, cop};

#[derive(Default)]
pub struct AmbiguousBlockAssociation;

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "AllowedMethods",
        default = [],
        description = "Method names whose ambiguous block association is allowed."
    )]
    pub allowed_methods: Vec<String>,

    #[option(
        name = "AllowedPatterns",
        default = [],
        description = "Regex patterns of method names whose ambiguous block association is allowed."
    )]
    pub allowed_patterns: Vec<String>,
}

#[cop(
    name = "Lint/AmbiguousBlockAssociation",
    description = "Flag ambiguous block association with a method passed without parentheses.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl AmbiguousBlockAssociation {
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

impl AmbiguousBlockAssociation {
    fn check(&self, node: NodeId, args: NodeList, cx: &Cx<'_>) {
        let args_list = cx.list(args);
        // `return unless node.arguments?`
        let Some(&last_argument) = args_list.last() else {
            return;
        };

        // `return unless ambiguous_block_association?(node)`
        if !ambiguous_block_association(last_argument, cx) {
            return;
        }

        // `return if node.parenthesized? || node.last_argument.lambda_or_proc? ||
        //            allowed_method_pattern?(node)`
        if cx.is_parenthesized(node)
            || is_lambda_or_proc(last_argument, cx)
            || self.allowed_method_pattern(node, last_argument, cx)
        {
            return;
        }

        let param_src = cx.raw_source(cx.range(last_argument));
        let method_name = cx.method_name(node).unwrap_or("");
        let msg = format!(
            "Parenthesize the param `{param_src}` to make sure that the block will be associated with the `{method_name}` method call."
        );
        cx.emit_offense(cx.range(node), &msg, None);

        self.wrap_in_parentheses(node, args_list, cx);
    }

    /// `allowed_method_pattern?(node)` — assignment / operator / `[]` accessor,
    /// or the inner method name is allowed by config.
    fn allowed_method_pattern(&self, node: NodeId, last_argument: NodeId, cx: &Cx<'_>) -> bool {
        if cx.is_assignment_method(node) || cx.is_operator_method(node) {
            return true;
        }
        if cx.method_name(node) == Some("[]") {
            return true;
        }

        let inner_send = match block_inner_send(last_argument, cx).get() {
            Some(call) => call,
            None => return false,
        };
        let Some(inner_method) = cx.method_name(inner_send) else {
            return false;
        };

        let opts = cx.options_or_default::<Options>();
        if opts.allowed_methods.iter().any(|m| m == inner_method) {
            return true;
        }
        // `matches_allowed_pattern?(node.last_argument.send_node.source)`
        let inner_src = cx.raw_source(cx.range(inner_send));
        cx.matches_any_pattern(inner_src, &opts.allowed_patterns)
    }

    /// `wrap_in_parentheses(corrector, node)` — remove whitespace between the
    /// selector and the first argument, insert `(` before the first argument,
    /// and `)` after the call.
    fn wrap_in_parentheses(&self, node: NodeId, args_list: &[NodeId], cx: &Cx<'_>) {
        let Some(&first_argument) = args_list.first() else {
            return;
        };
        let selector_end = cx.loc(node).name.end;
        let first_arg_start = cx.range(first_argument).start;
        if first_arg_start >= selector_end {
            // Replace the whitespace between selector and first arg with `(`.
            cx.emit_edit(
                Range {
                    start: selector_end,
                    end: first_arg_start,
                },
                "(",
            );
        }
        // Insert `)` after the call.
        let node_end = cx.range(node).end;
        cx.emit_edit(
            Range {
                start: node_end,
                end: node_end,
            },
            ")",
        );
    }
}

/// The inner method-call (`call`/`send`) of any block flavour — plain
/// `Block`, numbered `Numblock` (`{ _1 }`), or `Itblock` (`{ it }`).
/// `cx.block_call` only resolves `Block`, so numblocks/itblocks are handled
/// explicitly here (RuboCop's `send_node` works for all three).
fn block_inner_send(last_argument: NodeId, cx: &Cx<'_>) -> OptNodeId {
    match *cx.kind(last_argument) {
        NodeKind::Block { call, .. } => OptNodeId::some(call),
        NodeKind::Numblock { send, .. } | NodeKind::Itblock { send, .. } => OptNodeId::some(send),
        _ => OptNodeId::NONE,
    }
}

/// `ambiguous_block_association?(send_node)` — the last argument is a block
/// whose inner send has no arguments.
fn ambiguous_block_association(last_argument: NodeId, cx: &Cx<'_>) -> bool {
    if !cx.is_any_block_type(last_argument) {
        return false;
    }
    let Some(inner_send) = block_inner_send(last_argument, cx).get() else {
        return false;
    };
    cx.call_arguments(inner_send).is_empty()
}

/// `lambda_or_proc?` — the block is a lambda (`-> {}` / `lambda {}`) or a
/// `proc {}` call.
fn is_lambda_or_proc(last_argument: NodeId, cx: &Cx<'_>) -> bool {
    if cx.is_lambda(last_argument) {
        return true;
    }
    let Some(call) = block_inner_send(last_argument, cx).get() else {
        return false;
    };
    cx.call_receiver(call).get().is_none() && cx.method_name(call) == Some("proc")
}

murphy_plugin_api::submit_cop!(AmbiguousBlockAssociation);

#[cfg(test)]
mod tests {
    use super::{AmbiguousBlockAssociation, Options};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_ambiguous_block_association() {
        test::<AmbiguousBlockAssociation>().expect_offense(indoc! {r#"
            some_method a { |val| puts val }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Parenthesize the param `a { |val| puts val }` to make sure that the block will be associated with the `some_method` method call.
        "#});
    }

    #[test]
    fn corrects_ambiguous_block_association() {
        test::<AmbiguousBlockAssociation>().expect_correction(
            indoc! {r#"
                some_method a { |val| puts val }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Parenthesize the param `a { |val| puts val }` to make sure that the block will be associated with the `some_method` method call.
            "#},
            "some_method(a { |val| puts val })\n",
        );
    }

    #[test]
    fn accepts_parenthesized_param_with_block() {
        test::<AmbiguousBlockAssociation>()
            .expect_no_offenses("some_method(a { |val| puts val })\n");
    }

    #[test]
    fn accepts_block_attached_to_outer_method() {
        test::<AmbiguousBlockAssociation>()
            .expect_no_offenses("some_method(a) { |val| puts val }\n");
    }

    #[test]
    fn accepts_inner_send_with_arguments() {
        test::<AmbiguousBlockAssociation>().expect_no_offenses("some_method a(b) { |val| puts val }\n");
    }

    #[test]
    fn accepts_parenthesized_to_call() {
        test::<AmbiguousBlockAssociation>()
            .expect_no_offenses("expect { order.save }.to(change { orders.size })\n");
    }

    #[test]
    fn flags_unparenthesized_to_call() {
        test::<AmbiguousBlockAssociation>().expect_offense(indoc! {r#"
            expect { order.expire }.to change { order.events }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Parenthesize the param `change { order.events }` to make sure that the block will be associated with the `to` method call.
        "#});
    }

    #[test]
    fn accepts_assignment() {
        test::<AmbiguousBlockAssociation>().expect_no_offenses("foo = bar { |x| x }\n");
    }

    #[test]
    fn accepts_element_reference() {
        test::<AmbiguousBlockAssociation>().expect_no_offenses("rest.public_send(:[], 0) { |item| item }\n");
    }

    #[test]
    fn accepts_proc_argument() {
        test::<AmbiguousBlockAssociation>().expect_no_offenses("some_method proc { |val| puts val }\n");
    }

    #[test]
    fn accepts_stabby_lambda_argument() {
        test::<AmbiguousBlockAssociation>().expect_no_offenses("some_method -> { puts 'hi' }\n");
    }

    #[test]
    fn allowed_methods_skips_inner_method() {
        test::<AmbiguousBlockAssociation>()
            .with_options(&Options {
                allowed_methods: vec!["change".to_string()],
                allowed_patterns: vec![],
            })
            .expect_no_offenses("expect { order.expire }.to change { order.events }\n");
    }

    #[test]
    fn allowed_patterns_skips_inner_method() {
        test::<AmbiguousBlockAssociation>()
            .with_options(&Options {
                allowed_methods: vec![],
                allowed_patterns: vec!["change".to_string()],
            })
            .expect_no_offenses("expect { order.expire }.to change { order.events }\n");
    }

    #[test]
    fn flags_numbered_block_param() {
        test::<AmbiguousBlockAssociation>().expect_offense(indoc! {r#"
            some_method a { puts _1 }
            ^^^^^^^^^^^^^^^^^^^^^^^^^ Parenthesize the param `a { puts _1 }` to make sure that the block will be associated with the `some_method` method call.
        "#});
    }

    #[test]
    fn flags_safe_navigation_call() {
        test::<AmbiguousBlockAssociation>().expect_offense(indoc! {r#"
            foo&.some_method a { |val| puts val }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Parenthesize the param `a { |val| puts val }` to make sure that the block will be associated with the `some_method` method call.
        "#});
    }
}
