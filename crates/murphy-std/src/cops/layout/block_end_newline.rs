//! `Layout/BlockEndNewline` — the `end`/`}` of a multi-line block must be on
//! its own line.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/BlockEndNewline
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Ports `on_block` (aliased to `on_numblock`/`on_itblock`). A single-line
//!   block is skipped (`return if node.single_line?`). The `end`/`}` is also
//!   accepted when it already begins its own line
//!   (`begins_its_line?(node.loc.end)`).
//!
//!   The offense range RuboCop builds is `node.children.compact.last
//!   .source_range.end.join(node.loc.end)` — from the end of the block's last
//!   non-nil child (its body, or its arguments when there is no body) to the
//!   end of the closing delimiter. If that range's source, left-stripped,
//!   starts with `;` the offense is suppressed (a `foo; end` body terminator
//!   is acceptable). Otherwise the `end`/`}` token is flagged with message
//!   ``Expression at L, C should be on its own line.`` where `C` is the
//!   delimiter's (0-based) column + 1.
//!
//!   Autocorrect replaces the offense range with `"\n" + source.lstrip`,
//!   moving the closing delimiter onto its own line. When the block body's last
//!   argument is a heredoc, replacing in place would drop the delimiter between
//!   the opener line and the heredoc body and corrupt the literal, so RuboCop
//!   instead removes the offense range and inserts the replacement after the
//!   heredoc terminator (`heredoc.loc.heredoc_end`). Murphy ports that
//!   rearrangement (murphy-in6p): the heredoc terminator end is the last
//!   heredoc argument's `HeredocEnd` token (FIFO-paired by opener index), with
//!   the prism token's trailing newline trimmed to match the parser gem's
//!   label-only `heredoc_end`.
//! ```
//!
//! ## Matched shapes
//!
//! Multi-line `block`/`numblock`/`itblock` nodes whose closing `end`/`}` shares
//! a line with the body or arguments.

use crate::cops::util::block_opener;
use murphy_plugin_api::{Cx, NodeId, NodeKind, Range, SourceTokenKind, cop};

const MSG_PREFIX: &str = "Expression at ";

/// Stateless unit struct (ADR 0035).
#[derive(Default)]
pub struct BlockEndNewline;

#[cop(
    name = "Layout/BlockEndNewline",
    description = "Put end statement of multiline block on its own line.",
    default_severity = "warning",
    default_enabled = true,
)]
impl BlockEndNewline {
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
    // `return if node.single_line?`.
    if cx.is_single_line(node) {
        return;
    }

    // The closing delimiter token (`end` / `}`).
    let Some(end_loc) = block_end_delimiter(node, cx) else {
        return;
    };

    // `return if begins_its_line?(node.loc.end)`.
    if begins_its_line(end_loc.start, cx) {
        return;
    }

    // `offense_range = node.children.compact.last.source_range.end.join(node.loc.end)`.
    let last_child_end = last_child_end(node, cx);
    let offense_range = Range {
        start: last_child_end,
        end: end_loc.end,
    };

    // `return if offense_range.source.lstrip.start_with?(';')`.
    let offense_src = cx.raw_source(offense_range);
    if offense_src.trim_start().starts_with(';') {
        return;
    }

    let (line, col) = line_and_column(cx, end_loc.start);
    let message = format!("{MSG_PREFIX}{line}, {} should be on its own line.", col + 1);
    cx.emit_offense(end_loc, &message, None);

    // Autocorrect: move the closing delimiter onto its own line.
    //
    // ```ruby
    // replacement = "\n#{offense_range.source.lstrip}"
    // if (heredoc = last_heredoc_argument(node.body))
    //   corrector.remove(offense_range)
    //   corrector.insert_after(heredoc.loc.heredoc_end, replacement)
    // else
    //   corrector.replace(offense_range, replacement)
    // end
    // ```
    //
    // When the body's last argument is a heredoc, replacing the offense range in
    // place would drop the `end` between the opener line and the heredoc body,
    // corrupting the literal. RuboCop instead deletes the offense range and
    // re-inserts the `\nend` after the heredoc terminator (`heredoc_end`), so the
    // delimiter lands below the closing label (murphy-in6p).
    let replacement = format!("\n{}", offense_src.trim_start());
    if let Some(heredoc_end) = last_heredoc_argument_end(node, cx) {
        cx.emit_edit(offense_range, "");
        cx.emit_edit(
            Range {
                start: heredoc_end,
                end: heredoc_end,
            },
            &replacement,
        );
    } else {
        cx.emit_edit(offense_range, &replacement);
    }
}

/// The closing delimiter token (`end` / `}`) of a block — the token ending
/// exactly at the block's expression end. `None` when no such delimiter token
/// is found.
fn block_end_delimiter(node: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let block_end = cx.range(node).end;
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.end < block_end);
    let tok = toks.get(idx)?;
    if tok.range.end == block_end
        && (tok.kind == SourceTokenKind::RightBrace
            || (tok.kind == SourceTokenKind::Other
                && &source[tok.range.start as usize..tok.range.end as usize] == b"end"))
    {
        Some(tok.range)
    } else {
        None
    }
}

/// RuboCop's `node.children.compact.last.source_range.end` — the end offset of
/// the block's last non-nil child. `cx.children` yields `[call, args, body?]`
/// for a `block` and `[send, body?]` for a `numblock`/`itblock`, with absent
/// optional children already compacted out, so the last element is the body
/// when present, otherwise the arguments (or the call for `numblock`/`itblock`
/// without a body). Falls back to the opener for a body-less, args-empty block.
fn last_child_end(node: NodeId, cx: &Cx<'_>) -> u32 {
    if let Some(&last) = cx.children(node).last() {
        let range = cx.range(last);
        // An empty `(args)` node is zero-width; prefer the opener end so the
        // offense range starts after the `{`/`do` rather than at the call.
        if range.end > range.start {
            return range.end;
        }
    }
    block_opener(node, cx).map_or_else(|| cx.range(node).start, |r| r.end)
}

/// `begins_its_line?(loc)` — whether everything before `offset` on its line is
/// whitespace.
fn begins_its_line(offset: u32, cx: &Cx<'_>) -> bool {
    let src = cx.source();
    let offset = offset as usize;
    let line_start = src[..offset].rfind('\n').map_or(0, |pos| pos + 1);
    src[line_start..offset].bytes().all(|b| b == b' ' || b == b'\t')
}

/// RuboCop's `last_heredoc_argument(node.body)` followed by
/// `heredoc.loc.heredoc_end.end_pos`: the byte offset just past the terminator
/// label of the block body's last heredoc argument, or `None` when the body is
/// not a call whose last (recursively, through the leading receiver) argument is
/// a heredoc string.
///
/// ```ruby
/// def last_heredoc_argument(node)
///   return unless node.respond_to?(:arguments)
///   node.arguments.reverse_each do |arg|
///     return arg if arg.respond_to?(:heredoc?) && arg.heredoc?
///   end
///   last_heredoc_argument(node.children.first)
/// end
/// ```
///
/// The insertion point is the *end* of `heredoc.loc.heredoc_end` (the terminator
/// label, no trailing newline), which equals the `HeredocEnd` token's end.
fn last_heredoc_argument_end(node: NodeId, cx: &Cx<'_>) -> Option<u32> {
    let mut current = cx.block_body(node).get()?;
    // Walk the receiver chain like RuboCop's recursion on `node.children.first`.
    loop {
        if !matches!(cx.kind(current), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
            return None;
        }
        // `node.arguments.reverse_each` — the first (rightmost) heredoc argument.
        if let Some(&heredoc) = cx
            .call_arguments(current)
            .iter()
            .rev()
            .find(|&&arg| is_heredoc_string(arg, cx))
        {
            return heredoc_end_token_end(heredoc, cx);
        }
        match cx.call_receiver(current).get() {
            Some(recv) => current = recv,
            None => return None,
        }
    }
}

/// The end byte offset of `heredoc`'s terminator label — RuboCop's
/// `heredoc.loc.heredoc_end.end_pos`, which covers the indentation + label but
/// **not** the trailing newline.
///
/// The terminator is paired by FIFO index: the k-th `HeredocStart` in source
/// order is closed by the k-th `HeredocEnd`. The node's own opener is the first
/// `HeredocStart` inside its range. prism's `HeredocEnd` token spans the label
/// *and* its trailing newline, so the newline (and a preceding `\r`) is trimmed
/// to land the insertion point right after the label, matching the parser gem.
fn heredoc_end_token_end(heredoc: NodeId, cx: &Cx<'_>) -> Option<u32> {
    let range = cx.range(heredoc);
    let toks = cx.sorted_tokens();
    // This node's opener: the first `HeredocStart` within the node's range.
    let opener = toks
        .iter()
        .find(|t| {
            t.kind == SourceTokenKind::HeredocStart
                && t.range.start >= range.start
                && t.range.end <= range.end
        })?
        .range;
    // Index of this opener among all openers in source order.
    let index = toks
        .iter()
        .filter(|t| t.kind == SourceTokenKind::HeredocStart)
        .take_while(|t| t.range.start < opener.start)
        .count();
    let term = toks
        .iter()
        .filter(|t| t.kind == SourceTokenKind::HeredocEnd)
        .nth(index)?
        .range;
    // Trim the trailing newline (and a preceding `\r`) the prism token includes.
    let bytes = cx.source().as_bytes();
    let mut end = term.end as usize;
    while end > term.start as usize && matches!(bytes.get(end - 1), Some(b'\n' | b'\r')) {
        end -= 1;
    }
    Some(end as u32)
}

/// Whether `node` is a heredoc string literal (a `Str`/`Dstr` whose opener is a
/// `<<` heredoc token).
fn is_heredoc_string(node: NodeId, cx: &Cx<'_>) -> bool {
    if !matches!(cx.kind(node), NodeKind::Str(_) | NodeKind::Dstr(_)) {
        return false;
    }
    cx.token_after(cx.range(node).start)
        .is_some_and(|t| t.kind == SourceTokenKind::HeredocStart)
}

/// 1-based line and 0-based character column of `offset`.
fn line_and_column(cx: &Cx<'_>, offset: u32) -> (usize, usize) {
    let src = cx.source();
    let upper = (offset as usize).min(src.len());
    let line = src[..upper].bytes().filter(|&b| b == b'\n').count() + 1;
    let line_start = src[..upper].rfind('\n').map_or(0, |pos| pos + 1);
    let col = src[line_start..upper].chars().count();
    (line, col)
}

murphy_plugin_api::submit_cop!(BlockEndNewline);

#[cfg(test)]
mod tests {
    use super::BlockEndNewline as Cop;
    use murphy_plugin_api::test_support::{CapturedEdit, run_cop, run_cop_with_edits, test};

    fn apply(source: &str, edits: &[CapturedEdit]) -> String {
        let mut sorted: Vec<&CapturedEdit> = edits.iter().collect();
        sorted.sort_by_key(|e| e.range.start);
        let mut out = String::new();
        let mut cursor = 0usize;
        for e in sorted {
            out.push_str(&source[cursor..e.range.start as usize]);
            out.push_str(&e.replacement);
            cursor = e.range.end as usize;
        }
        out.push_str(&source[cursor..]);
        out
    }

    #[test]
    fn accepts_one_liner() {
        test::<Cop>().expect_no_offenses("test do foo end\n");
    }

    #[test]
    fn accepts_multiline_with_end_on_own_line() {
        test::<Cop>().expect_no_offenses("test do\n  foo\nend\n");
    }

    #[test]
    fn accepts_multiline_brace_with_brace_on_own_line() {
        test::<Cop>().expect_no_offenses("test {\n  foo\n}\n");
    }

    #[test]
    fn accepts_semicolon_before_end_on_own_line() {
        // `end` already on its own line — accepted via `begins_its_line`.
        test::<Cop>().expect_no_offenses("test do\n  foo;\nend\n");
    }

    #[test]
    fn accepts_semicolon_before_inline_end() {
        // `foo; end` on one line — the `;`-skip branch suppresses the offense.
        test::<Cop>().expect_no_offenses("test do\n  foo; end\n");
    }

    #[test]
    fn flags_do_end_not_on_own_line() {
        let src = "test do\n  foo end\n";
        let offenses = run_cop::<Cop>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Expression at 2, 7 should be on its own line."
        );
    }

    #[test]
    fn flags_brace_not_on_own_line() {
        let src = "test {\n  foo }\n";
        let offenses = run_cop::<Cop>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Expression at 2, 7 should be on its own line."
        );
    }

    #[test]
    fn flags_brace_with_chain() {
        let src = "test {\n  foo }.bar.baz\n";
        let offenses = run_cop::<Cop>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Expression at 2, 7 should be on its own line."
        );
    }

    #[test]
    fn flags_brace_no_body_only_args() {
        let src = "test {\n  |foo| }\n";
        let offenses = run_cop::<Cop>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Expression at 2, 9 should be on its own line."
        );
    }

    #[test]
    fn corrects_do_end() {
        let src = "test do\n  foo end\n";
        let run = run_cop_with_edits::<Cop>(src);
        assert_eq!(apply(src, &run.edits), "test do\n  foo\nend\n");
    }

    #[test]
    fn corrects_brace() {
        let src = "test {\n  foo }\n";
        let run = run_cop_with_edits::<Cop>(src);
        assert_eq!(apply(src, &run.edits), "test {\n  foo\n}\n");
    }

    #[test]
    fn corrects_brace_with_chain() {
        let src = "test {\n  foo }.bar.baz\n";
        let run = run_cop_with_edits::<Cop>(src);
        assert_eq!(apply(src, &run.edits), "test {\n  foo\n}.bar.baz\n");
    }

    #[test]
    fn correction_is_idempotent() {
        let src = "test do\n  foo end\n";
        let run = run_cop_with_edits::<Cop>(src);
        let fixed = apply(src, &run.edits);
        assert!(run_cop::<Cop>(&fixed).is_empty(), "not idempotent: {fixed:?}");
    }

    #[test]
    fn flags_and_corrects_numblock() {
        let src = "[1, 2].each do\n  puts _1 end\n";
        let run = run_cop_with_edits::<Cop>(src);
        assert_eq!(run.offenses.len(), 1, "got {:?}", run.offenses);
        assert_eq!(apply(src, &run.edits), "[1, 2].each do\n  puts _1\nend\n");
        // Idempotent: re-running on the corrected source yields no offense.
        let fixed = apply(src, &run.edits);
        assert!(run_cop::<Cop>(&fixed).is_empty(), "not idempotent: {fixed:?}");
    }

    #[test]
    fn flags_and_corrects_itblock() {
        let src = "[1, 2].each do\n  puts it end\n";
        let run = run_cop_with_edits::<Cop>(src);
        assert_eq!(run.offenses.len(), 1, "got {:?}", run.offenses);
        assert_eq!(apply(src, &run.edits), "[1, 2].each do\n  puts it\nend\n");
        let fixed = apply(src, &run.edits);
        assert!(run_cop::<Cop>(&fixed).is_empty(), "not idempotent: {fixed:?}");
    }

    /// murphy-in6p: the body's last argument is a heredoc. RuboCop removes the
    /// offense range (` end`) and inserts the replacement (`\nend`) after the
    /// heredoc terminator, so the `end` lands below the closing label rather than
    /// inside the heredoc body (which would corrupt the literal).
    #[test]
    fn corrects_when_body_ends_with_heredoc() {
        let src = "test do\n  foo(<<~TEXT) end\n    hi\n  TEXT\n";
        let run = run_cop_with_edits::<Cop>(src);
        assert_eq!(run.offenses.len(), 1, "got {:?}", run.offenses);
        assert_eq!(
            apply(src, &run.edits),
            "test do\n  foo(<<~TEXT)\n    hi\n  TEXT\nend\n"
        );
    }


    /// murphy-in6p idempotency: re-running on the corrected source is clean.
    #[test]
    fn heredoc_correction_is_idempotent() {
        let src = "test do\n  foo(<<~TEXT) end\n    hi\n  TEXT\n";
        let run = run_cop_with_edits::<Cop>(src);
        let fixed = apply(src, &run.edits);
        assert!(run_cop::<Cop>(&fixed).is_empty(), "not idempotent: {fixed:?}");
    }
}
