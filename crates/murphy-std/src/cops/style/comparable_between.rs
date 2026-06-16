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
//!   Guards against mismatched receiver comparisons (x >= min && y <= max).
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
        if let Some((value, min, max)) = extract_comparison(lhs, rhs, cx) {
            let val_src = cx.raw_source(cx.range(value));
            let preferred = format!(
                "({}).between?({}, {})",
                val_src,
                cx.raw_source(cx.range(min)),
                cx.raw_source(cx.range(max)),
            );
            cx.emit_offense(cx.range(node), MSG, None);
            cx.emit_edit(cx.range(node), &preferred);
        }
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

    if !matches!((a_method_str, b_method_str), (">=", "<=") | ("<=", ">=")) {
        return None;
    }
    let a_arg_list = cx.list(a_args);
    let b_arg_list = cx.list(b_args);
    let a_val = unwrap_begin(a_recv.get()?, cx);
    let b_val = unwrap_begin(b_recv.get()?, cx);
    let a_arg = *a_arg_list.first()?;
    let b_arg = *b_arg_list.first()?;

    // Both comparisons must reference the same variable (compare by source text)
    if cx.raw_source(cx.range(a_val)) != cx.raw_source(cx.range(b_val)) {
        return None;
    }

    if a_method_str == ">=" {
        Some((a_val, a_arg, b_arg))
    } else {
        Some((a_val, b_arg, a_arg))
    }
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
    use super::ComparableBetween;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_x_ge_min_and_x_le_max() {
        test::<ComparableBetween>().expect_correction(
            indoc! {"
                x >= min && x <= max
                ^^^^^^^^^^^^^^^^^^^^ Prefer `between?` over logical comparison.
            "},
            "(x).between?(min, max)\n",
        );
    }

    #[test]
    fn flags_x_le_max_and_x_ge_min() {
        test::<ComparableBetween>().expect_correction(
            indoc! {"
                x <= max && x >= min
                ^^^^^^^^^^^^^^^^^^^^ Prefer `between?` over logical comparison.
            "},
            "(x).between?(min, max)\n",
        );
    }

    #[test]
    fn accepts_mismatched_receivers() {
        test::<ComparableBetween>().expect_no_offenses("x >= min && y <= max\n");
    }

    #[test]
    fn flags_when_one_receiver_is_parenthesized() {
        test::<ComparableBetween>().expect_correction(
            indoc! {"
                (x) >= min && x <= max
                ^^^^^^^^^^^^^^^^^^^^^^ Prefer `between?` over logical comparison.
            "},
            "(x).between?(min, max)\n",
        );
    }

    #[test]
    fn accepts_between() {
        test::<ComparableBetween>().expect_no_offenses("x.between?(min, max)\n");
    }

    #[test]
    fn corrects_operator_receiver() {
        test::<ComparableBetween>().expect_correction(
            indoc! {"
                a + b >= min && a + b <= max
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `between?` over logical comparison.
            "},
            "(a + b).between?(min, max)\n",
        );
    }

    #[test]
    fn corrects_method_call_receiver() {
        test::<ComparableBetween>().expect_correction(
            indoc! {"
                foo.bar >= min && foo.bar <= max
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `between?` over logical comparison.
            "},
            "(foo.bar).between?(min, max)\n",
        );
    }

    #[test]
    fn corrects_simple_receiver_with_parens() {
        test::<ComparableBetween>().expect_correction(
            indoc! {"
                x >= min && x <= max
                ^^^^^^^^^^^^^^^^^^^^ Prefer `between?` over logical comparison.
            "},
            "(x).between?(min, max)\n",
        );
    }

    #[test]
    fn accepts_unrelated_and() {
        test::<ComparableBetween>().expect_no_offenses("a && b\n");
    }
}
murphy_plugin_api::submit_cop!(ComparableBetween);
