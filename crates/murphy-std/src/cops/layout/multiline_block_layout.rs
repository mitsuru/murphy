//! `Layout/MultilineBlockLayout` — multi-line `do`/`end` (and brace) blocks
//! must put the body on a line after the block start, and block arguments on
//! the same line as the block start.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/MultilineBlockLayout
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Ports `on_block` (aliased to `on_numblock`/`on_itblock`). For a
//!   multi-line block (`single_line?` short-circuits) two checks run:
//!
//!   1. Argument placement (`ARG_MSG`): if the block has arguments and they
//!      are not on the block-opener line (`args_on_beginning_line?`) and a
//!      line break in the args is not "necessary" because they would
//!      overflow the line-length limit (`line_break_necessary_in_args?`),
//!      the arguments are flagged.
//!   2. Body placement (`MSG`): if the body begins on the same line as the
//!      block opener (`do`/`{`), the body is flagged.
//!
//!   `line_break_necessary_in_args?` reconstructs the single-line length of
//!   the block opener plus its arguments (`needed_length_for_args` /
//!   `block_arg_string`, including the `mlhs` `(a, b)` recursion and the
//!   single-destructured-arg trailing comma) and compares it to the line
//!   length limit.
//!
//!   Autocorrect: not implemented (v1 gap). RuboCop rewrites the arguments
//!   onto the opener line and/or inserts a newline before the body; the
//!   detect-only port ships without it.
//!
//!   Gap vs RuboCop: the line-length limit is hardcoded at RuboCop's default
//!   of 120 (`Layout/LineLength: Max`). A user-overridden `Max` is not read
//!   — the same foreign-config gap that `Style/IfUnlessModifier` documents.
//! ```
//!
//! ## Matched shapes
//!
//! Multi-line `block`/`numblock`/`itblock` nodes whose arguments are not on
//! the opener line, or whose body shares the opener line.

use crate::cops::util::{block_opener, gap_has_newline};
use murphy_plugin_api::{Cx, NodeId, NodeKind, Range, cop};

const MSG: &str = "Block body expression is on the same line as the block start.";
const ARG_MSG: &str = "Block argument expression is not on the same line as the block start.";

/// RuboCop's hardcoded fallback: `Layout/LineLength: Max` defaults to 120.
/// A user override is not read (documented gap).
const MAX_LINE_LENGTH: usize = 120;

/// Stateless unit struct (ADR 0035).
#[derive(Default)]
pub struct MultilineBlockLayout;

#[cop(
    name = "Layout/MultilineBlockLayout",
    description = "Ensures newlines after multiline block do statements.",
    default_severity = "warning",
    // RuboCop ships this cop `Enabled: true` — the odd one out among the
    // multiline-layout ports.
    default_enabled = true,
)]
impl MultilineBlockLayout {
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

fn check(node: NodeId, cx: &Cx<'_>) {
    // RuboCop: `return if node.single_line?`.
    if cx.is_single_line(node) {
        return;
    }

    let Some(opener) = block_opener(node, cx) else {
        return;
    };

    // --- argument placement ---
    let args = block_arg_nodes(node, cx);
    if !args.is_empty()
        && !args_on_beginning_line(opener, &args, cx)
        && !line_break_necessary_in_args(node, opener, &args, cx)
    {
        // RuboCop highlights `node.arguments.source_range` — the span from the
        // first to the last block argument.
        let range = args_range(&args, cx);
        cx.emit_offense(first_line_range(range, cx), ARG_MSG, None);
    }

    // --- body placement ---
    let Some(body) = cx.block_body(node).get() else {
        return;
    };
    // RuboCop: `return unless ... same_line?(node.loc.begin, node.body)`. The
    // body shares the opener's line iff no newline lies between them.
    let src = cx.source().as_bytes();
    if !gap_has_newline(src, opener.start, cx.range(body).start) {
        cx.emit_offense(first_line_range(cx.range(body), cx), MSG, None);
    }
}

/// RuboCop's `args_on_beginning_line?` (the `node.arguments?` short-circuit is
/// handled by the caller's `!args.is_empty()` guard): the block opener's line
/// equals the arguments' *last* line — i.e. no newline lies between the opener
/// and the last argument's final byte.
fn args_on_beginning_line(opener: Range, args: &[NodeId], cx: &Cx<'_>) -> bool {
    let last = *args.last().expect("args is non-empty");
    let r = cx.range(last);
    let last_end = r.end.saturating_sub(1).max(r.start);
    let src = cx.source().as_bytes();
    !gap_has_newline(src, opener.start, last_end)
}

/// RuboCop's `line_break_necessary_in_args?`: a line break in the arguments is
/// acceptable when reconstructing them on the opener line would exceed the
/// line-length limit.
fn line_break_necessary_in_args(node: NodeId, opener: Range, args: &[NodeId], cx: &Cx<'_>) -> bool {
    needed_length_for_args(node, opener, args, cx) > MAX_LINE_LENGTH
}

/// RuboCop's `needed_length_for_args`: the column of the block plus the
/// space/pipe overhead plus the opener line's existing length plus the
/// reconstructed argument string length.
fn needed_length_for_args(node: NodeId, opener: Range, args: &[NodeId], cx: &Cx<'_>) -> usize {
    block_start_column(node, cx)
        + characters_needed_for_space_and_pipes(opener, cx)
        + first_line_len(node, cx)
        + block_arg_string(args, cx).chars().count()
}

/// RuboCop's `node.source_range.column` — the visible column at which the
/// block expression starts.
fn block_start_column(node: NodeId, cx: &Cx<'_>) -> usize {
    let start = cx.range(node).start as usize;
    let src = cx.source();
    let line_start = src[..start].rfind('\n').map_or(0, |pos| pos + 1);
    src[line_start..start].chars().count()
}

/// RuboCop's `characters_needed_for_space_and_pipes`. If the opener line
/// already ends with the opening pipe (`|\n`), only one pipe is needed;
/// otherwise space + two pipes.
fn characters_needed_for_space_and_pipes(opener: Range, cx: &Cx<'_>) -> usize {
    const PIPE_SIZE: usize = 1;
    let src = cx.source();
    let start = opener.start as usize;
    let line_end = src[start..]
        .find('\n')
        .map_or(src.len(), |pos| start + pos);
    if src[start..line_end].ends_with('|') {
        PIPE_SIZE
    } else {
        (PIPE_SIZE * 2) + 1
    }
}

/// RuboCop's `node.source.lines.first.chomp.length` — the length of the
/// block's first physical line (without the trailing newline).
fn first_line_len(node: NodeId, cx: &Cx<'_>) -> usize {
    let r = cx.range(node);
    let src = cx.raw_source(r);
    src.lines().next().unwrap_or("").chars().count()
}

/// RuboCop's `block_arg_string`: the comma-joined argument sources, with
/// `mlhs` arguments wrapped in parentheses and a trailing comma appended when
/// a single destructured argument is present.
fn block_arg_string(args: &[NodeId], cx: &Cx<'_>) -> String {
    let mut out = String::new();
    for (i, &arg) in args.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        if let NodeKind::Mlhs(list) = *cx.kind(arg) {
            out.push('(');
            out.push_str(&block_arg_string(cx.list(list), cx));
            out.push(')');
        } else {
            out.push_str(cx.raw_source(cx.range(arg)));
        }
    }
    // RuboCop: `arg_string += ',' if include_trailing_comma?(node.arguments)` —
    // a single `(a,)`-style destructuring argument keeps its trailing comma.
    if include_trailing_comma(args, cx) {
        out.push(',');
    }
    out
}

/// RuboCop's `include_trailing_comma?`: exactly one descendant `arg` and the
/// argument source contains a comma (single-element destructuring, `|(a,)|`).
fn include_trailing_comma(args: &[NodeId], cx: &Cx<'_>) -> bool {
    let arg_count = args
        .iter()
        .map(|&a| count_arg_descendants(a, cx))
        .sum::<usize>();
    if arg_count != 1 {
        return false;
    }
    args.iter()
        .any(|&a| cx.raw_source(cx.range(a)).contains(','))
}

/// Count `arg`-type descendants (including the node itself), recursing through
/// `mlhs`. Mirrors RuboCop's `args.each_descendant(:arg).to_a.size`.
fn count_arg_descendants(node: NodeId, cx: &Cx<'_>) -> usize {
    match *cx.kind(node) {
        NodeKind::Arg(_) => 1,
        NodeKind::Mlhs(list) => cx
            .list(list)
            .iter()
            .map(|&c| count_arg_descendants(c, cx))
            .sum(),
        _ => 0,
    }
}

/// The block's argument nodes (the `Args` list). Empty for a no-arg block.
fn block_arg_nodes(node: NodeId, cx: &Cx<'_>) -> Vec<NodeId> {
    let Some(args) = cx.block_arguments(node).get() else {
        return Vec::new();
    };
    match *cx.kind(args) {
        NodeKind::Args(list) => cx.list(list).to_vec(),
        _ => Vec::new(),
    }
}

/// The span from the first to the last block argument — RuboCop's
/// `node.arguments.source_range`.
fn args_range(args: &[NodeId], cx: &Cx<'_>) -> Range {
    let first = cx.range(args[0]);
    let last = cx.range(*args.last().expect("args is non-empty"));
    Range {
        start: first.start,
        end: last.end,
    }
}

/// Clamp a range to its first physical line so the offense caret is single-line
/// (codebase convention — see `util::first_line_range`).
fn first_line_range(range: Range, cx: &Cx<'_>) -> Range {
    let src = cx.source().as_bytes();
    let end = (range.end as usize).min(src.len());
    let line_end = src[range.start as usize..end]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(range.end, |pos| range.start + pos as u32);
    Range {
        start: range.start,
        end: line_end,
    }
}

#[cfg(test)]
mod tests {
    use super::MultilineBlockLayout;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_body_on_block_start_line_do() {
        test::<MultilineBlockLayout>().expect_offense(indoc! {"
            blah do |i| foo(i)
                        ^^^^^^ Block body expression is on the same line as the block start.
              bar(i)
            end
        "});
    }

    #[test]
    fn flags_args_not_on_block_start_line_do() {
        test::<MultilineBlockLayout>().expect_offense(indoc! {"
            blah do
              |i| foo(i)
               ^ Block argument expression is not on the same line as the block start.
              bar(i)
            end
        "});
    }

    #[test]
    fn flags_body_on_block_start_line_brace() {
        test::<MultilineBlockLayout>().expect_offense(indoc! {"
            blah { |i| foo(i)
                       ^^^^^^ Block body expression is on the same line as the block start.
              bar(i)
            }
        "});
    }

    #[test]
    fn accepts_well_formed_do_block() {
        test::<MultilineBlockLayout>().expect_no_offenses(indoc! {"
            blah do |i|
              foo(i)
              bar(i)
            end
        "});
    }

    #[test]
    fn accepts_well_formed_brace_block() {
        test::<MultilineBlockLayout>().expect_no_offenses(indoc! {"
            blah { |i|
              foo(i)
              bar(i)
            }
        "});
    }

    #[test]
    fn accepts_single_line_block() {
        test::<MultilineBlockLayout>().expect_no_offenses("blah { |i| foo(i) }\n");
    }

    #[test]
    fn accepts_block_with_no_args() {
        test::<MultilineBlockLayout>().expect_no_offenses(indoc! {"
            blah do
              foo
              bar
            end
        "});
    }

    // RuboCop's "good" example: block arguments split across lines *because*
    // they would overflow the line-length limit. The joined argument string
    // genuinely exceeds 120 characters here, so `line_break_necessary_in_args?`
    // returns true and the args are not flagged.
    #[test]
    fn accepts_long_args_split_across_lines() {
        test::<MultilineBlockLayout>().expect_no_offenses(indoc! {"
            blah { |
              aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa,
              bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb,
              cccccccccccccccccccccccccccccccccccc,
              dddddddddddddddddddddddddddddddddddd
            |
              foo
              bar
            }
        "});
    }

    // Short args split across lines are *not* necessary — they would fit on
    // one line — so they are flagged.
    #[test]
    fn flags_short_args_split_across_lines() {
        test::<MultilineBlockLayout>().expect_offense(indoc! {"
            blah do
              |i, j| foo(i)
               ^^^^ Block argument expression is not on the same line as the block start.
              bar(i)
            end
        "});
    }
}

murphy_plugin_api::submit_cop!(MultilineBlockLayout);
