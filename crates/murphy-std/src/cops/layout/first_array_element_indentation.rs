//! `Layout/FirstArrayElementIndentation` — checks the indentation of the
//! first element in an array literal when the opening bracket and the first
//! element are on separate lines. The other elements are handled by
//! `Layout/ArrayAlignment`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/FirstArrayElementIndentation
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Node-driven (on_array + on_send/on_csend) mirroring RuboCop's
//!   FirstElementIndentation base. Implements all three EnforcedStyle values:
//!   special_inside_parentheses (default), consistent, and align_brackets.
//!   Fires only when the `[` and the first element are on separate lines
//!   (RuboCop's `same_line?` guard), checks both the first element and a
//!   right bracket that begins its own line, and autocorrects by rewriting
//!   the offending line's leading whitespace to the expected column.
//!   IndentationWidth defaults to 2.
//!
//!   Single-surface ABI blockers (intentionally NOT bypassed): RuboCop reads
//!   two pieces of *other cops'* configuration that Murphy's per-cop
//!   `CopOptions` surface does not expose:
//!     * `Layout/ArrayAlignment` `EnforcedStyle: with_fixed_indentation`
//!       disables this cop for the non-`consistent` styles. Murphy cannot see
//!       a sibling cop's config, so this cop always runs (the common case,
//!       since `with_fixed_indentation` is not the ArrayAlignment default).
//!     * `Layout/IndentationWidth` `Width` is RuboCop's fallback when this
//!       cop's own `IndentationWidth` is unset. Murphy falls back to the
//!       literal default of 2.
//!   The ambiguous-style detection RuboCop performs (`detected_styles`) only
//!   feeds style-inference diagnostics and never changes which offenses fire,
//!   so it is intentionally omitted.
//!
//!   Known behavioural gaps (vs. RuboCop), pending follow-up:
//!     * The `parent_hash_key` indent base is not modelled. When the array is
//!       the value of a hash pair whose key/value start on the same line and
//!       whose sibling pair begins on a later line, RuboCop measures the first
//!       element relative to the parent hash key. Murphy uses the start-of-line
//!       (or paren) base instead — a different expected column in that shape.
//!     * Argument scanning is shallow: `check_call_args` only inspects the
//!       *direct* array arguments of a call (`cx.call_arguments`). RuboCop's
//!       `each_argument_node` recurses through non-`send` children, so an array
//!       nested inside a hash/array argument is checked via the paren path
//!       there but as a plain array (start-of-line base) here.
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, Range, cop};

#[derive(Default)]
pub struct FirstArrayElementIndentation;

/// Options for [`FirstArrayElementIndentation`].
#[derive(CopOptions)]
pub struct FirstArrayElementIndentationOptions {
    #[option(
        name = "EnforcedStyle",
        default = "special_inside_parentheses",
        description = "How the first array element is indented relative to its base."
    )]
    pub enforced_style: ArrayElementStyle,
    // `Option<i64>` so the bundled default `IndentationWidth: ~` (JSON null)
    // decodes to `None` instead of erroring the option struct and discarding the
    // user's other keys; `None` falls back to width 2.
    #[option(
        name = "IndentationWidth",
        description = "Indentation width in spaces (null/unset falls back to RuboCop's default of 2)."
    )]
    pub indentation_width: Option<i64>,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum ArrayElementStyle {
    /// First element indented relative to the first position after a
    /// surrounding `(` when the `[` shares that line; otherwise like
    /// `consistent`.
    #[option(value = "special_inside_parentheses")]
    SpecialInsideParentheses,
    /// First element indented relative to the start of the line holding `[`.
    #[option(value = "consistent")]
    Consistent,
    /// First element indented relative to the `[` column; brackets align.
    #[option(value = "align_brackets")]
    AlignBrackets,
}

/// What the expected indentation is measured against — drives the message.
#[derive(Clone, Copy)]
enum BaseType {
    LeftBracket,
    FirstColumnAfterParen,
    StartOfLine,
}

#[cop(
    name = "Layout/FirstArrayElementIndentation",
    description = "Checks the indentation of the first element in an array literal.",
    default_severity = "warning",
    default_enabled = true,
    options = FirstArrayElementIndentationOptions,
)]
impl FirstArrayElementIndentation {
    #[on_node(kind = "array")]
    fn check_array(
        &self,
        node: NodeId,
        cx: &Cx<'_>,
        options: &FirstArrayElementIndentationOptions,
    ) {
        // Plain array literal: no surrounding `(` to measure against.
        check(node, None, cx, options);
    }

    #[on_node(kind = "send")]
    fn check_send(
        &self,
        node: NodeId,
        cx: &Cx<'_>,
        options: &FirstArrayElementIndentationOptions,
    ) {
        check_call_args(node, cx, options);
    }

    #[on_node(kind = "csend")]
    fn check_csend(
        &self,
        node: NodeId,
        cx: &Cx<'_>,
        options: &FirstArrayElementIndentationOptions,
    ) {
        check_call_args(node, cx, options);
    }
}

/// RuboCop's `each_argument_node`: for every array argument whose `[` shares a
/// line with the call's `(`, check it against that `(` column. Such arrays are
/// then ignored by `on_array` — Murphy approximates by re-running the same
/// per-array check with the `left_parenthesis` column supplied; the array-only
/// `on_array` pass produces the identical result for these because the special
/// rule only changes indentation under `special_inside_parentheses`.
fn check_call_args(
    node: NodeId,
    cx: &Cx<'_>,
    options: &FirstArrayElementIndentationOptions,
) {
    let lparen = cx.loc(node).begin();
    if lparen == Range::ZERO {
        return;
    }
    for &arg in cx.call_arguments(node) {
        if !is_square_bracket_array(arg, cx) {
            continue;
        }
        let lbracket_start = cx.range(arg).start;
        // The `[` must be on the same line as the `(`.
        if line_of(cx, lparen.start) == line_of(cx, lbracket_start) {
            check(arg, Some(lparen.start), cx, options);
        }
    }
}

/// Is `node` a `[`-delimited array literal (not `%w[...]` / `%i[...]`)?
fn is_square_bracket_array(node: NodeId, cx: &Cx<'_>) -> bool {
    if !cx.is_square_brackets(node) {
        return false;
    }
    let r = cx.range(node);
    let src = cx.raw_source(r);
    src.starts_with('[') && src.ends_with(']') && r.end > r.start
}

/// The shared per-array check. `left_paren_col` is the byte column of a
/// surrounding `(` when the array is a same-line parenthesized argument.
fn check(
    node: NodeId,
    left_paren_start: Option<u32>,
    cx: &Cx<'_>,
    options: &FirstArrayElementIndentationOptions,
) {
    if !is_square_bracket_array(node, cx) {
        return;
    }
    // When called from `on_array`, a same-line parenthesized array is also
    // covered by `check_call_args`; skip the array-only pass for those so the
    // offense is not emitted twice. Detect by looking at the immediate parent
    // being a call whose `(` shares the `[` line.
    if left_paren_start.is_none() && covered_by_parent_call(node, cx) {
        return;
    }

    let node_range = cx.range(node);
    let left_bracket_start = node_range.start;
    // `]` is the final byte of the array's source range.
    let right_bracket_start = node_range.end - 1;

    let elements = cx.array_elements(node);
    let first = elements.first().copied();

    if let Some(first) = first {
        let first_start = cx.range(first).start;
        // RuboCop returns from `check` entirely when the first element shares
        // the `[` line — the right bracket is then NOT checked. (Empty arrays
        // have no first element and still fall through, matching the upstream
        // `if first_elem` guard.)
        if line_of(cx, left_bracket_start) == line_of(cx, first_start) {
            return;
        }
        check_first(first, left_bracket_start, left_paren_start, cx, options);
    }

    check_right_bracket(
        right_bracket_start,
        left_bracket_start,
        left_paren_start,
        cx,
        options,
    );
}

/// True when `node`'s parent is a call whose `(` is on the same line as the
/// array's `[`, meaning `check_call_args` already handles it.
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
    line_of(cx, lparen.start) == line_of(cx, cx.range(node).start)
}

/// Check the first element's column against the expected indentation.
fn check_first(
    first: NodeId,
    left_bracket_start: u32,
    left_paren_start: Option<u32>,
    cx: &Cx<'_>,
    options: &FirstArrayElementIndentationOptions,
) {
    let first_start = cx.range(first).start;
    let actual = column_of(cx, first_start);
    let (base_col, base_type) =
        indent_base(left_bracket_start, left_paren_start, cx, options.enforced_style);
    let expected = base_col + options.indentation_width.unwrap_or(2).max(0) as usize;
    if expected == actual {
        return;
    }
    let msg = message(base_type, options.indentation_width.unwrap_or(2).max(0) as usize);
    cx.emit_offense(cx.range(first), &msg, None);
    reindent_line(first_start, expected, cx);
}

/// Check a right bracket that begins its own line; it must align to the base
/// column (NOT base + width).
fn check_right_bracket(
    right_bracket_start: u32,
    left_bracket_start: u32,
    left_paren_start: Option<u32>,
    cx: &Cx<'_>,
    options: &FirstArrayElementIndentationOptions,
) {
    // If anything other than whitespace precedes `]` on its line, accept.
    if !begins_its_line(cx, right_bracket_start) {
        return;
    }
    let actual = column_of(cx, right_bracket_start);
    let (base_col, base_type) =
        indent_base(left_bracket_start, left_paren_start, cx, options.enforced_style);
    if base_col == actual {
        return;
    }
    let msg = message_for_right_bracket(base_type);
    let bracket_range = Range {
        start: right_bracket_start,
        end: right_bracket_start + 1,
    };
    cx.emit_offense(bracket_range, &msg, None);
    reindent_line(right_bracket_start, base_col, cx);
}

/// RuboCop's `indent_base`: returns `(base_column, base_type)`.
fn indent_base(
    left_bracket_start: u32,
    left_paren_start: Option<u32>,
    cx: &Cx<'_>,
    style: ArrayElementStyle,
) -> (usize, BaseType) {
    if style == ArrayElementStyle::AlignBrackets {
        return (column_of(cx, left_bracket_start), BaseType::LeftBracket);
    }
    if let Some(paren_start) = left_paren_start
        && style == ArrayElementStyle::SpecialInsideParentheses
    {
        return (
            column_of(cx, paren_start) + 1,
            BaseType::FirstColumnAfterParen,
        );
    }
    (
        first_non_ws_column(cx, left_bracket_start),
        BaseType::StartOfLine,
    )
}

fn message(base_type: BaseType, width: usize) -> String {
    let base = match base_type {
        BaseType::LeftBracket => "the position of the opening bracket",
        BaseType::FirstColumnAfterParen => {
            "the first position after the preceding left parenthesis"
        }
        BaseType::StartOfLine => "the start of the line where the left square bracket is",
    };
    format!("Use {width} spaces for indentation in an array, relative to {base}.")
}

fn message_for_right_bracket(base_type: BaseType) -> String {
    match base_type {
        BaseType::LeftBracket => "Indent the right bracket the same as the left bracket.",
        BaseType::FirstColumnAfterParen => {
            "Indent the right bracket the same as the first position after the preceding left parenthesis."
        }
        BaseType::StartOfLine => {
            "Indent the right bracket the same as the start of the line where the left bracket is."
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

/// 1-based line number of `offset`.
fn line_of(cx: &Cx<'_>, offset: u32) -> usize {
    let src = cx.source();
    src[..offset as usize].bytes().filter(|&b| b == b'\n').count() + 1
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

murphy_plugin_api::submit_cop!(FirstArrayElementIndentation);
