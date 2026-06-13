//! `Layout/SingleLineBlockChain` — flags a method call chained directly
//! onto a single-line block (`foo.map { |x| x }.first`), where the chained
//! method call shares the closing-delimiter line.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/SingleLineBlockChain
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's `offending_range` exactly: the receiver must be a
//!   single-line block (begin/end delimiters on the same line), the chained
//!   call's dot must be on the block's closing-delimiter line, and the dot
//!   column must precede the selector column. Autocorrect inserts a newline
//!   before the dot. `autocorrect_incompatible_with [Style::MapToHash]` is a
//!   RuboCop cross-cop scheduling concern with no Murphy analog and is not
//!   ported.
//! ```
//!
//! ## Matched shapes
//!
//! `Send` / `Csend` whose receiver is a `Block` / `Numblock` / `Itblock`
//! whose `{`…`}` (or `do`…`end`) sit on a single line, and whose dot is on
//! that same line with `dot.column < selector.column`.
//!
//! ## Autocorrect
//!
//! Insert `\n` before the dot, putting the chained call on its own line.

use murphy_plugin_api::{Cx, NodeId, Range, SourceTokenKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SingleLineBlockChain;

#[cop(
    name = "Layout/SingleLineBlockChain",
    description = "Put method call on a separate line if chained to a single line block.",
    default_severity = "warning",
    default_enabled = true
)]
impl SingleLineBlockChain {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

const MSG: &str = "Put method call on a separate line if chained to a single line block.";

fn check(node: NodeId, cx: &Cx<'_>) {
    let Some(receiver) = cx.call_receiver(node).get() else {
        return;
    };
    // The receiver must be a single-line block.
    if !cx.is_any_block_type(receiver) {
        return;
    }

    let source = cx.source();
    let receiver_range = cx.range(receiver);

    // Block opening (`{` / `do`) and closing (`}` / `end`) delimiters.
    let Some(block_begin) = block_begin_delimiter(cx, receiver_range) else {
        return;
    };
    let block_end_start = block_end_delimiter_start(cx, receiver_range);

    // RuboCop: `return if receiver_location.begin.line < closing_block_delimiter_line_num`
    // i.e. only flag single-line blocks where begin and end share a line.
    let closing_line = line_of(source, block_end_start);
    if line_of(source, block_begin) < closing_line {
        return;
    }

    // The chained call must have a dot operator.
    let dot_range = cx.loc(node).dot();
    if dot_range == Range::ZERO {
        return;
    }

    // Selector range: the method name (`loc.name`). RuboCop falls back to
    // `loc.begin` (the opening paren of an implicit call); Murphy's `loc.name`
    // is `Range::ZERO` for those, so fall back to the first paren.
    let selector_range = selector_range(node, cx);
    let Some(selector_range) = selector_range else {
        return;
    };

    // `call_method_after_block?`: dot must be on the closing-delimiter line
    // (not after it), and the dot column must precede the selector column.
    if line_of(source, dot_range.start) > closing_line {
        return;
    }
    if column_of(source, dot_range.start) >= column_of(source, selector_range.start) {
        return;
    }

    let offense_range = Range {
        start: dot_range.start,
        end: selector_range.end,
    };
    cx.emit_offense(offense_range, MSG, None);
    cx.emit_edit(
        Range {
            start: dot_range.start,
            end: dot_range.start,
        },
        "\n",
    );
}

/// The selector range — `loc.name` for a named call, or the opening paren of
/// an implicit call (`foo.()`), matching RuboCop's `node.loc.selector ||
/// node.loc.begin`.
fn selector_range(node: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let name = cx.loc(node).name;
    if name != Range::ZERO {
        return Some(name);
    }
    let begin = cx.loc(node).begin();
    if begin != Range::ZERO {
        return Some(begin);
    }
    None
}

/// The start offset of the block's opening delimiter (`{` or `do`) — the first
/// such token within the block's range.
fn block_begin_delimiter(cx: &Cx<'_>, block_range: Range) -> Option<u32> {
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < block_range.start);
    toks[idx..]
        .iter()
        .take_while(|t| t.range.end <= block_range.end)
        .find(|t| {
            t.kind == SourceTokenKind::LeftBrace
                || (t.kind == SourceTokenKind::Other
                    && &source[t.range.start as usize..t.range.end as usize] == b"do")
        })
        .map(|t| t.range.start)
}

/// The start offset of the block's closing delimiter (`}` or `end`) — the
/// delimiter token ending at the block's expression end.
fn block_end_delimiter_start(cx: &Cx<'_>, block_range: Range) -> u32 {
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    // The closing delimiter ends exactly at the block's expression end.
    let idx = toks.partition_point(|t| t.range.end < block_range.end);
    if let Some(tok) = toks.get(idx)
        && tok.range.end == block_range.end
        && (tok.kind == SourceTokenKind::RightBrace
            || (tok.kind == SourceTokenKind::Other
                && &source[tok.range.start as usize..tok.range.end as usize] == b"end"))
    {
        return tok.range.start;
    }
    // Fall back to the block end (no delimiter token found — shouldn't happen
    // for well-formed blocks).
    block_range.end
}

/// 1-based line number of `offset` within `source`.
fn line_of(source: &str, offset: u32) -> usize {
    source.as_bytes()[..offset as usize]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

/// 0-based column (character count, not bytes) of `offset` on its line.
fn column_of(source: &str, offset: u32) -> usize {
    let start = offset as usize;
    let line_start = source.as_bytes()[..start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |i| i + 1);
    source[line_start..start].chars().count()
}

#[cfg(test)]
mod tests {
    use super::SingleLineBlockChain;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_method_chained_on_single_line_brace_block() {
        test::<SingleLineBlockChain>().expect_offense(indoc! {"
            example.select { |item| item.cond? }.join('-')
                                                ^^^^^ Put method call on a separate line if chained to a single line block.
        "});
    }

    #[test]
    fn corrects_by_inserting_newline_before_dot() {
        test::<SingleLineBlockChain>().expect_correction(
            indoc! {"
                example.select { |item| item.cond? }.join('-')
                                                    ^^^^^ Put method call on a separate line if chained to a single line block.
            "},
            indoc! {"
                example.select { |item| item.cond? }
                .join('-')
            "},
        );
    }

    #[test]
    fn accepts_call_on_separate_line() {
        test::<SingleLineBlockChain>().expect_no_offenses(indoc! {"
            example.select { |item| item.cond? }
                   .join('-')
        "});
    }

    #[test]
    fn accepts_single_line_block_without_chain() {
        test::<SingleLineBlockChain>().expect_no_offenses("example.select { |item| item.cond? }\n");
    }

    #[test]
    fn accepts_multiline_block_chain() {
        // The block spans multiple lines, so chaining on the `}` line is fine.
        test::<SingleLineBlockChain>().expect_no_offenses(indoc! {"
            example.select do |item|
              item.cond?
            end.join('-')
        "});
    }

    #[test]
    fn accepts_plain_method_call() {
        test::<SingleLineBlockChain>().expect_no_offenses("foo.bar\n");
    }

    #[test]
    fn flags_do_end_single_line_block_chain() {
        // A single-line `do...end` block with a chain on the same line.
        test::<SingleLineBlockChain>().expect_offense(indoc! {"
            example.select do |item| item.cond? end.join('-')
                                                   ^^^^^ Put method call on a separate line if chained to a single line block.
        "});
    }

    #[test]
    fn flags_safe_navigation_chain() {
        test::<SingleLineBlockChain>().expect_offense(indoc! {"
            example.select { |item| item.cond? }&.join('-')
                                                ^^^^^^ Put method call on a separate line if chained to a single line block.
        "});
    }

    #[test]
    fn accepts_chain_when_dot_after_block_line() {
        // The dot leads the next line — already on a separate line.
        test::<SingleLineBlockChain>().expect_no_offenses(indoc! {"
            example.select { |item| item.cond? }
              .join('-')
        "});
    }
}

murphy_plugin_api::submit_cop!(SingleLineBlockChain);
