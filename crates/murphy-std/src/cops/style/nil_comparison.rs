//! `Style/NilComparison` — flags `x == nil` and `x != nil`, preferring `.nil?`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/NilComparison
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Murphy v1 enforces the predicate style only (x == nil → x.nil?, x != nil → !x.nil?).
//!   RuboCop's `EnforcedStyle = comparison` direction (nil? → == nil) is not supported.
//!   `===` is not handled (RuboCop's RESTRICT_ON_SEND includes :=== in addition to :== and :nil?).
//!   `!=` is a Murphy addition not present in RuboCop's default predicate-style enforcement.
//! ```
//!
//! ## Matched shapes
//!
//! `Send` nodes with method `==` or `!=` with exactly one argument that is a
//! `nil` literal, and whose receiver is non-absent. Examples:
//!
//! - `x == nil` → offense, suggest `.nil?`
//! - `x != nil` → offense, suggest `!x.nil?`
//! - `nil == x` → no offense (receiver is nil, argument is `x`)
//!
//! ## Autocorrect
//!
//! - `recv == nil` → `recv.nil?`: single surgical edit replacing `[recv.end..node.end]`
//!   with `.nil?`.
//! - `recv != nil` → `!recv.nil?`: whole-node replacement with `!<recv_src>.nil?`.
//!   This is a structural rewrite (adding a `!` prefix), so whole-node interpolation
//!   is the cleaner choice per `.claude/rules/autocorrect-pattern.md`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, Range, cop};

const MSG: &str = "Prefer the use of the `nil?` predicate.";

/// Stateless unit struct.
#[derive(Default)]
pub struct NilComparison;

#[cop(
    name = "Style/NilComparison",
    description = "Prefer `nil?` over comparison to nil.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl NilComparison {
    #[on_node(kind = "send", methods = ["==", "!="])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(node)
    else {
        return;
    };

    // Receiver must be present (guard against bare `== nil` with no receiver,
    // though the parser won't produce that in practice).
    let OptNodeId(recv_idx) = receiver;
    if recv_idx == u32::MAX {
        return;
    }
    let recv_id = NodeId(recv_idx);

    // Exactly one argument, and it must be a nil literal.
    let arg_list = cx.list(args);
    if arg_list.len() != 1 {
        return;
    }
    let arg_id = arg_list[0];
    if !matches!(cx.kind(arg_id), NodeKind::Nil) {
        return;
    }

    let method_name = cx.symbol_str(method);
    let node_range = cx.range(node);
    let recv_range = cx.range(recv_id);

    match method_name {
        "==" => {
            // recv == nil → recv.nil?
            // Surgical edit: replace everything after the receiver with `.nil?`
            let tail = Range {
                start: recv_range.end,
                end: node_range.end,
            };
            cx.emit_offense(node_range, MSG, None);
            cx.emit_edit(tail, ".nil?");
        }
        "!=" => {
            // recv != nil → !recv.nil?
            // Structural rewrite: insert `!` prefix and change operator.
            let recv_src = cx.raw_source(recv_range);
            let replacement = format!("!{recv_src}.nil?");
            cx.emit_offense(node_range, MSG, None);
            cx.emit_edit(node_range, &replacement);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::NilComparison;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- `==` cases -----

    #[test]
    fn flags_eq_nil() {
        test::<NilComparison>().expect_correction(
            indoc! {"
                x == nil
                ^^^^^^^^ Prefer the use of the `nil?` predicate.
            "},
            "x.nil?\n",
        );
    }

    #[test]
    fn flags_eq_nil_with_complex_receiver() {
        test::<NilComparison>().expect_correction(
            indoc! {"
                foo.bar == nil
                ^^^^^^^^^^^^^^ Prefer the use of the `nil?` predicate.
            "},
            "foo.bar.nil?\n",
        );
    }

    #[test]
    fn flags_eq_nil_in_if_condition() {
        test::<NilComparison>().expect_correction(
            indoc! {"
                if x == nil
                   ^^^^^^^^ Prefer the use of the `nil?` predicate.
                  y
                end
            "},
            indoc! {"
                if x.nil?
                  y
                end
            "},
        );
    }

    // ----- `!=` cases -----

    #[test]
    fn flags_neq_nil() {
        test::<NilComparison>().expect_correction(
            indoc! {"
                x != nil
                ^^^^^^^^ Prefer the use of the `nil?` predicate.
            "},
            "!x.nil?\n",
        );
    }

    #[test]
    fn flags_neq_nil_with_complex_receiver() {
        test::<NilComparison>().expect_correction(
            indoc! {"
                foo.bar != nil
                ^^^^^^^^^^^^^^ Prefer the use of the `nil?` predicate.
            "},
            "!foo.bar.nil?\n",
        );
    }

    // ----- Negative cases -----

    #[test]
    fn accepts_nil_predicate() {
        test::<NilComparison>().expect_no_offenses("x.nil?\n");
    }

    #[test]
    fn accepts_nil_on_left_side() {
        // nil == x: receiver is nil, argument is x — not matched.
        test::<NilComparison>().expect_no_offenses("nil == x\n");
    }

    #[test]
    fn accepts_eq_non_nil() {
        test::<NilComparison>().expect_no_offenses("x == 1\n");
    }

    #[test]
    fn accepts_neq_non_nil() {
        test::<NilComparison>().expect_no_offenses("x != 1\n");
    }
}
