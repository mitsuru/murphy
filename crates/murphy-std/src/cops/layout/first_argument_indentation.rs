//! `Layout/FirstArgumentIndentation` — checks the indentation of the first
//! argument in a method call. Arguments after the first are checked by
//! `Layout/ArgumentAlignment`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/FirstArgumentIndentation
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Node-driven (on_send + on_csend) mirroring RuboCop. Implements all four
//!   EnforcedStyle values: consistent, consistent_relative_to_receiver,
//!   special_for_inner_method_call, and
//!   special_for_inner_method_call_in_parentheses (default). Fires only when
//!   the call has arguments, the selector is not a bare operator method
//!   (`a + b`) and not a setter (`x.y = z`), and the first argument starts on
//!   a later line than the call and begins its own line. Autocorrects by
//!   rewriting the first argument's line leading whitespace to the expected
//!   column. IndentationWidth defaults to 2.
//!
//!   Single-surface ABI blockers (intentionally NOT bypassed):
//!     * `on_super`: RuboCop aliases `on_super` to `on_send`, but Murphy's
//!       `call_arguments`/`first_argument` helpers resolve only `send`/`csend`
//!       (a `super(...)` node is `NodeKind::Super`, whose argument list is not
//!       surfaced through the per-cop ABI). `super` calls are therefore not
//!       checked. This is a small scope gap, not a behaviour divergence on the
//!       calls that *are* checked.
//!     * `Layout/ArgumentAlignment` `EnforcedStyle: with_fixed_indentation`
//!       disables this cop (unless `Layout/FirstMethodArgumentLineBreak` is
//!       enabled). Murphy cannot read a sibling cop's config, so this cop
//!       always runs (the common case — `with_fixed_indentation` is not the
//!       ArgumentAlignment default).
//!     * `Layout/IndentationWidth` `Width` is RuboCop's fallback when this
//!       cop's own `IndentationWidth` is unset. Murphy falls back to 2.
//!   The "correct the entire receiver chain" autocorrect refinement
//!   (`should_correct_entire_chain?`) only changes *which* node the
//!   AlignmentCorrector rewrites in deeply chained calls; the per-line
//!   re-indentation Murphy emits reaches the same fixpoint, so it is not
//!   separately modelled.
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, Range, cop};

#[derive(Default)]
pub struct FirstArgumentIndentation;

/// Options for [`FirstArgumentIndentation`].
#[derive(CopOptions)]
pub struct FirstArgumentIndentationOptions {
    #[option(
        name = "EnforcedStyle",
        default = "special_for_inner_method_call_in_parentheses",
        description = "How the first argument is indented relative to its base."
    )]
    pub enforced_style: ArgIndentStyle,
    #[option(
        name = "IndentationWidth",
        default = 2,
        description = "Indentation width in spaces (RuboCop falls back to Layout/IndentationWidth)."
    )]
    pub indentation_width: i64,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum ArgIndentStyle {
    /// Always one step more than the preceding code line.
    #[option(value = "consistent")]
    Consistent,
    /// One level relative to the receiver of the call.
    #[option(value = "consistent_relative_to_receiver")]
    ConsistentRelativeToReceiver,
    /// Inner-call args indent relative to the inner method.
    #[option(value = "special_for_inner_method_call")]
    SpecialForInnerMethodCall,
    /// Like `special_for_inner_method_call` but only when the outer call uses
    /// parentheses.
    #[option(value = "special_for_inner_method_call_in_parentheses")]
    SpecialForInnerMethodCallInParentheses,
}

#[cop(
    name = "Layout/FirstArgumentIndentation",
    description = "Checks the indentation of the first argument in a method call.",
    default_severity = "warning",
    default_enabled = true,
    options = FirstArgumentIndentationOptions,
)]
impl FirstArgumentIndentation {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>, options: &FirstArgumentIndentationOptions) {
        check(node, cx, options);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>, options: &FirstArgumentIndentationOptions) {
        check(node, cx, options);
    }
}

fn check(node: NodeId, cx: &Cx<'_>, options: &FirstArgumentIndentationOptions) {
    if !should_check(node, cx) {
        return;
    }
    let Some(&first_arg) = cx.call_arguments(node).first() else {
        return;
    };
    let node_start = cx.range(node).start;
    let first_arg_start = cx.range(first_arg).start;

    // Skip if the call and the first argument share a line.
    if line_of(cx, node_start) == line_of(cx, first_arg_start) {
        return;
    }

    // `check_alignment` only fires when the first argument begins its own line.
    if !begins_its_line(cx, first_arg_start) {
        return;
    }

    let width = options.indentation_width.max(0) as usize;
    let base = base_indentation(node, first_arg, cx, options.enforced_style);
    let expected = base + width;
    let actual = column_of(cx, first_arg_start);
    if expected == actual {
        return;
    }

    let msg = message(node, first_arg, cx, options.enforced_style);
    cx.emit_offense(cx.range(first_arg), &msg, None);
    reindent_line(first_arg_start, expected, cx);
}

/// RuboCop's `should_check?`: has args, not a bare operator method, not a
/// setter method.
fn should_check(node: NodeId, cx: &Cx<'_>) -> bool {
    if cx.call_arguments(node).is_empty() {
        return false;
    }
    if bare_operator(node, cx) {
        return false;
    }
    if cx.is_setter_method(node) {
        return false;
    }
    true
}

/// `operator_method? && !dot?` — `a + b` but not `a.+(b)`.
fn bare_operator(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.is_operator_method(node) && cx.loc(node).dot() == Range::ZERO
}

/// RuboCop's `base_indentation`.
fn base_indentation(
    node: NodeId,
    first_arg: NodeId,
    cx: &Cx<'_>,
    style: ArgIndentStyle,
) -> usize {
    if special_inner_call_indentation(node, cx, style) {
        column_of_range(base_range(node, first_arg, cx), cx)
    } else {
        // Column of the first non-whitespace char of the previous code line.
        let first_arg_line = line_of(cx, cx.range(first_arg).start);
        previous_code_line_indent(cx, first_arg_line)
    }
}

/// RuboCop's `special_inner_call_indentation?`.
fn special_inner_call_indentation(node: NodeId, cx: &Cx<'_>, style: ArgIndentStyle) -> bool {
    match style {
        ArgIndentStyle::Consistent => return false,
        ArgIndentStyle::ConsistentRelativeToReceiver => return true,
        _ => {}
    }

    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    if !eligible_method_call(parent, cx) {
        return false;
    }
    // `special_for_inner_method_call_in_parentheses` requires the parent call
    // to be parenthesized.
    let parent_parenthesized = cx.loc(parent).begin() != Range::ZERO;
    if !parent_parenthesized
        && style == ArgIndentStyle::SpecialForInnerMethodCallInParentheses
    {
        return false;
    }

    // The node must begin inside the parent — otherwise it is the first part
    // of a chained method call.
    cx.range(node).start > cx.range(parent).start
}

/// RuboCop's `eligible_method_call?`: `(send _ !:[]= ...)`.
fn eligible_method_call(node: NodeId, cx: &Cx<'_>) -> bool {
    if !matches!(*cx.kind(node), murphy_plugin_api::NodeKind::Send { .. }) {
        return false;
    }
    cx.method_name(node) != Some("[]=")
}

/// RuboCop's `base_range(send_node, arg_node)`: from the call's start (or its
/// parent's start when the parent is a splat/kwsplat) to the argument start.
fn base_range(node: NodeId, first_arg: NodeId, cx: &Cx<'_>) -> Range {
    let start = match cx.parent(node).get() {
        Some(parent)
            if matches!(
                *cx.kind(parent),
                murphy_plugin_api::NodeKind::Splat(_) | murphy_plugin_api::NodeKind::Kwsplat(_)
            ) =>
        {
            cx.range(parent).start
        }
        _ => cx.range(node).start,
    };
    Range {
        start,
        end: cx.range(first_arg).start,
    }
}

/// RuboCop's `column_of(range)`: for single-line ranges, the column of the
/// range start; for ranges spanning lines, the indent of the line before the
/// range's last line.
fn column_of_range(range: Range, cx: &Cx<'_>) -> usize {
    let source = cx.raw_source(range).trim();
    if source.contains('\n') {
        // Multi-line: RuboCop walks to `range.line + newlines + 1` and takes
        // that line's indent (the previous non-blank, non-comment code line).
        let start_line = line_of(cx, range.start);
        let newlines = source.matches('\n').count();
        previous_code_line_indent(cx, start_line + newlines + 1)
    } else {
        column_of(cx, range.start)
    }
}

fn message(
    node: NodeId,
    first_arg: NodeId,
    cx: &Cx<'_>,
    style: ArgIndentStyle,
) -> String {
    let range = base_range(node, first_arg, cx);
    let text = cx.raw_source(range).trim();
    let base = if !text.contains('\n') && special_inner_call_indentation(node, cx, style) {
        format!("`{text}`")
    } else {
        // The comment-suffix variant is omitted: it only changes wording, and
        // Murphy's previous-code-line walk already skips comment lines.
        "the start of the previous line".to_string()
    };
    format!("Indent the first argument one step more than {base}.")
}

// ── column / line helpers ────────────────────────────────────────────────

/// Char column of `offset` within its line.
fn column_of(cx: &Cx<'_>, offset: u32) -> usize {
    let src = cx.source();
    let off = offset as usize;
    let line_start = src[..off].rfind('\n').map_or(0, |p| p + 1);
    src[line_start..off].chars().count()
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

/// Indent (column of first non-whitespace char) of the nearest non-blank,
/// non-comment-only code line strictly above `line_number` (1-based).
fn previous_code_line_indent(cx: &Cx<'_>, line_number: usize) -> usize {
    let src = cx.source();
    let lines: Vec<&str> = src.split('\n').collect();
    let mut n = line_number; // 1-based; we step to n-1 first.
    loop {
        if n <= 1 {
            return 0;
        }
        n -= 1;
        let line = lines.get(n - 1).copied().unwrap_or("");
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        return line.chars().take_while(|c| c.is_whitespace()).count();
    }
}

/// Rewrite the leading whitespace of `offset`'s line so `offset` lands at
/// `expected_column` (spaces only).
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

murphy_plugin_api::submit_cop!(FirstArgumentIndentation);
