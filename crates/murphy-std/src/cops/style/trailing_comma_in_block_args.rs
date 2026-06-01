//! `Style/TrailingCommaInBlockArgs` — flags useless trailing commas in block arguments.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/TrailingCommaInBlockArgs
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Murphy v1 matches RuboCop's core behavior: flag trailing comma in block args
//!   when there are 2+ real (Arg/Optarg/Kwoptarg) arguments.  Single-arg trailing
//!   commas are intentional destructuring hints and are never flagged.
//!   The cop is disabled by default (Safe: false) — a trailing comma on a single arg
//!   can change runtime semantics for hash destructuring.
//!   Lambda literals (-> { ... }) are skipped (no pipe delimiters).
//!   Numblock / itblock nodes are skipped (they have no pipe-delimited params).
//! ```
//!
//! ## Matched shapes
//!
//! `Block` nodes with an `Args` node whose argument count (counting
//! `Arg`, `Optarg`, `Kwoptarg` children but not the trailing `Unknown`
//! pseudo-node) exceeds 1 and whose last non-empty token before the closing
//! `|` is a `Comma`.
//!
//! - `add { |foo, bar,| foo + bar }` — offense on the trailing `,`
//! - `add { |foo,| foo }` — no offense (single arg, may be intentional)
//! - `add { |foo, bar| foo + bar }` — no offense (no trailing comma)
//!
//! ## Autocorrect
//!
//! Deletes the trailing comma token.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, SourceTokenKind, cop};

const MSG: &str = "Useless trailing comma present in block arguments.";

/// Stateless unit struct.
#[derive(Default)]
pub struct TrailingCommaInBlockArgs;

#[cop(
    name = "Style/TrailingCommaInBlockArgs",
    description = "Checks for useless trailing commas in block arguments.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl TrailingCommaInBlockArgs {
    #[on_node(kind = "block")]
    fn check(&self, node: NodeId, cx: &Cx<'_>) {
        check_block(node, cx);
    }
}

fn check_block(node: NodeId, cx: &Cx<'_>) {
    // Skip lambda literals (`-> { ... }` — no pipe-delimited params).
    if cx.is_lambda_literal(node) {
        return;
    }

    let NodeKind::Block { args, .. } = *cx.kind(node) else {
        return;
    };

    let NodeKind::Args(args_list) = *cx.kind(args) else {
        return;
    };

    let arg_nodes = cx.list(args_list);

    // Count "real" arguments (Arg / Optarg / Kwoptarg), ignoring any trailing
    // Unknown pseudo-node that represents the trailing comma in the AST.
    let real_arg_count = arg_nodes
        .iter()
        .filter(|&&n| {
            matches!(
                cx.kind(n),
                NodeKind::Arg(_) | NodeKind::Optarg { .. } | NodeKind::Kwoptarg { .. }
            )
        })
        .count();

    // Fewer than 2 real args — trailing comma may be intentional (destructuring).
    if real_arg_count < 2 {
        return;
    }

    // Find the opening and closing pipe tokens within the block.
    // The `|` tokens are `SourceTokenKind::Other` with source `b"|"`.
    let block_range = cx.range(node);
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();

    let lo = toks.partition_point(|t| t.range.start < block_range.start);
    let mut pipes = toks[lo..]
        .iter()
        .take_while(|t| t.range.start < block_range.end)
        .filter(|t| {
            t.kind == SourceTokenKind::Other
                && &source[t.range.start as usize..t.range.end as usize] == b"|"
        })
        .copied();

    let Some(open_pipe) = pipes.next() else {
        return;
    };
    let Some(close_pipe) = pipes.next() else {
        return;
    };

    // Find the last non-comment token between the pipes.
    let toks_between_start = toks.partition_point(|t| t.range.start <= open_pipe.range.end);
    let last_inner_tok = toks[toks_between_start..]
        .iter()
        .take_while(|t| t.range.start < close_pipe.range.start)
        .filter(|t| !matches!(t.kind, SourceTokenKind::Comment))
        .last();

    let Some(last_tok) = last_inner_tok else {
        return;
    };

    // If the last meaningful token before the closing pipe is a comma, flag it.
    if last_tok.kind == SourceTokenKind::Comma {
        let comma_range = last_tok.range;
        cx.emit_offense(comma_range, MSG, None);
        // Autocorrect: delete the trailing comma.
        cx.emit_edit(comma_range, "");
    }
}

#[cfg(test)]
mod tests {
    use super::TrailingCommaInBlockArgs;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Offense cases -----

    #[test]
    fn flags_trailing_comma_brace_block() {
        test::<TrailingCommaInBlockArgs>().expect_correction(
            indoc! {"
                add { |foo, bar,| foo + bar }
                               ^ Useless trailing comma present in block arguments.
            "},
            "add { |foo, bar| foo + bar }\n",
        );
    }

    #[test]
    fn flags_trailing_comma_do_block() {
        test::<TrailingCommaInBlockArgs>().expect_correction(
            indoc! {"
                add do |foo, bar,|
                                ^ Useless trailing comma present in block arguments.
                  foo + bar
                end
            "},
            "add do |foo, bar|\n  foo + bar\nend\n",
        );
    }

    #[test]
    fn flags_three_args_trailing_comma() {
        test::<TrailingCommaInBlockArgs>().expect_correction(
            indoc! {"
                foo { |a, b, c,| a + b + c }
                              ^ Useless trailing comma present in block arguments.
            "},
            "foo { |a, b, c| a + b + c }\n",
        );
    }

    // ----- Non-offense cases -----

    #[test]
    fn accepts_single_arg_trailing_comma() {
        // Single arg trailing comma is intentional (hash destructuring hint).
        test::<TrailingCommaInBlockArgs>().expect_no_offenses("add { |foo,| foo }\n");
    }

    #[test]
    fn accepts_no_trailing_comma_two_args() {
        test::<TrailingCommaInBlockArgs>().expect_no_offenses("add { |foo, bar| foo + bar }\n");
    }

    #[test]
    fn accepts_no_args() {
        test::<TrailingCommaInBlockArgs>().expect_no_offenses("add { foo + bar }\n");
    }

    #[test]
    fn accepts_empty_pipes() {
        test::<TrailingCommaInBlockArgs>().expect_no_offenses("add { || foo }\n");
    }

    #[test]
    fn accepts_do_block_no_trailing_comma() {
        test::<TrailingCommaInBlockArgs>().expect_no_offenses(indoc! {"
            add do |foo, bar|
              foo + bar
            end
        "});
    }

    #[test]
    fn accepts_do_block_single_arg_trailing_comma() {
        test::<TrailingCommaInBlockArgs>().expect_no_offenses(indoc! {"
            add do |foo,|
              foo
            end
        "});
    }
}
murphy_plugin_api::submit_cop!(TrailingCommaInBlockArgs);
