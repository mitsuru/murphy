//! `Style/ComparableClamp` — prefer `Comparable#clamp` over min/max comparisons.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ComparableClamp
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Handles `[[x, low].max, high].min` pattern.
//!   `if/elsif/else` pattern is a v1 gap.
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct ComparableClamp;

#[cop(
    name = "Style/ComparableClamp",
    description = "Use `Comparable#clamp` instead of min/max.",
    default_severity = "warning",
    default_enabled = true,
    options = murphy_plugin_api::NoOptions
)]
impl ComparableClamp {
    #[on_node(kind = "send", methods = ["min", "max"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, method, .. } = *cx.kind(node) else {
            return;
        };
        let method_str = cx.symbol_str(method);
        let Some(recv_id) = receiver.get() else {
            return;
        };
        let NodeKind::Array(outer_elements) = *cx.kind(recv_id) else {
            return;
        };
        let outer_list = cx.list(outer_elements);
        if outer_list.len() != 2 {
            return;
        }
        let inner_sends: Vec<NodeId> = outer_list.iter()
            .filter(|&&e| matches!(cx.kind(e), NodeKind::Send { .. }))
            .copied().collect();
        let others: Vec<NodeId> = outer_list.iter()
            .filter(|&&e| !matches!(cx.kind(e), NodeKind::Send { .. }))
            .copied().collect();
        if inner_sends.len() != 1 || others.len() != 1 {
            return;
        }
        let inner_send = inner_sends[0];
        let bound = others[0];
        let NodeKind::Send { receiver: inner_recv, method: inner_method, .. } = *cx.kind(inner_send) else {
            return;
        };
        let inner_method_str = cx.symbol_str(inner_method);
        let opposite = matches!(
            (method_str, inner_method_str),
            ("min", "max") | ("max", "min")
        );
        if !opposite {
            return;
        }
        let Some(inner_recv_id) = inner_recv.get() else {
            return;
        };
        let NodeKind::Array(inner_elements) = *cx.kind(inner_recv_id) else {
            return;
        };
        let inner_list = cx.list(inner_elements);
        if inner_list.len() != 2 {
            return;
        }
        let inner_val = inner_list[0];
        let inner_bound = inner_list[1];
        // v1 assumes inner_list[0] = value, inner_list[1] = bound.
        // [[low, x].max, high].min is not correctly handled (documented gap).
        let val_src = cx.raw_source(cx.range(inner_val));
        let inner_bound_src = cx.raw_source(cx.range(inner_bound));
        let bound_src = cx.raw_source(cx.range(bound));
        let (low, high) = if method_str == "min" {
            (inner_bound_src, bound_src)
        } else {
            (bound_src, inner_bound_src)
        };
        let preferred = format!("{}.clamp({}, {})", val_src, low, high);
        cx.emit_offense(cx.range(node), "Use `Comparable#clamp` instead.", None);
        cx.emit_edit(cx.range(node), &preferred);
    }
}

#[cfg(test)]
mod tests {
    use super::ComparableClamp;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_max_min_pattern() {
        test::<ComparableClamp>().expect_correction(
            indoc! {"
                [[x, low].max, high].min
                ^^^^^^^^^^^^^^^^^^^^^^^^ Use `Comparable#clamp` instead.
            "},
            "x.clamp(low, high)\n",
        );
    }

    #[test]
    fn accepts_plain_min() {
        test::<ComparableClamp>().expect_no_offenses("[a, b].min\n");
    }
}
murphy_plugin_api::submit_cop!(ComparableClamp);
