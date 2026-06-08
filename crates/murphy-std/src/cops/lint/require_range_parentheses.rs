//! `Lint/RequireRangeParentheses` — flag range literals whose end is on a
//! different line and the whole range is not parenthesized.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/RequireRangeParentheses
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's Lint/RequireRangeParentheses cop: range literals whose
//!   end expression is on a different line must be wrapped in parentheses to
//!   avoid precedence ambiguity.
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, NoOptions, Range, cop};

#[derive(Default)]
pub struct RequireRangeParentheses;

#[cop(
    name = "Lint/RequireRangeParentheses",
    description = "Wrap range literals in parentheses when the end is on a different line.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl RequireRangeParentheses {
    #[on_node(kind = "range")]
    fn check_range(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::RangeExpr { begin_, end_, exclusive } = *cx.kind(node) else { return; };
        let Some(begin_id) = begin_.get() else { return; };
        let Some(end_id) = end_.get() else { return; };
        if let Some(parent_id) = cx.parent(node).get()
            && crate::cops::util::is_parenthesized(parent_id, cx) {
                return;
            }
        let operator_end = cx.range(begin_id).end;
        let end_start = cx.range(end_id).start;
        let between = cx.raw_source(Range { start: operator_end, end: end_start });
        if between.contains('\n') {
            let range_op = if exclusive { "..." } else { ".." };
            let begin_src = cx.raw_source(cx.range(begin_id)).trim();
            let node_range = cx.range(node);
            let src = cx.raw_source(node_range);
            let first_line_len = src.lines().next().map(|l| l.len()).unwrap_or(src.len());
            let offense_range = Range {
                start: cx.range(begin_id).start,
                end: node_range.start + first_line_len as u32,
            };
            cx.emit_offense(
                offense_range,
                &format!("Wrap the endless range literal `{begin_src}{range_op}` to avoid precedence ambiguity."),
                None,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RequireRangeParentheses;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_unparenthesized_range_with_end_on_next_line() {
        test::<RequireRangeParentheses>().expect_offense(indoc! {r#"
            42..
            ^^^^ Wrap the endless range literal `42..` to avoid precedence ambiguity.
            do_something
        "#});
    }

    #[test]
    fn flags_exclusive_range_with_end_on_next_line() {
        test::<RequireRangeParentheses>().expect_offense(indoc! {r#"
            42...
            ^^^^^ Wrap the endless range literal `42...` to avoid precedence ambiguity.
            do_something
        "#});
    }

    #[test]
    fn ignores_parenthesized_range() {
        test::<RequireRangeParentheses>().expect_no_offenses(indoc! {r#"
            (42..
            do_something)
        "#});
    }

    #[test]
    fn ignores_same_line_range() {
        test::<RequireRangeParentheses>().expect_no_offenses("42..42\n");
    }

    #[test]
    fn ignores_beginless_range() {
        test::<RequireRangeParentheses>().expect_no_offenses("..42\n");
    }
}
murphy_plugin_api::submit_cop!(RequireRangeParentheses);
