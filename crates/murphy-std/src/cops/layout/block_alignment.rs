//! `Layout/BlockAlignment` — the `end` of a `do…end` (or `{…}`) block must be
//! aligned with the configured anchor.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/BlockAlignment
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: [murphy-vafs]
//! notes: >
//!   Ports `on_block` (aliased to `on_numblock`/`on_itblock`). Three styles via
//!   `EnforcedStyleAlignWith` (`SupportedStylesAlignWith: [either,
//!   start_of_block, start_of_line]`, default `either`):
//!
//!   - `start_of_line`: `end` aligns with the start column of the line where
//!     the aligning expression starts.
//!   - `start_of_block`: `end` aligns with the indentation of the line where
//!     the `do`/`{` appears.
//!   - `either` (default): `end` is accepted at either column.
//!
//!   The aligning expression is resolved by `block_end_align_target`: walk the
//!   block's ancestor chain and stop at the topmost ancestor that is an
//!   assignment / `def` / splat / `and` / `or` / `_ << block` / `block.method`
//!   (non-`[]`) on the same line as its child (RuboCop's
//!   `block_end_align_target?` pattern + `disqualified_parent?`). For an
//!   assignment, the message LHS is the assignment's target (`find_lhs_node`).
//!
//!   Acceptance (`check_block_alignment`): the `end` must begin its own line;
//!   then it is accepted when its column equals the start-node column
//!   (start_of_line) or the do-line indentation (start_of_block), per style.
//!   The offense is on the `end`/`}` token with RuboCop's
//!   ``<cur> is not aligned with <prefer>[ or <alt>].`` message (the `alt`
//!   alternative is only shown in `either` when the two columns differ).
//!   Autocorrect re-indents the `end`/`}` line to the target column.
//!
//!   Autocorrect re-indents the `end`/`}` line to the target column. For
//!   `start_of_block` the column comes from `start_for_block_node`; for
//!   `either`/`start_of_line` it comes from `start_for_line_node` — the topmost
//!   ancestor sharing the block-align target's first line (RuboCop's
//!   `each_ancestor` reverse find, then `find_lhs_node`). The two differ when
//!   the block is an argument to a call on the same line (`puts(foo do … end)`
//!   re-indents `end` to `puts`'s column, not `foo`'s) — murphy-o3b2. The
//!   *message* always anchors on `start_for_block_node`, which already matched
//!   upstream including the deep `a.b do … end.c do … end` chain (the outer
//!   block has no same-line ancestor to climb to, so the line node equals the
//!   block node and no message permutation differs).
//!
//!   Gaps vs upstream:
//!   - Column is counted by Unicode scalar (`chars().count()`), not RuboCop's
//!     `display_column` (East-Asian wide glyph = width 2). Shared edge gap with
//!     the other Layout column cops.
//! ```
//!
//! ## Matched shapes
//!
//! Multi-line `block`/`numblock`/`itblock` nodes whose `end`/`}` begins its own
//! line but is not aligned with the configured anchor.

use crate::cops::util::block_opener;
use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, SourceTokenKind, cop};

/// Stateless unit struct (ADR 0035).
#[derive(Default)]
pub struct BlockAlignment;

/// Options for [`BlockAlignment`].
#[derive(CopOptions)]
pub struct BlockAlignmentOptions {
    #[option(
        name = "EnforcedStyleAlignWith",
        default = "either",
        description = "Where the block `end` aligns: with the start of the line, the start of the block, or either."
    )]
    pub enforced_style_align_with: AlignWith,
}

/// `SupportedStylesAlignWith: [either, start_of_block, start_of_line]`.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlignWith {
    #[option(value = "either")]
    Either,
    #[option(value = "start_of_block")]
    StartOfBlock,
    #[option(value = "start_of_line")]
    StartOfLine,
}

#[cop(
    name = "Layout/BlockAlignment",
    description = "Align block ends correctly.",
    default_severity = "warning",
    default_enabled = true,
    options = BlockAlignmentOptions
)]
impl BlockAlignment {
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

/// A source-line-column triple as in RuboCop's message hashes.
struct SrcLineCol {
    source: String,
    line: usize,
    column: usize,
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let Some(end_loc) = block_end_delimiter(node, cx) else {
        return;
    };
    // `return unless begins_its_line?(end_loc)`.
    if !begins_its_line(end_loc.start, cx) {
        return;
    }

    let opts = cx.options_or_default::<BlockAlignmentOptions>();
    let style = opts.enforced_style_align_with;

    // `start_for_block_node`: the aligning node (then its LHS for assignments).
    let start_node = start_for_block_node(node, cx);
    let start_loc = cx.range(start_node);
    let end_col = column_of(cx, end_loc.start);
    let start_col = column_of(cx, start_loc.start);

    // `return unless start_loc.column != end_loc.column || style == :start_of_block`.
    if start_col == end_col && style != AlignWith::StartOfBlock {
        return;
    }

    // `compute_do_source_line_column`: the indentation of the `do`/`{` line.
    let Some(do_slc) = compute_do_source_line_column(node, end_col, style, cx) else {
        return;
    };

    // The start-of-line anchor (the aligning node's first-line text/column).
    let start_slc = loc_to_source_line_column(start_loc, cx);

    let error_slc = if style == AlignWith::StartOfBlock {
        &do_slc
    } else {
        &start_slc
    };

    let current = loc_to_source_line_column(end_loc, cx);
    let message = format_message(&current, error_slc, &start_slc, &do_slc, style);
    cx.emit_offense(end_loc, &message, None);

    // Autocorrect: re-indent the `end`/`}` line to the target column. RuboCop's
    // `autocorrect` picks the column from `start_for_block_node` only for
    // `start_of_block`; for `either`/`start_of_line` it uses
    // `start_for_line_node` — the topmost ancestor sharing the block-align
    // target's first line. The two differ when the block is an argument to a
    // call on the same line (`puts(foo do … end)` re-indents `end` to `puts`'s
    // column, not `foo`'s). The *message* keeps anchoring on
    // `start_for_block_node`.
    let target_col = if style == AlignWith::StartOfBlock {
        do_slc.column
    } else {
        let line_node = start_for_line_node(node, cx);
        column_of(cx, cx.range(line_node).start)
    };
    if let Some(line_start) = line_start_if_leads(end_loc.start, cx) {
        let indent = " ".repeat(target_col);
        cx.emit_edit(
            Range {
                start: line_start,
                end: end_loc.start,
            },
            &indent,
        );
    }
}

/// `compute_do_source_line_column`: the `do`/`{` line's leading text and
/// indentation. Returns `None` when the `end` already matches the do-line
/// indentation and the style is not `start_of_line` (RuboCop's early return).
fn compute_do_source_line_column(
    node: NodeId,
    end_col: usize,
    style: AlignWith,
    cx: &Cx<'_>,
) -> Option<SrcLineCol> {
    let opener = block_opener(node, cx)?;
    let src = cx.source();
    let do_start = opener.start as usize;
    let line_start = src[..do_start].rfind('\n').map_or(0, |pos| pos + 1);
    let line_end = src[line_start..]
        .find('\n')
        .map_or(src.len(), |idx| line_start + idx);
    let line = &src[line_start..line_end];
    let leading_ws = line.len() - line.trim_start().len();
    let indentation_of_do_line = src[line_start..line_start + leading_ws].chars().count();

    // `return unless end_loc.column != indentation_of_do_line || style == :start_of_line`.
    if end_col == indentation_of_do_line && style != AlignWith::StartOfLine {
        return None;
    }

    let (line_num, _) = line_and_column_at(line_start as u32 + leading_ws as u32, cx);
    Some(SrcLineCol {
        source: line[leading_ws..].trim_end().to_owned(),
        line: line_num,
        column: indentation_of_do_line,
    })
}

/// `loc_to_source_line_column`: the first physical line of `loc`'s source, plus
/// its line and column.
fn loc_to_source_line_column(loc: Range, cx: &Cx<'_>) -> SrcLineCol {
    let src = cx.source();
    let start = loc.start as usize;
    let end = loc.end as usize;
    let first_line = src[start..end]
        .split('\n')
        .next()
        .unwrap_or("")
        .trim_end_matches('\r');
    let (line, column) = line_and_column_at(loc.start, cx);
    SrcLineCol {
        source: first_line.to_owned(),
        line,
        column,
    }
}

/// `format_message` + `alt_start_msg`.
fn format_message(
    current: &SrcLineCol,
    error: &SrcLineCol,
    start: &SrcLineCol,
    do_slc: &SrcLineCol,
    style: AlignWith,
) -> String {
    let alt = alt_start_msg(start, do_slc, style);
    format!(
        "{} is not aligned with {}{}.",
        fmt_slc(current),
        fmt_slc(error),
        alt
    )
}

/// `alt_start_msg`: ` or <do-line>` only in `either` when start and do-line
/// differ.
fn alt_start_msg(start: &SrcLineCol, do_slc: &SrcLineCol, style: AlignWith) -> String {
    if style != AlignWith::Either
        || (start.line == do_slc.line && start.column == do_slc.column)
    {
        String::new()
    } else {
        format!(" or {}", fmt_slc(do_slc))
    }
}

fn fmt_slc(slc: &SrcLineCol) -> String {
    format!("`{}` at {}, {}", slc.source, slc.line, slc.column)
}

/// `start_for_block_node`: the aligning node, descended to the assignment LHS.
fn start_for_block_node(block: NodeId, cx: &Cx<'_>) -> NodeId {
    let target = block_end_align_target(block, cx);
    find_lhs_node(target, cx)
}

/// `start_for_line_node`: the start-of-line autocorrect anchor.
///
/// ```ruby
/// def start_for_line_node(block_node)
///   start_node = start_for_block_node(block_node)
///   start_node = start_node.each_ancestor.to_a.reverse.find do |node|
///     same_line?(start_node, node)
///   end || start_node
///   find_lhs_node(start_node)
/// end
/// ```
///
/// Climb from `start_for_block_node` through ancestors that share its first
/// line and take the topmost such ancestor (source ranges nest, so any ancestor
/// on the start node's first line implies every intermediate ancestor is too —
/// a simple parent walk while first lines match yields the same node). Then
/// descend the assignment LHS again. Used only by the autocorrect column for the
/// `either`/`start_of_line` styles; the message keeps `start_for_block_node`.
fn start_for_line_node(block: NodeId, cx: &Cx<'_>) -> NodeId {
    let start_node = start_for_block_node(block, cx);
    let bytes = cx.source().as_bytes();
    let start_first = cx.range(start_node).start;
    let mut topmost = start_node;
    let mut current = start_node;
    while let Some(parent) = cx.parent(current).get() {
        // `same_line?(start_node, parent)`: the parent's first line equals the
        // start node's first line iff no newline lies in `[parent.start,
        // start_node.start)` (the parent always begins at or before the node).
        let (lo, hi) = (cx.range(parent).start.min(start_first), start_first);
        if bytes[lo as usize..hi as usize].contains(&b'\n') {
            break;
        }
        topmost = parent;
        current = parent;
    }
    find_lhs_node(topmost, cx)
}

/// `block_end_align_target`: walk `[block, *ancestors]` pairwise and return the
/// first `current` for which `end_align_target?(current, parent)` holds;
/// otherwise the topmost ancestor.
fn block_end_align_target(block: NodeId, cx: &Cx<'_>) -> NodeId {
    let mut current = block;
    loop {
        let Some(parent) = cx.parent(current).get() else {
            return current;
        };
        if end_align_target(current, parent, cx) {
            return current;
        }
        current = parent;
    }
}

/// `end_align_target?`: stop climbing when the parent is disqualified or is not
/// one of the alignment-target shapes.
fn end_align_target(node: NodeId, parent: NodeId, cx: &Cx<'_>) -> bool {
    disqualified_parent(parent, node, cx) || !block_end_align_target_shape(parent, node, cx)
}

/// `disqualified_parent?`: parent exists, is on a different first line than the
/// node, and is not a `masgn`.
fn disqualified_parent(parent: NodeId, node: NodeId, cx: &Cx<'_>) -> bool {
    if matches!(cx.kind(parent), NodeKind::Masgn { .. }) {
        return false;
    }
    let bytes = cx.source().as_bytes();
    // first lines differ iff a newline lies in [parent.start, node.start).
    let (lo, hi) = (cx.range(parent).start, cx.range(node).start);
    let (lo, hi) = (lo.min(hi), lo.max(hi));
    bytes[lo as usize..hi as usize].contains(&b'\n')
}

/// `block_end_align_target?` PATTERN: parent is an assignment / any_def / splat
/// / and / or / `_ << node` / `node.method` (non-`[]`).
fn block_end_align_target_shape(parent: NodeId, node: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(parent) {
        NodeKind::Lvasgn { .. }
        | NodeKind::Ivasgn { .. }
        | NodeKind::Cvasgn { .. }
        | NodeKind::Gvasgn { .. }
        | NodeKind::Casgn { .. }
        | NodeKind::Masgn { .. }
        | NodeKind::OpAsgn { .. }
        | NodeKind::OrAsgn { .. }
        | NodeKind::AndAsgn { .. }
        | NodeKind::Def { .. }
        | NodeKind::Defs { .. }
        | NodeKind::Splat(_)
        | NodeKind::And { .. }
        | NodeKind::Or { .. } => true,
        NodeKind::Send { .. } | NodeKind::Csend { .. } => {
            let method = cx.method_name(parent);
            // `(send _ :<< ...)` — any receiver, `<<` selector.
            if method == Some("<<") {
                return true;
            }
            // `(send equal?(%1) !:[] ...)` — receiver is exactly the block
            // node and selector is not `[]`.
            cx.call_receiver(parent).get() == Some(node) && method != Some("[]")
        }
        _ => false,
    }
}

/// `find_lhs_node`: descend through `op_asgn`/`masgn` to the LHS target so the
/// message shows the assignment LHS rather than the whole assignment.
fn find_lhs_node(mut node: NodeId, cx: &Cx<'_>) -> NodeId {
    loop {
        match *cx.kind(node) {
            NodeKind::OpAsgn { target, .. } => node = target,
            NodeKind::Masgn { lhs, .. } => node = lhs,
            _ => return node,
        }
    }
}

/// The closing delimiter token (`end` / `}`) of a block, or `None`.
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

/// `begins_its_line?` — everything before `offset` on its line is whitespace.
fn begins_its_line(offset: u32, cx: &Cx<'_>) -> bool {
    let src = cx.source();
    let offset = offset as usize;
    let line_start = src[..offset].rfind('\n').map_or(0, |pos| pos + 1);
    src[line_start..offset].bytes().all(|b| b == b' ' || b == b'\t')
}

/// 0-based character column of `offset`.
fn column_of(cx: &Cx<'_>, offset: u32) -> usize {
    let src = cx.source();
    let upper = (offset as usize).min(src.len());
    let line_start = src[..upper].rfind('\n').map_or(0, |pos| pos + 1);
    src[line_start..upper].chars().count()
}

/// 1-based line and 0-based char column of `offset`.
fn line_and_column_at(offset: u32, cx: &Cx<'_>) -> (usize, usize) {
    let src = cx.source();
    let upper = (offset as usize).min(src.len());
    let line = src[..upper].bytes().filter(|&b| b == b'\n').count() + 1;
    let line_start = src[..upper].rfind('\n').map_or(0, |pos| pos + 1);
    let col = src[line_start..upper].chars().count();
    (line, col)
}

/// If `offset` is the first non-whitespace on its line, return the line start.
fn line_start_if_leads(offset: u32, cx: &Cx<'_>) -> Option<u32> {
    let src = cx.source();
    let offset = offset as usize;
    let line_start = src[..offset].rfind('\n').map_or(0, |pos| pos + 1);
    if src[line_start..offset].bytes().all(|b| b == b' ' || b == b'\t') {
        Some(line_start as u32)
    } else {
        None
    }
}

murphy_plugin_api::submit_cop!(BlockAlignment);

#[cfg(test)]
mod tests {
    use super::{AlignWith, BlockAlignment as Cop, BlockAlignmentOptions};
    use murphy_plugin_api::test_support::{
        CapturedEdit, run_cop, run_cop_with_edits, run_cop_with_options, test,
    };

    fn style(s: AlignWith) -> BlockAlignmentOptions {
        BlockAlignmentOptions {
            enforced_style_align_with: s,
        }
    }

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
    fn accepts_aligned_block_no_args() {
        test::<Cop>().expect_no_offenses("test do\nend\n");
    }

    #[test]
    fn flags_mismatched_block_end_no_args() {
        let src = "test do\n  end\n";
        let offenses = run_cop::<Cop>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "`end` at 2, 2 is not aligned with `test do` at 1, 0."
        );
    }

    #[test]
    fn flags_mismatched_block_end_with_args() {
        let src = "test do |ala|\n  end\n";
        let offenses = run_cop::<Cop>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "`end` at 2, 2 is not aligned with `test do |ala|` at 1, 0."
        );
    }

    #[test]
    fn flags_mismatched_block_with_variable() {
        let src = "variable = test do |ala|\n  end\n";
        let offenses = run_cop::<Cop>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "`end` at 2, 2 is not aligned with `variable = test do |ala|` at 1, 0."
        );
    }

    #[test]
    fn flags_assignment_chain_aligns_with_leftmost() {
        let src = "a = b = c = test do |ala|\n    end\n";
        let offenses = run_cop::<Cop>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "`end` at 2, 4 is not aligned with `a = b = c = test do |ala|` at 1, 0."
        );
    }

    #[test]
    fn flags_ivar_assignment() {
        let src = "@variable = test do |ala|\n  end\n";
        let offenses = run_cop::<Cop>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "`end` at 2, 2 is not aligned with `@variable = test do |ala|` at 1, 0."
        );
    }

    #[test]
    fn accepts_end_aligned_with_assignment_lhs() {
        test::<Cop>().expect_no_offenses("variable = test do |ala|\nend\n");
    }

    #[test]
    fn either_accepts_start_of_block_alignment() {
        // `end` aligned with the `do` line indentation (2), which differs from
        // the start-of-line column (0). `either` accepts both.
        test::<Cop>().expect_no_offenses("foo.bar\n  .each do\n    baz\n  end\n");
    }

    #[test]
    fn start_of_block_flags_start_of_line_alignment() {
        // `end` at column 0 matches start_of_line but not start_of_block (2).
        let src = "foo.bar\n  .each do\n    baz\nend\n";
        let offenses = run_cop_with_options::<Cop>(src, &style(AlignWith::StartOfBlock));
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
    }

    #[test]
    fn corrects_mismatched_end() {
        let src = "variable = test do |ala|\n  end\n";
        let run = run_cop_with_edits::<Cop>(src);
        assert_eq!(apply(src, &run.edits), "variable = test do |ala|\nend\n");
    }

    #[test]
    fn correction_is_idempotent() {
        let src = "variable = test do |ala|\n      end\n";
        let run = run_cop_with_edits::<Cop>(src);
        let fixed = apply(src, &run.edits);
        assert!(run_cop::<Cop>(&fixed).is_empty(), "not idempotent: {fixed:?}");
    }

    #[test]
    fn accepts_inline_end() {
        // `end` does not begin its own line — not checked.
        test::<Cop>().expect_no_offenses("test do foo end\n");
    }

    #[test]
    fn flags_lshift_block() {
        // `x << test do … end`: align target is the `<<` send, starting at `x`.
        let src = "x << test do\n  end\n";
        let offenses = run_cop::<Cop>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "`end` at 2, 2 is not aligned with `x << test do` at 1, 0."
        );
    }

    /// murphy-o3b2: a block passed as an argument whose enclosing call shares the
    /// block's first line. RuboCop's `start_for_block_node` (the message anchor)
    /// is the block (`foo do` at 1, 5), but `autocorrect` uses
    /// `start_for_line_node` — it climbs to the topmost ancestor on the block's
    /// first line (`puts`, col 0) and re-indents `end` there. The message must
    /// still anchor on the block, and the alt is the do-line.
    #[test]
    fn corrects_block_argument_to_enclosing_line_node() {
        let src = "puts(foo do\n  x\n   end)\n";
        let run = run_cop_with_edits::<Cop>(src);
        assert_eq!(run.offenses.len(), 1, "got {:?}", run.offenses);
        assert_eq!(
            run.offenses[0].message,
            "`end` at 3, 3 is not aligned with `foo do` at 1, 5 or `puts(foo do` at 1, 0."
        );
        // `start_for_line_node` climbs to `puts` (col 0), not the block (col 5).
        assert_eq!(apply(src, &run.edits), "puts(foo do\n  x\nend)\n");
    }

    /// murphy-o3b2 idempotency: re-running on the corrected source is clean.
    #[test]
    fn block_argument_correction_is_idempotent() {
        let src = "puts(foo do\n  x\n   end)\n";
        let run = run_cop_with_edits::<Cop>(src);
        let fixed = apply(src, &run.edits);
        assert!(run_cop::<Cop>(&fixed).is_empty(), "not idempotent: {fixed:?}");
    }

    /// murphy-o3b2 no-regression: a deep `a.b do … end.c do … end` chain. The
    /// outer block has no same-line ancestor to climb to, so `start_for_line_node`
    /// == `start_for_block_node` and both message and correction already matched
    /// RuboCop before the line-node fix.
    #[test]
    fn corrects_chained_block_outer_end() {
        let src = "a.b do\n  x\nend.c do\n  y\n  end\n";
        let run = run_cop_with_edits::<Cop>(src);
        assert_eq!(run.offenses.len(), 1, "got {:?}", run.offenses);
        assert_eq!(
            run.offenses[0].message,
            "`end` at 5, 2 is not aligned with `a.b do` at 1, 0 or `end.c do` at 3, 0."
        );
        assert_eq!(apply(src, &run.edits), "a.b do\n  x\nend.c do\n  y\nend\n");
    }
}
