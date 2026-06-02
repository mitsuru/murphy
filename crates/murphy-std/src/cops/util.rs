//! Shared utilities for standard cops.

use murphy_plugin_api::{Cx, NodeId, NodeKind, Range, SourceTokenKind};

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
        .map_or(false, |t| t.kind == SourceTokenKind::LeftParen && t.range.start == range_start)
}

/// Emit an edit that replaces `cond_range` with `replacement`, prepending a
/// space if the character immediately before `cond_range.start` is not
/// whitespace.
///
/// Used by `NegatedIf/NegatedUnless/NegatedWhile` when replacing a
/// parenthesized condition like `(!x.even?)` with its inner receiver source
/// `x.even?`. Without this guard, `if(!x.even?)` would autocorrect to
/// `unlessx.even?` (keyword and replacement run together).
pub fn emit_edit_with_preceding_space(
    cond_range: Range,
    replacement: &str,
    cx: &Cx<'_>,
) {
    let source = cx.source().as_bytes();
    let needs_space = cond_range.start > 0
        && !source[(cond_range.start - 1) as usize].is_ascii_whitespace();
    if needs_space {
        cx.emit_edit(cond_range, &format!(" {replacement}"));
    } else {
        cx.emit_edit(cond_range, replacement);
    }
}

// Note: is_parenthesized is tested indirectly via the cops that use it:
// - `cops::style::parentheses_around_condition::tests::flags_if_with_paren_condition`
//   verifies `is_parenthesized` returns true for `(x > 10)`.
// - `cops::style::negated_if::tests::flags_modifier_if_with_parenthesized_negation`
//   verifies `is_parenthesized` returns true for `(!x.even?)`.
// - `cops::style::parentheses_around_condition::tests::no_offense_begin_end_condition`
//   verifies `is_parenthesized` returns false for `begin...end`.
