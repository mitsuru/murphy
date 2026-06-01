//! `Style/SingleLineDoEndBlock` — flags single-line `do`...`end` blocks.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/SingleLineDoEndBlock
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detects Block, Numblock, and Itblock nodes that are single-line and use
//!   do...end delimiters (not braces). Brace blocks are skipped.
//!   Autocorrect inserts a newline after the do opener (after the args or after
//!   the do keyword for no-arg / numblock / itblock / lambda-literal blocks)
//!   and a newline before the `end` keyword.
//!   Gap: does not check Layout/RedundantLineBreak configuration (Murphy has no
//!   equivalent concept yet).
//! ```
//!
//! ## Matched shapes
//!
//! `block`, `numblock`, and `itblock` nodes that:
//! - Are single-line
//! - Use `do`...`end` delimiters (no `LeftBrace` token in range)
//!
//! ## Autocorrect
//!
//! Inserts newlines to expand the block to multiple lines:
//! 1. After the block args (or after the `do` keyword for no-arg / numblock /
//!    itblock / lambda-literal blocks)
//! 2. Before the `end` keyword

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

const MSG: &str = "Prefer multiline `do`...`end` block.";

#[derive(Default)]
pub struct SingleLineDoEndBlock;

#[cop(
    name = "Style/SingleLineDoEndBlock",
    description = "Checks for single-line `do`...`end` blocks.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl SingleLineDoEndBlock {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// Returns true if the node uses `do`...`end` block delimiters.
/// A brace block has a `LeftBrace` token within its range; a do/end block does not.
fn is_do_end_block(node: NodeId, cx: &Cx<'_>) -> bool {
    let range = cx.range(node);
    let toks = cx.tokens_in(range);
    !toks.iter().any(|t| t.kind == SourceTokenKind::LeftBrace)
}

/// Find the `do` keyword token in the range `[from, to)`.
fn find_do_token(cx: &Cx<'_>, from: u32, to: u32) -> Option<Range> {
    if from >= to {
        return None;
    }
    let toks = cx.sorted_tokens();
    let src = cx.source().as_bytes();
    let idx = toks.partition_point(|t| t.range.start < from);
    for tok in &toks[idx..] {
        if tok.range.start >= to {
            break;
        }
        if tok.kind == SourceTokenKind::Other
            && &src[tok.range.start as usize..tok.range.end as usize] == b"do"
        {
            return Some(tok.range);
        }
    }
    None
}

/// Find the `end` keyword token that closes the block.
fn find_end_token(node: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let node_end = cx.range(node).end;
    let toks = cx.sorted_tokens();
    let src = cx.source().as_bytes();
    let idx = toks.partition_point(|t| t.range.end < node_end);
    if let Some(tok) = toks.get(idx) {
        if tok.range.end == node_end
            && tok.kind == SourceTokenKind::Other
            && &src[tok.range.start as usize..tok.range.end as usize] == b"end"
        {
            return Some(tok.range);
        }
    }
    None
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Must be single-line.
    if cx.is_multiline(node) {
        return;
    }

    // Must be a do...end block (not a brace block).
    if !is_do_end_block(node, cx) {
        return;
    }

    cx.emit_offense(cx.range(node), MSG, None);

    // Autocorrect: find the do token and end token, insert newlines.
    let node_start = cx.range(node).start;
    let node_end = cx.range(node).end;

    let Some(do_tok) = find_do_token(cx, node_start, node_end) else {
        return;
    };
    let Some(end_tok) = find_end_token(node, cx) else {
        return;
    };

    // Determine where to insert the first newline.
    // RuboCop: insert after `do` for: no args, numblock, itblock, lambda literal.
    // Insert after args (closing `|`) for: blocks with arguments.
    let insert_after = match *cx.kind(node) {
        NodeKind::Block { args, .. } => {
            let is_lambda_lit = cx.is_lambda_literal(node);
            let has_args = if let NodeKind::Args(list) = *cx.kind(args) {
                !cx.list(list).is_empty()
            } else {
                false
            };
            if is_lambda_lit || !has_args {
                do_tok.end
            } else {
                // Insert after the closing `|` of block args.
                cx.range(args).end
            }
        }
        // Numblock and Itblock have no explicit args node — insert after `do`.
        NodeKind::Numblock { .. } | NodeKind::Itblock { .. } => do_tok.end,
        _ => do_tok.end,
    };

    // Insert newline after do / args.
    cx.emit_edit(
        Range {
            start: insert_after,
            end: insert_after,
        },
        "\n",
    );

    // Insert newline before `end`.
    cx.emit_edit(
        Range {
            start: end_tok.start,
            end: end_tok.start,
        },
        "\n",
    );
}

#[cfg(test)]
mod tests {
    use super::SingleLineDoEndBlock;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_single_line_do_end_block_with_args() {
        test::<SingleLineDoEndBlock>().expect_offense(indoc! {"
            foo do |arg| bar(arg) end
            ^^^^^^^^^^^^^^^^^^^^^^^^^  Prefer multiline `do`...`end` block.
        "});
    }

    #[test]
    fn flags_single_line_do_end_block_no_args() {
        test::<SingleLineDoEndBlock>().expect_offense(indoc! {"
            foo do bar end
            ^^^^^^^^^^^^^^  Prefer multiline `do`...`end` block.
        "});
    }

    #[test]
    fn flags_lambda_do_end_block() {
        test::<SingleLineDoEndBlock>().expect_offense(indoc! {"
            ->(arg) do bar(arg) end
            ^^^^^^^^^^^^^^^^^^^^^^^  Prefer multiline `do`...`end` block.
        "});
    }

    #[test]
    fn accepts_brace_block() {
        test::<SingleLineDoEndBlock>().expect_no_offenses("foo { |arg| bar(arg) }\n");
    }

    #[test]
    fn accepts_multiline_do_end_block() {
        test::<SingleLineDoEndBlock>().expect_no_offenses(indoc! {"
            foo do |arg|
              bar(arg)
            end
        "});
    }

    #[test]
    fn flags_numblock_do_end() {
        test::<SingleLineDoEndBlock>().expect_offense(indoc! {"
            foo do _1 end
            ^^^^^^^^^^^^^  Prefer multiline `do`...`end` block.
        "});
    }
}

murphy_plugin_api::submit_cop!(SingleLineDoEndBlock);
