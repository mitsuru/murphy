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

use murphy_plugin_api::{cop, Cx, NodeId, NodeKind, NoOptions, SourceTokenKind};

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
            // An enclosing method or block (including lambdas/procs) scopes the
            // `return` to itself — RuboCop's `on_kwbegin` only flags returns
            // directly inside a `begin..end` value block, never those nested in
            // a method or block.
            if matches!(
                kind,
                NodeKind::Def { .. }
                    | NodeKind::Defs { .. }
                    | NodeKind::Block { .. }
                    | NodeKind::Numblock { .. }
                    | NodeKind::Itblock { .. }
            ) || cx.is_lambda(ancestor)
            {
                return;
            }
            if matches!(kind, NodeKind::Begin(_)) {
                // Murphy lowers explicit `begin..end`, parenthesised groups AND
                // implicit statement sequences (if/case branches, block/method
                // bodies) all to `Begin`. Only the first two are value blocks;
                // an implicit sequence is skipped so the walk continues to the
                // real `begin..end` or scope boundary above it.
                if is_kwbegin_or_paren(ancestor, cx) {
                    begin_block = Some(ancestor);
                    break;
                }
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

/// True when the `Begin` node is an explicit `begin..end` block or a
/// parenthesised group `(...)` — the only `Begin` shapes that act as a value
/// block. The first source token at the node's start is `begin` or `(`
/// respectively; an implicit statement sequence starts with its first
/// statement's token instead.
fn is_kwbegin_or_paren(node: NodeId, cx: &Cx<'_>) -> bool {
    let start = cx.range(node).start;
    cx.token_after(start).is_some_and(|t| {
        t.range.start == start
            && (t.kind == SourceTokenKind::LeftParen
                || (t.kind == SourceTokenKind::Other && cx.raw_source(t.range) == "begin"))
    })
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

    #[test]
    fn accepts_guard_return_in_assigned_multiline_lambda() {
        // Mastodon shape: a multi-statement lambda body (lowered to `Begin`)
        // assigned to a constant. The `return` belongs to the lambda, not a
        // `begin..end` value block, so it must not be flagged.
        test::<NoReturnInBeginEndBlocks>().expect_no_offenses(indoc! {r#"
            TRANSFORMER = lambda do |env|
              return unless env[:node]

              env[:node].do_something
            end
        "#});
    }

    #[test]
    fn accepts_return_inside_if_branch_within_assigned_lambda() {
        // The `if` branch body is also lowered to `Begin`; the `return` still
        // belongs to the lambda, not a `begin..end` value block.
        test::<NoReturnInBeginEndBlocks>().expect_no_offenses(indoc! {r#"
            X = lambda do |env|
              if env[:node]
                env[:node].do_something
                return
              end

              env
            end
        "#});
    }

    #[test]
    fn accepts_guard_return_in_assigned_multiline_block() {
        // The same with a non-lambda block assigned via a method call.
        test::<NoReturnInBeginEndBlocks>().expect_no_offenses(indoc! {r#"
            X = [1].each do |env|
              return unless env

              env.to_s
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
