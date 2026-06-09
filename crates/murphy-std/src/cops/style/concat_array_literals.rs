//! `Style/ConcatArrayLiterals` — enforces `push(item)` over `concat([item])`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ConcatArrayLiterals
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   v1 gap: percent-literal args are not transformed to `push(...)` form.
//!   csend (safe-navigation) variant is not handled.
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct ConcatArrayLiterals;

#[cop(
    name = "Style/ConcatArrayLiterals",
    description = "Use `push(item)` instead of `concat([item])`.",
    default_severity = "warning",
    default_enabled = true,
    options = murphy_plugin_api::NoOptions
)]
impl ConcatArrayLiterals {
    #[on_node(kind = "send", methods = ["concat"])]
    fn check_concat(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, args, .. } = *cx.kind(node) else {
            return;
        };
        if receiver.get().is_none() {
            return;
        }
        let arg_list = cx.list(args);
        if arg_list.is_empty() {
            return;
        }
        let all_arrays = arg_list.iter().all(|&a| matches!(cx.kind(unwrap_begin(a, cx)), NodeKind::Array(_)));
        if !all_arrays {
            return;
        }
        let empty_array = arg_list.iter().any(|&a| {
            if let NodeKind::Array(elements) = cx.kind(unwrap_begin(a, cx)) {
                cx.list(*elements).is_empty()
            } else {
                false
            }
        });
        if empty_array {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            "Use `push` with elements as arguments instead of `concat` with an array literal.",
            None,
        );
        cx.emit_edit(cx.range(node), &build_push_call(node, cx));
    }
}

fn build_push_call(node: NodeId, cx: &Cx<'_>) -> String {
    let NodeKind::Send { receiver, args, .. } = *cx.kind(node) else {
        return String::new();
    };
    let recv_src = match receiver.get() {
        Some(r) => cx.raw_source(cx.range(r)).to_string(),
        None => String::new(),
    };
    let push_args: Vec<String> = cx.list(args).iter().map(|&a| {
        let NodeKind::Array(elements) = *cx.kind(unwrap_begin(a, cx)) else {
            return cx.raw_source(cx.range(a)).to_string();
        };
        let elems: Vec<String> = cx.list(elements).iter()
            .map(|&e| cx.raw_source(cx.range(e)).to_string())
            .collect();
        if elems.is_empty() {
            return String::new();
        }
        elems.join(", ")
    }).collect();
    format!("{}.push({})", recv_src, push_args.join(", "))
}

fn unwrap_begin(mut node: NodeId, cx: &Cx<'_>) -> NodeId {
    while let NodeKind::Begin(children) = cx.kind(node) {
        let child_list = cx.list(*children);
        if child_list.len() != 1 {
            break;
        }
        node = child_list[0];
    }
    node
}

#[cfg(test)]
mod tests {
    use super::ConcatArrayLiterals;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_concat_single_element() {
        test::<ConcatArrayLiterals>().expect_correction(
            indoc! {"
                list.concat([foo])
                ^^^^^^^^^^^^^^^^^^ Use `push` with elements as arguments instead of `concat` with an array literal.
            "},
            "list.push(foo)\n",
        );
    }

    #[test]
    fn flags_parenthesized_array_arg() {
        test::<ConcatArrayLiterals>().expect_correction(
            indoc! {"
                list.concat(([foo]))
                ^^^^^^^^^^^^^^^^^^^^ Use `push` with elements as arguments instead of `concat` with an array literal.
            "},
            "list.push(foo)\n",
        );
    }

    #[test]
    fn flags_concat_multiple_elements() {
        test::<ConcatArrayLiterals>().expect_correction(
            indoc! {"
                list.concat([bar, baz])
                ^^^^^^^^^^^^^^^^^^^^^^^ Use `push` with elements as arguments instead of `concat` with an array literal.
            "},
            "list.push(bar, baz)\n",
        );
    }

    #[test]
    fn accepts_push() {
        test::<ConcatArrayLiterals>().expect_no_offenses("list.push(foo)\n");
    }

    #[test]
    fn accepts_concat_non_array() {
        test::<ConcatArrayLiterals>().expect_no_offenses("list.concat(other)\n");
    }
}
murphy_plugin_api::submit_cop!(ConcatArrayLiterals);
