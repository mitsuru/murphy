//! `Layout/MultilineBlockLayout` — multi-line `do`/`end` (and brace) blocks
//! must put the body on a line after the block start, and block arguments on
//! the same line as the block start.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/MultilineBlockLayout
//! upstream_version_checked: 1.87.0
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
//!   The `single_line?` short-circuit uses `util::block_is_single_line`,
//!   matching RuboCop's `BlockNode#single_line?` (`loc.begin.line ==
//!   loc.end.line`) rather than the whole-expression range — so a one-line
//!   `{ … }` at the tail of a multi-line receiver chain is correctly
//!   single-line (murphy-un83). The body-placement check resolves the body's
//!   first line via `body_first_offset`, which descends through `Begin` /
//!   `Rescue` / `Ensure` wrappers to the first contained statement, mirroring
//!   RuboCop's `node.body.first_line`; Murphy's wrapper range can otherwise
//!   begin at the `do` (e.g. `do…rescue…end`).
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

use crate::cops::util::{block_is_single_line, block_opener, gap_has_newline};
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
    // RuboCop: `return if node.single_line?`. `BlockNode#single_line?` compares
    // the opener/closing delimiter lines, not the whole expression — a one-line
    // `{ … }` at a multi-line chain tail is single-line (murphy-un83).
    if block_is_single_line(node, cx) {
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
    // RuboCop: `return unless same_line?(node.loc.begin, node.body)` — the body
    // shares the opener's line iff no newline lies between the opener and the
    // body's first *statement*. `body_first_offset` descends through `Begin` /
    // `Rescue` / `Ensure` wrappers, whose range can begin at the block opener
    // (`do`/`{`) when the body carries a rescue/ensure clause; RuboCop's
    // `node.body.first_line` is the first contained statement's line (murphy-un83).
    let src = cx.source().as_bytes();
    let body_start = body_first_offset(body, opener.start, cx);
    if !gap_has_newline(src, opener.start, body_start) {
        let range = Range {
            start: body_start,
            end: cx.range(body).end,
        };
        cx.emit_offense(first_line_range(range, cx), MSG, None);
    }
}

/// RuboCop's `node.body.first_line`: the start offset of the first *statement*
/// in the block body. Murphy's *implicit* body-wrapper nodes (`Begin`, `Rescue`,
/// `Ensure`) can carry a range that begins at the block opener (`do`/`{`) when
/// the body has a rescue/ensure clause, so descending to the first contained
/// statement is required to recover the true first-body offset.
///
/// Only descend through a wrapper whose range begins at the opener. An *explicit*
/// `begin … end` body (kwbegin) starts at its own `begin` keyword, and RuboCop's
/// `node.body.first_line` is that keyword's line — so for `foo do begin\n … end`
/// the body-on-opener-line offense must still fire. Unwrapping it unconditionally
/// would move `body_start` to the first inner statement (on the next line) and
/// suppress the offense (murphy-un83).
fn body_first_offset(body: NodeId, opener_start: u32, cx: &Cx<'_>) -> u32 {
    let mut cur = body;
    loop {
        // A wrapper that starts after the opener is a visible body expression
        // (e.g. an explicit `begin`/parenthesized group) whose own first line is
        // what RuboCop compares against — stop and report it.
        if cx.range(cur).start > opener_start {
            return cx.range(cur).start;
        }
        let next = match *cx.kind(cur) {
            NodeKind::Begin(list) => cx.list(list).first().copied(),
            NodeKind::Rescue { body, .. } | NodeKind::Ensure { body, .. } => body.get(),
            _ => None,
        };
        match next {
            Some(n) => cur = n,
            None => return cx.range(cur).start,
        }
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

    // Regression (murphy-un83): a one-line `{ … }` block at the tail of a
    // multi-line method chain. RuboCop's `BlockNode#single_line?` (opener line
    // == closing-delimiter line) is true, so the cop short-circuits. Murphy's
    // old full-range single-line check read the chain as multi-line and flagged
    // the body. Verified no offense against RuboCop 1.87.
    #[test]
    fn accepts_single_line_brace_at_multiline_chain_tail() {
        test::<MultilineBlockLayout>().expect_no_offenses(indoc! {"
            def f
              params
                .permit(:a)
                .transform_keys { |k| k.to_s }
            end
        "});
    }

    // Regression (murphy-un83): a single-line stabby lambda used as an argument
    // *inside* a multi-line block. The lambda's `block_opener` must resolve to
    // its own `{` — not the enclosing block's `do`. A `Lambda` marker call has a
    // `{0,0}` name loc, so the opener scan must be floored at the block's own
    // start; otherwise `block_is_single_line` reads the lambda as multi-line and
    // its argument is wrongly flagged. RuboCop 1.87 reports no offense.
    #[test]
    fn accepts_single_line_lambda_arg_inside_block() {
        test::<MultilineBlockLayout>().expect_no_offenses(indoc! {"
            root date_detection: false do
              field(:id, type: 'long')
              field(:props, type: 'keyword', value: ->(account) { account.searchable_props })
            end
        "});
    }

    // Regression (murphy-un83): a `do…rescue…end` block. Murphy's body-wrapper
    // range begins at the `do`, so the naive `same_line?` check mistook the
    // body for being on the opener line. Descending to the first statement
    // (line after `do`) recovers RuboCop's `node.body.first_line`. RuboCop 1.87
    // reports no offense.
    #[test]
    fn accepts_do_rescue_block_body_on_next_line() {
        test::<MultilineBlockLayout>().expect_no_offenses(indoc! {"
            define_method provider do
              a = provider

              if cond
                d
              end
            rescue Foo
              handle
            end
        "});
    }

    // Regression (murphy-un83): an *explicit* `begin … end` body opening on the
    // block-start line. Unlike the implicit wrappers above, a kwbegin starts at
    // its own `begin` keyword, so RuboCop's `node.body.first_line` is that
    // keyword's line — on the opener line here — and the body-on-opener-line
    // offense still fires. The descent must stop at the visible `begin`, not
    // dive to the first inner statement on the next line.
    #[test]
    fn flags_explicit_begin_body_on_block_start_line() {
        test::<MultilineBlockLayout>().expect_offense(indoc! {"
            blah do begin
                    ^^^^^ Block body expression is on the same line as the block start.
              foo
              bar
            end
            end
        "});
    }

    // Discriminator for the explicit-`begin` gate: when the `begin` keyword is on
    // the line *after* `do`, the body is well-formed and no offense fires.
    #[test]
    fn accepts_explicit_begin_body_on_next_line() {
        test::<MultilineBlockLayout>().expect_no_offenses(indoc! {"
            blah do
              begin
                foo
                bar
              end
            end
        "});
    }

    // Regression (murphy-un83): a well-formed `do |row|` block whose body
    // contains a `next` guard and a multi-line method call. RuboCop 1.87 reports
    // no offense; the body begins on the line after `do`.
    #[test]
    fn accepts_do_block_with_next_and_multiline_call() {
        test::<MultilineBlockLayout>().expect_no_offenses(indoc! {"
            list.filter_map do |row|
              domain = row.strip
              next if cond

              build(domain,
                    extra: 1)
            end
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
