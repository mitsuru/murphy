//! `Layout/FirstHashElementIndentation` — checks the indentation of the first
//! key in a hash literal where the opening brace and the first key are on
//! separate lines. The other keys are handled by `Layout/HashAlignment`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/FirstHashElementIndentation
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Node-driven (on_hash + on_send/on_csend) mirroring RuboCop's
//!   MultilineElementIndentation base, and structurally parallel to Murphy's
//!   Layout/FirstArrayElementIndentation. Implements all three EnforcedStyle
//!   values: special_inside_parentheses (default), consistent, and
//!   align_braces. Fires only when the `{` and the first key are on separate
//!   lines (RuboCop's `same_line?` guard), checks both the first key and a
//!   right brace that begins its own line, and autocorrects by rewriting the
//!   offending line's leading whitespace to the expected column.
//!   IndentationWidth defaults to 2.
//!
//!   Single-surface ABI blockers (intentionally NOT bypassed): RuboCop reads
//!   two pieces of *other cops'* configuration that Murphy's per-cop
//!   `CopOptions` surface does not expose:
//!     * `Layout/ArgumentAlignment` `EnforcedStyle: with_fixed_indentation`
//!       disables the `on_send` argument path. Murphy cannot see a sibling
//!       cop's config, so the argument path always runs (the common case,
//!       since `with_fixed_indentation` is not the ArgumentAlignment default).
//!     * `Layout/HashAlignment` `EnforcedColonStyle` / `EnforcedHashRocketStyle`
//!       `== 'separator'` switches RuboCop to the longest-key table layout
//!       (`check_based_on_longest_key`). Murphy cannot read that sibling
//!       config; with the default (`key`) the separator path is never taken,
//!       so Murphy always uses the standard `check_first` (delta 0) path.
//!     * `Layout/IndentationWidth` `Width` is RuboCop's fallback when this
//!       cop's own `IndentationWidth` is unset. Murphy falls back to the
//!       literal default of 2.
//!
//!   Known behavioural gaps (vs. RuboCop), pending follow-up:
//!     * The `parent_hash_key` indent base is not modelled (same as the array
//!       cop). When the hash is the value of a hash pair whose key/value start
//!       on the same line and whose sibling pair begins on a later line,
//!       RuboCop measures the first key relative to the parent hash key. Murphy
//!       uses the start-of-line (or paren) base instead.
//!     * Argument scanning is shallow: only the *direct* hash arguments of a
//!       call are inspected. RuboCop's `each_argument_node` recurses through
//!       non-`send` children, so a hash nested inside another hash/array
//!       argument is checked as a plain hash (start-of-line base) here.
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

#[derive(Default)]
pub struct FirstHashElementIndentation;

/// Options for [`FirstHashElementIndentation`].
#[derive(CopOptions)]
pub struct FirstHashElementIndentationOptions {
    #[option(
        name = "EnforcedStyle",
        default = "special_inside_parentheses",
        description = "How the first hash key is indented relative to its base."
    )]
    pub enforced_style: HashElementStyle,
    #[option(
        name = "IndentationWidth",
        default = 2,
        description = "Indentation width in spaces (RuboCop falls back to Layout/IndentationWidth)."
    )]
    pub indentation_width: i64,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum HashElementStyle {
    /// First key indented relative to the first position after a surrounding
    /// `(` when the `{` shares that line; otherwise like `consistent`.
    #[option(value = "special_inside_parentheses")]
    SpecialInsideParentheses,
    /// First key indented relative to the start of the line holding `{`.
    #[option(value = "consistent")]
    Consistent,
    /// First key indented relative to the `{` column; braces align.
    #[option(value = "align_braces")]
    AlignBraces,
}

/// What the expected indentation is measured against — drives the message.
#[derive(Clone, Copy)]
enum BaseType {
    LeftBrace,
    FirstColumnAfterParen,
    StartOfLine,
}

#[cop(
    name = "Layout/FirstHashElementIndentation",
    description = "Checks the indentation of the first key in a hash literal.",
    default_severity = "warning",
    default_enabled = true,
    options = FirstHashElementIndentationOptions,
)]
impl FirstHashElementIndentation {
    #[on_node(kind = "hash")]
    fn check_hash(
        &self,
        node: NodeId,
        cx: &Cx<'_>,
        options: &FirstHashElementIndentationOptions,
    ) {
        // Plain hash literal: no surrounding `(` to measure against.
        check(node, None, cx, options);
    }

    #[on_node(kind = "send")]
    fn check_send(
        &self,
        node: NodeId,
        cx: &Cx<'_>,
        options: &FirstHashElementIndentationOptions,
    ) {
        check_call_args(node, cx, options);
    }

    #[on_node(kind = "csend")]
    fn check_csend(
        &self,
        node: NodeId,
        cx: &Cx<'_>,
        options: &FirstHashElementIndentationOptions,
    ) {
        check_call_args(node, cx, options);
    }
}

/// RuboCop's `each_argument_node(node, :hash)`: for every hash argument whose
/// `{` shares a line with the call's `(`, check it against that `(` column.
/// Such hashes are then ignored by `on_hash`.
fn check_call_args(
    node: NodeId,
    cx: &Cx<'_>,
    options: &FirstHashElementIndentationOptions,
) {
    let lparen = cx.loc(node).begin();
    if lparen == Range::ZERO {
        return;
    }
    for &arg in cx.call_arguments(node) {
        if !is_braced_hash(arg, cx) {
            continue;
        }
        let lbrace_start = cx.range(arg).start;
        // The `{` must be on the same line as the `(`.
        if same_line(cx, lparen.start, lbrace_start) {
            check(arg, Some(lparen.start), cx, options);
        }
    }
}

/// Is `node` a `{`-delimited hash literal (not a braceless kwargs hash like
/// `foo(a: 1)`)? Mirrors RuboCop's `node.loc.begin` being non-nil.
fn is_braced_hash(node: NodeId, cx: &Cx<'_>) -> bool {
    if !matches!(*cx.kind(node), NodeKind::Hash(_)) {
        return false;
    }
    let r = cx.range(node);
    if r.end <= r.start {
        return false;
    }
    let src = cx.raw_source(r);
    src.starts_with('{') && src.ends_with('}')
}

/// The shared per-hash check. `left_paren_start` is the byte offset of a
/// surrounding `(` when the hash is a same-line parenthesized argument.
fn check(
    node: NodeId,
    left_paren_start: Option<u32>,
    cx: &Cx<'_>,
    options: &FirstHashElementIndentationOptions,
) {
    if !is_braced_hash(node, cx) {
        return;
    }
    // When called from `on_hash`, a same-line parenthesized hash is also
    // covered by `check_call_args`; skip the hash-only pass for those so the
    // offense is not emitted twice.
    if left_paren_start.is_none() && covered_by_parent_call(node, cx) {
        return;
    }

    let node_range = cx.range(node);
    let left_brace_start = node_range.start;
    // `}` is the final byte of the hash's source range.
    let right_brace_start = node_range.end - 1;

    let pairs = cx.hash_pairs(node);
    let first = pairs.first().copied();

    if let Some(first) = first {
        let first_start = cx.range(first).start;
        // RuboCop returns from `check` entirely when the first key shares the
        // `{` line — the right brace is then NOT checked.
        if same_line(cx, left_brace_start, first_start) {
            return;
        }
        check_first(first, left_brace_start, left_paren_start, cx, options);
    }

    check_right_brace(
        right_brace_start,
        left_brace_start,
        left_paren_start,
        cx,
        options,
    );
}

/// True when `node`'s parent is a call whose `(` is on the same line as the
/// hash's `{`, meaning `check_call_args` already handles it.
fn covered_by_parent_call(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    if !cx.call_arguments(parent).contains(&node) {
        return false;
    }
    let lparen = cx.loc(parent).begin();
    if lparen == Range::ZERO {
        return false;
    }
    same_line(cx, lparen.start, cx.range(node).start)
}

/// Check the first key's column against the expected indentation.
fn check_first(
    first: NodeId,
    left_brace_start: u32,
    left_paren_start: Option<u32>,
    cx: &Cx<'_>,
    options: &FirstHashElementIndentationOptions,
) {
    let first_start = cx.range(first).start;
    let actual = column_of(cx, first_start);
    let (base_col, base_type) =
        indent_base(left_brace_start, left_paren_start, cx, options.enforced_style);
    let expected = base_col + options.indentation_width.max(0) as usize;
    if expected == actual {
        return;
    }
    let msg = message(base_type, options.indentation_width.max(0) as usize);
    cx.emit_offense(cx.range(first), &msg, None);
    reindent_line(first_start, expected, cx);
}

/// Check a right brace that begins its own line; it must align to the base
/// column (NOT base + width).
fn check_right_brace(
    right_brace_start: u32,
    left_brace_start: u32,
    left_paren_start: Option<u32>,
    cx: &Cx<'_>,
    options: &FirstHashElementIndentationOptions,
) {
    // If anything other than whitespace precedes `}` on its line, accept.
    if !begins_its_line(cx, right_brace_start) {
        return;
    }
    let actual = column_of(cx, right_brace_start);
    let (base_col, base_type) =
        indent_base(left_brace_start, left_paren_start, cx, options.enforced_style);
    if base_col == actual {
        return;
    }
    let msg = message_for_right_brace(base_type);
    let brace_range = Range {
        start: right_brace_start,
        end: right_brace_start + 1,
    };
    cx.emit_offense(brace_range, &msg, None);
    reindent_line(right_brace_start, base_col, cx);
}

/// RuboCop's `indent_base`: returns `(base_column, base_type)`.
fn indent_base(
    left_brace_start: u32,
    left_paren_start: Option<u32>,
    cx: &Cx<'_>,
    style: HashElementStyle,
) -> (usize, BaseType) {
    if style == HashElementStyle::AlignBraces {
        return (column_of(cx, left_brace_start), BaseType::LeftBrace);
    }
    if let Some(paren_start) = left_paren_start
        && style == HashElementStyle::SpecialInsideParentheses
    {
        return (
            column_of(cx, paren_start) + 1,
            BaseType::FirstColumnAfterParen,
        );
    }
    (
        first_non_ws_column(cx, left_brace_start),
        BaseType::StartOfLine,
    )
}

fn message(base_type: BaseType, width: usize) -> String {
    let base = match base_type {
        BaseType::LeftBrace => "the position of the opening brace",
        BaseType::FirstColumnAfterParen => {
            "the first position after the preceding left parenthesis"
        }
        BaseType::StartOfLine => "the start of the line where the left curly brace is",
    };
    format!("Use {width} spaces for indentation in a hash, relative to {base}.")
}

fn message_for_right_brace(base_type: BaseType) -> String {
    match base_type {
        BaseType::LeftBrace => "Indent the right brace the same as the left brace.",
        BaseType::FirstColumnAfterParen => {
            "Indent the right brace the same as the first position after the preceding left parenthesis."
        }
        BaseType::StartOfLine => {
            "Indent the right brace the same as the start of the line where the left brace is."
        }
    }
    .to_string()
}

// ── column / line helpers ────────────────────────────────────────────────

/// Byte column (chars before `offset` on its line) of `offset`.
fn column_of(cx: &Cx<'_>, offset: u32) -> usize {
    let src = cx.source();
    let off = offset as usize;
    let line_start = src[..off].rfind('\n').map_or(0, |p| p + 1);
    src[line_start..off].chars().count()
}

/// Column of the first non-whitespace char on the line containing `offset`.
fn first_non_ws_column(cx: &Cx<'_>, offset: u32) -> usize {
    let src = cx.source();
    let off = offset as usize;
    let line_start = src[..off].rfind('\n').map_or(0, |p| p + 1);
    let line_end = src[line_start..]
        .find('\n')
        .map_or(src.len(), |p| line_start + p);
    let line = &src[line_start..line_end];
    line.chars()
        .position(|c| !c.is_whitespace())
        .unwrap_or(0)
}

/// True when byte offsets `a` and `b` sit on the same source line. Checks the
/// source bytes between them for a `\n` (O(distance)), avoiding a BOF line-
/// number scan for each offset (which is O(N) per call → O(N^2) overall).
/// `a` and `b` may be given in either order.
fn same_line(cx: &Cx<'_>, a: u32, b: u32) -> bool {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    let src = cx.source().as_bytes();
    let hi = (hi as usize).min(src.len());
    !src[lo as usize..hi].contains(&b'\n')
}

/// True when only whitespace precedes `offset` on its line.
fn begins_its_line(cx: &Cx<'_>, offset: u32) -> bool {
    let src = cx.source().as_bytes();
    let mut i = offset as usize;
    while i > 0 {
        match src[i - 1] {
            b' ' | b'\t' => i -= 1,
            b'\n' => return true,
            _ => return false,
        }
    }
    true
}

/// Rewrite the leading whitespace of `offset`'s line so `offset` lands at
/// `expected_column` (spaces only — RuboCop's AlignmentCorrector behaviour).
fn reindent_line(offset: u32, expected_column: usize, cx: &Cx<'_>) {
    let src = cx.source();
    let off = offset as usize;
    let line_start = src[..off].rfind('\n').map_or(0, |p| p + 1);
    let leading = Range {
        start: line_start as u32,
        end: offset,
    };
    let replacement = " ".repeat(expected_column);
    cx.emit_edit(leading, &replacement);
}

#[cfg(test)]
mod tests;

murphy_plugin_api::submit_cop!(FirstHashElementIndentation);
