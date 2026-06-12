//! `Layout/SpaceInsideRangeLiteral` — flags spaces around the `..`/`...`
//! operator of a range literal.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/SpaceInsideRangeLiteral
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's `check`: subscribe to range nodes, locate the
//!   `..`/`...` operator token, and flag a same-line space directly before or
//!   after the operator. A newline immediately after the operator (`1..\n 2`)
//!   is collapsed first, matching RuboCop's `sub!(/op\n\s*/, op)`, so genuinely
//!   multiline ranges without inline spaces are accepted. The autocorrect
//!   surgically removes only the offending whitespace runs adjacent to the
//!   operator (two non-overlapping edits), so operand source passes through
//!   byte-for-byte.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

const MSG: &str = "Space inside range literal.";

#[derive(Default)]
pub struct SpaceInsideRangeLiteral;

#[cop(
    name = "Layout/SpaceInsideRangeLiteral",
    description = "Checks for spaces inside range literals.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl SpaceInsideRangeLiteral {
    #[on_node(kind = "range")]
    fn check_range(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::RangeExpr { exclusive, .. } = *cx.kind(node) else {
        return;
    };
    let op_len: u32 = if exclusive { 3 } else { 2 };

    let Some(op_range) = operator_range(node, cx, op_len) else {
        return;
    };
    let src = cx.source().as_bytes();

    // Whitespace run directly before the operator, bounded by the node start.
    let node_start = cx.range(node).start;
    let before_ws = whitespace_before(src, op_range.start, node_start);
    // Whitespace run directly after the operator, bounded by the node end.
    let node_end = cx.range(node).end;
    let after_ws = whitespace_after(src, op_range.end, node_end);

    let has_space_before = before_ws < op_range.start;
    // A newline immediately after the operator is the multiline form
    // (`1..\n  2`): RuboCop collapses `op\n\s*` before testing, so it is not
    // an offense. Only flag the trailing side when the gap is non-empty *and*
    // does not start with a newline.
    let after_starts_with_newline = (op_range.end as usize) < src.len()
        && matches!(src[op_range.end as usize], b'\n' | b'\r');
    let has_space_after = after_ws > op_range.end && !after_starts_with_newline;

    if !has_space_before && !has_space_after {
        return;
    }

    cx.emit_offense(cx.range(node), MSG, None);

    if has_space_before {
        cx.emit_edit(
            Range {
                start: before_ws,
                end: op_range.start,
            },
            "",
        );
    }
    if has_space_after {
        cx.emit_edit(
            Range {
                start: op_range.end,
                end: after_ws,
            },
            "",
        );
    }
}

/// Locates the `..`/`...` operator token of a range node. The operator is the
/// `Other` token whose source equals the operator string and which sits
/// between the begin and end operands. Beginless/endless ranges still surface
/// the operator token.
fn operator_range(node: NodeId, cx: &Cx<'_>, op_len: u32) -> Option<Range> {
    let NodeKind::RangeExpr { begin_, end_, .. } = *cx.kind(node) else {
        return None;
    };
    let node_range = cx.range(node);
    // Lower bound: just past the begin operand (if any).
    let search_start = begin_.get().map_or(node_range.start, |b| cx.range(b).end);
    // Upper bound: just before the end operand (if any).
    let search_end = end_.get().map_or(node_range.end, |e| cx.range(e).start);

    let op_str: &[u8] = if op_len == 3 { b"..." } else { b".." };
    let src = cx.source().as_bytes();

    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < search_start);
    toks[idx..]
        .iter()
        .take_while(|t| t.range.end <= node_range.end)
        .filter(|t| t.range.start >= search_start && t.range.end <= search_end.max(node_range.end))
        .find(|t| {
            t.kind == SourceTokenKind::Other
                && t.range.end - t.range.start == op_len
                && &src[t.range.start as usize..t.range.end as usize] == op_str
        })
        .map(|t| t.range)
}

/// Returns the start offset of the contiguous run of spaces/tabs immediately
/// before `pos`, bounded below by `floor`. Equal to `pos` when there is no
/// whitespace.
fn whitespace_before(src: &[u8], pos: u32, floor: u32) -> u32 {
    let mut start = pos;
    while start > floor && matches!(src[(start - 1) as usize], b' ' | b'\t') {
        start -= 1;
    }
    start
}

/// Returns the end offset of the contiguous run of spaces/tabs immediately
/// after `pos`, bounded above by `ceil`. Equal to `pos` when there is no
/// whitespace.
fn whitespace_after(src: &[u8], pos: u32, ceil: u32) -> u32 {
    let mut end = pos;
    while end < ceil && matches!(src[end as usize], b' ' | b'\t') {
        end += 1;
    }
    end
}

murphy_plugin_api::submit_cop!(SpaceInsideRangeLiteral);

#[cfg(test)]
mod tests {
    use super::SpaceInsideRangeLiteral;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn corrects_space_before_and_after_inclusive() {
        test::<SpaceInsideRangeLiteral>().expect_correction(
            indoc! {r#"
                x = 1 .. 2
                    ^^^^^^ Space inside range literal.
            "#},
            "x = 1..2\n",
        );
    }

    #[test]
    fn corrects_space_after_inclusive() {
        test::<SpaceInsideRangeLiteral>().expect_correction(
            indoc! {r#"
                x = 1.. 2
                    ^^^^^ Space inside range literal.
            "#},
            "x = 1..2\n",
        );
    }

    #[test]
    fn corrects_space_before_inclusive() {
        test::<SpaceInsideRangeLiteral>().expect_correction(
            indoc! {r#"
                x = 1 ..2
                    ^^^^^ Space inside range literal.
            "#},
            "x = 1..2\n",
        );
    }

    #[test]
    fn corrects_space_before_and_after_exclusive() {
        test::<SpaceInsideRangeLiteral>().expect_correction(
            indoc! {r#"
                x = 1 ... 2
                    ^^^^^^^ Space inside range literal.
            "#},
            "x = 1...2\n",
        );
    }

    #[test]
    fn corrects_space_after_exclusive() {
        test::<SpaceInsideRangeLiteral>().expect_correction(
            indoc! {r#"
                x = 1... 2
                    ^^^^^^ Space inside range literal.
            "#},
            "x = 1...2\n",
        );
    }

    #[test]
    fn corrects_space_before_exclusive() {
        test::<SpaceInsideRangeLiteral>().expect_correction(
            indoc! {r#"
                x = 1 ...2
                    ^^^^^^ Space inside range literal.
            "#},
            "x = 1...2\n",
        );
    }

    #[test]
    fn corrects_multiline_space_before_operator() {
        // `0 ..` has a space before the operator → flagged; the newline after
        // the operator is collapsed (multiline form), so only the leading space
        // is corrected. The caret format cannot span a multiline node, so this
        // is verified via the offense+edits helper.
        use murphy_plugin_api::test_support::run_cop_with_edits;
        let result = run_cop_with_edits::<SpaceInsideRangeLiteral>("x = 0 ..\n    10\n");
        assert_eq!(
            result.offenses.len(),
            1,
            "expected 1 offense, got {:?}",
            result.offenses
        );
        assert_eq!(result.offenses[0].message, super::MSG);
        assert_eq!(result.edits.len(), 1, "expected 1 edit, got {:?}", result.edits);
        // Only the space before `..` is removed.
        assert_eq!(result.edits[0].replacement, "");
    }

    #[test]
    fn accepts_tight_ranges() {
        test::<SpaceInsideRangeLiteral>()
            .expect_no_offenses("x = 1..2\n")
            .expect_no_offenses("x = 1...2\n")
            .expect_no_offenses("x = 0...(line - 1)\n");
    }

    #[test]
    fn accepts_multiline_range_without_inline_space() {
        test::<SpaceInsideRangeLiteral>().expect_no_offenses("x = 0..\n    10\n");
    }
}
