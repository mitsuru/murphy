//! `Lint/HeredocMethodCallPosition` — checks calls on heredoc receivers are on the opener line.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/HeredocMethodCallPosition
//! upstream_version_checked: master
//! status: partial
//! gap_issues: [murphy-k16b]
//! notes: >
//!   Covers simple method calls placed after a heredoc terminator and
//!   autocorrects by moving a one-line call to the heredoc opener. Known v1
//!   limitation: RuboCop's chained calls, trailing-comma argument lists,
//!   multiline argument safety checks, and complete safe-navigation parity need
//!   richer heredoc loc/call-line helpers than the current cop uses from token
//!   ranges.
//! ```

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, Range, SourceTokenKind};

const MSG: &str = "Put a method call with a HEREDOC receiver on the same line as the HEREDOC opening.";

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
    let Some(receiver) = cx.call_receiver(node).get() else {
        return;
    };
    let Some((start, end)) = heredoc_tokens(receiver, cx) else {
        return;
    };
    if end.range.end > cx.range(node).end {
        return;
    }
    let selector = cx.selector(node);
    if selector.start < end.range.end {
        return;
    }

    let offense = Range {
        start: selector.start.saturating_sub(1),
        end: selector.start,
    };
    cx.emit_offense(offense, MSG, None);

    let call_range = Range {
        start: end.range.end,
        end: cx.range(node).end,
    };
    let call_source = cx.raw_source(call_range).trim();
    if call_source.lines().count() == 1 {
        cx.emit_edit(remove_call_line_range(call_range, cx.source()), "");
        let insert_at = opener_line_end(start.range, cx.source());
        cx.emit_edit(
            Range {
                start: insert_at,
                end: insert_at,
            },
            call_source,
        );
    }
}

fn remove_call_line_range(call_range: Range, source: &str) -> Range {
    let bytes = source.as_bytes();
    let mut end = call_range.end as usize;
    if end < bytes.len() && bytes[end] == b'\n' {
        end += 1;
    }
    Range {
        start: call_range.start,
        end: end as u32,
    }
}

fn heredoc_tokens(
    receiver: NodeId,
    cx: &Cx<'_>,
) -> Option<(murphy_plugin_api::SourceToken, murphy_plugin_api::SourceToken)> {
    let range = cx.range(receiver);
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
    fn accepts_heredoc_without_call_and_call_on_opener_line() {
        test::<HeredocMethodCallPosition>()
            .expect_no_offenses("<<~SQL\n  foo\nSQL\n")
            .expect_no_offenses("<<~SQL.bar\n  foo\nSQL\n");
    }
}
