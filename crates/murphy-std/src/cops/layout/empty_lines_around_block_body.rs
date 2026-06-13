//! `Layout/EmptyLinesAroundBlockBody` — keeps track of empty lines around
//! block bodies (`do ... end` and `{ ... }`, including numbered/`it` blocks).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/EmptyLinesAroundBlockBody
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Ports RuboCop's `on_block` (aliased to `on_numblock`/`on_itblock`) for
//!   the default `EnforcedStyle: no_empty_lines`. RuboCop anchors the
//!   beginning check on `node.send_node.last_line`, but in Murphy's AST a
//!   block's `call`/`send` node spans the whole `foo do ... end` expression,
//!   so the send range cannot stand in for the header line. Instead the
//!   block opener (`do` keyword or `{`) is located by scanning the tokens
//!   immediately before the block body (the last `do`/`{` token in the node
//!   before the body's first statement). This is robust against
//!   brace-arguments (`foo({}) { }`) and multi-line call headers (`foo(\n a)
//!   do`). A blank line immediately after the opener line or immediately
//!   before the closing `end`/`}` is flagged and removed. RuboCop's `&:empty?`
//!   blank test is literal — a whitespace-only line is NOT blank — matched
//!   exactly. Single-line blocks are skipped. A block whose only inner line is
//!   blank emits two offenses (beginning + end) and one de-duplicated edit.
//!   Messages:
//!     "Extra empty line detected at block body beginning."
//!     "Extra empty line detected at block body end."
//!   Gaps (documented, not bypassed):
//!     - The non-default `EnforcedStyle: empty_lines` (insert-a-blank
//!       direction with the `MSG_MISSING` message) is not implemented; only
//!       `no_empty_lines` (the config default) is ported.
//!     - An empty-bodied block falls back to the node start as the opener
//!       anchor (the backward scan has no body to scan from); an empty block
//!       has no inner line to flag, so this is inconsequential.
//! ```

use crate::cops::util::check_empty_lines_around_body_no_empty_lines;
use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, SourceTokenKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct EmptyLinesAroundBlockBody;

#[cop(
    name = "Layout/EmptyLinesAroundBlockBody",
    description = "Keeps track of empty lines around block bodies.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl EmptyLinesAroundBlockBody {
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

/// The block body for any of the three block kinds, or `None`.
fn block_body(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    match *cx.kind(node) {
        NodeKind::Block { body, .. }
        | NodeKind::Numblock { body, .. }
        | NodeKind::Itblock { body, .. } => body.get(),
        _ => None,
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let node_range = cx.range(node);

    // Find the block opener (`do` keyword or `{`). RuboCop anchors on the
    // send node's last line, but Murphy's block `send` spans the entire
    // expression, so locate the opener directly: the last `do`/`{` token
    // before the block body's first statement.
    let scan_from = block_body(node, cx)
        .map(|b| cx.range(b).start)
        .unwrap_or(node_range.end);
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < scan_from);
    let opener_start = toks[..idx]
        .iter()
        .rev()
        .take_while(|t| t.range.start >= node_range.start)
        .find(|t| {
            t.kind == SourceTokenKind::LeftBrace
                || (t.kind == SourceTokenKind::Other && cx.raw_source(t.range) == "do")
        })
        .map(|t| t.range.start)
        .unwrap_or(node_range.start);

    check_empty_lines_around_body_no_empty_lines(node, opener_start, "block", cx);
}

#[cfg(test)]
mod tests {
    use super::EmptyLinesAroundBlockBody;
    use murphy_plugin_api::test_support::{run_cop, run_cop_with_edits, test};

    #[test]
    fn accepts_do_block_without_surrounding_blank_lines() {
        test::<EmptyLinesAroundBlockBody>().expect_no_offenses("foo do\n  bar\nend\n");
    }

    #[test]
    fn flags_blank_line_at_beginning_of_do_block() {
        let src = "foo do\n\n  bar\nend\n";
        let offenses = run_cop::<EmptyLinesAroundBlockBody>(src);
        assert_eq!(offenses.len(), 1, "expected 1 offense, got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Extra empty line detected at block body beginning."
        );
    }

    #[test]
    fn flags_blank_line_at_end_of_do_block() {
        let src = "foo do\n  bar\n\nend\n";
        let offenses = run_cop::<EmptyLinesAroundBlockBody>(src);
        assert_eq!(offenses.len(), 1, "expected 1 offense, got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Extra empty line detected at block body end."
        );
    }

    #[test]
    fn flags_blank_lines_in_brace_block() {
        let src = "foo {\n\n  bar\n\n}\n";
        let offenses = run_cop::<EmptyLinesAroundBlockBody>(src);
        assert_eq!(offenses.len(), 2, "expected 2 offenses, got {offenses:?}");
    }

    #[test]
    fn corrects_beginning_blank_line() {
        // "foo do\n" = bytes 0..7; blank "\n" = byte 7..8.
        let src = "foo do\n\n  bar\nend\n";
        let result = run_cop_with_edits::<EmptyLinesAroundBlockBody>(src);
        assert_eq!(result.offenses.len(), 1);
        let edit = &result.edits[0];
        assert_eq!(edit.replacement, "");
        assert_eq!(edit.range.start, 7);
        assert_eq!(edit.range.end, 8);
    }

    #[test]
    fn flags_numbered_block() {
        let src = "foo do\n\n  _1\nend\n";
        let offenses = run_cop::<EmptyLinesAroundBlockBody>(src);
        assert_eq!(offenses.len(), 1, "expected 1 offense, got {offenses:?}");
    }

    #[test]
    fn accepts_single_line_block() {
        test::<EmptyLinesAroundBlockBody>().expect_no_offenses("foo { bar }\n");
    }

    #[test]
    fn ignores_whitespace_only_line() {
        test::<EmptyLinesAroundBlockBody>().expect_no_offenses("foo do\n  \n  bar\nend\n");
    }

    #[test]
    fn single_blank_inner_line_emits_two_offenses_one_edit() {
        let src = "foo do\n\nend\n";
        let result = run_cop_with_edits::<EmptyLinesAroundBlockBody>(src);
        assert_eq!(result.offenses.len(), 2, "{:?}", result.offenses);
        assert_eq!(result.edits.len(), 1, "edits must be de-duplicated");
    }

    #[test]
    fn no_false_positive_with_brace_argument() {
        // `foo({}) do ... end` — the `{}` argument must not be mistaken for
        // the block opener. No blank line, so no offense.
        test::<EmptyLinesAroundBlockBody>().expect_no_offenses("foo({}) do\n  bar\nend\n");
    }

    #[test]
    fn flags_beginning_with_multiline_header() {
        // The `do` is on the last header line; the blank line after it is
        // flagged even though the call header spans two lines.
        let src = "foo(a,\n  b) do\n\n  bar\nend\n";
        let offenses = run_cop::<EmptyLinesAroundBlockBody>(src);
        assert_eq!(offenses.len(), 1, "expected 1 offense, got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Extra empty line detected at block body beginning."
        );
    }
}

murphy_plugin_api::submit_cop!(EmptyLinesAroundBlockBody);
