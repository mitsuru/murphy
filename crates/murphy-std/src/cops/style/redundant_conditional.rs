//! `Style/RedundantConditional` — flags conditionals that return `true`/`false`
//! explicitly when the condition itself suffices.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantConditional
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   All primary cases are implemented: ternary and block-form
//!   `if`/`elsif` with `true`/`false` branches using comparison operators.
//!   The elsif autocorrect produces `else\n<indent><expr>` matching RuboCop.
//!   For block-form multi-line nodes, the offense range is bounded to the
//!   first line (keyword + condition) for clean single-line offense highlighting;
//!   RuboCop highlights the whole node. The autocorrect edit covers the whole node.
//!   RuboCop includes `<=>` in its COMPARISON_OPERATORS but Murphy's
//!   is_comparison_method excludes it; this is a minor known difference.
//! ```
//!
//! ## Matched shapes
//!
//! - `x OP y ? true : false` → offense, suggest `x OP y`
//! - `x OP y ? false : true` → offense, suggest `!(x OP y)`
//! - Block-form `if x OP y; true; else; false; end` → offense
//! - `elsif x OP y; true; else; false; end` → offense
//!
//! where `OP` is one of `==`, `===`, `!=`, `<`, `>`, `<=`, `>=`.
//!
//! ## Autocorrect
//!
//! Non-inverted: replace the whole node with the condition source.
//! Inverted: replace the whole node with `!(condition source)`.
//! `elsif` form: replace with `else\n<indent><expr>`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop, method_predicates};

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantConditional;

#[cop(
    name = "Style/RedundantConditional",
    description = "Don't return true/false from a conditional.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RedundantConditional {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// Returns `true` if the node is a `(send _ OP _)` using a comparison operator.
fn is_comparison_send(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(node)
    else {
        return false;
    };
    if receiver.is_none() {
        return false;
    }
    let arg_list = cx.list(args);
    if arg_list.len() != 1 {
        return false;
    }
    method_predicates::is_comparison_method(cx.symbol_str(method))
}

/// Compute indentation of `node` from the start of its line.
fn node_indentation<'a>(node: NodeId, cx: &Cx<'a>) -> &'a str {
    let source = cx.source();
    let bytes = source.as_bytes();
    let start = cx.range(node).start as usize;
    let line_start = bytes[..start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    let indent_end = bytes[line_start..start]
        .iter()
        .position(|&b| b != b' ' && b != b'\t')
        .map(|i| line_start + i)
        .unwrap_or(start);
    &source[line_start..indent_end]
}

/// Returns the range from `start` to the end of its line (exclusive of `\n`).
fn first_line_range(start: u32, source: &str) -> Range {
    let bytes = source.as_bytes();
    let s = start as usize;
    let line_end = bytes[s..]
        .iter()
        .position(|&b| b == b'\n')
        .map(|i| s + i)
        .unwrap_or(source.len());
    Range {
        start,
        end: line_end as u32,
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Skip modifier forms (`b if a`) — they can't have both branches.
    if cx.is_modifier_form(node) {
        return;
    }

    let NodeKind::If { cond, then_, else_ } = *cx.kind(node) else {
        return;
    };

    // Condition must be a comparison-operator send.
    let cond_id = cond;
    if !is_comparison_send(cond_id, cx) {
        return;
    }

    // Both then and else branches must be present.
    let (Some(then_id), Some(else_id)) = (then_.get(), else_.get()) else {
        return;
    };

    let inverted = if matches!(cx.kind(then_id), NodeKind::True_)
        && matches!(cx.kind(else_id), NodeKind::False_)
    {
        false
    } else if matches!(cx.kind(then_id), NodeKind::False_)
        && matches!(cx.kind(else_id), NodeKind::True_)
    {
        true
    } else {
        return;
    };

    let cond_src = cx.raw_source(cx.range(cond_id));
    let replacement = if inverted {
        format!("!({cond_src})")
    } else {
        cond_src.to_owned()
    };

    let node_range = cx.range(node);

    // Offense range: first line of the node (keyword + condition).
    // For ternary, the whole node is on one line so this is the full node.
    // For block-form, this caps at the line end to keep offense highlighting
    // on a single line.
    let offense_range = first_line_range(node_range.start, cx.source());

    let msg = format!(
        "This conditional expression can just be replaced by `{replacement}`."
    );

    cx.emit_offense(offense_range, &msg, None);

    // Autocorrect edit: cover the whole node (may span multiple lines).
    // For elsif, replace the whole elsif-end block with `else\n<indent><expr>`.
    let edit_src = if cx.is_elsif(node) {
        let indent = node_indentation(node, cx);
        format!("else\n{indent}{replacement}")
    } else {
        replacement
    };
    cx.emit_edit(node_range, &edit_src);
}

#[cfg(test)]
mod tests {
    use super::RedundantConditional;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- ternary true/false -----

    #[test]
    fn flags_ternary_true_false() {
        test::<RedundantConditional>().expect_correction(
            indoc! {"
                x == y ? true : false
                ^^^^^^^^^^^^^^^^^^^^^ This conditional expression can just be replaced by `x == y`.
            "},
            "x == y\n",
        );
    }

    #[test]
    fn flags_ternary_false_true() {
        test::<RedundantConditional>().expect_correction(
            indoc! {"
                x == y ? false : true
                ^^^^^^^^^^^^^^^^^^^^^ This conditional expression can just be replaced by `!(x == y)`.
            "},
            "!(x == y)\n",
        );
    }

    #[test]
    fn flags_ternary_neq() {
        test::<RedundantConditional>().expect_correction(
            indoc! {"
                x != y ? true : false
                ^^^^^^^^^^^^^^^^^^^^^ This conditional expression can just be replaced by `x != y`.
            "},
            "x != y\n",
        );
    }

    #[test]
    fn flags_ternary_lt() {
        test::<RedundantConditional>().expect_correction(
            indoc! {"
                x < y ? true : false
                ^^^^^^^^^^^^^^^^^^^^ This conditional expression can just be replaced by `x < y`.
            "},
            "x < y\n",
        );
    }

    #[test]
    fn flags_ternary_eqq() {
        test::<RedundantConditional>().expect_correction(
            indoc! {"
                x === y ? true : false
                ^^^^^^^^^^^^^^^^^^^^^^ This conditional expression can just be replaced by `x === y`.
            "},
            "x === y\n",
        );
    }

    // ----- block-form if/else -----
    // The offense range is capped to the first line (keyword + condition).

    #[test]
    fn flags_block_if_true_false() {
        test::<RedundantConditional>().expect_correction(
            indoc! {"
                if x == y
                ^^^^^^^^^ This conditional expression can just be replaced by `x == y`.
                  true
                else
                  false
                end
            "},
            "x == y\n",
        );
    }

    #[test]
    fn flags_block_if_false_true() {
        test::<RedundantConditional>().expect_correction(
            indoc! {"
                if x == y
                ^^^^^^^^^ This conditional expression can just be replaced by `!(x == y)`.
                  false
                else
                  true
                end
            "},
            "!(x == y)\n",
        );
    }

    // ----- elsif form -----

    #[test]
    fn flags_elsif_true_false() {
        test::<RedundantConditional>().expect_offense(indoc! {"
            if a
              1
            elsif x == y
            ^^^^^^^^^^^^ This conditional expression can just be replaced by `x == y`.
              true
            else
              false
            end
        "});
    }

    // ----- Negative cases -----

    #[test]
    fn accepts_non_comparison_condition() {
        // foo? is not a comparison operator — no offense.
        test::<RedundantConditional>().expect_no_offenses("foo? ? true : false\n");
    }

    #[test]
    fn accepts_non_boolean_branches() {
        test::<RedundantConditional>().expect_no_offenses("x == y ? 1 : 2\n");
    }

    #[test]
    fn accepts_true_without_false_else() {
        test::<RedundantConditional>().expect_no_offenses(indoc! {"
            if x == y
              true
            end
        "});
    }

    #[test]
    fn accepts_modifier_form() {
        test::<RedundantConditional>().expect_no_offenses("true if x == y\n");
    }

    #[test]
    fn accepts_method_call_condition() {
        test::<RedundantConditional>().expect_no_offenses("x ? true : false\n");
    }
}
murphy_plugin_api::submit_cop!(RedundantConditional);
