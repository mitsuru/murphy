//! `Layout/SpaceInsideParens` — flags extra spaces immediately inside
//! parentheses. Mirrors RuboCop's same-named cop.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, Range, SourceToken, SourceTokenKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SpaceInsideParens;

#[derive(CopOptions)]
pub struct SpaceInsideParensOptions {
    #[option(
        name = "EnforcedStyle",
        default = "no_space",
        description = "Parenthesis spacing style."
    )]
    pub enforced_style: SpaceInsideParensStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum SpaceInsideParensStyle {
    #[option(value = "no_space")]
    NoSpace,
    #[option(value = "space")]
    Space,
    #[option(value = "compact")]
    Compact,
}

#[cop(
    name = "Layout/SpaceInsideParens",
    description = "Flag extra spaces immediately inside parentheses.",
    default_severity = "warning",
    default_enabled = true,
    options = SpaceInsideParensOptions,
)]
impl SpaceInsideParens {
    #[on_new_investigation]
    fn investigate(&self, cx: &Cx<'_>, options: &SpaceInsideParensOptions) {
        match options.enforced_style {
            SpaceInsideParensStyle::Space => check_space_style(cx),
            SpaceInsideParensStyle::Compact => check_compact_style(cx),
            SpaceInsideParensStyle::NoSpace => check_no_space_style(cx),
        }
    }
}

fn check_no_space_style(cx: &Cx<'_>) {
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

    cx.emit_offense(range, "Space inside parentheses detected.", None);
    cx.emit_edit(range, "");
}

fn check_space_style(cx: &Cx<'_>) {
    for pair in cx.sorted_tokens().windows(2) {
        let left = pair[0];
        let right = pair[1];
        remove_space_in_empty_parens(cx, left, right);
        add_missing_space(cx, left, right);
    }
}

fn check_compact_style(cx: &Cx<'_>) {
    for pair in cx.sorted_tokens().windows(2) {
        let left = pair[0];
        let right = pair[1];
        remove_space_in_empty_parens(cx, left, right);
        if consecutive_parens(left, right) {
            remove_single_space_between_consecutive_parens(cx, left, right);
        } else {
            add_missing_space(cx, left, right);
        }
    }
}

fn add_missing_space(cx: &Cx<'_>, left: SourceToken, right: SourceToken) {
    if can_ignore_missing_space(cx, left, right) {
        return;
    }

    let offset = right.range.start;
    let range = Range {
        start: offset,
        end: offset,
    };
    cx.emit_offense(range, "No space inside parentheses detected.", None);
    cx.emit_edit(range, " ");
}

fn can_ignore_missing_space(cx: &Cx<'_>, left: SourceToken, right: SourceToken) -> bool {
    if !parens(left, right) {
        return true;
    }
    if empty_parens(left, right) {
        return true;
    }
    if right.kind == SourceTokenKind::Comment {
        return true;
    }
    !same_line_gap(cx, left.range.end, right.range.start)
        || has_space_after(cx, left.range.end, right.range.start)
}

fn remove_space_in_empty_parens(cx: &Cx<'_>, left: SourceToken, right: SourceToken) {
    if left.kind != SourceTokenKind::LeftParen || right.kind != SourceTokenKind::RightParen {
        return;
    }
    if left.range.end == right.range.start {
        return;
    }

    let range = Range {
        start: left.range.end,
        end: right.range.start,
    };
    cx.emit_offense(range, "Space inside parentheses detected.", None);
    cx.emit_edit(range, "");
}

fn remove_single_space_between_consecutive_parens(
    cx: &Cx<'_>,
    left: SourceToken,
    right: SourceToken,
) {
    let range = Range {
        start: left.range.end,
        end: right.range.start,
    };
    if range.start >= range.end {
        return;
    }
    let gap = cx.raw_source(range);
    if !gap.bytes().all(|b| matches!(b, b' ' | b'\t')) {
        return;
    }

    cx.emit_offense(range, "Space inside parentheses detected.", None);
    cx.emit_edit(range, "");
}

fn parens(left: SourceToken, right: SourceToken) -> bool {
    left.kind == SourceTokenKind::LeftParen || right.kind == SourceTokenKind::RightParen
}

fn consecutive_parens(left: SourceToken, right: SourceToken) -> bool {
    matches!(
        (left.kind, right.kind),
        (SourceTokenKind::LeftParen, SourceTokenKind::LeftParen)
            | (SourceTokenKind::RightParen, SourceTokenKind::RightParen)
    )
}

fn empty_parens(left: SourceToken, right: SourceToken) -> bool {
    left.kind == SourceTokenKind::LeftParen && right.kind == SourceTokenKind::RightParen
}

fn same_line_gap(cx: &Cx<'_>, start: u32, end: u32) -> bool {
    cx.raw_source(Range { start, end })
        .bytes()
        .all(|b| b != b'\n' && b != b'\r')
}

fn has_space_after(cx: &Cx<'_>, start: u32, end: u32) -> bool {
    start < end && cx.raw_source(Range { start, end }).starts_with(' ')
}

#[cfg(test)]
mod tests {
    use super::SpaceInsideParens;
    use murphy_plugin_api::test_support::{expect_correction, expect_no_corrections, indoc};

    #[test]
    fn corrects_spaces_inside_parentheses() {
        expect_correction!(
            SpaceInsideParens,
            indoc! {r#"
                foo( 1)
                    ^ Space inside parentheses detected.
                bar(1 )
                     ^ Space inside parentheses detected.
            "#},
            "foo(1)\nbar(1)\n"
        );
    }

    #[test]
    fn leaves_clean_parentheses_without_corrections() {
        expect_no_corrections!(SpaceInsideParens, "foo(1, 2)\nbar()\n");
    }
}
