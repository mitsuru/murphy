//! `Lint/OrAssignmentToConstant` — flags `CONST ||= value`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/OrAssignmentToConstant
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Ported from RuboCop Lint/OrAssignmentToConstant.
//!   Known v1 limitation: scoped constant or-assignment (`M::CONST ||= 1`)
//!   parses to `NodeKind::Unknown` in Murphy and is not flagged. Single-segment
//!   `CONST ||= 1` is correctly handled via `OrAsgn { target: Casgn }`.
//! ```
//!
//! ## Matched shapes
//!
//! - `CONST ||= value` — or-assignment to a bare constant
//!
//! ## Why this shape
//!
//! Constants should always be assigned in the same location. Ruby warns on
//! constant re-assignment, and `||=` silently fails when the constant is
//! already defined — it reads the existing value and does not re-assign.
//!
//! ## Autocorrect
//!
//! Replaces `||=` with `=` except when the or-assignment appears inside a
//! method definition (`def` / `defs`), matching RuboCop's behaviour
//! (`each_ancestor(:any_def)` gate).

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

#[derive(Default)]
pub struct OrAssignmentToConstant;

const MSG: &str = "Avoid using or-assignment with constants.";

#[cop(
    name = "Lint/OrAssignmentToConstant",
    description = "Or-assignment to a constant is not supported.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl OrAssignmentToConstant {
    #[on_node(kind = "or_asgn")]
    fn check_or_asgn(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::OrAsgn { target, value } = *cx.kind(node) else {
            return;
        };

        if !matches!(*cx.kind(target), NodeKind::Casgn { .. }) {
            return;
        }

        let gap = Range {
            start: cx.range(target).end,
            end: cx.range(value).start,
        };
        let Some(op_range) = find_op_in_gap(cx, gap, "||=") else {
            return;
        };

        cx.emit_offense(op_range, MSG, None);

        // Autocorrect: replace `||=` with `=` unless inside a def/defs.
        if !is_inside_def(node, cx) {
            cx.emit_edit(op_range, "=");
        }
    }
}

/// Finds `op` in the gap text, returning its byte range.
fn find_op_in_gap(cx: &Cx<'_>, gap: Range, op: &str) -> Option<Range> {
    if gap.start >= gap.end {
        return None;
    }
    let gap_text = cx.raw_source(gap);
    let pos = gap_text.find(op)?;
    Some(Range {
        start: gap.start + pos as u32,
        end: gap.start + (pos + op.len()) as u32,
    })
}

/// Returns `true` when `node` has a `Def` or `Defs` ancestor.
fn is_inside_def(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.ancestors(node).any(|a| {
        matches!(*cx.kind(a), NodeKind::Def { .. } | NodeKind::Defs { .. })
    })
}

murphy_plugin_api::submit_cop!(OrAssignmentToConstant);

#[cfg(test)]
mod tests {
    use super::OrAssignmentToConstant;
    use murphy_plugin_api::test_support::{indoc, test, run_cop_with_edits};

    #[test]
    fn flags_or_assignment_to_constant() {
        test::<OrAssignmentToConstant>().expect_offense(indoc! {r#"
            CONST ||= 1
                  ^^^ Avoid using or-assignment with constants.
        "#});
    }

    #[test]
    fn autocorrects_or_assignment_to_constant() {
        test::<OrAssignmentToConstant>().expect_correction(
            indoc! {r#"
                CONST ||= 1
                      ^^^ Avoid using or-assignment with constants.
            "#},
            "CONST = 1\n",
        );
    }

    #[test]
    fn does_not_autocorrect_inside_def() {
        let run = run_cop_with_edits::<OrAssignmentToConstant>(
            "def foo\n  CONST ||= 1\nend\n",
        );
        assert!(!run.offenses.is_empty(), "should have offense");
        assert_eq!(run.edits.len(), 0, "should have no autocorrect edits");
    }

    #[test]
    fn does_not_autocorrect_inside_defs() {
        let run = run_cop_with_edits::<OrAssignmentToConstant>(
            "def self.foo\n  CONST ||= 1\nend\n",
        );
        assert!(!run.offenses.is_empty(), "should have offense");
        assert_eq!(run.edits.len(), 0, "should have no autocorrect edits");
    }

    #[test]
    fn autocorrects_inside_define_method() {
        test::<OrAssignmentToConstant>().expect_correction(
            indoc! {r#"
                define_method :foo do
                  CONST ||= 1
                        ^^^ Avoid using or-assignment with constants.
                end
            "#},
            "define_method :foo do\n  CONST = 1\nend\n",
        );
    }

    #[test]
    fn does_not_flag_plain_assignment_to_constant() {
        test::<OrAssignmentToConstant>().expect_no_offenses("CONST = 1\n");
    }

    #[test]
    fn does_not_flag_or_assignment_to_local_variable() {
        test::<OrAssignmentToConstant>().expect_no_offenses("var ||= 1\n");
    }

    #[test]
    fn does_not_flag_or_assignment_to_instance_variable() {
        test::<OrAssignmentToConstant>().expect_no_offenses("@var ||= 1\n");
    }

    #[test]
    fn does_not_flag_or_assignment_to_class_variable() {
        test::<OrAssignmentToConstant>().expect_no_offenses("@@var ||= 1\n");
    }

    #[test]
    fn does_not_flag_or_assignment_to_global_variable() {
        test::<OrAssignmentToConstant>().expect_no_offenses("$var ||= 1\n");
    }

    #[test]
    fn does_not_flag_or_assignment_to_attribute() {
        test::<OrAssignmentToConstant>().expect_no_offenses("self.var ||= 1\n");
    }
}
