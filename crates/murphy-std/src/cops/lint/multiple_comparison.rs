//! `Lint/MultipleComparison` — flags chained comparisons like `a < b < c`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/MultipleComparison
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Ported from RuboCop Lint/MultipleComparison. Flags chained comparison
//!   operators (any combination of `<`, `>`, `<=`, `>=`) and autocorrects by
//!   splitting with `&&`. Set operation operators (`&`, `|`, `^`) in the
//!   center value suppress the offense, matching RuboCop.
//! ```
//!
//! ## Matched shapes
//! - `a < b < c` — chained comparison (`<`  with `<`)
//! - `a < b <= c` — chained comparison (`<`  with `<=`)
//! - `a <= b <= c` — chained comparison (`<=` with `<=`)
//! - `a > b > c` — chained comparison (`>`  with `>`)
//! - Any combination of `<`, `>`, `<=`, `>=` chained
//!
//! ## Why this shape
//!
//! In Ruby, `a < b < c` evaluates left-to-right: `(a < b) < c`. The result
//! of `a < b` is a boolean, which is then compared to `c`. This is almost
//! always a bug — the user intended `a < b && b < c`.
//!
//! ## Autocorrect
//!
//! The cop replaces the center value with `center && center` to split the
//! chained comparison into two comparisons joined by `&&`.
//! `x < y < z` → `x < y && y < z`

use murphy_plugin_api::{Cx, NodeId, NodeKind, NoOptions, cop};

#[derive(Default)]
pub struct MultipleComparison;

const COMPARISON_OPERATORS: &[&str] = &["<", ">", "<=", ">="];
const SET_OPERATORS: &[&str] = &["&", "|", "^"];

#[cop(
    name = "Lint/MultipleComparison",
    description = "Use the `&&` operator to compare multiple values.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl MultipleComparison {
    #[on_node(kind = "send", methods = ["<", ">", "<=", ">="])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, .. } = *cx.kind(node) else {
            return;
        };

        let Some(recv_id) = receiver.get() else {
            return;
        };

        let mut check_id = recv_id;
        while let NodeKind::Begin(list) = *cx.kind(check_id) {
            let children = cx.list(list);
            if children.len() == 1 {
                check_id = children[0];
            } else {
                break;
            }
        }
        let (inner_args, inner_method) = match *cx.kind(check_id) {
            NodeKind::Send { args, method, .. } => (args, method),
            NodeKind::Csend { args, method, .. } => (args, method),
            _ => return,
        };

        let inner_method_str = cx.symbol_str(inner_method);
        if !COMPARISON_OPERATORS.contains(&inner_method_str) {
            return;
        }

        let inner_args_slice = cx.list(inner_args);
        if inner_args_slice.is_empty() {
            return;
        }
        let center = inner_args_slice[0];

        if is_set_operation(center, cx) {
            return;
        }

        cx.emit_offense(
            cx.range(node),
            "Use the `&&` operator to compare multiple values.",
            None,
        );

        let center_src = cx.raw_source(cx.range(center));
        let replacement = format!("{center_src} && {center_src}");
        cx.emit_edit(cx.range(center), &replacement);
    }
}

fn is_set_operation(node: NodeId, cx: &Cx<'_>) -> bool {
    let mut id = node;
    while let NodeKind::Begin(list) = *cx.kind(id) {
        let children = cx.list(list);
        if children.len() == 1 {
            id = children[0];
        } else {
            break;
        }
    }
    match *cx.kind(id) {
        NodeKind::Send { method, .. } | NodeKind::Csend { method, .. } => {
            SET_OPERATORS.contains(&cx.symbol_str(method))
        }
        _ => false,
    }
}

murphy_plugin_api::submit_cop!(MultipleComparison);

#[cfg(test)]
mod tests {
    use super::MultipleComparison;
    use murphy_plugin_api::test_support::{indoc, run_cop, run_cop_with_edits, test};

    #[test]
    fn flags_simple_chain_less_than() {
        test::<MultipleComparison>()
            .expect_offense(indoc! {r#"
                x < y < z
                ^^^^^^^^^ Use the `&&` operator to compare multiple values.
            "#})
            .expect_correction(
                indoc! {r#"
                    x < y < z
                    ^^^^^^^^^ Use the `&&` operator to compare multiple values.
                "#},
                "x < y && y < z\n",
            );
    }

    #[test]
    fn flags_chain_less_equal() {
        test::<MultipleComparison>()
            .expect_offense(indoc! {r#"
                x <= y <= z
                ^^^^^^^^^^^ Use the `&&` operator to compare multiple values.
            "#})
            .expect_correction(
                indoc! {r#"
                    x <= y <= z
                    ^^^^^^^^^^^ Use the `&&` operator to compare multiple values.
                "#},
                "x <= y && y <= z\n",
            );
    }

    #[test]
    fn flags_chain_greater_than() {
        test::<MultipleComparison>()
            .expect_offense(indoc! {r#"
                x > y > z
                ^^^^^^^^^ Use the `&&` operator to compare multiple values.
            "#})
            .expect_correction(
                indoc! {r#"
                    x > y > z
                    ^^^^^^^^^ Use the `&&` operator to compare multiple values.
                "#},
                "x > y && y > z\n",
            );
    }

    #[test]
    fn flags_chain_greater_equal() {
        test::<MultipleComparison>()
            .expect_offense(indoc! {r#"
                x >= y >= z
                ^^^^^^^^^^^ Use the `&&` operator to compare multiple values.
            "#})
            .expect_correction(
                indoc! {r#"
                    x >= y >= z
                    ^^^^^^^^^^^ Use the `&&` operator to compare multiple values.
                "#},
                "x >= y && y >= z\n",
            );
    }

    #[test]
    fn flags_mixed_chain_less_than_less_equal() {
        test::<MultipleComparison>()
            .expect_offense(indoc! {r#"
                x < y <= z
                ^^^^^^^^^^ Use the `&&` operator to compare multiple values.
            "#})
            .expect_correction(
                indoc! {r#"
                    x < y <= z
                    ^^^^^^^^^^ Use the `&&` operator to compare multiple values.
                "#},
                "x < y && y <= z\n",
            );
    }

    #[test]
    fn flags_mixed_chain_greater_equal_greater_than() {
        test::<MultipleComparison>()
            .expect_offense(indoc! {r#"
                x >= y > z
                ^^^^^^^^^^ Use the `&&` operator to compare multiple values.
            "#})
            .expect_correction(
                indoc! {r#"
                    x >= y > z
                    ^^^^^^^^^^ Use the `&&` operator to compare multiple values.
                "#},
                "x >= y && y > z\n",
            );
    }

    #[test]
    fn flags_all_16_operator_combinations() {
        let ops = ["<", ">", "<=", ">="];
        for op1 in &ops {
            for op2 in &ops {
                let src = format!("x {op1} y {op2} z\n");
                let offenses = run_cop::<MultipleComparison>(&src);
                assert_eq!(
                    offenses.len(),
                    1,
                    "should flag: x {op1} y {op2} z"
                );

                let run = run_cop_with_edits::<MultipleComparison>(&src);
                assert_eq!(run.offenses.len(), 1);
                assert_eq!(run.edits.len(), 1);
                let expected = format!("x {op1} y && y {op2} z\n");
                // Apply the edit to src and check
                let edit = &run.edits[0];
                let mut corrected = src.as_bytes().to_vec();
                let range_start = edit.range.start as usize;
                let range_end = edit.range.end as usize;
                corrected.splice(range_start..range_end, edit.replacement.as_bytes().iter().copied());
                let corrected_str = String::from_utf8(corrected).unwrap();
                assert_eq!(
                    corrected_str, expected,
                    "autocorrect for: x {op1} y {op2} z"
                );
            }
        }
    }

    #[test]
    fn accepts_single_comparison() {
        test::<MultipleComparison>().expect_no_offenses("x < 1\n");
    }

    #[test]
    fn accepts_single_comparison_with_variable() {
        test::<MultipleComparison>().expect_no_offenses(indoc! {r#"
            x < y
            top
        "#});
    }

    #[test]
    fn accepts_set_operation_ampersand() {
        test::<MultipleComparison>().expect_no_offenses("x >= y & x < z\n");
    }

    #[test]
    fn accepts_set_operation_pipe() {
        test::<MultipleComparison>().expect_no_offenses("x >= y | x < z\n");
    }

    #[test]
    fn accepts_set_operation_caret() {
        test::<MultipleComparison>().expect_no_offenses("x >= y ^ x < z\n");
    }

    #[test]
    fn accepts_non_comparison_receiver() {
        test::<MultipleComparison>().expect_no_offenses("foo(x) < z\n");
    }
}
