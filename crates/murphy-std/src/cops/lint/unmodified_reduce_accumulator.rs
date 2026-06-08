//! `Lint/UnmodifiedReduceAccumulator` — Checks for `reduce`/`inject` blocks
//! where the accumulator is never modified in the return value.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/UnmodifiedReduceAccumulator
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   reduce/inject blocks where the accumulator is not included in any
//!   return value and the element is not modified in the block body are
//!   flagged.  Accumulator index returns are always flagged.  Inner blocks
//!   are skipped when collecting return values.
//! ```
//!
//! ## Matched shapes
//!
//! - `(1..4).reduce(0) { |acc, el| el }` — element returned directly.
//! - `(1..4).reduce(0) { |acc, el| el + 2 }` — expression with only element.
//! - `%w(a b c).reduce({}) { |acc, letter| acc[foo] }` — accumulator index.
//!
//! ## No autocorrect
//!
//! There is no safe mechanical rewrite: the fix depends on understanding
//! how the accumulator should be modified.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Symbol, cop};

#[derive(Default)]
pub struct UnmodifiedReduceAccumulator;

#[cop(
    name = "Lint/UnmodifiedReduceAccumulator",
    description = "Checks for unmodified accumulator in reduce/inject blocks.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl UnmodifiedReduceAccumulator {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call: _, args, body, .. } = *cx.kind(node) else { return; };

        let method = cx.method_name(node);
        if method != Some("reduce") && method != Some("inject") {
            return;
        }

        let NodeKind::Args(args_list) = *cx.kind(args) else { return; };
        let arg_nodes = cx.list(args_list);
        if arg_nodes.len() < 2 {
            return;
        }
        let NodeKind::Arg(acc_name) = *cx.kind(arg_nodes[0]) else { return; };
        let NodeKind::Arg(el_name) = *cx.kind(arg_nodes[1]) else { return; };

        let Some(body_id) = body.get() else { return; };
        let return_values = collect_return_values(body_id, node, cx);

        let method_str = method.unwrap();

        if let Some(&idx_node) = return_values
            .iter()
            .find(|&&rv| is_accumulator_index(rv, acc_name, el_name, cx))
        {
            let msg = format!(
                "Do not return an element of the accumulator in `{}`.",
                method_str
            );
            cx.emit_offense(cx.range(idx_node), &msg, None);
            return;
        }

        if return_values
            .iter()
            .any(|&rv| lvar_used(rv, acc_name, cx))
        {
            return;
        }

        if element_modified(body_id, el_name, cx) {
            return;
        }

        for &rv in &return_values {
            if !acceptable_return(rv, el_name, cx) {
                let msg = format!(
                    "Ensure the accumulator `{}` will be modified by `{}`.",
                    cx.symbol_str(acc_name),
                    method_str,
                );
                cx.emit_offense(cx.range(rv), &msg, None);
            }
        }
    }
}

fn collect_return_values(body_id: NodeId, block_id: NodeId, cx: &Cx<'_>) -> Vec<NodeId> {
    let mut values = Vec::new();

    if let NodeKind::Begin(list) | NodeKind::Kwbegin(list) = *cx.kind(body_id) {
        let children = cx.list(list);
        if let Some(&last) = children.last() {
            values.push(last);
        }
    } else {
        values.push(body_id);
    }

    for &d in cx.descendants(body_id).iter() {
        match *cx.kind(d) {
            NodeKind::Next(val) | NodeKind::Break(val) => {
                if let Some(v) = val.get()
                    && !inside_inner_block(d, block_id, cx)
                {
                    values.push(v);
                }
            }
            _ => {}
        }
    }

    values
}

fn inside_inner_block(node: NodeId, outer_block: NodeId, cx: &Cx<'_>) -> bool {
    for ancestor in cx.ancestors(node) {
        if ancestor == outer_block {
            return false;
        }
        if matches!(
            *cx.kind(ancestor),
            NodeKind::Block { .. }
                | NodeKind::Numblock { .. }
                | NodeKind::Itblock { .. }
                | NodeKind::Lambda
        ) {
            return true;
        }
    }
    false
}

fn is_accumulator_index(
    node: NodeId,
    acc_name: Symbol,
    el_name: Symbol,
    cx: &Cx<'_>,
) -> bool {
    let NodeKind::Send { receiver, method, args, .. } = *cx.kind(node) else {
        return false;
    };
    let method_str = cx.symbol_str(method);
    if method_str != "[]" && method_str != "[]=" {
        return false;
    }
    let Some(recv_id) = receiver.get() else {
        return false;
    };
    if !matches!(*cx.kind(recv_id), NodeKind::Lvar(n) if n == acc_name) {
        return false;
    }
    if method_str == "[]=" {
        return true;
    }
    let args_list = cx.list(args);
    if args_list.is_empty() {
        return true;
    }
    !args_list
        .iter()
        .any(|&a| matches!(*cx.kind(a), NodeKind::Lvar(n) if n == el_name))
}

fn lvar_used(node: NodeId, name: Symbol, cx: &Cx<'_>) -> bool {
    match cx.kind(node) {
        NodeKind::Lvar(n) if *n == name => return true,
        NodeKind::Lvasgn { name: n, .. } if *n == name => return true,
        NodeKind::Send { receiver, method, .. } => {
            if let Some(recv_id) = receiver.get()
                && matches!(*cx.kind(recv_id), NodeKind::Lvar(n) if n == name)
                && cx.symbol_str(*method) == "<<"
            {
                return true;
            }
        }
        NodeKind::Dstr(list) | NodeKind::Dsym(list) | NodeKind::Xstr(list) => {
            return cx.list(*list).iter().any(|&child| {
                if let NodeKind::Begin(inner) = *cx.kind(child) {
                    cx.list(inner).iter().any(|&c| matches!(*cx.kind(c), NodeKind::Lvar(n) if n == name))
                } else {
                    false
                }
            });
        }
        _ => {}
    }

    cx.descendants(node).iter().any(|&d| {
        matches!(*cx.kind(d), NodeKind::Lvar(n) if n == name)
    })
}

fn element_modified(node: NodeId, el_name: Symbol, cx: &Cx<'_>) -> bool {
    let mut nodes = vec![node];
    nodes.extend(cx.descendants(node));
    for &d in &nodes {
        match *cx.kind(d) {
            NodeKind::Lvasgn { name, value } if name == el_name && value.get().is_some() => {
                return true;
            }
            NodeKind::OpAsgn { target, .. }
            | NodeKind::OrAsgn { target, .. }
            | NodeKind::AndAsgn { target, .. } => {
                if matches!(*cx.kind(target), NodeKind::Lvasgn { name: n, .. } if n == el_name) {
                    return true;
                }
            }
            NodeKind::Send { receiver, method, args, .. } => {
                let m = cx.symbol_str(method);
                if m != "[]" && m != "[]=" {
                    let args_list = cx.list(args);
                    if args_list
                        .iter()
                        .any(|&a| matches!(*cx.kind(a), NodeKind::Lvar(n) if n == el_name))
                    {
                        return true;
                    }
                }
                if let Some(recv_id) = receiver.get()
                    && matches!(*cx.kind(recv_id), NodeKind::Lvar(n) if n == el_name)
                {
                    if m == "<<" {
                        return true;
                    }
                    let args_list = cx.list(args);
                    if args_list.iter().any(|&a| {
                        matches!(
                            *cx.kind(a),
                            NodeKind::Lvar(_)
                                | NodeKind::Ivar(_)
                                | NodeKind::Cvar(_)
                                | NodeKind::Gvar(_)
                                | NodeKind::Send { .. }
                        )
                    }) {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

fn acceptable_return(node: NodeId, el_name: Symbol, cx: &Cx<'_>) -> bool {
    let vars = collect_expression_vars(node, cx);
    vars.is_empty() || vars.iter().any(|&v| v != el_name)
}

fn collect_expression_vars(node: NodeId, cx: &Cx<'_>) -> Vec<Symbol> {
    let mut result = Vec::new();
    if let NodeKind::Lvar(s) = *cx.kind(node) {
        result.push(s);
    }
    if let NodeKind::Lvasgn { name, .. } = *cx.kind(node) {
        result.push(name);
    }
    for &d in cx.descendants(node).iter() {
        if let NodeKind::Lvar(s) = *cx.kind(d) {
            result.push(s);
        } else if let NodeKind::Lvasgn { name, .. } = *cx.kind(d) {
            result.push(name);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::UnmodifiedReduceAccumulator;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_returning_element() {
        test::<UnmodifiedReduceAccumulator>().expect_offense(indoc! {r##"
            (1..4).reduce(0) do |acc, el|
              el
              ^^ Ensure the accumulator `acc` will be modified by `reduce`.
            end
        "##});
    }

    #[test]
    fn flags_returning_element_with_inject() {
        test::<UnmodifiedReduceAccumulator>().expect_offense(indoc! {r##"
            (1..4).inject(0) do |acc, el|
              el
              ^^ Ensure the accumulator `acc` will be modified by `inject`.
            end
        "##});
    }

    #[test]
    fn does_not_flag_returning_accumulator() {
        test::<UnmodifiedReduceAccumulator>().expect_no_offenses(indoc! {"
            (1..4).reduce(0) do |acc, el|
              acc
            end
        "});
    }

    #[test]
    fn does_not_flag_accumulator_in_expression() {
        test::<UnmodifiedReduceAccumulator>().expect_no_offenses(indoc! {"
            (1..4).reduce(0) do |acc, el|
              acc + el * 2
            end
        "});
    }

    #[test]
    fn flags_expression_with_only_element() {
        test::<UnmodifiedReduceAccumulator>().expect_offense(indoc! {r#"
            (1..4).reduce(0) do |acc, el|
              el + 2
              ^^^^^^ Ensure the accumulator `acc` will be modified by `reduce`.
            end
        "#});
    }

    #[test]
    fn does_not_flag_undetermined_return_value() {
        test::<UnmodifiedReduceAccumulator>().expect_no_offenses(indoc! {"
            (1..4).reduce(0) do |acc, el|
              x + el
            end
        "});
    }

    #[test]
    fn does_not_flag_element_modified_and_returned() {
        test::<UnmodifiedReduceAccumulator>().expect_no_offenses(indoc! {"
            values.reduce do |acc, el|
              el << acc
              el
            end
        "});
    }

    #[test]
    fn flags_accumulator_index() {
        test::<UnmodifiedReduceAccumulator>().expect_offense(indoc! {r#"
            %w(a b c).reduce({}) do |acc, letter|
              acc[foo]
              ^^^^^^^^ Do not return an element of the accumulator in `reduce`.
            end
        "#});
    }

    #[test]
    fn flags_accumulator_index_setter() {
        test::<UnmodifiedReduceAccumulator>().expect_offense(indoc! {r#"
            %w(a b c).reduce({}) do |acc, letter|
              acc[foo] = bar
              ^^^^^^^^^^^^^^ Do not return an element of the accumulator in `reduce`.
            end
        "#});
    }

    #[test]
    fn flags_next_with_element() {
        test::<UnmodifiedReduceAccumulator>().expect_offense(indoc! {r#"
            (1..4).reduce(0) do |acc, el|
              next el if el.even?
                   ^^ Ensure the accumulator `acc` will be modified by `reduce`.
              acc += 1
            end
        "#});
    }

    #[test]
    fn flags_break_with_element() {
        test::<UnmodifiedReduceAccumulator>().expect_offense(indoc! {r#"
            (1..4).reduce(0) do |acc, el|
              break el if el.even?
                    ^^ Ensure the accumulator `acc` will be modified by `reduce`.
              acc += 1
            end
        "#});
    }

    #[test]
    fn does_not_flag_accumulator_in_any_branch() {
        test::<UnmodifiedReduceAccumulator>().expect_no_offenses(indoc! {"
            values.reduce(nil) do |result, value|
              break result if something?
              value
            end
        "});
    }

    #[test]
    fn does_not_flag_literal_return() {
        test::<UnmodifiedReduceAccumulator>().expect_no_offenses(indoc! {"
            values.reduce(true) do |result, value|
              next false if something?
              true
            end
        "});
    }

    #[test]
    fn does_not_flag_accumulator_method_call() {
        test::<UnmodifiedReduceAccumulator>().expect_no_offenses(indoc! {"
            (1..4).reduce(0) do |acc, el|
              acc.method
            end
        "});
    }

    #[test]
    fn does_not_flag_method_called_with_accumulator() {
        test::<UnmodifiedReduceAccumulator>().expect_no_offenses(indoc! {"
            (1..4).reduce(0) do |acc, el|
              method(acc)
            end
        "});
    }

    #[test]
    fn does_not_flag_comparison() {
        test::<UnmodifiedReduceAccumulator>().expect_no_offenses(indoc! {"
            values.reduce(false) do |acc, el|
              acc == el
            end
        "});
    }

    #[test]
    fn does_not_flag_assignment() {
        test::<UnmodifiedReduceAccumulator>().expect_no_offenses(indoc! {"
            (1..4).reduce(0) do |acc, el|
              acc = el
            end
        "});
    }

    #[test]
    fn does_not_flag_op_assign() {
        test::<UnmodifiedReduceAccumulator>().expect_no_offenses(indoc! {"
            (1..4).reduce(0) do |acc, el|
              acc += 5
            end
        "});
    }

    #[test]
    fn does_not_flag_or_assign() {
        test::<UnmodifiedReduceAccumulator>().expect_no_offenses(indoc! {"
            (1..4).reduce(0) do |acc, el|
              acc ||= el
            end
        "});
    }

    #[test]
    fn does_not_flag_shovel() {
        test::<UnmodifiedReduceAccumulator>().expect_no_offenses(indoc! {"
            (1..4).reduce(0) do |acc, el|
              acc << el
            end
        "});
    }

    #[test]
    fn does_not_flag_boolean_expression_with_accumulator() {
        test::<UnmodifiedReduceAccumulator>().expect_no_offenses(indoc! {"
            (1..4).reduce(0) do |acc, el|
              acc || el
            end
        "});
    }

    #[test]
    fn does_not_ignore_receiver_calls() {
        test::<UnmodifiedReduceAccumulator>().expect_no_offenses(indoc! {"
            foo.reduce { |result, key| result[key] }
        "});
    }

    #[test]
    fn does_not_flag_element_modified_shovel() {
        test::<UnmodifiedReduceAccumulator>().expect_no_offenses(indoc! {"
            values.reduce do |acc, el|
              el << acc
              el
            end
        "});
    }

    #[test]
    fn does_not_flag_break_without_value() {
        test::<UnmodifiedReduceAccumulator>().expect_no_offenses(indoc! {"
            foo.reduce([]) do |acc, el|
              break if something?
              acc << el
            end
        "});
    }
}

murphy_plugin_api::submit_cop!(UnmodifiedReduceAccumulator);
