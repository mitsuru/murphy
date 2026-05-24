//! `Layout/SpaceInsideParens` — flags extra spaces immediately inside
//! parentheses. Mirrors RuboCop's same-named cop.

use murphy_plugin_api::{Cx, NoOptions, Range, SourceTokenKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SpaceInsideParens;

#[cop(
    name = "Layout/SpaceInsideParens",
    description = "Flag extra spaces immediately inside parentheses.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl SpaceInsideParens {
    #[on_new_investigation]
    fn investigate(&self, cx: &Cx<'_>) {
        check_token_adjacency(cx);
    }
}

fn check_token_adjacency(cx: &Cx<'_>) {
    for pair in cx.sorted_tokens().windows(2) {
        let left = pair[0];
        let right = pair[1];

        match (left.kind, right.kind) {
            (SourceTokenKind::LeftParen, SourceTokenKind::Comment) => {}
            (SourceTokenKind::LeftParen, _) => {
                emit_inline_gap(cx, left.range.end, right.range.start)
            }
            (_, SourceTokenKind::RightParen) if left.kind != SourceTokenKind::LeftParen => {
                emit_inline_gap(cx, left.range.end, right.range.start);
            }
            _ => {}
        }
    }
}

fn emit_inline_gap(cx: &Cx<'_>, start: u32, end: u32) {
    if start >= end {
        return;
    }

    let range = Range { start, end };
    let gap = cx.raw_source(range);
    if !gap.bytes().all(|b| matches!(b, b' ' | b'\t')) {
        return;
    }

    cx.emit_offense(range, "Space inside parentheses detected", None);
    cx.emit_edit(range, "");
}
