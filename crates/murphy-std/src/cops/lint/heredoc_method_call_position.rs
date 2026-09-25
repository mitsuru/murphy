//! `Lint/HeredocMethodCallPosition` — checks calls on heredoc receivers are on the opener line.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/HeredocMethodCallPosition
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's `on_send`/`on_csend` detection, including chained calls
//!   (receiver-chain walk to the heredoc), the single offense at the first
//!   `.`/`&` after the heredoc terminator, and the `calls_on_multiple_lines?`
//!   / trailing-comma autocorrect safety checks. Only the outermost call of a
//!   heredoc-receiver chain emits, matching RuboCop's same-range dedup
//!   (`current_offense_locations`). `String#strip` is rendered as `str::trim`
//!   (equivalent on the ASCII call sources this cop moves).
//! ```
//!
//! ## Matched shapes
//!
//! A `send`/`csend` whose receiver chain bottoms out at a heredoc string
//! (`str`/`dstr`/`xstr` holding a `HeredocStart` token) and whose expression
//! ends after the heredoc terminator. Correctly positioned calls (expression
//! ends before the terminator, e.g. `<<~SQL.bar`) are accepted, as are bare
//! heredocs with no call.
//!
//! ## Why this shape
//!
//! RuboCop walks `node.receiver` through `call_type?` nodes until it finds a
//! `heredoc?` receiver (`heredoc_node_descendent_receiver`). The Murphy port
//! walks `Send`/`Csend` receivers the same way. Chained inner calls are
//! skipped via `is_chained?` so the chain reports exactly one offense at the
//! terminator, like RuboCop's `current_offense_locations` dedup (the outer
//! call fires first and wins).
//!
//! ## Autocorrect
//!
//! Moves the call text (`heredoc_end..call_end`, plus a trailing comma when
//! the call line carries one) to the end of the heredoc opener line.
//! Multiline chains or argument lists (`calls_on_multiple_lines?`) are
//! reported without an edit, matching RuboCop's `expect_no_corrections`
//! cases.

use murphy_plugin_api::{
    cop, Cx, NoOptions, NodeId, NodeKind, Range, SourceToken, SourceTokenKind,
};

use crate::cops::util::line_of;

const MSG: &str =
    "Put a method call with a HEREDOC receiver on the same line as the HEREDOC opening.";

#[derive(Default)]
pub struct HeredocMethodCallPosition;

#[cop(
    name = "Lint/HeredocMethodCallPosition",
    description = "Checks method calls on HEREDOC receivers.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl HeredocMethodCallPosition {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check_call(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check_call(node, cx);
    }
}

fn check_call(node: NodeId, cx: &Cx<'_>) {
    // RuboCop reports the same terminator-adjacent range for every call in a
    // heredoc-receiver chain and `current_offense_locations` keeps only the
    // first (outermost). Skip chained inner calls so the chain emits once.
    if cx.is_chained(node) {
        return;
    }
    // `heredoc_node_descendent_receiver`: walk `Send`/`Csend` receivers to the
    // heredoc.
    let Some(heredoc) = heredoc_chain_receiver(node, cx) else {
        return;
    };
    let Some((start, end)) = heredoc_tokens(heredoc, cx) else {
        return;
    };
    // Parser-gem's `heredoc_end.end_pos` stops at the terminator text; the
    // Murphy `HeredocEnd` token folds the line ending. Strip it back so the
    // positions below match RuboCop's byte arithmetic exactly.
    let heredoc_end = heredoc_text_end(end.range.end, cx.source());
    // `correctly_positioned?`: the terminator sits past the call end, i.e. the
    // call already lives on the opener line (`<<~SQL.bar`).
    if heredoc_end > cx.range(node).end {
        return;
    }

    // `call_after_heredoc_range`: the single char after the terminator's
    // newline — the `.` (or `&` of `&.`) starting the misplaced call.
    let source_len = cx.source().len() as u32;
    let offense_end = heredoc_end.saturating_add(2).min(source_len);
    let offense_start = heredoc_end.saturating_add(1).min(offense_end);
    if offense_start >= offense_end {
        return;
    }
    cx.emit_offense(
        Range {
            start: offense_start,
            end: offense_end,
        },
        MSG,
        None,
    );

    // `call_range_to_safely_reposition`: report-only when nil.
    let Some(call_range) = safe_call_range(node, heredoc_end, cx) else {
        return;
    };
    let call_source = cx.raw_source(call_range).trim().to_owned();
    cx.emit_edit(call_range, "");
    let insert_at = opener_line_end(start.range, cx.source());
    cx.emit_edit(
        Range {
            start: insert_at,
            end: insert_at,
        },
        &call_source,
    );
}

/// RuboCop's `heredoc_node_descendent_receiver`: follow `Send`/`Csend`
/// receivers down the chain and return the heredoc string at its root, or
/// `None` when the chain bottoms out elsewhere.
fn heredoc_chain_receiver(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let mut current = node;
    loop {
        if !matches!(
            *cx.kind(current),
            NodeKind::Send { .. } | NodeKind::Csend { .. }
        ) {
            return None;
        }
        let receiver = cx.call_receiver(current).get()?;
        if is_heredoc_node(receiver, cx) {
            return Some(receiver);
        }
        current = receiver;
    }
}

/// RuboCop's `node.heredoc?`: a string node holding a heredoc opener.
fn is_heredoc_node(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        *cx.kind(node),
        NodeKind::Str(_) | NodeKind::Dstr(_) | NodeKind::Xstr(_)
    ) && cx
        .tokens_in(cx.range(node))
        .iter()
        .any(|tok| tok.kind == SourceTokenKind::HeredocStart)
}

fn heredoc_tokens(heredoc: NodeId, cx: &Cx<'_>) -> Option<(SourceToken, SourceToken)> {
    let range = cx.range(heredoc);
    let start = cx
        .tokens_in(range)
        .iter()
        .find(|tok| tok.kind == SourceTokenKind::HeredocStart)
        .copied()?;
    let sorted_tokens = cx.sorted_tokens();
    let idx = sorted_tokens.partition_point(|tok| tok.range.start < start.range.end);
    let end = sorted_tokens[idx..]
        .iter()
        .find(|tok| tok.kind == SourceTokenKind::HeredocEnd && tok.range.start >= start.range.end)
        .copied()?;
    Some((start, end))
}

/// Rewind a `HeredocEnd` token end past its folded line ending (`\n`,
/// `\r\n`) to the terminator text end — RuboCop's `heredoc_end.end_pos`.
fn heredoc_text_end(token_end: u32, source: &str) -> u32 {
    let bytes = source.as_bytes();
    let mut end = (token_end as usize).min(bytes.len());
    while end > 0 && (bytes[end - 1] == b'\n' || bytes[end - 1] == b'\r') {
        end -= 1;
    }
    end as u32
}

/// RuboCop's `call_range_to_safely_reposition`: the removable call text, or
/// `None` when repositioning is unsafe (multiline chain/arguments, or extra
/// text on the call line). Ranges use RuboCop's `heredoc_end.end_pos` basis.
fn safe_call_range(node: NodeId, heredoc_end: u32, cx: &Cx<'_>) -> Option<Range> {
    // `return nil if calls_on_multiple_lines?(node, heredoc)`.
    if calls_on_multiple_lines(node, cx) {
        return None;
    }
    let call_end = cx.range(node).end;
    if call_end < heredoc_end {
        return None;
    }
    let call_range = Range {
        start: heredoc_end,
        end: call_end,
    };
    let call_line = cx.range_by_whole_lines(
        Range {
            start: call_end,
            end: call_end,
        },
        false,
    );
    let call_source = cx.raw_source(call_range).trim();
    let call_line_source = cx.raw_source(call_line).trim();
    // `return call_range if call_source == call_line_source`.
    if call_source == call_line_source {
        return Some(call_range);
    }
    // Trailing comma: the call line holds the call plus `,`, e.g.
    // `bar(<<-SQL\n...\n.abc,\n)`. Leave the `\n` after the terminator in
    // place by extending through the comma.
    if format!("{call_source},") == call_line_source
        && cx
            .source()
            .as_bytes()
            .get(call_end as usize)
            .is_some_and(|&b| b == b',')
    {
        return Some(Range {
            start: heredoc_end,
            end: call_end + 1,
        });
    }
    None
}

/// RuboCop's `calls_on_multiple_lines?`: walking the receiver chain from the
/// outermost call, every call must end on the outermost call's last line and
/// every argument list must sit on one line.
fn calls_on_multiple_lines(node: NodeId, cx: &Cx<'_>) -> bool {
    let last = last_line(cx.range(node).end, cx);
    let mut current = Some(node);
    while let Some(call) = current {
        if !matches!(
            *cx.kind(call),
            NodeKind::Send { .. } | NodeKind::Csend { .. }
        ) {
            return false;
        }
        if last_line(cx.range(call).end, cx) != last {
            return true;
        }
        if !all_on_same_line(cx.call_arguments(call), cx) {
            return true;
        }
        current = cx.call_receiver(call).get();
    }
    false
}

/// RuboCop's `all_on_same_line?`: empty, or first argument starts where the
/// last argument ends.
fn all_on_same_line(args: &[NodeId], cx: &Cx<'_>) -> bool {
    let (Some(&first), Some(&last)) = (args.first(), args.last()) else {
        return true;
    };
    line_of(cx.range(first).start, cx) == last_line(cx.range(last).end, cx)
}

/// 0-based last source line touching byte `end` (one-past-end range edge).
fn last_line(end: u32, cx: &Cx<'_>) -> u32 {
    line_of(end.saturating_sub(1), cx)
}

fn opener_line_end(start_range: Range, source: &str) -> u32 {
    source.as_bytes()[start_range.end as usize..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(source.len() as u32, |pos| start_range.end + pos as u32)
}

murphy_plugin_api::submit_cop!(HeredocMethodCallPosition);

#[cfg(test)]
mod tests {
    use super::HeredocMethodCallPosition;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn accepts_simple_correct_case() {
        test::<HeredocMethodCallPosition>().expect_no_offenses("<<~SQL\n  foo\nSQL\n");
    }

    #[test]
    fn accepts_chained_correct_case() {
        test::<HeredocMethodCallPosition>().expect_no_offenses("<<~SQL.bar\n  foo\nSQL\n");
    }

    #[test]
    fn ignores_heredoc_without_call() {
        test::<HeredocMethodCallPosition>().expect_no_offenses("<<-SQL\n  foo\nSQL\n");
    }

    #[test]
    fn flags_and_corrects_call_after_heredoc_end() {
        test::<HeredocMethodCallPosition>().expect_correction(
            indoc! {r#"
                <<-SQL
                  foo
                SQL
                .strip_indent
                ^ Put a method call with a HEREDOC receiver on the same line as the HEREDOC opening.
            "#},
            "<<-SQL.strip_indent\n  foo\nSQL\n",
        );
    }

    #[test]
    fn flags_and_corrects_call_with_paren_arguments() {
        test::<HeredocMethodCallPosition>().expect_correction(
            indoc! {r#"
                <<-SQL
                  foo
                SQL
                .foo(bar_baz)
                ^ Put a method call with a HEREDOC receiver on the same line as the HEREDOC opening.
            "#},
            "<<-SQL.foo(bar_baz)\n  foo\nSQL\n",
        );
    }

    #[test]
    fn flags_and_corrects_chained_call_without_parens() {
        // One offense at the terminator even though two calls hang off the
        // heredoc — RuboCop dedupes the shared range and corrects the whole
        // chain from the outermost call.
        test::<HeredocMethodCallPosition>().expect_correction(
            indoc! {r#"
                <<-SQL
                  foo
                SQL
                .strip_indent.foo
                ^ Put a method call with a HEREDOC receiver on the same line as the HEREDOC opening.
            "#},
            "<<-SQL.strip_indent.foo\n  foo\nSQL\n",
        );
    }

    #[test]
    fn flags_and_corrects_chained_call_with_parens() {
        test::<HeredocMethodCallPosition>().expect_correction(
            indoc! {r#"
                <<-SQL
                  foo
                SQL
                .abc(1, 2, 3).foo
                ^ Put a method call with a HEREDOC receiver on the same line as the HEREDOC opening.
            "#},
            "<<-SQL.abc(1, 2, 3).foo\n  foo\nSQL\n",
        );
    }

    #[test]
    fn flags_and_corrects_trailing_comma_call() {
        test::<HeredocMethodCallPosition>().expect_correction(
            indoc! {r#"
                bar(<<-SQL
                  foo
                SQL
                .abc,
                ^ Put a method call with a HEREDOC receiver on the same line as the HEREDOC opening.
                )
            "#},
            "bar(<<-SQL.abc,\n  foo\nSQL\n)\n",
        );
    }

    #[test]
    fn flags_multiline_arguments_without_correcting() {
        test::<HeredocMethodCallPosition>()
            .expect_offense(indoc! {r#"
                <<-SQL
                  foo
                SQL
                .abc(1, 2,
                ^ Put a method call with a HEREDOC receiver on the same line as the HEREDOC opening.
                3).foo
            "#})
            .expect_no_corrections(indoc! {r#"
                <<-SQL
                  foo
                SQL
                .abc(1, 2,
                3).foo
            "#});
    }

    #[test]
    fn flags_multiline_chain_without_correcting() {
        test::<HeredocMethodCallPosition>()
            .expect_offense(indoc! {r#"
                <<-SQL
                  foo
                SQL
                .abc
                ^ Put a method call with a HEREDOC receiver on the same line as the HEREDOC opening.
                .foo
            "#})
            .expect_no_corrections(indoc! {r#"
                <<-SQL
                  foo
                SQL
                .abc
                .foo
            "#});
    }

    #[test]
    fn flags_and_corrects_safe_navigation_call() {
        test::<HeredocMethodCallPosition>().expect_correction(
            indoc! {r#"
                <<-SQL
                  foo
                SQL
                &.strip_indent
                ^ Put a method call with a HEREDOC receiver on the same line as the HEREDOC opening.
            "#},
            "<<-SQL&.strip_indent\n  foo\nSQL\n",
        );
    }

    #[test]
    fn corrected_simple_case_is_stable() {
        // Idempotency: the corrected opener-line call is the accepted shape.
        test::<HeredocMethodCallPosition>().expect_no_offenses("<<-SQL.strip_indent\n  foo\nSQL\n");
    }

    #[test]
    fn corrected_chained_case_is_stable() {
        test::<HeredocMethodCallPosition>()
            .expect_no_offenses("<<-SQL.strip_indent.foo\n  foo\nSQL\n");
    }

    #[test]
    fn corrected_safe_navigation_case_is_stable() {
        test::<HeredocMethodCallPosition>()
            .expect_no_offenses("<<-SQL&.strip_indent\n  foo\nSQL\n");
    }
}
