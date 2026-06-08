//! `Lint/RedundantSplatExpansion` — detects unneeded splat expansion.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/RedundantSplatExpansion
//! upstream_version_checked: master
//! status: partial
//! gap_issues: []
//! notes: >
//!   Initial port covers splatting string/dstr/int/float/array literals and
//!   `Array.new` into assignments, method arguments, array literals, `when`, and
//!   `rescue` positions where Murphy exposes the splat node. Known v1 limitation:
//!   percent literal expansion and the `AllowPercentLiteralArrayArgument` option
//!   are not implemented; empty array splats are conservatively ignored.
//! ```

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind};

const MSG: &str = "Replace splat expansion with comma separated values.";
const ARRAY_PARAM_MSG: &str = "Pass array contents as separate arguments.";

#[derive(Default)]
pub struct RedundantSplatExpansion;

#[cop(
    name = "Lint/RedundantSplatExpansion",
    description = "Checks for unneeded usages of splat expansion.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RedundantSplatExpansion {
    #[on_node(kind = "splat")]
    fn check_splat(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Splat(inner) = *cx.kind(node) else {
        return;
    };
    let Some(inner) = inner.get() else {
        return;
    };
    if empty_array(inner, cx) || !redundant_inner(inner, cx) {
        return;
    }
    let parent = cx.parent(node).get();
    let array_context = parent.is_some_and(|p| {
        is_call(p, cx)
            || bracketed_array(p, cx)
            || matches!(cx.kind(p), NodeKind::When { .. } | NodeKind::Resbody { .. })
    });
    let msg = if array_context && matches!(cx.kind(inner), NodeKind::Array(_)) {
        ARRAY_PARAM_MSG
    } else {
        MSG
    };
    cx.emit_offense(cx.range(node), msg, None);
    if let Some(replacement) = replacement(node, inner, parent, cx) {
        cx.emit_edit(cx.range(node), &replacement);
    }
}

fn redundant_inner(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        cx.kind(node),
        NodeKind::Str(_)
            | NodeKind::Dstr(_)
            | NodeKind::Int(_)
            | NodeKind::Float(_)
            | NodeKind::Array(_)
    ) || is_array_new(node, cx)
}

fn empty_array(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(cx.kind(node), NodeKind::Array(list) if cx.list(*list).is_empty())
}

fn replacement(node: NodeId, inner: NodeId, parent: Option<NodeId>, cx: &Cx<'_>) -> Option<String> {
    if is_array_new(inner, cx) {
        return Some(cx.raw_source(cx.range(inner)).to_string());
    }
    if let NodeKind::Array(list) = *cx.kind(inner) {
        let elements: Vec<_> = cx
            .list(list)
            .iter()
            .map(|&child| cx.raw_source(cx.range(child)).to_string())
            .collect();
        if parent.is_some_and(|p| {
            is_call(p, cx)
                || bracketed_array(p, cx)
                || matches!(cx.kind(p), NodeKind::When { .. } | NodeKind::Resbody { .. })
        }) {
            return Some(elements.join(", "));
        }
        return Some(cx.raw_source(cx.range(inner)).to_string());
    }
    let src = cx.raw_source(cx.range(inner));
    if parent.is_some_and(|p| matches!(cx.kind(p), NodeKind::Array(_)) && !bracketed_array(p, cx)) {
        Some(format!("[{src}]"))
    } else if parent.is_some_and(|p| {
        bracketed_array(p, cx)
            || is_call(p, cx)
            || matches!(cx.kind(p), NodeKind::When { .. } | NodeKind::Resbody { .. })
    }) {
        Some(src.to_string())
    } else if parent.is_some_and(|p| assignment_value_is(p, node, cx)) {
        Some(format!("[{src}]"))
    } else {
        None
    }
}

fn is_call(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        cx.kind(node),
        NodeKind::Send { .. } | NodeKind::Csend { .. }
    )
}

fn bracketed_array(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(cx.kind(node), NodeKind::Array(_))
        && cx.raw_source(cx.range(node)).trim_start().starts_with('[')
}

fn assignment_value_is(node: NodeId, value: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Lvasgn { value: v, .. }
        | NodeKind::Ivasgn { value: v, .. }
        | NodeKind::Casgn { value: v, .. }
        | NodeKind::Gvasgn { value: v, .. }
        | NodeKind::Cvasgn { value: v, .. } => v.get() == Some(value),
        _ => false,
    }
}

fn is_array_new(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(cx.method_name(node), Some("new"))
        && cx
            .call_receiver(node)
            .get()
            .is_some_and(|recv| cx.is_global_const(recv, "Array"))
}

murphy_plugin_api::submit_cop!(RedundantSplatExpansion);

#[cfg(test)]
mod tests {
    use super::RedundantSplatExpansion;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_and_corrects_literal_assignment() {
        test::<RedundantSplatExpansion>().expect_correction(
            indoc! {r#"
                a = *"a"
                    ^^^^ Replace splat expansion with comma separated values.
            "#},
            "a = [\"a\"]\n",
        );
    }

    #[test]
    fn flags_and_corrects_array_method_argument() {
        test::<RedundantSplatExpansion>().expect_correction(
            indoc! {r#"
                array.push(*[1, 2, 3])
                           ^^^^^^^^^^ Pass array contents as separate arguments.
            "#},
            "array.push(1, 2, 3)\n",
        );
    }

    #[test]
    fn accepts_variable_and_empty_array_splats() {
        test::<RedundantSplatExpansion>()
            .expect_no_offenses("a = *items\n")
            .expect_no_offenses("do_something(*[])\n");
    }
}
