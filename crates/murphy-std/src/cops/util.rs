//! Shared utilities for standard cops.

use murphy_plugin_api::{CommentDirectiveKind, Cx, NodeId, NodeKind, Range, SourceTokenKind};

/// Byte ranges of string/symbol literal *content* nodes (`Str`, `Sym`).
///
/// A structural-looking token — most notably a lone `;` — whose position falls
/// inside one of these ranges is literal text, not Ruby syntax. RuboCop's lexer
/// never emits a `tSEMI` (or other structural token) inside a string, so cops
/// that scan the token stream for `;` must skip these. Interpolation code
/// (`#{ ... }`) lives in non-`Str` child nodes, so a genuine separator inside
/// `#{}` is correctly *not* covered (a `(dstr (str ";") (begin …))` keeps the
/// literal `;` in the `Str` part and the interpolated code in the `begin`).
pub fn string_literal_content_ranges(cx: &Cx<'_>) -> Vec<Range> {
    // Include the root itself: when the whole file is a bare literal (`';'` or
    // `:';'`), the root *is* the `Str`/`Sym` node, which `descendants` omits.
    std::iter::once(cx.root())
        .chain(cx.descendants(cx.root()))
        .filter(|&id| matches!(*cx.kind(id), NodeKind::Str(_) | NodeKind::Sym(_)))
        .map(|id| cx.range(id))
        .collect()
}

/// True when `offset` lies within any half-open range in `ranges`.
pub fn offset_within_any(offset: u32, ranges: &[Range]) -> bool {
    ranges.iter().any(|r| offset >= r.start && offset < r.end)
}

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

/// Display width of a column prefix, mirroring RuboCop's
/// `Alignment#display_column`.
///
/// RuboCop computes alignment columns as
/// `Unicode::DisplayWidth.of(line[0, range.column])` — the East-Asian-width of
/// the substring from the line start up to the target column. A wide (CJK)
/// glyph therefore counts as **2** columns, not 1, so two lines that look
/// vertically aligned in a monospace editor are treated as aligned even when
/// their leading text contains wide characters.
///
/// `prefix` must be the *characters* from the start of the line up to (but not
/// including) the column being measured — exactly what the layout cops obtain
/// with `src[line_start..offset]`. Callers should pass that slice here instead
/// of `chars().count()`.
///
/// Tabs and other zero-width control characters: the Rust `unicode-width` crate
/// reports width `0` for control characters, whereas RuboCop's
/// `Unicode::DisplayWidth.of("\t")` is `1`. To stay faithful to RuboCop we
/// count every control character (anything the crate maps to width `0` that is
/// also a `char::is_control`) as width `1`. Ordinary zero-width combining marks
/// keep their `0` width.
pub fn display_column(prefix: &str) -> usize {
    use unicode_width::UnicodeWidthChar;
    prefix
        .chars()
        .map(|c| UnicodeWidthChar::width(c).unwrap_or_else(|| usize::from(c.is_control())))
        .sum()
}

/// Returns `true` if `byte` is whitespace under Ruby's `\s` / `String#strip`
/// semantics. Unlike Rust's [`u8::is_ascii_whitespace`] (which matches the five
/// bytes `[ \t\n\r\x0C]`), this also matches the vertical tab `\v` (`0x0B`), so
/// blank-line detection in layout cops mirrors RuboCop's `line.strip.empty?` /
/// `blank?` checks faithfully.
#[inline]
pub fn is_ruby_blank_byte(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C)
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
/// non-whitespace character on the nearest preceding or following *content*
/// source line. Mirrors RuboCop's `AllowForAlignment` /
/// `PrecedingFollowingAlignment` vertical-alignment heuristic: extra spacing is
/// treated as intentional alignment when something lines up above or below.
///
/// Like RuboCop's `aligned_with_line?`, blank lines and full-line comments are
/// skipped — the nearest line with real content in each direction is the one
/// compared (so an aligned pair separated by a blank line or a comment block,
/// e.g. successive `let(...)  {` blocks or constant assignments, still counts
/// as aligned).
///
/// Shared by `Layout/ExtraSpacing`, `Layout/SpaceAroundOperators` (operator
/// column) and `Layout/SpaceBeforeFirstArg` (first-argument column).
pub fn is_alignment_at_column(src: &[u8], offset: usize) -> bool {
    let line_start = src[..offset]
        .iter()
        .rposition(|&b| b == b'\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    let col = offset - line_start;

    let non_ws_at_col =
        |line: &[u8]| -> bool { col < line.len() && !is_ruby_blank_byte(line[col]) };
    // A blank line or a full-line comment (first non-blank byte is `#`) is
    // skipped when searching for the line to align against. Blankness uses
    // Ruby's `\s` set (incl. VT/FF) so it matches `is_ruby_blank_byte` and
    // RuboCop's `line.blank?`, not just space/tab/CR.
    let is_skippable = |line: &[u8]| -> bool {
        match line.iter().position(|&b| !is_ruby_blank_byte(b)) {
            None => true,
            Some(i) => line[i] == b'#',
        }
    };

    // Nearest preceding content line.
    let mut end = line_start;
    while end > 0 {
        let prev_end = end - 1; // strip the '\n'
        let prev_start = src[..prev_end]
            .iter()
            .rposition(|&b| b == b'\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        let line = &src[prev_start..prev_end];
        if !is_skippable(line) {
            if non_ws_at_col(line) {
                return true;
            }
            break;
        }
        end = prev_start;
    }

    // Nearest following content line.
    let mut start = src[offset..]
        .iter()
        .position(|&b| b == b'\n')
        .map(|i| offset + i + 1)
        .unwrap_or(src.len());
    while start < src.len() {
        let line_end = src[start..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|i| start + i)
            .unwrap_or(src.len());
        let line = &src[start..line_end];
        if !is_skippable(line) {
            if non_ws_at_col(line) {
                return true;
            }
            break;
        }
        start = if line_end < src.len() {
            line_end + 1
        } else {
            src.len()
        };
    }

    false
}

/// RuboCop's `ASSIGNMENT_OR_COMPARISON_TOKENS` source spellings: operators that
/// end with `=` (assignment, op-assignment, comparison) plus the append `<<`
/// (`tLSHFT`). These are the tokens `aligned_equals_operator?` aligns by their
/// trailing-`=` end column.
fn is_assignment_or_comparison_operator(text: &str) -> bool {
    // `=`, `==`, `===`, `!=`, `<=`, `>=` (the `=`-ending comparison/assignment
    // tokens), every op-assignment `<op>=` (`+=`, `||=`, `<<=`, …), and `<<`.
    if text == "<<" {
        return true;
    }
    // Everything reaching here must end with `=`. `<=>` (the spaceship) ends
    // with `>`, so it was already filtered out. Op-assignments and the
    // `=`-ending comparisons all qualify; a lone `=` (setter / assignment) is
    // the bare-`=` case and qualifies too, matching RuboCop's `tEQL`. The
    // all-punctuation guard excludes a setter-method identifier token like
    // `foo=`, which ends with `=` but is not an operator.
    text.ends_with('=') && text.bytes().all(|b| b.is_ascii_punctuation())
}

/// RuboCop's `aligned_equals_operator?` (the `aligned_token?`/`aligned_operator?`
/// disjunct that `is_alignment_at_column` does not cover): the operator at
/// `op_range` is aligned when its trailing-`=` END column equals the END column
/// of the first assignment/comparison operator on the nearest preceding or
/// following *content* line.
///
/// Faithful to `aligned_with_preceding_equals?`: the operator must itself end
/// with `=` (or be `<<`), and its `last_column` must match the adjacent
/// operator's `last_column`. The adjacent operator is found via the token
/// stream (not a raw `=` byte scan) so an `=` inside a string literal on the
/// adjacent line is not mistaken for an alignment anchor.
///
/// Shared by `Layout/ExtraSpacing` (`aligned_tok`) and
/// `Layout/SpaceAroundOperators` (`is_alignment_spacing`).
pub fn is_equals_aligned(cx: &Cx<'_>, op_range: Range) -> bool {
    let src = cx.source().as_bytes();
    let op_end = op_range.end as usize;
    if op_end == 0 || op_end > src.len() {
        return false;
    }
    let op_text = cx.raw_source(op_range);
    // `range.source[-1] == '=' || range.source == '<<'`.
    if !(op_text.ends_with('=') || op_text == "<<") {
        return false;
    }

    // End column (exclusive) of the operator, i.e. RuboCop's `last_column`.
    let op_line_start = src[..op_range.start as usize]
        .iter()
        .rposition(|&b| b == b'\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    let op_end_col = op_end - op_line_start;

    let is_skippable = |line: &[u8]| -> bool {
        match line.iter().position(|&b| !is_ruby_blank_byte(b)) {
            None => true,
            Some(i) => line[i] == b'#',
        }
    };

    // First assignment/comparison operator token whose end column equals
    // `op_end_col` on `line` (byte range `[line_start, line_end)`), found via the
    // token stream to avoid matching `=` inside string literals.
    let line_has_aligned_op = |line_start: usize, line_end: usize| -> bool {
        let toks = cx.tokens_in(Range {
            start: line_start as u32,
            end: line_end as u32,
        });
        for tok in toks {
            if tok.kind != SourceTokenKind::Other {
                continue;
            }
            let text = cx.raw_source(tok.range);
            if is_assignment_or_comparison_operator(text) {
                // First such token on the line (RuboCop's `detect`): its end
                // column decides alignment.
                let tok_end_col = tok.range.end as usize - line_start;
                return tok_end_col == op_end_col;
            }
        }
        false
    };

    // Nearest preceding content line.
    let mut end = op_line_start;
    while end > 0 {
        let prev_end = end - 1;
        let prev_start = src[..prev_end]
            .iter()
            .rposition(|&b| b == b'\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        let line = &src[prev_start..prev_end];
        if !is_skippable(line) {
            if line_has_aligned_op(prev_start, prev_end) {
                return true;
            }
            break;
        }
        end = prev_start;
    }

    // Nearest following content line.
    let mut start = src[op_end..]
        .iter()
        .position(|&b| b == b'\n')
        .map(|i| op_end + i + 1)
        .unwrap_or(src.len());
    while start < src.len() {
        let line_end = src[start..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|i| start + i)
            .unwrap_or(src.len());
        let line = &src[start..line_end];
        if !is_skippable(line) {
            if line_has_aligned_op(start, line_end) {
                return true;
            }
            break;
        }
        start = if line_end < src.len() {
            line_end + 1
        } else {
            src.len()
        };
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
    bytes[start..end].iter().all(|&b| is_ruby_blank_byte(b))
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
    // Start the scan after the call's last argument so a `{` inside the
    // arguments (e.g. `foo({ a: 1 }) { ... }`) is not mistaken for the block
    // opener. Falling back to the selector end covers the no-argument case.
    // (`cx.range(call).end` is unusable here: a call node's range spans its
    // attached block, so it would skip past the opener entirely.)
    //
    // Floor the start at the block's own range start: a stabby-lambda block
    // (`->(x) { … }`) has a `Lambda` marker call whose name loc is `{0,0}`, so
    // the bare fallback would scan from byte 0 and latch onto an *enclosing*
    // block's `do`/`{` (murphy-un83). The opener always lies within the block,
    // so the node start is a safe lower bound that never crosses into a
    // sibling/parent block.
    let search_from = cx
        .call_arguments(call)
        .last()
        .map(|&arg| cx.range(arg).end)
        .unwrap_or(cx.node(call).loc.name.end)
        .max(cx.range(node).start);
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

/// RuboCop's `BlockNode#single_line?` — `loc.begin.line == loc.end.line`.
///
/// `Cx::is_single_line` measures the node's *whole* expression range, which for
/// a block whose receiver is a multi-line method chain
/// (`a\n  .b\n  .c { |x| x }`) spans the entire chain and reads as multi-line.
/// RuboCop overrides `single_line?` for blocks to compare only the opener
/// (`do`/`{`) line with the closing delimiter (`end`/`}`) line, so a one-line
/// `{ … }` at a multi-line chain tail is correctly single-line. Falls back to
/// `Cx::is_single_line` when the opener cannot be located.
pub fn block_is_single_line(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(opener) = block_opener(node, cx) else {
        return cx.is_single_line(node);
    };
    // The closing delimiter ends at the block's expression end; `end - 1` lands
    // inside it (on its line). "Same line as the opener" ⇔ no intervening newline.
    let close = cx.range(node).end.saturating_sub(1).max(opener.start);
    !gap_has_newline(cx.source().as_bytes(), opener.start, close)
}

/// Returns true if `cond` contains a local-variable assignment anywhere in its
/// subtree (the node itself or any descendant).
///
/// Mirrors RuboCop's `StatementModifier#parenthesized_lvasgn?` precondition
/// `condition.each_node.any?(&:lvasgn_type?)`: a modifier conversion of an
/// `if`/`unless`/`while`/`until` whose condition assigns a local variable
/// (e.g. `if (batch = next_batch)`) is suppressed, because the assignment is
/// commonly intentional and the modifier form reads worse.
///
/// `each_node` includes the receiver node itself, so this checks `cond`
/// directly *and* its descendants — a bare `if x = foo` has `cond` as a lone
/// `Lvasgn` with no `begin` wrapper, which a descendants-only walk would miss.
pub fn condition_contains_lvasgn(cond: NodeId, cx: &Cx<'_>) -> bool {
    if matches!(cx.kind(cond), NodeKind::Lvasgn { .. }) {
        return true;
    }
    cx.descendants(cond)
        .iter()
        .any(|&n| matches!(cx.kind(n), NodeKind::Lvasgn { .. }))
}

/// Count physical lines in `node`'s source that are not blank (whitespace-only).
///
/// Mirrors RuboCop's `nonempty_line_count` — `source.lines.grep_v(/\A\s*\z/).size`
/// — used by the `StatementModifier` mixin to exempt nodes spanning more than 3
/// nonempty physical lines from `Style/IfUnlessModifier` /
/// `Style/WhileUntilModifier`.
pub fn nonempty_line_count(node: NodeId, cx: &Cx<'_>) -> usize {
    cx.raw_source(cx.range(node))
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count()
}

// Note: is_parenthesized is tested indirectly via the cops that use it:
// - `cops::style::parentheses_around_condition::tests::flags_if_with_paren_condition`
//   verifies `is_parenthesized` returns true for `(x > 10)`.
// - `cops::style::negated_if::tests::flags_modifier_if_with_parenthesized_negation`
//   verifies `is_parenthesized` returns true for `(!x.even?)`.
// - `cops::style::parentheses_around_condition::tests::no_offense_begin_end_condition`
//   verifies `is_parenthesized` returns false for `begin...end`.

// --- Layout/EmptyLinesAround*Body shared helpers (no_empty_lines style) ---

/// Returns the 0-based byte range of the physical line that contains
/// `offset`, *excluding* the trailing `\n` (and a `\r` before it). Returns
/// the whole-line span `[line_start, content_end)`.
fn line_span_at(source: &[u8], offset: usize) -> (usize, usize) {
    let line_start = source[..offset]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |pos| pos + 1);
    let mut content_end = source[offset..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(source.len(), |pos| offset + pos);
    if content_end > line_start && source[content_end - 1] == b'\r' {
        content_end -= 1;
    }
    (line_start, content_end)
}

/// Returns the start offset of the line *following* the line that contains
/// `offset`, or `None` if `offset` is on the last line of the source.
fn next_line_start(source: &[u8], offset: usize) -> Option<usize> {
    source[offset..]
        .iter()
        .position(|&b| b == b'\n')
        .map(|pos| offset + pos + 1)
        .filter(|&start| start < source.len())
}

/// Returns the start offset of the line *preceding* the line that contains
/// `offset`, or `None` if `offset` is on the first line of the source.
fn prev_line_start(source: &[u8], offset: usize) -> Option<usize> {
    let line_start = source[..offset]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |pos| pos + 1);
    if line_start == 0 {
        return None;
    }
    // The byte before `line_start - 1` is the `\n` that ends the previous line.
    Some(
        source[..line_start - 1]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |pos| pos + 1),
    )
}

/// Implements RuboCop's `EmptyLinesAroundBody` mixin for the default
/// `no_empty_lines` `EnforcedStyle`: flags (and removes) a blank line at the
/// beginning and/or end of a multi-line body.
///
/// - `header_anchor`: a byte offset somewhere on the construct's *header*
///   line (RuboCop's `adjusted_first_line`). For a class with a superclass
///   this is the superclass's last line; for a block it is the send node's
///   last line; otherwise it is the node's own start.
/// - `kind`: the `KIND` string used in the message (`class` / `block` /
///   `begin`).
///
/// RuboCop's `&:empty?` predicate treats a line as blank only when it is
/// *literally* empty after stripping the newline — a whitespace-only line is
/// NOT blank. This matches that exactly (no `.trim()`).
pub fn check_empty_lines_around_body_no_empty_lines(
    node: NodeId,
    header_anchor: u32,
    kind: &str,
    cx: &Cx<'_>,
) {
    // `return if node.single_line?`
    if cx.is_single_line(node) {
        return;
    }

    let source = cx.source().as_bytes();
    let node_range = cx.range(node);
    let header_anchor = header_anchor as usize;

    // Beginning candidate: the line immediately after the header line
    // (RuboCop's `processed_source.lines[first_line]`).
    let begin_candidate = next_line_start(source, header_anchor);

    // Ending candidate: the line immediately before the `end` line
    // (RuboCop's `processed_source.lines[last_line - 2]`). The `end` line is
    // the line containing the last byte of the node.
    let end_byte = (node_range.end as usize).saturating_sub(1);
    let end_candidate = prev_line_start(source, end_byte);

    // Track which line we've already emitted an edit for, so a construct
    // whose single inner line is both the beginning and ending candidate
    // (e.g. `class Foo\n\nend`) does not produce overlapping edits.
    let mut removed_line_start: Option<usize> = None;

    let mut handle = |line_start: usize, location: &str| {
        let (start, content_end) = line_span_at(source, line_start);
        // `&:empty?` — literally empty (no whitespace-only allowance).
        if start != content_end {
            return;
        }
        let line_end = if content_end < source.len() {
            content_end + 1
        } else {
            content_end
        };
        let range = Range {
            start: start as u32,
            end: line_end as u32,
        };
        let message = format!("Extra empty line detected at {kind} body {location}.");
        cx.emit_offense(range, &message, None);
        if removed_line_start != Some(start) {
            cx.emit_edit(range, "");
            removed_line_start = Some(start);
        }
    };

    if let Some(begin_line) = begin_candidate {
        handle(begin_line, "beginning");
    }
    if let Some(end_line) = end_candidate {
        handle(end_line, "end");
    }
}

// --- Layout/EmptyLinesAround{Module,Method}Body shared helpers (blank-run style) ---
// NOTE: `check_empty_lines_around_body_blank_run` is a parallel-developed variant of
// `check_empty_lines_around_body_no_empty_lines` above; they differ in signature and
// autocorrect granularity (single line vs whole blank run). See bd issue for unification.

/// Byte-offset boundaries of a physical source line, plus whether the line
/// (excluding its trailing `\n`) is blank (only ASCII whitespace).
#[derive(Clone, Copy)]
pub struct PhysicalLine {
    /// Byte offset of the first character of the line.
    pub start: u32,
    /// Byte offset just past the line's terminating `\n` (or EOF).
    pub end: u32,
    /// `true` when the line contains only whitespace (RuboCop's `line.empty?`
    /// where `lines` are already `chomp`ed, so a line of spaces is *not*
    /// empty — but RuboCop strips the final `\n`; a line that is just `\n`
    /// becomes `""` which is empty. We treat a line containing only the
    /// newline as blank, matching RuboCop, and a whitespace-only line as
    /// non-blank to match `String#empty?`).
    pub blank: bool,
}

/// Split `source` into physical lines with byte boundaries. The returned
/// vector is 0-indexed; `lines[i]` is the (i+1)-th physical line.
///
/// A line's `blank` flag is `true` only when the line is exactly empty after
/// removing its trailing `\n` — i.e. RuboCop's `processed_source.lines[i]`
/// (which is `String#chomp`-ed) returns `""` and `"".empty?` is `true`.
/// A line of spaces is therefore *not* blank, matching RuboCop's `&:empty?`.
pub fn physical_lines(source: &str) -> Vec<PhysicalLine> {
    let bytes = source.as_bytes();
    let mut lines = Vec::new();
    let mut start = 0usize;
    while start <= bytes.len() {
        // EOF with no trailing newline already consumed.
        if start == bytes.len() {
            // A trailing empty "line" only exists if the source is empty or
            // ended exactly on a `\n` (handled by the loop's end condition).
            break;
        }
        let nl = bytes[start..].iter().position(|&b| b == b'\n');
        let (content_end, next_start) = match nl {
            Some(i) => (start + i, start + i + 1),
            None => (bytes.len(), bytes.len()),
        };
        // Normalize a CRLF terminator: `"\r\n"` leaves `content_end` one byte
        // past `start`, so strip a trailing `\r` before the blank test —
        // otherwise a visually empty CRLF line is mis-classified as non-blank.
        let content_end = if content_end > start && bytes[content_end - 1] == b'\r' {
            content_end - 1
        } else {
            content_end
        };
        let blank = content_end == start; // empty after chomp / CRLF-normalized
        lines.push(PhysicalLine {
            start: start as u32,
            end: next_start as u32,
            blank,
        });
        if nl.is_none() {
            break;
        }
        start = next_start;
    }
    lines
}

/// Shared port of RuboCop's `EmptyLinesAroundBody` mixin for the
/// `no_empty_lines` style, used by `Layout/EmptyLinesAroundModuleBody` and
/// `Layout/EmptyLinesAroundMethodBody`.
///
/// `first_line` / `last_line` are 1-based physical source line numbers of the
/// construct (for method bodies, `first_line` is the *args' last line*, the
/// `adjusted_first_line`). `kind` is the message noun (`"module"` /
/// `"method"`).
///
/// Behaviour (mirrors the mixin's `check`/`check_both`/`check_source`):
/// - `node.single_line?` → return (no offense). We treat `first_line ==
///   last_line` as single-line.
/// - **beginning**: the line at 0-index `first_line` (the line right after the
///   opener). If blank → "Extra empty line detected at <kind> body beginning."
/// - **end**: the line at 0-index `last_line - 2` (the line right before
///   `end`). If blank → "Extra empty line detected at <kind> body end."
///
/// Each boundary fires independently, so `module Foo\n\nend` emits two
/// offenses — exactly matching RuboCop. The autocorrect removes the full run
/// of consecutive blank lines at the boundary; when the two boundaries
/// resolve to the same blank-line run (the nil-body case) only one edit is
/// emitted to keep the edits non-overlapping.
pub fn check_empty_lines_around_body_blank_run(
    cx: &Cx<'_>,
    kind: &str,
    first_line: usize,
    last_line: usize,
) {
    // `return if node.single_line?`
    if first_line >= last_line {
        return;
    }

    let lines = physical_lines(cx.source());

    // RuboCop indexes `processed_source.lines` 0-based; `lines[first_line]` is
    // the line after the 1-based `first_line`, and `lines[last_line - 2]` is
    // the line before the 1-based `last_line`.
    let begin_idx = first_line; // 0-based index of the line after the opener
    let end_idx = last_line.checked_sub(2); // 0-based index of the line before `end`

    // Track the blank-run byte range already scheduled for removal so the two
    // boundaries do not emit overlapping edits when they target the same run.
    let mut emitted_edit: Option<Range> = None;

    if let Some(&line) = lines.get(begin_idx)
        && line.blank
    {
        let range = blank_run_range(&lines, begin_idx, BlankRunDirection::Down);
        cx.emit_offense(
            Range {
                start: line.start,
                end: line.end,
            },
            &format!("Extra empty line detected at {kind} body beginning."),
            None,
        );
        cx.emit_edit(range, "");
        emitted_edit = Some(range);
    }

    if let Some(end_idx) = end_idx
        && let Some(&line) = lines.get(end_idx)
        && line.blank
    {
        let range = blank_run_range(&lines, end_idx, BlankRunDirection::Up);
        cx.emit_offense(
            Range {
                start: line.start,
                end: line.end,
            },
            &format!("Extra empty line detected at {kind} body end."),
            None,
        );
        // Skip a duplicate/overlapping edit when the end boundary's
        // blank run is the same run already removed at the beginning.
        let overlaps = emitted_edit.is_some_and(|e| range.start < e.end && e.start < range.end);
        if !overlaps {
            cx.emit_edit(range, "");
        }
    }
}

#[derive(Clone, Copy)]
enum BlankRunDirection {
    Down,
    Up,
}

/// The byte range covering the maximal run of consecutive blank lines that
/// includes `idx`, scanning down (toward EOF) or up (toward BOF). Used as the
/// autocorrect removal range so all blank lines at a body boundary are removed
/// in one edit.
fn blank_run_range(lines: &[PhysicalLine], idx: usize, dir: BlankRunDirection) -> Range {
    let mut lo = idx;
    let mut hi = idx;
    match dir {
        BlankRunDirection::Down => {
            while hi + 1 < lines.len() && lines[hi + 1].blank {
                hi += 1;
            }
        }
        BlankRunDirection::Up => {
            while lo > 0 && lines[lo - 1].blank {
                lo -= 1;
            }
        }
    }
    Range {
        start: lines[lo].start,
        end: lines[hi].end,
    }
}

// --- Metrics code-length counting (RuboCop `Metrics::Utils::CodeLengthCalculator`) ---

/// Foldable construct types for `CountAsOne` (RuboCop `FOLDABLE_TYPES`).
/// Each variant, when enabled, collapses a top-level descendant of that kind
/// to a single counted line.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FoldableType {
    /// `array` — array literals.
    Array,
    /// `hash` — hash literals.
    Hash,
    /// `heredoc` — `str`/`dstr` nodes that are heredocs.
    Heredoc,
    /// `method_call` — `send`/`csend` call nodes.
    MethodCall,
}

impl FoldableType {
    /// Parse a `CountAsOne` config string (`"array"`, `"hash"`, `"heredoc"`,
    /// `"method_call"`). Unknown strings are ignored (RuboCop raises a warning;
    /// Murphy silently drops them — the offending value simply does not fold).
    pub fn from_config(name: &str) -> Option<Self> {
        match name {
            "array" => Some(Self::Array),
            "hash" => Some(Self::Hash),
            "heredoc" => Some(Self::Heredoc),
            "method_call" => Some(Self::MethodCall),
            _ => None,
        }
    }
}

/// Parse a `CountAsOne` string list into the deduplicated foldable-type set.
pub fn parse_foldable_types(names: &[String]) -> Vec<FoldableType> {
    let mut out: Vec<FoldableType> = Vec::new();
    for name in names {
        if let Some(ty) = FoldableType::from_config(name)
            && !out.contains(&ty)
        {
            out.push(ty);
        }
    }
    out
}

/// `true` when 0-based source `line` shall not be counted: blank, or (when
/// `count_comments` is false) a comment line. Mirrors RuboCop's
/// `irrelevant_line?`.
fn irrelevant_line(cx: &Cx<'_>, line: u32, count_comments: bool) -> bool {
    line_is_blank(cx, line) || (!count_comments && line_is_comment(cx, line))
}

/// Returns `true` when `node` is a heredoc string (`str`/`dstr` whose opening
/// delimiter is `<<` — RuboCop's `heredoc?`). Detected from the source bytes at
/// the node's start, since a heredoc literal always begins with `<<`.
fn is_heredoc_node(node: NodeId, cx: &Cx<'_>) -> bool {
    if !matches!(*cx.kind(node), NodeKind::Str(_) | NodeKind::Dstr(..)) {
        return false;
    }
    let start = cx.range(node).start as usize;
    let bytes = cx.source().as_bytes();
    bytes.get(start..start + 2) == Some(b"<<")
}

/// The 0-based source line of the `HeredocEnd` terminator for the heredoc whose
/// `HeredocStart` opener begins at byte `opener_start`, or `None` when no
/// heredoc opens there. Pairs openers to terminators FIFO via a stack, so
/// stacked sibling heredocs on one line (`foo(<<~A, <<~B)`) resolve correctly.
///
/// KNOWN GAP (murphy-e7bz.71): FIFO pairing is wrong for *nested interpolated*
/// heredocs (an OUTER heredoc whose body contains `#{<<~INNER}`), which close
/// LIFO — the OUTER opener is mispaired with the INNER terminator. This only
/// affects [`heredoc_length`] (the non-default `CountAsOne: [heredoc]` fold);
/// [`node_line_span`] is unaffected because it takes the max end-line over all
/// in-range openers, so the correct furthest terminator is found regardless of
/// pairing (default config matches rubocop).
fn heredoc_end_line_of_opener(opener_start: u32, cx: &Cx<'_>) -> Option<u32> {
    let mut opener_stack: Vec<u32> = Vec::new(); // opener byte starts, FIFO order
    for tok in cx.sorted_tokens() {
        match tok.kind {
            SourceTokenKind::HeredocStart => opener_stack.push(tok.range.start),
            SourceTokenKind::HeredocEnd => {
                if opener_stack.is_empty() {
                    continue;
                }
                // FIFO: the earliest-opened heredoc terminates first.
                let opener = opener_stack.remove(0);
                if opener == opener_start {
                    return Some(line_of(tok.range.start, cx));
                }
            }
            _ => {}
        }
    }
    None
}

/// The 0-based first and (heredoc-extended) last source line of a node's byte
/// range. For a node containing a trailing heredoc whose AST range stops at the
/// `<<~LABEL` opener, the last line is extended to the matching `HeredocEnd`
/// token's line so the heredoc body is counted (RuboCop's
/// `source_from_node_with_heredoc`). Robust whether or not Prism's range
/// already reaches the terminator.
fn node_line_span(node: NodeId, cx: &Cx<'_>) -> (u32, u32) {
    let range = cx.range(node);
    let first = line_of(range.start, cx);
    // `range.end` is one-past-the-last byte; the last *content* byte is
    // `range.end - 1`. Empty ranges collapse to a single line.
    let last_byte = range.end.saturating_sub(1).max(range.start);
    let mut last = line_of(last_byte, cx);

    // Extend through heredoc bodies whose opener lies within the node range.
    // RuboCop keys on the heredoc opener position, not the terminator, because
    // a trailing heredoc's terminator can sit past the node's AST range.
    for tok in cx.sorted_tokens() {
        if tok.kind == SourceTokenKind::HeredocStart
            && tok.range.start >= range.start
            && tok.range.start < range.end
            && let Some(end_line) = heredoc_end_line_of_opener(tok.range.start, cx)
        {
            last = last.max(end_line);
        }
    }
    (first, last.max(first))
}

/// Count non-irrelevant lines in a node's (heredoc-extended) line span.
fn count_lines(node: NodeId, count_comments: bool, cx: &Cx<'_>) -> i64 {
    let (first, last) = node_line_span(node, cx);
    (first..=last)
        .filter(|&line| !irrelevant_line(cx, line, count_comments))
        .count() as i64
}

/// RuboCop `CodeLengthCalculator#heredoc_length`: count non-irrelevant lines of
/// the heredoc *body* and add 2 for the opening and closing delimiter lines.
fn heredoc_length(node: NodeId, count_comments: bool, cx: &Cx<'_>) -> i64 {
    let range = cx.range(node);
    let opener_line = line_of(range.start, cx);
    let end_line = heredoc_end_line_of_opener(range.start, cx).unwrap_or(opener_line);
    let body_count = if end_line > opener_line + 1 {
        (opener_line + 1..end_line)
            .filter(|&line| !irrelevant_line(cx, line, count_comments))
            .count() as i64
    } else {
        0
    };
    body_count + 2
}

/// RuboCop `CodeLengthCalculator#code_length` for a non-classlike node: count
/// non-irrelevant lines of the node's source span, with heredoc nodes counted
/// via `heredoc_length`.
fn code_length(node: NodeId, count_comments: bool, cx: &Cx<'_>) -> i64 {
    if is_heredoc_node(node, cx) {
        heredoc_length(node, count_comments, cx)
    } else {
        count_lines(node, count_comments, cx)
    }
}

/// Does `node`'s kind match an enabled foldable type? (RuboCop `foldable_node?`.)
fn foldable_node(node: NodeId, types: &[FoldableType], cx: &Cx<'_>) -> bool {
    types.iter().any(|ty| match ty {
        FoldableType::Array => matches!(*cx.kind(node), NodeKind::Array(_)),
        FoldableType::Hash => matches!(*cx.kind(node), NodeKind::Hash(_)),
        FoldableType::Heredoc => is_heredoc_node(node, cx),
        FoldableType::MethodCall => {
            matches!(
                *cx.kind(node),
                NodeKind::Send { .. } | NodeKind::Csend { .. }
            )
        }
    })
}

/// `true` when a node kind participates in the normalized foldable-descendant
/// walk (the kinds `each_top_level_descendant` stops at). Mirrors RuboCop's
/// `normalize_foldable_types`: heredoc → `str`/`dstr`, method_call →
/// `send`/`csend`.
fn matches_normalized_foldable(node: NodeId, types: &[FoldableType], cx: &Cx<'_>) -> bool {
    types.iter().any(|ty| match ty {
        FoldableType::Array => matches!(*cx.kind(node), NodeKind::Array(_)),
        FoldableType::Hash => matches!(*cx.kind(node), NodeKind::Hash(_)),
        FoldableType::Heredoc => {
            matches!(*cx.kind(node), NodeKind::Str(_) | NodeKind::Dstr(..))
        }
        FoldableType::MethodCall => {
            matches!(
                *cx.kind(node),
                NodeKind::Send { .. } | NodeKind::Csend { .. }
            )
        }
    })
}

/// `true` for a classlike node (`class`/`module`) — skipped by the
/// top-level-descendant walk (RuboCop `classlike_node?`).
fn is_classlike(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        *cx.kind(node),
        NodeKind::Class { .. } | NodeKind::Module { .. }
    )
}

/// RuboCop `each_top_level_descendant`: walk children, stopping (yielding) at
/// the first descendant matching a normalized foldable type, never recursing
/// into a matched node or into a classlike node. Collected nodes are then
/// re-filtered by `foldable_node` (so a plain non-heredoc string halts the
/// recursion but is not folded).
fn collect_top_level_foldables(
    node: NodeId,
    types: &[FoldableType],
    cx: &Cx<'_>,
    out: &mut Vec<NodeId>,
) {
    for child in cx.children(node) {
        if is_classlike(child, cx) {
            continue;
        }
        if matches_normalized_foldable(child, types, cx) {
            out.push(child);
        } else {
            collect_top_level_foldables(child, types, cx, out);
        }
    }
}

/// RuboCop `Metrics::Utils::CodeLengthCalculator#calculate` for a method/block
/// **body** node: count non-irrelevant lines, then fold each enabled
/// `CountAsOne` construct to a single line.
///
/// `body` is the def/block body node (RuboCop's `extract_body(node)`); pass
/// the body, not the enclosing `def`. Returns the code-line count used for the
/// `[length/max]` message. Reusable by `ClassLength`/`ModuleLength`/
/// `BlockLength` (each extracts its own body / classlike span).
pub fn body_code_length(
    body: NodeId,
    count_comments: bool,
    foldable_types: &[FoldableType],
    cx: &Cx<'_>,
) -> i64 {
    let mut length = code_length(body, count_comments, cx);
    if foldable_types.is_empty() {
        return length;
    }

    // RuboCop's `each_top_level_descendant` is seeded with the def/block
    // *node*, whose direct child is the body. So the body node itself is a
    // top-level fold candidate when it is a foldable kind (e.g. the whole body
    // is a single multiline `foo(...)` call). Mirror that by checking the body
    // first; only when it is not itself foldable do we recurse into its
    // children.
    let mut descendants = Vec::new();
    if matches_normalized_foldable(body, foldable_types, cx) {
        descendants.push(body);
    } else {
        collect_top_level_foldables(body, foldable_types, cx, &mut descendants);
    }
    for descendant in descendants {
        if !foldable_node(descendant, foldable_types, cx) {
            continue;
        }
        let descendant_length = code_length(descendant, count_comments, cx);
        length = length - descendant_length + 1;
    }
    length
}

/// RuboCop `CodeLengthCalculator#classlike_code_length` for a `class`/`module`
/// node — the path taken by `code_length` when `classlike_node?(node)` is true
/// (`Metrics/ClassLength`'s `on_class`/`on_sclass` and `Metrics/ModuleLength`'s
/// `on_module`). This is **not** the `extract_body` path used by
/// [`body_code_length`]; pass the whole class/module node, not its body.
///
/// Mirrors RuboCop exactly:
///
/// 1. `namespace_module?` — when the node's sole body is itself a `class`/
///    `module`, the length is `0` (a pure namespace wrapper is not measured).
/// 2. Base count = the line numbers strictly between the first (`class Foo` /
///    `module Foo`) and last (`end`) line — `line_range(node).to_a[1...-1]` —
///    minus every line covered by an inner `class`/`module` descendant
///    (`line_numbers_of_inner_nodes(node, :module, :class)`), then dropping
///    blank/comment lines (`irrelevant_line?`).
/// 3. `CountAsOne` folding via `each_top_level_descendant` seeded with the whole
///    class/module node: `length = length - code_length(descendant) + 1` per
///    enabled foldable.
///
/// Like [`body_code_length`], the `omit_length` unbraced-hash subtraction is not
/// applied (the same documented fold gap).
pub fn classlike_code_length(
    node: NodeId,
    count_comments: bool,
    foldable_types: &[FoldableType],
    cx: &Cx<'_>,
) -> i64 {
    // `namespace_module?(node)` — body is a single class/module → length 0.
    if is_namespace_module(node, cx) {
        return 0;
    }

    let range = cx.range(node);
    let first = line_of(range.start, cx);
    let last_byte = range.end.saturating_sub(1).max(range.start);
    let last = line_of(last_byte, cx).max(first);

    // The classlike body span is the line numbers strictly between the header
    // line and the `end` line (`line_range(node).to_a[1...-1]`). A single-line
    // class/module (`first == last`, or no interior line) has length 0.
    if last <= first + 1 {
        // No interior body lines; folds cannot apply either. Length is 0.
        return 0;
    }

    let inner_lines = inner_classlike_lines(node, cx);

    let mut length: i64 = (first + 1..last)
        .filter(|line| !inner_lines.contains(line))
        .filter(|&line| !irrelevant_line(cx, line, count_comments))
        .count() as i64;

    if foldable_types.is_empty() {
        return length;
    }

    // `each_top_level_descendant(@node, …)` is seeded with the whole class/
    // module node, halting at (and never recursing into) inner classlike nodes.
    let mut descendants = Vec::new();
    collect_top_level_foldables(node, foldable_types, cx, &mut descendants);
    for descendant in descendants {
        if !foldable_node(descendant, foldable_types, cx) {
            continue;
        }
        let descendant_length = code_length(descendant, count_comments, cx);
        length = length - descendant_length + 1;
    }
    length
}

/// RuboCop `namespace_module?(node)` — `classlike_node?(node.body)`: the class/
/// module's body is itself a single `class`/`module` node.
fn is_namespace_module(node: NodeId, cx: &Cx<'_>) -> bool {
    let body = match *cx.kind(node) {
        NodeKind::Class { body, .. } | NodeKind::Module { body, .. } => body.get(),
        _ => None,
    };
    body.is_some_and(|b| is_classlike(b, cx))
}

/// RuboCop `line_numbers_of_inner_nodes(node, :module, :class)`: the set of
/// 0-based source lines covered by every inner `class`/`module` descendant's
/// full line range (`Sclass` is deliberately excluded — RuboCop passes only
/// `:module`/`:class`).
fn inner_classlike_lines(node: NodeId, cx: &Cx<'_>) -> std::collections::HashSet<u32> {
    let mut lines = std::collections::HashSet::new();
    for descendant in cx.descendants(node) {
        if !matches!(
            *cx.kind(descendant),
            NodeKind::Class { .. } | NodeKind::Module { .. }
        ) {
            continue;
        }
        let range = cx.range(descendant);
        let first = line_of(range.start, cx);
        let last_byte = range.end.saturating_sub(1).max(range.start);
        let last = line_of(last_byte, cx).max(first);
        for line in first..=last {
            lines.insert(line);
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::{display_column, is_assignment_or_comparison_operator};

    #[test]
    fn display_column_matches_rubocop_unicode_display_width() {
        // ASCII: width == scalar count.
        assert_eq!(display_column(""), 0);
        assert_eq!(display_column("  "), 2);
        assert_eq!(display_column("abc"), 3);
        // East-Asian wide glyph counts as 2 (RuboCop's `Unicode::DisplayWidth.of`).
        assert_eq!(display_column("あ"), 2);
        assert_eq!(display_column("  あ"), 4);
        // Half-width katakana stays width 1, matching the gem.
        assert_eq!(display_column("ｱ"), 1);
        // Tabs count as 1 each, matching `Unicode::DisplayWidth.of("\t")` == 1
        // (the raw unicode-width crate reports 0 for control chars).
        assert_eq!(display_column("\t\t"), 2);
    }

    #[test]
    fn operator_classifier_accepts_operators_and_rejects_setter_identifiers() {
        // `=`-ending operators and `<<` qualify (RuboCop's `aligned_equals_operator?`).
        for op in [
            "=", "==", "===", "!=", "<=", ">=", "+=", "-=", "*=", "||=", "&&=", "<<=", "<<",
        ] {
            assert!(
                is_assignment_or_comparison_operator(op),
                "{op} should qualify"
            );
        }
        // A setter-method identifier token also ends with `=` but is NOT an
        // operator — it must be rejected so it is not treated as alignment.
        for ident in ["foo=", "bar=", "value="] {
            assert!(
                !is_assignment_or_comparison_operator(ident),
                "{ident} (setter identifier) must not qualify"
            );
        }
        // The spaceship does not end with `=` and never reaches the guard.
        assert!(!is_assignment_or_comparison_operator("<=>"));
    }
}
