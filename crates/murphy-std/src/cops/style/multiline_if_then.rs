//! `Style/MultilineIfThen` — flags `then` in multi-line `if`/`unless` statements.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/MultilineIfThen
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Detects `then` keywords in multi-line if/unless statements where `then`
//!   is redundant because a newline already separates the condition from the body.
//!   A `then` is flagged when the node has a `then` token AND there is a newline
//!   between the `then` token and the if-branch body start (or no body at all).
//!   Single-line uses of `then` (e.g. `if cond then a`) are accepted.
//!   `elsif` branches with multiline `then` are also flagged (each gets its own
//!   dispatch visit as a nested If node).
//!   Autocorrect removes the ` then` (from cond.end to then.end).
//! ```
//!
//! ## Matched shapes
//!
//! `If` nodes (including `elsif`) that:
//! - Are NOT modifier-form
//! - Are NOT ternary
//! - Have a `then` keyword token
//! - Have a newline between `then.end` and the if-branch body start,
//!   OR have no body (then is the last thing before `end`)
//!
//! ## Autocorrect
//!
//! Removes from `cond.end` to `then.end` — deletes the space + `then`.
//! If there are comments in the gap between cond and `then`, only the
//! `then` token itself is removed.

use murphy_plugin_api::{Cx, NoOptions, NodeId, Range, SourceTokenKind, cop};

const MSG: &str = "Do not use `then` for multi-line `%<keyword>s`.";

#[derive(Default)]
pub struct MultilineIfThen;

#[cop(
    name = "Style/MultilineIfThen",
    description = "Do not use `then` for multi-line `if`/`unless`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl MultilineIfThen {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Skip modifier-form (e.g. `x if cond`).
    if cx.is_modifier_form(node) {
        return;
    }

    // Skip ternary (e.g. `cond ? a : b`).
    if cx.is_ternary(node) {
        return;
    }

    // Must have a `then` keyword.
    if !cx.is_then(node) {
        return;
    }

    // Find the `then` token range for this If node (excluding child ranges).
    let then_range = find_then_token(node, cx);
    if then_range == Range::ZERO {
        return;
    }

    // A `then` is multiline when there is a Newline token between `then.end`
    // and the body start, or when there is no body.
    let if_branch = cx.if_branch(node);
    let body_start = if_branch.get().map(|b| cx.range(b).start);

    if !is_multiline_then(then_range, body_start, cx.range(node).end, cx) {
        return;
    }

    // Determine keyword for the message.
    let keyword = cx.if_keyword(node);
    let message = MSG.replace("%<keyword>s", keyword);

    cx.emit_offense(then_range, &message, None);

    // Autocorrect: remove from cond.end to then.end (space + `then`).
    // If there are comments between cond and `then`, only remove `then`.
    let remove_range = if let Some(cond_id) = cx.if_condition(node).get() {
        let cond_end = cx.range(cond_id).end;
        let gap = Range {
            start: cond_end,
            end: then_range.start,
        };
        if cx.comments_in_range(gap).is_empty() {
            Range {
                start: cond_end,
                end: then_range.end,
            }
        } else {
            then_range
        }
    } else {
        then_range
    };

    cx.emit_edit(remove_range, "");
}

/// Find the `then` token in the given `If` node, excluding tokens that fall
/// inside any direct child node ranges.
fn find_then_token(node: NodeId, cx: &Cx<'_>) -> Range {
    let children = cx.children(node);
    let src = cx.source().as_bytes();
    for tok in cx.tokens_in(cx.range(node)) {
        let outside_children = children.iter().all(|&child| {
            let r = cx.range(child);
            tok.range.start < r.start || tok.range.end > r.end
        });
        if outside_children
            && tok.kind == SourceTokenKind::Other
            && &src[tok.range.start as usize..tok.range.end as usize] == b"then"
        {
            return tok.range;
        }
    }
    Range::ZERO
}

/// Returns true if the `then` token is followed by a newline before the body
/// starts (multiline form). Also returns true when there is no body.
fn is_multiline_then(then_range: Range, body_start: Option<u32>, node_end: u32, cx: &Cx<'_>) -> bool {
    let search_end = match body_start {
        Some(start) if start > then_range.end => start,
        Some(_) => return false, // body starts at or before then.end — single-line
        None => {
            // No body: check for a Newline between `then.end` and the node
            // end (before the `end` keyword). Bounding to node_end avoids
            // picking up the trailing newline of `if cond then end\n`.
            return has_newline_in(then_range.end, node_end, cx);
        }
    };

    has_newline_in(then_range.end, search_end, cx)
}

/// Returns true if there is a Newline or IgnoredNewline token in `[from, to)`.
fn has_newline_in(from: u32, to: u32, cx: &Cx<'_>) -> bool {
    if from >= to {
        return false;
    }
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < from);
    for tok in &toks[idx..] {
        if tok.range.start >= to {
            break;
        }
        if matches!(
            tok.kind,
            SourceTokenKind::Newline | SourceTokenKind::IgnoredNewline
        ) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::MultilineIfThen;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_multiline_if_then() {
        // `if cond then` — `then` starts at column 8, length 4.
        test::<MultilineIfThen>().expect_correction(
            indoc! {"
                if cond then
                        ^^^^ Do not use `then` for multi-line `if`.
                  foo
                end
            "},
            "if cond\n  foo\nend\n",
        );
    }

    #[test]
    fn flags_multiline_unless_then() {
        // `unless cond then` — `then` starts at column 12, length 4.
        test::<MultilineIfThen>().expect_correction(
            indoc! {"
                unless cond then
                            ^^^^ Do not use `then` for multi-line `unless`.
                  foo
                end
            "},
            "unless cond\n  foo\nend\n",
        );
    }

    #[test]
    fn flags_multiline_elsif_then() {
        // `elsif b then` — `then` starts at column 8, length 4.
        test::<MultilineIfThen>().expect_correction(
            indoc! {"
                if a
                  foo
                elsif b then
                        ^^^^ Do not use `then` for multi-line `elsif`.
                  bar
                end
            "},
            "if a\n  foo\nelsif b\n  bar\nend\n",
        );
    }

    #[test]
    fn flags_if_then_with_no_body() {
        // No body after `then` — still multiline.
        test::<MultilineIfThen>().expect_correction(
            indoc! {"
                if cond then
                        ^^^^ Do not use `then` for multi-line `if`.
                end
            "},
            "if cond\nend\n",
        );
    }

    #[test]
    fn accepts_single_line_if_then() {
        // Single-line `if cond then a` is OK.
        test::<MultilineIfThen>().expect_no_offenses("if cond then foo\nend\n");
    }

    #[test]
    fn accepts_single_line_elsif_then() {
        // Single-line elsif with `then` is OK.
        test::<MultilineIfThen>().expect_no_offenses(indoc! {"
            if a
              foo
            elsif b then bar
            end
        "});
    }

    #[test]
    fn accepts_if_without_then() {
        test::<MultilineIfThen>().expect_no_offenses(indoc! {"
            if cond
              foo
            end
        "});
    }

    #[test]
    fn accepts_modifier_if() {
        test::<MultilineIfThen>().expect_no_offenses("foo if cond\n");
    }

    #[test]
    fn accepts_ternary() {
        test::<MultilineIfThen>().expect_no_offenses("cond ? a : b\n");
    }

    #[test]
    fn accepts_single_line_if_then_no_body() {
        // `if cond then end` on a single line is not multiline — no offense.
        test::<MultilineIfThen>().expect_no_offenses("if cond then end\n");
    }

    #[test]
    fn accepts_single_line_unless_then_no_body() {
        // `unless cond then end` on a single line is not multiline — no offense.
        test::<MultilineIfThen>().expect_no_offenses("unless cond then end\n");
    }
}

murphy_plugin_api::submit_cop!(MultilineIfThen);
