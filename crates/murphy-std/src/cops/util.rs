//! Shared utilities for standard cops.

use murphy_plugin_api::{CommentDirectiveKind, Cx, NodeId, NodeKind, Range, SourceTokenKind};

/// The portion of `node`'s source range up to (but excluding) the first
/// newline — i.e. the node's first physical line. Used to clamp whole-node
/// offenses that RuboCop renders across multiple lines: Murphy's
/// `expect_offense` annotation grammar cannot express a multiline caret span,
/// and the codebase convention (see `Lint/MissingSuper`) is to highlight the
/// node's first line. The start position is byte-identical to RuboCop's
/// whole-node range, so the reported line/column is faithful.
pub fn first_line_range(node: NodeId, cx: &Cx<'_>) -> Range {
    let range = cx.range(node);
    let start = range.start as usize;
    let end = cx
        .source()
        .as_bytes()
        .get(start..range.end as usize)
        .and_then(|line| line.iter().position(|&b| b == b'\n'))
        .map_or(range.end as usize, |idx| start + idx);
    Range {
        start: range.start,
        end: end as u32,
    }
}

/// Returns `true` if `node` is a parenthesized expression `(...)`.
///
/// After the translator change, prism's `ParenthesesNode` lowers to
/// `NodeKind::Begin` — the same variant used by `begin...end`. To
/// distinguish the two, we check that the first token at `range.start`
/// is `LeftParen`. For `begin...end`, the token at that offset is
/// `Other` with text `begin`.
///
/// # Example
/// ```text
/// (foo)           → Begin([Send]) with LeftParen at range.start → true
/// begin foo end   → Begin([Send]) with Other("begin") at range.start → false
/// ```
pub fn is_parenthesized(node: NodeId, cx: &Cx<'_>) -> bool {
    if !matches!(cx.kind(node), NodeKind::Begin(_)) {
        return false;
    }
    let range_start = cx.range(node).start;
    cx.token_after(range_start)
        .is_some_and(|t| t.kind == SourceTokenKind::LeftParen && t.range.start == range_start)
}

/// Unwraps arbitrarily nested parenthesized single-expressions.
///
/// `((expr))` → `expr`, `(expr)` → `expr`, anything else → unchanged.
/// Stops as soon as a layer is not a single-child parenthesized Begin.
pub fn unwrap_parenthesized(mut node_id: NodeId, cx: &Cx<'_>) -> NodeId {
    while is_parenthesized(node_id, cx) {
        let NodeKind::Begin(list) = cx.kind(node_id) else {
            break;
        };
        match cx.list(*list) {
            [single] => node_id = *single,
            _ => break,
        }
    }
    node_id
}

/// Emit an edit that replaces `cond_range` with `replacement`, prepending a
/// space if the character immediately before `cond_range.start` is not
/// whitespace.
///
/// Used by `NegatedIf/NegatedUnless/NegatedWhile` when replacing a
/// parenthesized condition like `(!x.even?)` with its inner receiver source
/// `x.even?`. Without this guard, `if(!x.even?)` would autocorrect to
/// `unlessx.even?` (keyword and replacement run together).
pub fn emit_edit_with_preceding_space(cond_range: Range, replacement: &str, cx: &Cx<'_>) {
    let source = cx.source().as_bytes();
    let needs_space =
        cond_range.start > 0 && !source[(cond_range.start - 1) as usize].is_ascii_whitespace();
    if needs_space {
        cx.emit_edit(cond_range, &format!(" {replacement}"));
    } else {
        cx.emit_edit(cond_range, replacement);
    }
}

/// Returns `true` when the byte at `offset` sits at a column that holds a
/// non-whitespace character on the immediately preceding or following source
/// line. Mirrors RuboCop's `AllowForAlignment` / `PrecedingFollowingAlignment`
/// vertical-alignment heuristic: extra spacing is treated as intentional
/// alignment when something lines up directly above or below.
///
/// Shared by `Layout/SpaceAroundOperators` (operator column) and
/// `Layout/SpaceBeforeFirstArg` (first-argument column).
pub fn is_alignment_at_column(src: &[u8], offset: usize) -> bool {
    let line_start = src[..offset]
        .iter()
        .rposition(|&b| b == b'\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    let col = offset - line_start;

    let non_ws_at_col = |line: &[u8]| -> bool {
        col < line.len() && !matches!(line[col], b' ' | b'\t' | b'\n' | b'\r')
    };

    // Check previous line.
    if line_start > 0 {
        let prev_end = line_start - 1;
        let prev_start = src[..prev_end]
            .iter()
            .rposition(|&b| b == b'\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        if non_ws_at_col(&src[prev_start..prev_end]) {
            return true;
        }
    }

    // Check next line.
    let rest_start = src[offset..]
        .iter()
        .position(|&b| b == b'\n')
        .map(|i| offset + i + 1)
        .unwrap_or(src.len());
    if rest_start < src.len() {
        let next_end = src[rest_start..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|i| rest_start + i)
            .unwrap_or(src.len());
        if non_ws_at_col(&src[rest_start..next_end]) {
            return true;
        }
    }

    false
}

/// RuboCop's `CommentsHelp#allow_comments?` comment clause for empty-branch
/// cops (`Lint/EmptyWhen`, `Lint/EmptyInPattern`):
///
/// ```text
/// AllowComments && contains_comments?(node) && !comments_contain_disables?(node, name)
/// ```
///
/// Returns `true` when `region` contains at least one comment that should
/// *allow* an otherwise-empty branch — i.e. the region has a comment and no
/// `disable` directive naming `cop_name` (or `all`) sits inside it. A bare
/// `# rubocop:disable Lint/EmptyWhen` is therefore NOT an allowing comment:
/// RuboCop computes the offense (which the directive engine then suppresses),
/// so the cop must still fire. A directive for an unrelated cop is an ordinary
/// comment and does allow the branch.
pub fn region_has_allowing_comment(cx: &Cx<'_>, region: Range, cop_name: &str) -> bool {
    if cx.comments_in_range(region).is_empty() {
        return false;
    }
    let has_disable_for_cop = cx.comment_directives().iter().any(|directive| {
        directive.kind == CommentDirectiveKind::Disable
            && in_range(directive.comment_range, region)
            && directive.cop.is_none_or(|cop| cop == cop_name)
    });
    !has_disable_for_cop
}

/// `true` when `inner` is fully contained within `outer`.
fn in_range(inner: Range, outer: Range) -> bool {
    inner.start >= outer.start && inner.end <= outer.end
}

/// The 0-based source line that contains byte `offset` (number of `\n`
/// bytes strictly before `offset`). Faithful to RuboCop's 1-based
/// `loc.line` only up to a constant offset — the `Layout/First*LineBreak`
/// cops compare lines for equality/inequality, so any consistent line
/// numbering works.
pub fn line_of(offset: u32, cx: &Cx<'_>) -> u32 {
    let src = cx.source().as_bytes();
    let upper = (offset as usize).min(src.len());
    src[..upper].iter().filter(|&&b| b == b'\n').count() as u32
}

/// Byte offset of the start of 0-based source line `line` (the line that
/// follows `line` newlines from the start of the file), or `None` when the
/// file has fewer lines. Counterpart of [`line_of`].
pub fn nth_line_start(cx: &Cx<'_>, line: u32) -> Option<u32> {
    if line == 0 {
        return Some(0);
    }
    let bytes = cx.source().as_bytes();
    let mut seen = 0u32;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'\n' {
            seen += 1;
            if seen == line {
                return Some(i as u32 + 1);
            }
        }
    }
    None
}

/// The byte range of the whole source line that begins at `line_start`,
/// including its terminating `\n` (or up to EOF for the final line). Used by
/// the `Layout/EmptyLines*` family to remove a blank line wholesale.
pub fn whole_line_range_with_newline(line_start: u32, cx: &Cx<'_>) -> Range {
    let bytes = cx.source().as_bytes();
    let start = (line_start as usize).min(bytes.len());
    let end = bytes[start..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(bytes.len(), |pos| start + pos + 1);
    Range {
        start: start as u32,
        end: end as u32,
    }
}

/// `true` when 0-based source `line` is a comment line — optional leading
/// whitespace followed by `#`. Mirrors RuboCop's `comment_line?`
/// (`/\A\s*#/`).
pub fn line_is_comment(cx: &Cx<'_>, line: u32) -> bool {
    let Some(start) = nth_line_start(cx, line) else {
        return false;
    };
    let bytes = cx.source().as_bytes();
    let start = start as usize;
    if start >= bytes.len() {
        return false;
    }
    let end = bytes[start..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(bytes.len(), |pos| start + pos);
    let mut i = start;
    while i < end && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    i < end && bytes[i] == b'#'
}

/// `true` when 0-based source `line` is empty or contains only whitespace.
/// Lines beyond the end of the file are treated as empty (mirrors RuboCop's
/// `processed_source[line].nil? || .blank?`).
pub fn line_is_blank(cx: &Cx<'_>, line: u32) -> bool {
    let Some(start) = nth_line_start(cx, line) else {
        return true;
    };
    let bytes = cx.source().as_bytes();
    let start = start as usize;
    if start >= bytes.len() {
        return true;
    }
    let end = bytes[start..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(bytes.len(), |pos| start + pos);
    bytes[start..end].iter().all(|b| b.is_ascii_whitespace())
}

/// Port of RuboCop's `FirstElementLineBreak#check_children_line_break`.
///
/// `open_offset` is the byte offset of the collection's opening delimiter
/// (the `[`, `{`, or `(` that RuboCop reads as `start.first_line`).
/// `children` are the element nodes in source order. When `ignore_last`
/// is set, the trailing element's `last_line` is replaced with its
/// `first_line` (RuboCop's `AllowMultilineFinalElement`).
///
/// Emits an offense on the earliest-line child and an autocorrect that
/// inserts a line break before it when the first element shares the
/// opening delimiter's line and the collection spans multiple lines.
pub fn check_children_line_break(
    cx: &Cx<'_>,
    open_offset: u32,
    children: &[NodeId],
    ignore_last: bool,
    message: &str,
) {
    if children.is_empty() {
        return;
    }

    let line = line_of(open_offset, cx);

    // `first_by_line(children)` — the child with the earliest first line.
    let Some(&min) = children
        .iter()
        .min_by_key(|&&c| line_of(cx.range(c).start, cx))
    else {
        return;
    };
    if line != line_of(cx.range(min).start, cx) {
        return;
    }

    // `last_line(children, ignore_last:)` — max over children of either
    // their last line (default) or their first line (ignore_last).
    let max_line = children
        .iter()
        .map(|&c| {
            if ignore_last {
                line_of(cx.range(c).start, cx)
            } else {
                line_of(cx.range(c).end.saturating_sub(1).max(cx.range(c).start), cx)
            }
        })
        .max()
        .unwrap_or(line);
    if line == max_line {
        return;
    }

    let min_start = cx.range(min).start;
    // RuboCop highlights the whole `min` node; clamp to its first physical
    // line so the offense annotation has a valid single-line caret span
    // (codebase convention — see `first_line_range`).
    cx.emit_offense(first_line_range(min, cx), message, None);
    cx.emit_edit(
        Range {
            start: min_start,
            end: min_start,
        },
        "\n",
    );
}

/// Port of RuboCop's `MultilineElementLineBreaks#check_line_breaks`.
///
/// `children` are the element nodes in source order (RuboCop passes
/// `node.children`). Each element after the first must begin on a line
/// strictly after the previous *kept* element's last line; otherwise an
/// offense (and a leading-newline autocorrect) is emitted on it.
///
/// `ignore_last` mirrors `AllowMultilineFinalElement`: when set, the
/// single-line guard compares the first and last elements' *start* lines
/// (`same_line?`) rather than first-element-first-line vs
/// last-element-last-line, so a multi-line trailing element does not force
/// the whole collection multi-line.
///
/// Elements are source-ordered and non-overlapping, so "the previous kept
/// element's last line equals this element's first line" is equivalent to
/// "no newline lies between the previous kept element's end and this
/// element's start". Using that gap check keeps the scan O(N) rather than
/// recomputing absolute line numbers per element.
pub fn check_element_line_breaks(
    cx: &Cx<'_>,
    children: &[NodeId],
    ignore_last: bool,
    message: &str,
) {
    if all_on_same_line(cx, children, ignore_last) {
        return;
    }

    let src = cx.source().as_bytes();
    // RuboCop tracks `last_seen_line`, updated only when a child is *kept*
    // (does not start its own line). We track the kept node instead and test
    // the gap to the next child for a newline.
    let mut last_seen_node: Option<NodeId> = None;
    for &child in children {
        let on_prev_line = last_seen_node
            .is_some_and(|prev| !gap_has_newline(src, cx.range(prev).end, cx.range(child).start));
        if on_prev_line {
            let start = cx.range(child).start;
            cx.emit_offense(first_line_range(child, cx), message, None);
            cx.emit_edit(Range { start, end: start }, "\n");
        } else {
            last_seen_node = Some(child);
        }
    }
}

/// RuboCop's `MultilineElementLineBreaks#all_on_same_line?`. `line_of(a) ==
/// line_of(b)` iff no newline lies in `[a, b)`, so this is a single gap scan.
fn all_on_same_line(cx: &Cx<'_>, nodes: &[NodeId], ignore_last: bool) -> bool {
    let (Some(&first), Some(&last)) = (nodes.first(), nodes.last()) else {
        return true;
    };
    let src = cx.source().as_bytes();
    let start = cx.range(first).start;
    // default: first.first_line == last.last_line; ignore_last: == last.first_line.
    let end = if ignore_last {
        cx.range(last).start
    } else {
        cx.range(last).end
    };
    !gap_has_newline(src, start, end)
}

/// Whether the source bytes in `[start, end)` contain a newline. Offsets are
/// clamped to the source length. Used to test "same line" between two
/// positions without recomputing absolute line numbers (O(N²)).
pub fn gap_has_newline(src: &[u8], start: u32, end: u32) -> bool {
    let lo = (start as usize).min(src.len());
    let hi = (end as usize).min(src.len()).max(lo);
    src[lo..hi].contains(&b'\n')
}

/// The opener (`do` keyword or `{`) of a block/numblock/itblock — RuboCop's
/// `BlockNode#loc.begin`. `LocRef::begin` only resolves a `LeftParen`, so this
/// scans the token stream for the first `do`/`{` after the block's call name.
/// `None` for a non-block node or a block with no locatable opener.
pub fn block_opener(node: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let call = match *cx.kind(node) {
        NodeKind::Block { call, .. } => call,
        NodeKind::Numblock { send, .. } | NodeKind::Itblock { send, .. } => send,
        _ => return None,
    };
    // Search from the call's selector end (falling back to its start) so a `{`
    // inside the receiver/arguments cannot be mistaken for the block opener.
    let search_from = cx.node(call).loc.name.end.max(cx.range(call).start);
    let node_end = cx.range(node).end;
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < search_from);
    toks[idx..]
        .iter()
        .take_while(|t| t.range.start < node_end)
        .find(|t| {
            t.kind == SourceTokenKind::LeftBrace
                || (t.kind == SourceTokenKind::Other
                    && &source[t.range.start as usize..t.range.end as usize] == b"do")
        })
        .map(|t| t.range)
}

// Note: is_parenthesized is tested indirectly via the cops that use it:
// - `cops::style::parentheses_around_condition::tests::flags_if_with_paren_condition`
//   verifies `is_parenthesized` returns true for `(x > 10)`.
// - `cops::style::negated_if::tests::flags_modifier_if_with_parenthesized_negation`
//   verifies `is_parenthesized` returns true for `(!x.even?)`.
// - `cops::style::parentheses_around_condition::tests::no_offense_begin_end_condition`
//   verifies `is_parenthesized` returns false for `begin...end`.
