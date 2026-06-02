//! `Style/MultilineTernaryOperator` — flags multi-line ternary operators.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/MultilineTernaryOperator
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detects ternary `?:` expressions that span more than one line.
//!
//!   Two messages are used depending on context:
//!   - In a return/break/next/send context: "Avoid multi-line ternary operators,
//!     use single-line."
//!   - Otherwise: "Avoid multi-line ternary operators, use `if` or `unless`."
//!
//!   The offense is highlighted at the `?` token (ternary question mark).
//!
//!   Autocorrect is not implemented (v1 gap); the cop is detect-only.
//! ```
//!
//! ## Matched shapes
//!
//! Ternary `if` nodes (where `cx.is_ternary` is true) that span more than one
//! line.
//!
//! ## No autocorrect
//!
//! Autocorrect involves structural rewriting (ternary → if/else or collapsing
//! to single-line). Deferred to a follow-up.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const MSG_IF: &str = "Avoid multi-line ternary operators, use `if` or `unless`.";
const MSG_SINGLE_LINE: &str = "Avoid multi-line ternary operators, use single-line.";

#[derive(Default)]
pub struct MultilineTernaryOperator;

#[cop(
    name = "Style/MultilineTernaryOperator",
    description = "Avoid multi-line ternary operators.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl MultilineTernaryOperator {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Only ternary expressions.
    if !cx.is_ternary(node) {
        return;
    }

    // Must span multiple lines.
    if !cx.is_multiline(node) {
        return;
    }

    let msg = if in_single_line_context(node, cx) {
        MSG_SINGLE_LINE
    } else {
        MSG_IF
    };

    // Highlight the ternary `?` token (single-line range).
    let offense_range = {
        let q = cx.ternary_question_loc(node);
        if q != Range::ZERO { q } else { cx.range(node) }
    };

    cx.emit_offense(offense_range, msg, None);
}

/// Returns `true` when the ternary appears in a context where a single-line
/// correction is preferred (return/break/next/send). Mirrors RuboCop's
/// `single_line_conditions?`.
fn in_single_line_context(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    matches!(
        cx.kind(parent),
        NodeKind::Return(_)
            | NodeKind::Break(_)
            | NodeKind::Next(_)
            | NodeKind::Send { .. }
            | NodeKind::Csend { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::MultilineTernaryOperator;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_multiline_ternary() {
        test::<MultilineTernaryOperator>().expect_offense(indoc! {"
            a = foo ?
                    ^ Avoid multi-line ternary operators, use `if` or `unless`.
              bar :
              baz
        "});
    }

    #[test]
    fn flags_multiline_ternary_in_return() {
        test::<MultilineTernaryOperator>().expect_offense(indoc! {"
            return foo ?
                       ^ Avoid multi-line ternary operators, use single-line.
              bar :
              baz
        "});
    }

    #[test]
    fn accepts_single_line_ternary() {
        test::<MultilineTernaryOperator>().expect_no_offenses("a = foo ? bar : baz\n");
    }

    #[test]
    fn accepts_multiline_if_not_ternary() {
        test::<MultilineTernaryOperator>().expect_no_offenses(indoc! {"
            if foo
              bar
            else
              baz
            end
        "});
    }

    #[test]
    fn flags_multiline_ternary_no_parent() {
        test::<MultilineTernaryOperator>().expect_offense(indoc! {"
            foo ?
                ^ Avoid multi-line ternary operators, use `if` or `unless`.
              bar :
              baz
        "});
    }
}

murphy_plugin_api::submit_cop!(MultilineTernaryOperator);
