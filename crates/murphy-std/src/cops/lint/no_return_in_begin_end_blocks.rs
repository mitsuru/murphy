//! `Lint/NoReturnInBeginEndBlocks` — flags `return` inside `begin..end` blocks
//! in assignment contexts.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/NoReturnInBeginEndBlocks
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Ported from RuboCop Lint/NoReturnInBeginEndBlocks.
//!   Murphy translates both explicit `begin..end` and parenthesised groups
//!   `(expr)` to `NodeKind::Begin`, so parenthesised returns in assignment
//!   are also flagged — a superset of RuboCop's `kwbegin`-only detection.
//! ```
//!
//! ## Matched shapes
//!
//! - `return` inside `begin..end` blocks in assignment contexts
//! - Assignment forms: `=`, `+=`, `-=`, `*=`, `/=`, `**=`, `||=`
//! - All variable/constant targets: local, instance, class, global, constant
//! - Parenthesised `return` in assignment (`x = (return 1)`) is also flagged
//!   because Murphy lowers parentheses to `Begin` (superset of RuboCop).
//! - `return` inside nested method definitions or lambdas within the begin
//!   block is excluded (the return exits the inner scope, not the assignment
//!   context)

use murphy_plugin_api::{cop, Cx, NodeId, NodeKind, NoOptions};

const MSG: &str = "Do not `return` in `begin..end` blocks in assignment contexts.";

#[derive(Default)]
pub struct NoReturnInBeginEndBlocks;

#[cop(
    name = "Lint/NoReturnInBeginEndBlocks",
    description = "Do not `return` in `begin..end` blocks in assignment contexts.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl NoReturnInBeginEndBlocks {
    #[on_node(kind = "return")]
    fn check_return(&self, node: NodeId, cx: &Cx<'_>) {
        let mut begin_block = None;
        for ancestor in cx.ancestors(node) {
            let kind = cx.kind(ancestor);
            if matches!(kind, NodeKind::Def { .. } | NodeKind::Defs { .. }) {
                return;
            }
            if cx.is_lambda(ancestor) {
                return;
            }
            if matches!(kind, NodeKind::Begin(_)) {
                begin_block = Some(ancestor);
                break;
            }
        }
        let Some(begin_block) = begin_block else {
            return;
        };

        if !is_inside_assignment(begin_block, cx) {
            return;
        }

        cx.emit_offense(cx.range(node), MSG, None);
    }
}

/// Check whether `node` has an assignment ancestor before any method
/// definition boundary.
fn is_inside_assignment(node: NodeId, cx: &Cx<'_>) -> bool {
    for ancestor in cx.ancestors(node) {
        match *cx.kind(ancestor) {
            NodeKind::Lvasgn { .. }
            | NodeKind::Ivasgn { .. }
            | NodeKind::Cvasgn { .. }
            | NodeKind::Gvasgn { .. }
            | NodeKind::Casgn { .. }
            |             NodeKind::OpAsgn { .. }
            | NodeKind::OrAsgn { .. }
            | NodeKind::AndAsgn { .. } => return true,
            NodeKind::Def { .. } | NodeKind::Defs { .. } => return false,
            _ => {}
        }
    }
    false
}

murphy_plugin_api::submit_cop!(NoReturnInBeginEndBlocks);

#[cfg(test)]
mod tests {
    use super::NoReturnInBeginEndBlocks;
    use murphy_plugin_api::test_support::{indoc, test};

    // ── offenses ────────────────────────────────────────────────────────

    #[test]
    fn rejects_return_in_begin_with_lvar() {
        test::<NoReturnInBeginEndBlocks>().expect_offense(indoc! {r#"
            x = begin
              return 1
              ^^^^^^^^ Do not `return` in `begin..end` blocks in assignment contexts.
            end
        "#});
    }

    #[test]
    fn rejects_return_in_begin_with_ivar() {
        test::<NoReturnInBeginEndBlocks>().expect_offense(indoc! {r#"
            @x = begin
              return 1
              ^^^^^^^^ Do not `return` in `begin..end` blocks in assignment contexts.
            end
        "#});
    }

    #[test]
    fn rejects_return_in_begin_with_cvar() {
        test::<NoReturnInBeginEndBlocks>().expect_offense(indoc! {r#"
            @@x = begin
              return 1
              ^^^^^^^^ Do not `return` in `begin..end` blocks in assignment contexts.
            end
        "#});
    }

    #[test]
    fn rejects_return_in_begin_with_gvar() {
        test::<NoReturnInBeginEndBlocks>().expect_offense(indoc! {r#"
            $x = begin
              return 1
              ^^^^^^^^ Do not `return` in `begin..end` blocks in assignment contexts.
            end
        "#});
    }

    #[test]
    fn rejects_return_in_begin_with_const() {
        test::<NoReturnInBeginEndBlocks>().expect_offense(indoc! {r#"
            CONST = begin
              return 1
              ^^^^^^^^ Do not `return` in `begin..end` blocks in assignment contexts.
            end
        "#});
    }

    #[test]
    fn rejects_return_in_begin_with_or_asgn() {
        test::<NoReturnInBeginEndBlocks>().expect_offense(indoc! {r#"
            x ||= begin
              return 1
              ^^^^^^^^ Do not `return` in `begin..end` blocks in assignment contexts.
            end
        "#});
    }

    #[test]
    fn rejects_return_in_begin_with_op_asgn() {
        test::<NoReturnInBeginEndBlocks>().expect_offense(indoc! {r#"
            x += begin
              return 1
              ^^^^^^^^ Do not `return` in `begin..end` blocks in assignment contexts.
            end
        "#});
    }

    #[test]
    fn rejects_bare_return_in_begin() {
        test::<NoReturnInBeginEndBlocks>().expect_offense(indoc! {r#"
            x = begin
              return
              ^^^^^^ Do not `return` in `begin..end` blocks in assignment contexts.
            end
        "#});
    }

    // ── no offense (no return) ──────────────────────────────────────────

    #[test]
    fn accepts_begin_with_no_return() {
        test::<NoReturnInBeginEndBlocks>().expect_no_offenses(indoc! {r#"
            x = begin
              1
            end
        "#});
    }

    #[test]
    fn accepts_begin_with_no_return_or_asgn() {
        test::<NoReturnInBeginEndBlocks>().expect_no_offenses(indoc! {r#"
            x ||= begin
              1
            end
        "#});
    }

    // ── no offense (nested def / lambda) ────────────────────────────────

    #[test]
    fn accepts_return_in_nested_def() {
        test::<NoReturnInBeginEndBlocks>().expect_no_offenses(indoc! {r#"
            x = begin
              def foo
                return 1
              end
            end
        "#});
    }

    #[test]
    fn accepts_return_in_nested_stabby_lambda() {
        test::<NoReturnInBeginEndBlocks>().expect_no_offenses(indoc! {r#"
            x = begin
              -> { return 1 }
            end
        "#});
    }

    #[test]
    fn accepts_return_in_nested_lambda_method() {
        test::<NoReturnInBeginEndBlocks>().expect_no_offenses(indoc! {r#"
            x = begin
              lambda { return 1 }
            end
        "#});
    }

    // ── no offense (not in assignment context) ──────────────────────────

    #[test]
    fn accepts_return_in_begin_not_in_assignment() {
        test::<NoReturnInBeginEndBlocks>().expect_no_offenses(indoc! {r#"
            def foo
              begin
                return 1
              end
            end
        "#});
    }

    // ── edge cases ──────────────────────────────────────────────────────

    #[test]
    fn rejects_return_in_nested_begin() {
        test::<NoReturnInBeginEndBlocks>().expect_offense(indoc! {r#"
            x = begin
              begin
                return 1
                ^^^^^^^^ Do not `return` in `begin..end` blocks in assignment contexts.
              end
            end
        "#});
    }
}
