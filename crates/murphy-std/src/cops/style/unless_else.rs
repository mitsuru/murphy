//! `Style/UnlessElse` — flags `unless` expressions with `else` clauses.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/UnlessElse
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Flags any `unless...else...end` block. Autocorrects by replacing the
//!   `unless` keyword with `if` and swapping the two source regions:
//!   - body_range: from end of condition (or after `then` if present) to start
//!     of `else` keyword. For the no-`then` case this includes any trailing
//!     comment on the `unless x # comment` line.
//!   - else_range: from end of `else` keyword to start of `end` keyword
//!   Follows RuboCop's `loc.begin = nil` path (prism): body_range starts
//!   at condition.source_range.end when no `then` present. When `then` is
//!   present, body_range starts after `then`. Offense range is the first
//!   source line of the node. The autocorrect edit covers the full node.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

const MSG: &str = "Do not use `unless` with `else`. Rewrite these with the positive case first.";

#[derive(Default)]
pub struct UnlessElse;

#[cop(
    name = "Style/UnlessElse",
    description = "Do not use `unless` with `else`. Rewrite these with the positive case first.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl UnlessElse {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Only `unless` (not `if`, not `elsif`).
    if !cx.is_unless(node) {
        return;
    }
    // Must have an `else` clause.
    if !cx.is_else(node) {
        return;
    }

    let node_range = cx.range(node);

    // Offense: first source line of the node (from node start to first newline).
    // Matches RuboCop's first-line caret display.
    let source = cx.source().as_bytes();
    let node_start = node_range.start as usize;
    let first_line_end = source[node_start..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(node_range.end as usize, |pos| node_start + pos);
    let offense_range = Range {
        start: node_range.start,
        end: first_line_end as u32,
    };
    cx.emit_offense(offense_range, MSG, None);

    // Autocorrect: replace `unless` with `if` and swap the two branches.
    let keyword_loc = cx.if_keyword_loc(node);
    if keyword_loc == Range::ZERO {
        return;
    }

    // Find the `else` keyword token within this node (excluding children).
    let Some(else_tok) = find_else_token(node, cx) else {
        return;
    };

    // Find the `end` keyword range.
    let end_range = cx.loc(node).end_keyword();
    if end_range == Range::ZERO {
        return;
    }

    let cond = match cx.kind(node) {
        NodeKind::If { cond, .. } => *cond,
        _ => return,
    };
    let cond_end = cx.range(cond).end;

    // Find the `then` keyword token on the header line only (from condition
    // end to the first newline). Limiting to the header line avoids picking up
    // `then` from nested `if y then...end` inside the body. If no `then` on
    // the header line, body starts at cond_end.
    let header_line_end = source[cond_end as usize..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(else_tok.start, |pos| cond_end + pos as u32);
    let scan_to = header_line_end.min(else_tok.start);
    let header_end = find_then_token_end(cond_end, scan_to, cx).unwrap_or(cond_end);

    // body_chunk: from header_end to start of `else`.
    // - No `then`: " # neg\n  a=1\n" (includes trailing comment and body)
    // - With `then`: "\n  a=1\n" (body only, `then` is in header)
    let body_chunk = cx.raw_source(Range {
        start: header_end,
        end: else_tok.start,
    });

    // else_chunk: from end of `else` to start of `end`.
    // e.g. " # pos\n  a=0\n" or "\n  a=0\n"
    let else_chunk = cx.raw_source(Range {
        start: else_tok.end,
        end: end_range.start,
    });

    // header_src: from `unless` end to header_end.
    // e.g. " x" (no `then`) or " x then" (with `then`)
    let header_src = cx.raw_source(Range {
        start: keyword_loc.end,
        end: header_end,
    });

    // Whole-node replacement:
    // Original: unless<header_src><body_chunk>else<else_chunk>end
    // Corrected: if<header_src><else_chunk>else<body_chunk>end
    let replacement = format!("if{header_src}{else_chunk}else{body_chunk}end");

    // Only emit the correction for the outermost unless-else. When this node
    // is nested inside another unless-else that is also being corrected,
    // skip the edit to avoid overlapping edits. (RuboCop uses ignore_node/
    // part_of_ignored_node? for the same purpose.)
    let has_unless_else_ancestor = cx
        .ancestors(node)
        .any(|anc| cx.is_unless(anc) && cx.is_else(anc));
    if !has_unless_else_ancestor {
        cx.emit_edit(node_range, &replacement);
    }
}

/// Finds the `else` keyword token within the `unless...else...end` node,
/// excluding tokens inside child nodes (nested conditionals).
fn find_else_token(node: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let node_range = cx.range(node);
    let children = cx.children(node);
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < node_range.start);

    for tok in &toks[idx..] {
        if tok.range.start >= node_range.end {
            break;
        }
        if tok.kind != SourceTokenKind::Other {
            continue;
        }
        let text = &source[tok.range.start as usize..tok.range.end as usize];
        if text != b"else" {
            continue;
        }
        // Exclude tokens inside child nodes.
        let inside_child = children.iter().any(|&child| {
            let r = cx.range(child);
            tok.range.start >= r.start && tok.range.end <= r.end
        });
        if !inside_child {
            return Some(tok.range);
        }
    }
    None
}

/// Looks for a header separator token (`then` or `;`) between `from` and `to`
/// (both exclusive). Returns the end of the first such token if found.
///
/// Handles both `unless x then` and `unless x; body; else ...` forms.
fn find_then_token_end(from: u32, to: u32, cx: &Cx<'_>) -> Option<u32> {
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < from);
    for tok in &toks[idx..] {
        if tok.range.start >= to {
            break;
        }
        if tok.kind != SourceTokenKind::Other {
            continue;
        }
        let text = &source[tok.range.start as usize..tok.range.end as usize];
        if text == b"then" || text == b";" {
            return Some(tok.range.end);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::UnlessElse;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_and_corrects_unless_else() {
        test::<UnlessElse>().expect_correction(
            indoc! {r#"
                unless x # negative 1
                ^^^^^^^^^^^^^^^^^^^^^ Do not use `unless` with `else`. Rewrite these with the positive case first.
                  a = 1 # negative 2
                else # positive 1
                  a = 0 # positive 2
                end
            "#},
            indoc! {"
                if x # positive 1
                  a = 0 # positive 2
                else # negative 1
                  a = 1 # negative 2
                end
            "},
        );
    }

    #[test]
    fn flags_and_corrects_nested_unless_else_outer_only() {
        test::<UnlessElse>().expect_correction(
            indoc! {r#"
                unless abc
                ^^^^^^^^^^ Do not use `unless` with `else`. Rewrite these with the positive case first.
                  a
                else
                  unless cde
                  ^^^^^^^^^^ Do not use `unless` with `else`. Rewrite these with the positive case first.
                    b
                  else
                    c
                  end
                end
            "#},
            indoc! {"
                if abc
                  unless cde
                    b
                  else
                    c
                  end
                else
                  a
                end
            "},
        );
    }

    #[test]
    fn flags_and_corrects_unless_with_nested_if_else() {
        // Tests the parenthesized condition `unless(x)` (condition is Unknown in
        // Murphy's AST) and verifies that nested if/elsif/else is preserved.
        test::<UnlessElse>().expect_correction(
            indoc! {r#"
                unless(x)
                ^^^^^^^^^ Do not use `unless` with `else`. Rewrite these with the positive case first.
                  if(y == 0)
                    a = 0
                  elsif(z == 0)
                    a = 1
                  else
                    a = 2
                  end
                else
                  a = 3
                end
            "#},
            indoc! {"
                if(x)
                  a = 3
                else
                  if(y == 0)
                    a = 0
                  elsif(z == 0)
                    a = 1
                  else
                    a = 2
                  end
                end
            "},
        );
    }

    #[test]
    fn accepts_unless_without_else() {
        test::<UnlessElse>().expect_no_offenses(indoc! {"
            unless x
              a = 1
            end
        "});
    }

    // Semicolon separator: `unless x; body; else ...`
    #[test]
    fn flags_and_corrects_semicolon_form() {
        test::<UnlessElse>().expect_correction(
            indoc! {r#"
                unless x; a; else b; end
                ^^^^^^^^^^^^^^^^^^^^^^^^ Do not use `unless` with `else`. Rewrite these with the positive case first.
            "#},
            "if x; b; else a; end
",
        );
    }

    // Regression: `then` inside the body must not be treated as the header `then`.
    #[test]
    fn body_with_then_is_not_confused_for_header_then() {
        test::<UnlessElse>().expect_correction(
            indoc! {r#"
                unless condition
                ^^^^^^^^^^^^^^^^ Do not use `unless` with `else`. Rewrite these with the positive case first.
                  if y then
                    a = 1
                  end
                else
                  a = 2
                end
            "#},
            indoc! {"
                if condition
                  a = 2
                else
                  if y then
                    a = 1
                  end
                end
            "},
        );
    }

    #[test]
    fn flags_unless_then_else() {
        test::<UnlessElse>().expect_correction(
            indoc! {r#"
                unless x then
                ^^^^^^^^^^^^^ Do not use `unless` with `else`. Rewrite these with the positive case first.
                  a = 1
                else
                  a = 0
                end
            "#},
            indoc! {"
                if x then
                  a = 0
                else
                  a = 1
                end
            "},
        );
    }
}

murphy_plugin_api::submit_cop!(UnlessElse);
