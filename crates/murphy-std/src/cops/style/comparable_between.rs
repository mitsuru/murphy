//! `Style/ComparableBetween` — prefer `between?` over logical comparison.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ComparableBetween
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Handles `x >= min && x <= max` and `x <= max && x >= min`.
//!   Autocorrect replaces the whole `&&` expression with `x.between?(min, max)`.
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, cop};

const MSG: &str = "Prefer `between?` over logical comparison.";

#[derive(Default)]
pub struct ComparableBetween;

#[cop(
    name = "Style/ComparableBetween",
    description = "Use `Comparable#between?` instead of logical comparison.",
    default_severity = "warning",
    default_enabled = true,
    options = murphy_plugin_api::NoOptions
)]
impl ComparableBetween {
    #[on_node(kind = "and")]
    fn check_and(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::And { lhs, rhs } = *cx.kind(node) else {
            return;
        };
        let Some((value, min, max)) = extract_comparison(lhs, rhs, cx) else {
            let Some((value, min, max)) = extract_comparison(rhs, lhs, cx) else {
                return;
            };
            let preferred = format!("{}.between?({}, {})",
                cx.raw_source(cx.range(value)),
                cx.raw_source(cx.range(min)),
                cx.raw_source(cx.range(max)),
            );
            cx.emit_offense(cx.range(node), MSG, None);
            cx.emit_edit(cx.range(node), &preferred);
            return;
        };
        let preferred = format!("{}.between?({}, {})",
            cx.raw_source(cx.range(value)),
            cx.raw_source(cx.range(min)),
            cx.raw_source(cx.range(max)),
        );
        cx.emit_offense(cx.range(node), MSG, None);
        cx.emit_edit(cx.range(node), &preferred);
    }
}

fn extract_comparison(a: NodeId, b: NodeId, cx: &Cx<'_>) -> Option<(NodeId, NodeId, NodeId)> {
    let NodeKind::Send { receiver: a_recv, method: a_method, args: a_args } = *cx.kind(a) else {
        return None;
    };
    let NodeKind::Send { receiver: b_recv, method: b_method, args: b_args } = *cx.kind(b) else {
        return None;
    };
    let a_method_str = cx.symbol_str(a_method);
    let b_method_str = cx.symbol_str(b_method);

    let (value, min, max) = match (a_method_str, b_method_str) {
        (">=", "<=") | ("<=", ">=") => {
            let a_arg_list = cx.list(a_args);
            let b_arg_list = cx.list(b_args);
            if a_arg_list.is_empty() || b_arg_list.is_empty() {
                return None;
            }
            let a_arg = a_arg_list[0];
            let b_arg = b_arg_list[0];
            match (a_method_str, b_method_str) {
                (">=", "<=") => {
                    (a_recv.get()?, b_arg, a_arg)
                }
                ("<=", ">=") => {
                    (b_recv.get()?, a_arg, b_arg)
                }
                _ => return None,
            }
        }
        _ => return None,
    };
    Some((value, min, max))
}

#[cfg(test)]
mod tests {
    use super::ComparableBetween;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_x_ge_min_and_x_le_max() {
        test::<ComparableBetween>().expect_correction(
            indoc! {"
                x >= min && x <= max
                ^^^^^^^^^^^^^^^^^^^^ Prefer `between?` over logical comparison.
            "},
            "x.between?(min, max)\n",
        );
    }

    #[test]
    fn flags_x_le_max_and_x_ge_min() {
        test::<ComparableBetween>().expect_correction(
            indoc! {"
                x <= max && x >= min
                ^^^^^^^^^^^^^^^^^^^^ Prefer `between?` over logical comparison.
            "},
            "x.between?(min, max)\n",
        );
    }

    #[test]
    fn accepts_between() {
        test::<ComparableBetween>().expect_no_offenses("x.between?(min, max)\n");
    }

    #[test]
    fn accepts_unrelated_and() {
        test::<ComparableBetween>().expect_no_offenses("a && b\n");
    }
}
murphy_plugin_api::submit_cop!(ComparableBetween);
