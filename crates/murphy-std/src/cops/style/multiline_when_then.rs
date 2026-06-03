//! `Style/MultilineWhenThen` — flags unnecessary `then` in multiline `when`
//! statements and removes it.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/MultilineWhenThen
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Detects multiline when branches that use `then` as separator and
//!   autocorrects by removing the `then` keyword (plus surrounding spaces,
//!   stopping at a newline). Guards: skip if then is absent, skip if
//!   conditions span multiple lines (including single multi-line conditions),
//!   skip if body is on same line as then.
//! ```
//!
//! ## Matched shapes
//!
//! `When` nodes that:
//! - Have a `then` keyword separator
//! - Would be valid without `then` (i.e. body is on a different line)
//! - Conditions do NOT span multiple lines
//! - Body is NOT on the same line as the `then` keyword
//!
//! ## Autocorrect
//!
//! Removes the `then` token and the whitespace to its left (stopping at a newline).

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind};

const MSG: &str = "Do not use `then` for multiline `when` statement.";

#[derive(Default)]
pub struct MultilineWhenThen;

#[cop(
    name = "Style/MultilineWhenThen",
    description = "Do not use `then` for multiline `when` statement.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl MultilineWhenThen {
    #[on_node(kind = "when")]
    fn check_when(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Must have a `then` keyword token present.
    let Some(then_range) = find_then_token(node, cx) else {
        return;
    };

    // `then` is required when conditions span multiple lines — skip.
    if conditions_span_multiple_lines(node, cx) {
        return;
    }

    // `then` is required when the body is on the same line as the then keyword.
    if body_on_same_line_as_then(node, then_range, cx) {
        return;
    }

    cx.emit_offense(then_range, MSG, None);

    // Autocorrect: remove `then` plus any spaces/tabs to the left,
    // stopping at a newline (mirrors RuboCop's range_with_surrounding_space
    // with side: :left, newlines: false).
    let edit_range = extend_left_over_spaces(then_range, cx);
    cx.emit_edit(edit_range, "");
}

/// Find the `then` keyword token in the gap between the last condition and
/// the body start. The search is deliberately bounded to avoid picking up
/// `then` tokens inside the body (e.g. `if x then y end` or nested `when`).
///
/// Bound: [last_cond_end, search_end) where `search_end` is:
/// - `body.start` if the body is present, or
/// - the first newline byte after `last_cond_end` if no body (or node end,
///   whichever is smaller). This covers `when bar then\nend` correctly.
///
/// Returns `None` if no `then` token is found in the gap.
fn find_then_token(node: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let conds = cx.when_conditions(node);
    if conds.is_empty() {
        return None;
    }
    let last_cond_end = cx.range(*conds.last().unwrap()).end;

    // Compute the upper search bound.
    let NodeKind::When { body, .. } = *cx.kind(node) else {
        return None;
    };
    let search_end = if let Some(body_id) = body.get() {
        cx.range(body_id).start
    } else {
        // No body: scan only to the end of the current line.
        first_newline_or_end(cx.source().as_bytes(), last_cond_end as usize)
    };

    if last_cond_end >= search_end {
        return None;
    }

    let src = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < last_cond_end);
    for tok in &toks[idx..] {
        if tok.range.start >= search_end {
            break;
        }
        if tok.kind == SourceTokenKind::Other
            && &src[tok.range.start as usize..tok.range.end as usize] == b"then"
        {
            return Some(tok.range);
        }
    }
    None
}

/// Returns the byte offset of the first `\n` character at or after `from`,
/// or the length of the source if none is found. Used to bound `then`-token
/// search when the `when` branch has no body.
fn first_newline_or_end(src: &[u8], from: usize) -> u32 {
    for (i, &byte) in src.iter().enumerate().skip(from) {
        if byte == b'\n' {
            return i as u32;
        }
    }
    src.len() as u32
}

/// Returns `true` when the conditions (taken as a whole) span multiple lines.
///
/// Checks from `conditions.first.start` to `conditions.last.end` for a `\n`,
/// which correctly handles:
/// - Multiple conditions where the last is on a different line than the first.
/// - A single condition that itself spans multiple lines (e.g. a method call
///   with arguments across lines).
fn conditions_span_multiple_lines(node: NodeId, cx: &Cx<'_>) -> bool {
    let conds = cx.when_conditions(node);
    if conds.is_empty() {
        return false;
    }
    let first_range = cx.range(*conds.first().unwrap());
    let last_range = cx.range(*conds.last().unwrap());
    let src = cx.source();
    src[first_range.start as usize..last_range.end as usize].contains('\n')
}

/// Returns `true` when the body is on the same line as the `then` keyword.
/// Equivalent to RuboCop's `same_line?(when_node, when_node.body)`.
///
/// Precondition: `then_range.end <= body_range.start` (the `then` separator
/// always precedes the body in source order). The guard `then_range.end <=
/// body_range.start` is checked explicitly to avoid a reversed slice panic.
fn body_on_same_line_as_then(node: NodeId, then_range: Range, cx: &Cx<'_>) -> bool {
    let NodeKind::When { body, .. } = *cx.kind(node) else {
        return false;
    };
    let Some(body_id) = body.get() else {
        // No body — `then` at end of line with no body after it.
        return false;
    };
    let body_range = cx.range(body_id);
    // Guard against the `then` token being after the body start (should not
    // happen with a correctly bounded `find_then_token`, but be defensive).
    if then_range.end > body_range.start {
        return false;
    }
    // Same line means no newline between `then` end and body start.
    let src = cx.source();
    !src[then_range.end as usize..body_range.start as usize].contains('\n')
}

/// Extend `range` leftward over spaces/tabs (not newlines) to include any
/// whitespace padding before `then`. This mirrors RuboCop's
/// `range_with_surrounding_space(side: :left, newlines: false)`.
fn extend_left_over_spaces(range: Range, cx: &Cx<'_>) -> Range {
    let src = cx.source().as_bytes();
    let mut start = range.start as usize;
    while start > 0 {
        let b = src[start - 1];
        if b == b' ' || b == b'\t' {
            start -= 1;
        } else {
            break;
        }
    }
    Range {
        start: start as u32,
        end: range.end,
    }
}

#[cfg(test)]
mod tests {
    use super::MultilineWhenThen;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_then_in_multiline_when_no_body() {
        // `when bar then` with body on the next line — offense.
        test::<MultilineWhenThen>().expect_correction(
            indoc! {"
                case foo
                when bar then
                         ^^^^ Do not use `then` for multiline `when` statement.
                  2
                end
            "},
            "case foo\nwhen bar\n  2\nend\n",
        );
    }

    #[test]
    fn flags_then_in_multiline_when_with_body_below() {
        test::<MultilineWhenThen>().expect_correction(
            indoc! {"
                case foo
                when 1 then
                       ^^^^ Do not use `then` for multiline `when` statement.
                  baz
                end
            "},
            "case foo\nwhen 1\n  baz\nend\n",
        );
    }

    #[test]
    fn accepts_single_line_when_then_with_body() {
        // `when bar then do_something` — body is on same line, no offense.
        test::<MultilineWhenThen>().expect_no_offenses(indoc! {"
            case foo
            when bar then do_something
            end
        "});
    }

    #[test]
    fn accepts_single_line_when_then_multiline_body() {
        // `when bar then do_something(arg1,\n  arg2)` — body starts same line.
        test::<MultilineWhenThen>().expect_no_offenses(
            "case foo\nwhen bar then do_something(arg1,\n                               arg2)\nend\n",
        );
    }

    #[test]
    fn accepts_when_without_then() {
        test::<MultilineWhenThen>().expect_no_offenses(indoc! {"
            case foo
            when bar
              2
            end
        "});
    }

    #[test]
    fn accepts_single_line_when_no_body() {
        // Bare `when bar` with nothing — no offense.
        test::<MultilineWhenThen>().expect_no_offenses(indoc! {"
            case foo
            when bar
            end
        "});
    }

    #[test]
    fn flags_multiple_when_branches() {
        test::<MultilineWhenThen>().expect_correction(
            indoc! {"
                case foo
                when 1 then
                       ^^^^ Do not use `then` for multiline `when` statement.
                  bar
                when 2 then
                       ^^^^ Do not use `then` for multiline `when` statement.
                  baz
                end
            "},
            "case foo\nwhen 1\n  bar\nwhen 2\n  baz\nend\n",
        );
    }

    #[test]
    fn accepts_then_when_body_has_nested_then() {
        // `then` inside body's `if`/`when` must not be mistaken for the separator.
        // The `when bar then` here: body is `if baz then qux end`.
        // The separator `then` is on the first line after `bar`; the `then` in
        // `if baz then qux end` is inside the body and must be ignored.
        test::<MultilineWhenThen>().expect_correction(
            indoc! {"
                case foo
                when bar then
                         ^^^^ Do not use `then` for multiline `when` statement.
                  if baz then qux end
                end
            "},
            "case foo\nwhen bar\n  if baz then qux end\nend\n",
        );
    }

    #[test]
    fn accepts_then_when_conditions_multiline() {
        // Single condition spanning multiple lines — `then` is required; no offense.
        test::<MultilineWhenThen>().expect_no_offenses(
            "case foo\nwhen bar(\n  baz\n) then\n  qux\nend\n",
        );
    }
}

murphy_plugin_api::submit_cop!(MultilineWhenThen);
