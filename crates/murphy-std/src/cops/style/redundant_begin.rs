//! `Style/RedundantBegin` — flags redundant `begin` blocks when the
//! `rescue`/`ensure` can be handled directly, or when there is no
//! rescue/ensure at all.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantBegin
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Murphy emits no `kwbegin` AST node — explicit `begin...end` compiles to a
//!   `Begin` node (same as the implicit method-body grouping). The explicit
//!   form is detected by checking whether the first token in the node's range
//!   is the `begin` keyword.
//!
//!   Implemented contexts:
//!   - def/defs body: begin is redundant when it is the sole child of the method
//!     body and is the entire method body (no other statements around it).
//!   - Standalone begin without rescue/ensure: always redundant.
//!   - while/until body: redundant when no rescue/ensure inside.
//!   - do-end block body: redundant (Ruby 2.5+, checked always since Murphy
//!     targets modern Ruby); not applied to brace blocks or lambda do-end.
//!   - begin in if/case branches without rescue/ensure: redundant.
//!
//!   Not implemented (conservative gaps):
//!   - Modifier-form if/while (already handled by the `begin` handler above).
//!   - post-condition loops: `begin...end while c` — parent is While{post:true}.
//!   - begin in an assignment (valid_begin_assignment check).
//!   - begin in a send expression (no autocorrect needed).
//!   - Autocorrect: offense only (no autocorrect emitted in this implementation).
//!     Removing a `begin`/`end` pair requires whitespace-aware correction that
//!     touches lines, which is complex; the offense guides the user.
//! ```
//!
//! ## Offense conditions
//!
//! An explicit `begin` block (one whose first token is the `begin` keyword) is
//! flagged when:
//!
//! 1. It is the sole body of a method definition (`def`/`defs`).
//! 2. It is standalone (parent is not a context that legitimises it) and has
//!    no rescue/ensure.
//! 3. It is the body of a `while`/`until` loop, and has no rescue/ensure.
//! 4. It is the body of a `do-end` block (not a brace block, not a lambda).
//! 5. It is in an `if`/`unless`/`case` branch, and has no rescue/ensure.
//!
//! ## Not flagged
//!
//! - Empty begin blocks.
//! - Multi-statement begin in an assignment context.
//! - `begin...end while c` (post-condition loop) — parent is `While{post:true}`.
//! - `begin` inside a `send` argument expression.
//! - Brace-delimited blocks.
//! - Lambda `do-end` blocks.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, SourceTokenKind, cop};

const MSG: &str = "Redundant `begin` block detected.";

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantBegin;

#[cop(
    name = "Style/RedundantBegin",
    description = "Don't use `begin` blocks when they are not needed.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RedundantBegin {
    /// Entry point: check every `Begin` node for being an explicit `begin` block.
    #[on_node(kind = "begin")]
    fn check_begin(&self, node: NodeId, cx: &Cx<'_>) {
        // Only check explicit begin blocks (those that start with the `begin` keyword).
        if !is_explicit_begin(node, cx) {
            return;
        }

        let parent_opt = cx.parent(node);
        let Some(parent) = parent_opt.get() else {
            // Root node — treat as standalone.
            if !has_rescue_or_ensure(node, cx) && !is_empty_begin(node, cx) {
                emit_offense(node, cx);
            }
            return;
        };

        match cx.kind(parent) {
            // --- def / defs: flag when begin is the sole body ---
            NodeKind::Def { .. } | NodeKind::Defs { .. }
                // The begin is the direct body of the method.
                // It is redundant regardless of rescue/ensure (RuboCop removes the
                // begin/end, keeping the rescue/ensure on the method itself).
                // Empty begin blocks are never flagged.
                if !is_empty_begin(node, cx) => {
                    emit_offense(node, cx);
                }

            // --- while/until: flag when no rescue/ensure ---
            NodeKind::While { post, .. } | NodeKind::Until { post, .. } => {
                // post-condition loops (`begin...end while c`) are not redundant.
                if *post {
                    return;
                }
                if !has_rescue_or_ensure(node, cx) && !is_empty_begin(node, cx) {
                    emit_offense(node, cx);
                }
            }

            // --- do-end block (non-lambda): flag unless empty ---
            NodeKind::Block { call, .. } => {
                // Skip brace blocks.
                if is_brace_block(parent, cx) {
                    return;
                }
                // Skip lambda do-end.
                if is_lambda_call(*call, cx) {
                    return;
                }
                // Empty begin blocks are never flagged.
                if !is_empty_begin(node, cx) {
                    emit_offense(node, cx);
                }
            }

            // --- if/unless/case branches: flag when no rescue/ensure ---
            NodeKind::If { .. } | NodeKind::Case { .. } | NodeKind::When { .. }
                if !has_rescue_or_ensure(node, cx) && !is_empty_begin(node, cx) => {
                    emit_offense(node, cx);
                }

            // --- standalone begin inside a program body (parent is Begin) ---
            NodeKind::Begin(_) => {
                // Skip if parent is an assignment or send.
                // Check grandparent for assignment context.
                if is_in_assignment_context(node, cx) {
                    return;
                }
                if is_in_send_context(parent, cx) {
                    return;
                }
                // standalone begin: redundant if no rescue/ensure and not empty.
                if !has_rescue_or_ensure(node, cx) && !is_empty_begin(node, cx) {
                    emit_offense(node, cx);
                }
            }

            // All other contexts: skip.
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Detection helpers
// ---------------------------------------------------------------------------

/// Returns `true` if the `Begin` node starts with the `begin` keyword token.
fn is_explicit_begin(node: NodeId, cx: &Cx<'_>) -> bool {
    let node_range = cx.range(node);
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();

    // Check that the first token is `begin`.
    let idx = toks.partition_point(|t| t.range.start < node_range.start);
    let first_tok = toks[idx..].iter().find(|t| t.range.start < node_range.end);
    let Some(first) = first_tok else {
        return false;
    };
    if !(first.kind == SourceTokenKind::Other
        && &source[first.range.start as usize..first.range.end as usize] == b"begin")
    {
        return false;
    }

    // Also check that the last token before the end of the node's range is `end`.
    // This distinguishes an explicit `begin...end` from an implicit method-body
    // `Begin` whose first statement happens to start with `begin`.
    let last_tok = toks[..idx + toks[idx..]
        .partition_point(|t| t.range.end <= node_range.end)]
        .iter()
        .rev()
        .find(|t| t.range.end <= node_range.end);
    let Some(last) = last_tok else {
        return false;
    };
    last.kind == SourceTokenKind::Other
        && &source[last.range.start as usize..last.range.end as usize] == b"end"
}

/// Returns `true` if this explicit begin block has a rescue or ensure child.
/// Structure: `Begin([Rescue{...}])` or `Begin([Ensure{...}])`.
fn has_rescue_or_ensure(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Begin(list) = cx.kind(node) else {
        return false;
    };
    let children = cx.list(*list);
    if let Some(&first_child) = children.first() {
        return matches!(
            cx.kind(first_child),
            NodeKind::Rescue { .. } | NodeKind::Ensure { .. }
        );
    }
    false
}

/// Returns `true` if this begin block has no children (empty).
fn is_empty_begin(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Begin(list) = cx.kind(node) else {
        return false;
    };
    cx.list(*list).is_empty()
}

/// Returns `true` if the `Block` node uses `{...}` (brace block).
/// Detection: the first token after the `Block`'s call range is `{`.
fn is_brace_block(block_node: NodeId, cx: &Cx<'_>) -> bool {
    let node_range = cx.range(block_node);
    let toks = cx.sorted_tokens();
    // Scan for `{` or `do` in the block node's range.
    let idx = toks.partition_point(|t| t.range.start < node_range.start);
    let source = cx.source().as_bytes();
    for tok in &toks[idx..] {
        if tok.range.start >= node_range.end {
            break;
        }
        if tok.kind == SourceTokenKind::LeftBrace {
            return true;
        }
        if tok.kind == SourceTokenKind::Other
            && &source[tok.range.start as usize..tok.range.end as usize] == b"do"
        {
            return false;
        }
    }
    false
}

/// Returns `true` if the call node is a lambda literal (`lambda` or `-> `).
fn is_lambda_call(call: NodeId, cx: &Cx<'_>) -> bool {
    match cx.kind(call) {
        NodeKind::Lambda => true,
        NodeKind::Send { method, receiver, .. } => {
            receiver.is_none() && cx.symbol_str(*method) == "lambda"
        }
        _ => false,
    }
}

/// Returns `true` if this `begin` node is in an assignment context (parent is
/// an assignment node). Multi-statement begin in assignment is valid.
fn is_in_assignment_context(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    let is_asgn = matches!(
        cx.kind(parent),
        NodeKind::Lvasgn { .. }
            | NodeKind::Ivasgn { .. }
            | NodeKind::Cvasgn { .. }
            | NodeKind::Gvasgn { .. }
            | NodeKind::Casgn { .. }
    );
    if !is_asgn {
        return false;
    }
    // Multi-statement begin in assignment is not redundant
    // (valid_begin_assignment?: children.count >= 2).
    // Single-statement IS redundant.
    let NodeKind::Begin(list) = cx.kind(node) else {
        return false;
    };
    cx.list(*list).len() >= 2
}

/// Returns `true` if the given `Begin` (parent of our node) is inside a `send`.
fn is_in_send_context(parent: NodeId, cx: &Cx<'_>) -> bool {
    let Some(grandparent) = cx.parent(parent).get() else {
        return false;
    };
    matches!(cx.kind(grandparent), NodeKind::Send { .. } | NodeKind::Csend { .. })
}

// ---------------------------------------------------------------------------
// Offense emission
// ---------------------------------------------------------------------------

/// Emit an offense at the `begin` keyword token of the given node.
fn emit_offense(node: NodeId, cx: &Cx<'_>) {
    let node_range = cx.range(node);
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < node_range.start);
    let begin_tok = toks[idx..].iter().find(|t| {
        t.range.start < node_range.end
            && t.kind == SourceTokenKind::Other
            && &source[t.range.start as usize..t.range.end as usize] == b"begin"
    });
    if let Some(tok) = begin_tok {
        cx.emit_offense(tok.range, MSG, None);
    }
}

#[cfg(test)]
mod tests {
    use super::RedundantBegin;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- def with redundant begin block (has rescue) ---

    #[test]
    fn flags_def_with_redundant_begin_with_rescue() {
        test::<RedundantBegin>().expect_offense(indoc! {r#"
            def func
              begin
              ^^^^^ Redundant `begin` block detected.
                ala
              rescue => e
                bala
              end
            end
        "#});
    }

    // --- defs with redundant begin block ---

    #[test]
    fn flags_defs_with_redundant_begin() {
        test::<RedundantBegin>().expect_offense(indoc! {r#"
            def Test.func
              begin
              ^^^^^ Redundant `begin` block detected.
                ala
              rescue => e
                bala
              end
            end
        "#});
    }

    // --- def: required begin block (not the sole body) ---

    #[test]
    fn no_offense_def_begin_not_sole_body() {
        test::<RedundantBegin>().expect_no_offenses(indoc! {r#"
            def func
              begin
                ala
              rescue => e
                bala
              end
              something
            end
        "#});
    }

    // --- Standalone begin without rescue/ensure ---

    #[test]
    fn flags_standalone_begin_without_rescue() {
        test::<RedundantBegin>().expect_offense(indoc! {r#"
            begin
            ^^^^^ Redundant `begin` block detected.
              do_something
            end
        "#});
    }

    #[test]
    fn flags_standalone_begin_multiple_statements() {
        test::<RedundantBegin>().expect_offense(indoc! {r#"
            begin
            ^^^^^ Redundant `begin` block detected.
              foo
              bar
            end
        "#});
    }

    // --- Standalone begin with rescue/ensure: NOT redundant ---

    #[test]
    fn no_offense_standalone_begin_with_rescue() {
        test::<RedundantBegin>().expect_no_offenses(indoc! {r#"
            begin
              do_something
            rescue
              handle_exception
            end
        "#});
    }

    #[test]
    fn no_offense_standalone_begin_with_ensure() {
        test::<RedundantBegin>().expect_no_offenses(indoc! {r#"
            begin
              do_something
            ensure
              finalize
            end
        "#});
    }

    // --- do-end block: redundant begin ---

    #[test]
    fn flags_begin_in_do_end_block_with_rescue() {
        test::<RedundantBegin>().expect_offense(indoc! {r#"
            do_something do
              begin
              ^^^^^ Redundant `begin` block detected.
                something
              rescue => ex
                anything
              end
            end
        "#});
    }

    // --- brace block: begin is NOT flagged ---

    #[test]
    fn no_offense_begin_in_brace_block() {
        test::<RedundantBegin>().expect_no_offenses(indoc! {r#"
            do_something {
              begin
                something
              rescue => ex
                anything
              end
            }
        "#});
    }

    // --- lambda do-end: begin is NOT flagged ---

    #[test]
    fn no_offense_begin_in_lambda_do_end() {
        test::<RedundantBegin>().expect_no_offenses(indoc! {r#"
            -> do
              begin
                foo
              rescue Bar
                baz
              end
            end
        "#});
    }

    // --- while/until: begin without rescue/ensure is redundant ---

    #[test]
    fn flags_begin_in_while_without_rescue() {
        test::<RedundantBegin>().expect_offense(indoc! {r#"
            while true
              begin
              ^^^^^ Redundant `begin` block detected.
                x = 1
              end
            end
        "#});
    }

    // --- while with post-condition: NOT flagged ---

    #[test]
    fn no_offense_post_condition_while() {
        test::<RedundantBegin>().expect_no_offenses(indoc! {r#"
            begin
              x = 1
            end while true
        "#});
    }

    // --- empty begin: not flagged ---

    #[test]
    fn no_offense_empty_begin_in_def() {
        test::<RedundantBegin>().expect_no_offenses(indoc! {r#"
            def func
              begin
              end
            end
        "#});
    }

    #[test]
    fn no_offense_empty_begin_in_do_end_block() {
        test::<RedundantBegin>().expect_no_offenses(indoc! {r#"
            do_something do
              begin
              end
            end
        "#});
    }

    #[test]
    fn no_offense_empty_standalone_begin() {
        test::<RedundantBegin>().expect_no_offenses(indoc! {r#"
            begin
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(RedundantBegin);
