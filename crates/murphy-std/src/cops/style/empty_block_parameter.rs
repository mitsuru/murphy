//! `Style/EmptyBlockParameter` — omit pipes for empty block parameters.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/EmptyBlockParameter
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detects `Block` nodes with an `Args` child that has no arguments but
//!   whose source range spans one or more `|` pipe tokens (i.e. `do ||` or
//!   `{ || }`). Only stabby lambda literals (`-> { || }`) are excluded —
//!   matching RuboCop's `lambda_literal?` guard. The `lambda { || }` method
//!   form IS flagged (consistent with RuboCop behavior). Autocorrect removes the empty pipes and surrounding
//!   whitespace, leaving only the block opener.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # offense
//! a do ||
//!   body
//! end
//!
//! b { || body }
//!
//! # no offense — block has no pipes
//! a do
//!   body
//! end
//!
//! # no offense — block has args
//! a do |x|
//!   body
//! end
//!
//! # no offense — lambda literal
//! lambda { || }
//! ```
//!
//! ## Autocorrect
//!
//! Removes the empty `||` pipes and whitespace before them:
//! `a do ||` => `a do`, `b { || }` => `b { }`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

const MSG: &str = "Omit pipes for the empty block parameters.";

/// Stateless unit struct.
#[derive(Default)]
pub struct EmptyBlockParameter;

#[cop(
    name = "Style/EmptyBlockParameter",
    description = "Omit pipes for empty block parameters.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl EmptyBlockParameter {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        // Exclude stabby lambda literals (`-> { || }`) per RuboCop's `lambda_literal?` guard.
        // The `lambda { || }` method-call form IS flagged.
        if cx.is_lambda_literal(node) {
            return;
        }

        let NodeKind::Block { args, .. } = *cx.kind(node) else {
            return;
        };

        let NodeKind::Args(list) = *cx.kind(args) else {
            return;
        };

        // Args must be empty (no parameters).
        if !cx.list(list).is_empty() {
            return;
        }

        // Detect empty `||` pipes by scanning for pipe tokens near the args node.
        let Some(pipes_range) = find_empty_pipes(node, args, cx) else {
            return;
        };

        cx.emit_offense(pipes_range, MSG, None);

        // Autocorrect: remove from after the block opener to the end of the
        // closing pipe (inclusive), stripping the empty `||` and whitespace.
        if let Some(removal) = pipes_removal_range(node, pipes_range, cx) {
            cx.emit_edit(removal, "");
        }
    }
}

/// Find the range spanning two empty `|` pipe tokens in the block.
///
/// Returns `None` when there are no pipes (normal parameterless block).
fn find_empty_pipes(block_node: NodeId, args_node: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let source = cx.source().as_bytes();
    let block_range = cx.range(block_node);
    let args_range = cx.range(args_node);

    // If the args node has zero width, there are no pipes.
    if args_range.start >= args_range.end {
        return None;
    }

    // Scan from block start with a generous window (past args end) for `|` tokens.
    let search_limit = args_range.end.saturating_add(4).min(block_range.end);
    let toks = cx.sorted_tokens();
    let lo = toks.partition_point(|t| t.range.start < block_range.start);

    let mut pipes = toks[lo..]
        .iter()
        .take_while(|t| t.range.start < search_limit)
        .filter(|t| {
            t.kind == SourceTokenKind::Other
                && &source[t.range.start as usize..t.range.end as usize] == b"|"
        })
        .copied();

    let open_pipe = pipes.next()?;
    let close_pipe = pipes.next()?;

    Some(Range {
        start: open_pipe.range.start,
        end: close_pipe.range.end,
    })
}

/// Compute the removal range: from after the block opener token to the end of
/// the closing pipe (inclusive).
///
/// This removes ` ||` from `a do ||` and ` ||` from `a { || body }`.
fn pipes_removal_range(block_node: NodeId, pipes_range: Range, cx: &Cx<'_>) -> Option<Range> {
    let source = cx.source().as_bytes();
    let open_pipe_start = pipes_range.start;
    let block_range = cx.range(block_node);
    let toks = cx.sorted_tokens();

    // Find the opener token (do keyword or { brace) just before the first pipe.
    let opener = toks
        .iter()
        .take_while(|t| t.range.start < open_pipe_start)
        .filter(|t| t.range.start >= block_range.start)
        .filter(|t| {
            t.kind == SourceTokenKind::LeftBrace
                || (t.kind == SourceTokenKind::Other
                    && &source[t.range.start as usize..t.range.end as usize] == b"do")
        })
        .last()?;

    // Remove from opener.end to pipes_range.end.
    Some(Range {
        start: opener.range.end,
        end: pipes_range.end,
    })
}

#[cfg(test)]
mod tests {
    use super::EmptyBlockParameter;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Positive cases (offense) ------------------------------------

    #[test]
    fn flags_do_block_with_empty_pipes() {
        test::<EmptyBlockParameter>().expect_offense(indoc! {"
            a do ||
                 ^^ Omit pipes for the empty block parameters.
              body
            end
        "});
    }

    #[test]
    fn flags_brace_block_with_empty_pipes() {
        test::<EmptyBlockParameter>().expect_offense(indoc! {"
            a { || body }
                ^^ Omit pipes for the empty block parameters.
        "});
    }

    // ----- Negative cases (no offense) --------------------------------

    #[test]
    fn accepts_do_block_without_pipes() {
        test::<EmptyBlockParameter>().expect_no_offenses(indoc! {"
            a do
              body
            end
        "});
    }

    #[test]
    fn accepts_brace_block_without_pipes() {
        test::<EmptyBlockParameter>().expect_no_offenses("a { body }\n");
    }

    #[test]
    fn accepts_block_with_args() {
        test::<EmptyBlockParameter>().expect_no_offenses(indoc! {"
            a do |x|
              body
            end
        "});
    }

    #[test]
    fn flags_lambda_method_block_with_empty_pipes() {
        // RuboCop guards on `lambda_literal?` (stabby `->` only); the `lambda {}` form is flagged.
        test::<EmptyBlockParameter>().expect_offense(indoc! {"
            lambda { || }
                     ^^ Omit pipes for the empty block parameters.
        "});
    }

    // ----- Autocorrect -----------------------------------------------

    #[test]
    fn corrects_do_block_empty_pipes() {
        test::<EmptyBlockParameter>().expect_correction(
            indoc! {"
                a do ||
                     ^^ Omit pipes for the empty block parameters.
                  body
                end
            "},
            "a do\n  body\nend\n",
        );
    }

    #[test]
    fn corrects_brace_block_empty_pipes() {
        test::<EmptyBlockParameter>().expect_correction(
            indoc! {"
                a { || body }
                    ^^ Omit pipes for the empty block parameters.
            "},
            "a { body }\n",
        );
    }
}
murphy_plugin_api::submit_cop!(EmptyBlockParameter);
