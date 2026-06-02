//! `Style/MultilineInPatternThen` — flags `then` in multi-line `in` pattern
//! clauses and removes it.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/MultilineInPatternThen
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Detects `then` keyword in multi-line `in` pattern clauses. `then` is
//!   required (and kept) when the pattern and body are on the same source
//!   line. When the body is on a subsequent line, `then` is redundant and
//!   should be removed. Autocorrects by removing the `then` token and the
//!   space before it.
//! ```
//!
//! ## Matched shapes
//!
//! `InPattern` nodes that:
//! - Have a `then` keyword (detected via token scanning)
//! - Do NOT require the `then` keyword (i.e., body is on a different line from
//!   the `in` keyword, or there is no body)
//!
//! `then` is required when:
//! - The pattern is multiline (a multiline pattern needs `then` to separate
//!   pattern from body)
//! - The body exists AND the body is on the same line as the `in` keyword
//!
//! ## Autocorrect
//!
//! Removes the `then` token and the space that precedes it (i.e., from the
//! end of the preceding token to the end of the `then` token).

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

const MSG: &str = "Do not use `then` for multiline `in` statement.";

#[derive(Default)]
pub struct MultilineInPatternThen;

#[cop(
    name = "Style/MultilineInPatternThen",
    description = "Do not use `then` for multiline `in` statement.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl MultilineInPatternThen {
    #[on_node(kind = "in_pattern")]
    fn check_in_pattern(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Find the `then` token for this in_pattern node.
    let Some(then_range) = find_then_token(node, cx) else {
        return; // No `then` — nothing to flag.
    };

    // If `then` is required, skip.
    if require_then(node, cx) {
        return;
    }

    cx.emit_offense(then_range, MSG, None);

    // Autocorrect: remove ` then` (space before `then` + the `then` token).
    let remove_range = space_and_then_range(then_range, cx);
    cx.emit_edit(remove_range, "");
}

/// Returns `true` when the `then` keyword is needed (must not be flagged).
///
/// `then` is required when:
/// 1. The pattern is multiline (needs `then` to separate pattern from body).
/// 2. The body exists AND is on the same source line as the `in` keyword.
fn require_then(node: NodeId, cx: &Cx<'_>) -> bool {
    let (pattern, body) = match *cx.kind(node) {
        NodeKind::InPattern { pattern, body, .. } => (pattern, body),
        _ => return true,
    };

    // Condition 1: multiline pattern requires `then`.
    if cx.is_multiline(pattern) {
        return true;
    }

    // No body — `then` is not required (trailing `then` with no body).
    let Some(body_id) = body.get() else {
        return false;
    };

    // Condition 2: body on the same line as the `in` keyword → `then` is needed.
    same_line_as_node(node, body_id, cx)
}

/// Returns `true` if node `a` starts on the same line as node `b`.
///
/// Checks whether the byte slice between the two start offsets contains a
/// newline — more efficient than counting from the start of the file.
fn same_line_as_node(a: NodeId, b: NodeId, cx: &Cx<'_>) -> bool {
    let source = cx.source().as_bytes();
    let a_start = cx.range(a).start as usize;
    let b_start = cx.range(b).start as usize;
    let (min, max) = if a_start < b_start {
        (a_start, b_start)
    } else {
        (b_start, a_start)
    };
    !source[min..max].contains(&b'\n')
}

/// Finds the `then` keyword token inside the `in_pattern` node range, in the
/// gap between the pattern/guard end and the body start (or node end).
///
/// Returns `None` if no `then` token is present.
fn find_then_token(node: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let (pattern, guard, body) = match *cx.kind(node) {
        NodeKind::InPattern { pattern, guard, body } => (pattern, guard, body),
        _ => return None,
    };

    // The `then` token, if present, lies after the pattern (or guard) and
    // before the body (or node end).
    let search_from = if let Some(g) = guard.get() {
        cx.range(g).end
    } else {
        cx.range(pattern).end
    };

    let search_to = if let Some(b) = body.get() {
        cx.range(b).start
    } else {
        cx.range(node).end
    };

    if search_from >= search_to {
        return None;
    }

    let src = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < search_from);
    for tok in &toks[idx..] {
        if tok.range.start >= search_to {
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

/// Computes the range to remove for autocorrect: from just before the space
/// that precedes `then` to the end of the `then` token. This removes ` then`.
///
/// Mirrors RuboCop's `range_with_surrounding_space(range, side: :left,
/// newlines: false)`.
fn space_and_then_range(then_range: Range, cx: &Cx<'_>) -> Range {
    let src = cx.source().as_bytes();
    let mut start = then_range.start as usize;
    // Walk backwards consuming space characters (but not newlines).
    while start > 0 && src[start - 1] == b' ' {
        start -= 1;
    }
    Range {
        start: start as u32,
        end: then_range.end,
    }
}

#[cfg(test)]
mod tests {
    use super::MultilineInPatternThen;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Flagged cases -----

    #[test]
    fn flags_then_in_multiline_in_pattern() {
        test::<MultilineInPatternThen>().expect_correction(
            indoc! {"
                case x
                in Integer then
                           ^^^^ Do not use `then` for multiline `in` statement.
                  :foo
                end
            "},
            "case x\nin Integer\n  :foo\nend\n",
        );
    }

    #[test]
    fn flags_then_with_guard_in_multiline_in_pattern() {
        test::<MultilineInPatternThen>().expect_correction(
            indoc! {"
                case x
                in Integer => m if m > 0 then
                                         ^^^^ Do not use `then` for multiline `in` statement.
                  :foo
                end
            "},
            "case x\nin Integer => m if m > 0\n  :foo\nend\n",
        );
    }

    #[test]
    fn flags_then_with_no_body() {
        test::<MultilineInPatternThen>().expect_correction(
            indoc! {"
                case x
                in Integer then
                           ^^^^ Do not use `then` for multiline `in` statement.
                end
            "},
            "case x\nin Integer\nend\n",
        );
    }

    // ----- Allowed cases -----

    #[test]
    fn accepts_in_pattern_without_then() {
        test::<MultilineInPatternThen>().expect_no_offenses(indoc! {"
            case x
            in Integer
              :foo
            end
        "});
    }

    #[test]
    fn accepts_then_on_same_line_as_body() {
        // `then` is required to separate pattern from body on the same line.
        test::<MultilineInPatternThen>().expect_no_offenses(indoc! {"
            case x
            in Integer then :foo
            end
        "});
    }

    #[test]
    fn accepts_multiple_in_patterns_without_then() {
        test::<MultilineInPatternThen>().expect_no_offenses(indoc! {"
            case x
            in Integer
              :foo
            in String
              :bar
            end
        "});
    }

    #[test]
    fn accepts_then_with_body_on_same_line_different_pattern() {
        // Single-line `in` with body — `then` separates pattern from body.
        test::<MultilineInPatternThen>().expect_no_offenses(indoc! {"
            case x
            in Integer then do_something(arg1,
              arg2)
            end
        "});
    }

    #[test]
    fn accepts_then_with_multiline_pattern_per_rubocop() {
        // RuboCop's require_then? returns true when the pattern is NOT single_line
        // (i.e., pattern.single_line? is false). In that case  is considered
        // required and is not flagged. This matches RuboCop's upstream behavior.
        test::<MultilineInPatternThen>().expect_no_offenses(indoc! {"
            case x
            in [
              Integer
            ] then
              :foo
            end
        "});
    }
}

murphy_plugin_api::submit_cop!(MultilineInPatternThen);
