//! Shared utilities for standard cops.

use murphy_plugin_api::{CommentDirectiveKind, Cx, NodeId, NodeKind, Range, SourceTokenKind};
use std::collections::{HashMap, HashSet};

/// True when `node` reads `expected`, including an implicit Ruby 3.4 `it`
/// parameter represented as a receiverless `send :it` inside an `Itblock`.
///
/// Callers should use this for a known block-parameter read, not as a general
/// equivalence between local variables and method calls. Explicit `it()` calls
/// and ordinary method calls named `it` do not match.
pub fn is_block_parameter_read(node: NodeId, expected: &str, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Lvar(sym) => cx.symbol_str(sym) == expected,
        NodeKind::Send {
            receiver,
            method,
            args,
        } => {
            expected == "it"
                && receiver.get().is_none()
                && cx.symbol_str(method) == "it"
                && cx.list(args).is_empty()
                && cx.raw_source(cx.range(node)) == "it"
                && is_itblock_body_read(node, cx)
        }
        _ => false,
    }
}

/// True if `node` is in an Itblock's lexical body. Nested blocks may read an
/// enclosing `it` parameter, so keep walking through their bodies. Stop at
/// method/class scopes, which do not capture block locals. A block's call child
/// is outside its own parameter scope and may still belong to an outer block.
fn is_itblock_body_read(node: NodeId, cx: &Cx<'_>) -> bool {
    let mut child = node;
    while let Some(parent) = cx.parent(child).get() {
        match *cx.kind(parent) {
            NodeKind::Itblock { send, body } => {
                if body.get() == Some(child) {
                    return true;
                }
                if send != child {
                    return false;
                }
            }
            NodeKind::Def { .. }
            | NodeKind::Defs { .. }
            | NodeKind::Class { .. }
            | NodeKind::Module { .. }
            | NodeKind::Sclass { .. } => return false,
            _ => {}
        }
        child = parent;
    }
    false
}

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

/// Per-file index for mapping byte offsets to 1-based source lines.
///
/// Building the index is O(N) in source size; each lookup is O(log L), where
/// L is the number of line breaks. This avoids rescanning the source prefix for
/// every node or comment in cops that make many line-number queries.
pub struct SourceLineIndex {
    newline_offsets: Vec<usize>,
}

impl SourceLineIndex {
    /// Index every newline byte once. Offsets are bytes, matching Prism ranges.
    pub fn new(source: &str) -> Self {
        let newline_offsets = source
            .as_bytes()
            .iter()
            .enumerate()
            .filter_map(|(offset, &byte)| (byte == b'\n').then_some(offset))
            .collect();
        Self { newline_offsets }
    }

    /// Return the 1-based line containing `offset`.
    pub fn line_of(&self, offset: u32) -> usize {
        self.preceding_newlines(offset) + 1
    }

    /// Return the byte offset of the start of the line containing `offset`.
    pub fn line_start(&self, offset: u32) -> usize {
        let preceding = self.preceding_newlines(offset);
        if preceding == 0 {
            0
        } else {
            self.newline_offsets[preceding - 1] + 1
        }
    }

    fn preceding_newlines(&self, offset: u32) -> usize {
        self.newline_offsets
            .partition_point(|&newline| newline < offset as usize)
    }
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

// --- Layout/EmptyLinesAroundBody shared helper -------------------------------

/// Boundary behavior used by RuboCop's `EmptyLinesAroundBody` mixin.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EmptyLinesAroundBodyStyle {
    /// Disallow a blank line at the body boundary.
    NoEmptyLines,
    /// Require a blank line at the body boundary.
    EmptyLines,
}

/// Byte-offset boundaries of a physical source line, plus whether the line is
/// empty after removing its line terminator. Whitespace-only lines are not
/// empty, matching RuboCop's `String#empty?` checks.
#[derive(Clone, Copy)]
pub struct PhysicalLine {
    /// Byte offset of the first character of the line.
    pub start: u32,
    /// Byte offset just past the line's terminating `\n` (or EOF).
    pub end: u32,
    /// True only when the line has no content after its line terminator is
    /// removed. CRLF is treated as one line terminator.
    pub blank: bool,
}

/// Split `source` into physical lines. The returned vector is 0-indexed.
/// A line containing spaces is not blank, matching RuboCop's `&:empty?`.
pub fn physical_lines(source: &str) -> Vec<PhysicalLine> {
    let bytes = source.as_bytes();
    let mut lines = Vec::new();
    let mut start = 0usize;
    while start < bytes.len() {
        let nl = bytes[start..].iter().position(|&b| b == b'\n');
        let (line_end, content_end) = match nl {
            Some(i) => (start + i + 1, start + i),
            None => (bytes.len(), bytes.len()),
        };
        // Normalize CRLF for the blank-line predicate. `line_end` retains the
        // full source span so corrections can remove exactly one line-ending
        // byte per RuboCop autocorrect pass (the leading `\r` for CRLF).
        let content_end = if content_end > start && bytes[content_end - 1] == b'\r' {
            content_end - 1
        } else {
            content_end
        };
        lines.push(PhysicalLine {
            start: start as u32,
            end: line_end as u32,
            blank: content_end == start,
        });
        if nl.is_none() {
            break;
        }
        start = line_end;
    }
    lines
}

/// Check both body boundaries using RuboCop's one-line-per-boundary behavior.
/// `first_line` and `last_line` are 1-based physical source lines. The styles
/// can differ for `Layout/EmptyLinesAroundModuleBody`'s special styles.
pub fn check_empty_lines_around_body(
    cx: &Cx<'_>,
    kind: &str,
    first_line: usize,
    last_line: usize,
    beginning_style: EmptyLinesAroundBodyStyle,
    ending_style: EmptyLinesAroundBodyStyle,
) {
    // `node.single_line?` (or, for adjusted method headers, no line remains
    // between the header and the closing `end`).
    if first_line >= last_line {
        return;
    }

    let lines = physical_lines(cx.source());
    let mut removed_line: Option<Range> = None;
    check_empty_lines_around_body_boundary(
        cx,
        kind,
        first_line,
        "beginning",
        beginning_style,
        &lines,
        &mut removed_line,
    );
    if let Some(end_line) = last_line.checked_sub(2) {
        check_empty_lines_around_body_boundary(
            cx,
            kind,
            end_line,
            "end",
            ending_style,
            &lines,
            &mut removed_line,
        );
    }
}

fn check_empty_lines_around_body_boundary(
    cx: &Cx<'_>,
    kind: &str,
    line_index: usize,
    location: &str,
    style: EmptyLinesAroundBodyStyle,
    lines: &[PhysicalLine],
    removed_line: &mut Option<Range>,
) {
    let Some(line) = lines.get(line_index).copied() else {
        return;
    };
    match style {
        EmptyLinesAroundBodyStyle::NoEmptyLines if line.blank => {
            // RuboCop removes a one-byte range at the beginning of the blank
            // line, then re-runs autocorrect. For LF this is the newline; for
            // CRLF it is the carriage return, with the remaining newline
            // removed on the next fixpoint pass.
            let range = Range {
                start: line.start,
                end: line.start.saturating_add(1).min(line.end),
            };
            cx.emit_offense(
                range,
                &format!("Extra empty line detected at {kind} body {location}."),
                None,
            );
            let duplicate =
                removed_line.is_some_and(|prev| prev.start == range.start && prev.end == range.end);
            if !duplicate {
                cx.emit_edit(range, "");
                *removed_line = Some(range);
            }
        }
        EmptyLinesAroundBodyStyle::EmptyLines if !line.blank => {
            let insertion_line = if location == "end" {
                line_index.saturating_add(1)
            } else {
                line_index
            };
            let insert_at = lines
                .get(insertion_line)
                .map(|next| next.start)
                .unwrap_or_else(|| cx.source().len() as u32);
            let range = Range {
                start: insert_at,
                end: insert_at,
            };
            cx.emit_offense(
                range,
                &format!("Empty line missing at {kind} body {location}."),
                None,
            );
            cx.emit_edit(range, "\n");
        }
        _ => {}
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
/// heredoc opens there. Start and end tokens are paired by their delimiter
/// labels, with FIFO pairing among pending heredocs that share a label. This
/// preserves sibling heredoc order while correctly matching nested interpolated
/// heredocs, whose terminators are not globally FIFO.
#[derive(Default)]
struct HeredocEndIndex {
    end_lines: HashMap<u32, u32>,
}

impl HeredocEndIndex {
    /// Pair every heredoc opener inside the measured node once. Token scanning
    /// starts at the node and stops after its last matching terminator, which
    /// covers curly blocks whose AST range ends before a trailing heredoc while
    /// avoiding scans of unrelated file tokens.
    fn new(node: NodeId, cx: &Cx<'_>) -> Self {
        let node_range = cx.range(node);
        let target_openers: HashSet<u32> = cx
            .tokens_in(node_range)
            .iter()
            .filter(|tok| tok.kind == SourceTokenKind::HeredocStart)
            .map(|tok| tok.range.start)
            .collect();
        if target_openers.is_empty() {
            return Self::default();
        }

        let source = cx.source();
        let scan_range = Range {
            start: node_range.start,
            end: u32::try_from(source.len()).expect("source length fits parser offsets"),
        };
        let mut openers: Vec<(Option<&str>, u32)> = Vec::new();
        let mut end_lines = HashMap::with_capacity(target_openers.len());
        let mut remaining = target_openers.len();
        for tok in cx.tokens_in(scan_range) {
            match tok.kind {
                SourceTokenKind::HeredocStart => {
                    let label = source
                        .get(tok.range.start as usize..tok.range.end as usize)
                        .and_then(heredoc_start_label);
                    openers.push((label, tok.range.start));
                }
                SourceTokenKind::HeredocEnd => {
                    if openers.is_empty() {
                        continue;
                    }
                    let end_label = source
                        .get(tok.range.start as usize..tok.range.end as usize)
                        .and_then(heredoc_end_label);
                    // Prefer the oldest opener with this label. Only fall back
                    // to FIFO when a label could not be parsed; a known end
                    // label must not consume an opener with a different label.
                    let same_label = end_label.and_then(|label| {
                        openers
                            .iter()
                            .position(|(open_label, _)| *open_label == Some(label))
                    });
                    let unknown_label = openers.iter().position(|(label, _)| label.is_none());
                    let opener_index = same_label
                        .or(unknown_label)
                        .or_else(|| end_label.is_none().then_some(0));
                    let Some(opener_index) = opener_index else {
                        continue;
                    };
                    let (_, opener) = openers.remove(opener_index);
                    if target_openers.contains(&opener) {
                        end_lines.insert(opener, line_of(tok.range.start, cx));
                        remaining -= 1;
                        if remaining == 0 {
                            break;
                        }
                    }
                }
                _ => {}
            }
        }
        Self { end_lines }
    }

    fn end_line(&self, opener_start: u32) -> Option<u32> {
        self.end_lines.get(&opener_start).copied()
    }
}

/// Extract the delimiter label from a Prism `HeredocStart` token's source text.
fn heredoc_start_label(token_text: &str) -> Option<&str> {
    let label = token_text.strip_prefix("<<")?;
    let label = label
        .strip_prefix('~')
        .or_else(|| label.strip_prefix('-'))
        .unwrap_or(label);
    match label.as_bytes().first().copied() {
        Some(quote @ (b'\'' | b'"' | b'`')) => label.get(1..)?.strip_suffix(quote as char),
        Some(_) => Some(label),
        None => None,
    }
}

/// Extract the delimiter label from a Prism `HeredocEnd` token's source text.
fn heredoc_end_label(token_text: &str) -> Option<&str> {
    let line = token_text.trim_end_matches(['\n', '\r']);
    let label = line
        .trim_start_matches([' ', '\t'])
        .trim_end_matches([' ', '\t']);
    (!label.is_empty()).then_some(label)
}

/// Source range used by `CodeLengthCalculator#code_length`. Prism gives a
/// block-call send the enclosing block's full range; RuboCop's send node range
/// ends at the call's arguments, before the block body. Trim that suffix when
/// a send is the call child of a block node.
fn code_length_range(node: NodeId, cx: &Cx<'_>) -> Range {
    let range = cx.range(node);
    let Some(parent) = cx.parent(node).get() else {
        return range;
    };
    if cx.block_call(parent).get() != Some(node) {
        return range;
    }

    let end = if cx.is_parenthesized(node) {
        let closing_paren = cx.loc(node).end();
        if closing_paren == Range::ZERO {
            return range;
        }
        closing_paren.end
    } else if let Some(last_argument) = cx.last_argument(node).get() {
        cx.range(last_argument).end
    } else {
        cx.loc(node).name.end
    };
    if end < range.start || end > range.end {
        return range;
    }
    Range {
        start: range.start,
        end,
    }
}

/// The 0-based first and (heredoc-extended) last source line of a node's byte
/// range. For a node containing a trailing heredoc whose AST range stops at the
/// `<<~LABEL` opener, the last line is extended to the matching `HeredocEnd`
/// token's line so the heredoc body is counted (RuboCop's
/// `source_from_node_with_heredoc`). Robust whether or not Prism's range
/// already reaches the terminator.
fn node_line_span(node: NodeId, heredoc_ends: &HeredocEndIndex, cx: &Cx<'_>) -> (u32, u32) {
    let range = code_length_range(node, cx);
    let first = line_of(range.start, cx);
    // `range.end` is one-past-the-last byte; the last *content* byte is
    // `range.end - 1`. Empty ranges collapse to a single line.
    let last_byte = range.end.saturating_sub(1).max(range.start);
    let mut last = line_of(last_byte, cx);

    // Extend through heredoc bodies whose opener lies within the node range.
    // The end index is built once for the measured node, so a trailing
    // terminator may safely sit past this descendant's AST range.
    for tok in cx.tokens_in(range) {
        if tok.kind == SourceTokenKind::HeredocStart
            && let Some(end_line) = heredoc_ends.end_line(tok.range.start)
        {
            last = last.max(end_line);
        }
    }
    (first, last.max(first))
}

/// Count non-irrelevant lines in a node's (heredoc-extended) line span.
fn count_lines(
    node: NodeId,
    count_comments: bool,
    heredoc_ends: &HeredocEndIndex,
    cx: &Cx<'_>,
) -> i64 {
    let (first, last) = node_line_span(node, heredoc_ends, cx);
    (first..=last)
        .filter(|&line| !irrelevant_line(cx, line, count_comments))
        .count() as i64
}

/// RuboCop `CodeLengthCalculator#heredoc_length`: count non-irrelevant lines of
/// the heredoc *body* and add 2 for the opening and closing delimiter lines.
fn heredoc_length(
    node: NodeId,
    count_comments: bool,
    heredoc_ends: &HeredocEndIndex,
    cx: &Cx<'_>,
) -> i64 {
    let range = cx.range(node);
    let opener_line = line_of(range.start, cx);
    let end_line = heredoc_ends.end_line(range.start).unwrap_or(opener_line);
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
fn code_length(
    node: NodeId,
    count_comments: bool,
    heredoc_ends: &HeredocEndIndex,
    cx: &Cx<'_>,
) -> i64 {
    if is_heredoc_node(node, cx) {
        heredoc_length(node, count_comments, heredoc_ends, cx)
    } else {
        count_lines(node, count_comments, heredoc_ends, cx)
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

/// RuboCop `CodeLengthCalculator#omit_length` for a folded implicit hash.
/// Subtract the absent opening/closing brace lines only for a one-argument,
/// parenthesized call. The comparisons are byte-offset adjacency checks, not
/// line-number checks; each side contributes independently.
fn omit_length(hash: NodeId, cx: &Cx<'_>) -> i64 {
    if !matches!(*cx.kind(hash), NodeKind::Hash(_)) {
        return 0;
    }
    let hash_range = cx.range(hash);
    if cx.raw_source(hash_range).starts_with('{') {
        return 0;
    }
    let Some(parent) = cx.parent(hash).get() else {
        return 0;
    };
    if !matches!(
        *cx.kind(parent),
        NodeKind::Send { .. } | NodeKind::Csend { .. }
    ) || cx.call_arguments(parent).len() > 1
        || !cx.is_parenthesized(parent)
    {
        return 0;
    }

    let call_loc = cx.loc(parent);
    let open_paren = call_loc.begin();
    let close_paren = call_loc.end();
    if open_paren == Range::ZERO || close_paren == Range::ZERO {
        return 0;
    }

    let mut omitted = 0;
    if open_paren.end != hash_range.start {
        omitted += 1;
    }
    if close_paren.start != hash_range.end {
        omitted += 1;
    }
    omitted
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

/// RuboCop `Metrics::Utils::CodeLengthCalculator#calculate` for a measured
/// method/block-like node: count its extracted body, then fold each enabled
/// top-level `CountAsOne` descendant of the original node.
///
/// `node` is RuboCop's calculator seed (`def`/`block`/`casgn`/`sclass` as
/// appropriate); `body` is `extract_body(node)`. Keeping both mirrors RuboCop's
/// distinction between the counted body and the node whose descendants are
/// folded. The node range also scopes token lookups so each metric visit does
/// not rescan unrelated tokens from the whole file.
pub fn body_code_length(
    node: NodeId,
    body: NodeId,
    count_comments: bool,
    foldable_types: &[FoldableType],
    cx: &Cx<'_>,
) -> i64 {
    let heredoc_ends = HeredocEndIndex::new(node, cx);
    let mut length = if is_heredoc_node(body, cx) {
        // RuboCop counts a heredoc body node's own source range as the base
        // body; only descendant heredocs extend the source span here.
        count_lines(body, count_comments, &HeredocEndIndex::default(), cx)
    } else {
        count_lines(body, count_comments, &heredoc_ends, cx)
    };
    if foldable_types.is_empty() {
        return length;
    }

    let mut descendants = Vec::new();
    collect_top_level_foldables(node, foldable_types, cx, &mut descendants);
    for descendant in descendants {
        if !foldable_node(descendant, foldable_types, cx) {
            continue;
        }
        let descendant_length = code_length(descendant, count_comments, &heredoc_ends, cx);
        length = length - descendant_length + 1;
        length -= omit_length(descendant, cx);
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
///    enabled foldable, then `omit_length` for an eligible implicit hash.
///
/// Token lookups use this node's range, not the whole file, when extending a
/// folded heredoc through its terminator.
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

    let heredoc_ends = HeredocEndIndex::new(node, cx);

    // `each_top_level_descendant(@node, …)` is seeded with the whole class/
    // module node, halting at (and never recursing into) inner classlike nodes.
    let mut descendants = Vec::new();
    collect_top_level_foldables(node, foldable_types, cx, &mut descendants);
    for descendant in descendants {
        if !foldable_node(descendant, foldable_types, cx) {
            continue;
        }
        let descendant_length = code_length(descendant, count_comments, &heredoc_ends, cx);
        length = length - descendant_length + 1;
        length -= omit_length(descendant, cx);
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
    use super::{
        SourceLineIndex, display_column, heredoc_end_label, heredoc_start_label,
        is_assignment_or_comparison_operator,
    };

    #[test]
    fn source_line_index_matches_prefix_newline_counts_for_byte_offsets() {
        let source = "あ\né\n";
        let lines = SourceLineIndex::new(source);

        // Include offsets inside multibyte characters and at newline bytes:
        // source offsets are bytes, and a newline advances the line only after
        // its own byte position.
        for offset in 0..=source.len() {
            let expected = source.as_bytes()[..offset]
                .iter()
                .filter(|&&byte| byte == b'\n')
                .count()
                + 1;
            assert_eq!(lines.line_of(offset as u32), expected, "line at {offset}");
            let expected_start = source.as_bytes()[..offset]
                .iter()
                .rposition(|&byte| byte == b'\n')
                .map_or(0, |newline| newline + 1);
            assert_eq!(
                lines.line_start(offset as u32),
                expected_start,
                "line start at {offset}"
            );
        }
    }

    #[test]
    fn source_line_index_handles_empty_and_single_line_sources() {
        assert_eq!(SourceLineIndex::new("").line_of(0), 1);
        assert_eq!(SourceLineIndex::new("one line").line_of(8), 1);
    }

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
    fn heredoc_labels_ignore_quotes_and_indentation() {
        assert_eq!(heredoc_start_label("<<~OUTER"), Some("OUTER"));
        assert_eq!(heredoc_start_label("<<-'INNER'"), Some("INNER"));
        assert_eq!(heredoc_start_label("<<\"TEXT\""), Some("TEXT"));
        assert_eq!(heredoc_end_label("  OUTER\n"), Some("OUTER"));
        assert_eq!(heredoc_end_label("\tINNER\r\n"), Some("INNER"));
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
