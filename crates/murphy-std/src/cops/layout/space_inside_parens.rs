//! `Layout/SpaceInsideParens` — flags extra spaces immediately inside
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/SpaceInsideParens
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues:
//!   - murphy-pvyl
//! notes: >
//!   Implementation appears broadly aligned, but expanded RuboCop spec parity coverage remains open.
//! ```
//!
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
            // A newline immediately after `(` means the call is multiline —
            // the gap to the newline token is trailing whitespace on that line,
            // not "space inside parentheses".  TrailingWhitespace owns that.
            (
                SourceTokenKind::LeftParen,
                SourceTokenKind::Newline | SourceTokenKind::IgnoredNewline,
            ) => {}
            (SourceTokenKind::LeftParen, _) => {
                emit_inline_gap(cx, left.range.end, right.range.start)
            }
            // A newline token just before `)` means `)` is on its own indented
            // line.  The gap is the indentation, not inline "space inside parens".
            (
                SourceTokenKind::Newline | SourceTokenKind::IgnoredNewline,
                SourceTokenKind::RightParen,
            ) => {}
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
    // Prism reports heredoc opener tokens with range.end past the body, so
    // sorted_tokens().windows(2) can yield reversed pairs where
    // left.range.end > right.range.start. Also, the forward pair `(` -> heredoc
    // opener has no meaningful inline gap because the heredoc body interleaves
    // between the opener and the matching `)`. Skip any pair involving a
    // heredoc boundary.
    if left.range.end > right.range.start {
        return true;
    }
    if is_heredoc_boundary(left) || is_heredoc_boundary(right) {
        return true;
    }
    if !parens(left, right) {
        return true;
    }
    if empty_parens(left, right) {
        return true;
    }
    if right.kind == SourceTokenKind::Comment {
        return true;
    }
    // A newline token directly bounding the paren means the call is multiline.
    // RuboCop's `space`/`compact` styles don't require inline spaces across
    // line breaks: `(` followed immediately by a newline, or `)` preceded
    // directly by a newline token, are both exempt.
    if matches!(
        left.kind,
        SourceTokenKind::Newline | SourceTokenKind::IgnoredNewline
    ) || matches!(
        right.kind,
        SourceTokenKind::Newline | SourceTokenKind::IgnoredNewline
    ) {
        return true;
    }
    !same_line_gap(cx, left.range.end, right.range.start)
        || has_space_after(cx, left.range.end, right.range.start)
}

fn is_heredoc_boundary(token: SourceToken) -> bool {
    matches!(
        token.kind,
        SourceTokenKind::HeredocStart | SourceTokenKind::HeredocEnd
    )
}

fn remove_space_in_empty_parens(cx: &Cx<'_>, left: SourceToken, right: SourceToken) {
    if left.kind != SourceTokenKind::LeftParen || right.kind != SourceTokenKind::RightParen {
        return;
    }
    // `>=` (not `==`) so a reversed-range pair from upstream token-emission
    // quirks (see can_ignore_missing_space) does not slice raw_source backwards
    // and panic. The `==` form covers truly empty `()`; the `>` form covers the
    // defensive reversed case.
    if left.range.end >= right.range.start {
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
    use super::{SpaceInsideParens, SpaceInsideParensOptions, SpaceInsideParensStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn corrects_spaces_inside_parentheses() {
        test::<SpaceInsideParens>().expect_correction(
            indoc! {r#"
                foo( 1)
                    ^ Space inside parentheses detected.
                bar(1 )
                     ^ Space inside parentheses detected.
            "#},
            "foo(1)\nbar(1)\n",
        );
    }

    #[test]
    fn leaves_clean_parentheses_without_corrections() {
        test::<SpaceInsideParens>().expect_no_corrections("foo(1, 2)\nbar()\n");
    }

    #[test]
    fn does_not_flag_indented_closing_paren_in_method_body() {
        test::<SpaceInsideParens>().expect_no_offenses(indoc! {r#"
            def foo
              a_request(
                :post,
                endpoint
              ).with(
                headers: {}
              )
            end
        "#});
    }

    #[test]
    fn space_style_does_not_flag_multiline_calls() {
        let opts = SpaceInsideParensOptions {
            enforced_style: SpaceInsideParensStyle::Space,
        };
        test::<SpaceInsideParens>()
            .with_options(&opts)
            .expect_no_offenses(indoc! {r#"
            def foo
              a_request(
                :post,
                endpoint
              ).with(
                headers: {}
              )
            end
        "#});
    }

    #[test]
    fn compact_style_does_not_flag_multiline_calls() {
        let opts = SpaceInsideParensOptions {
            enforced_style: SpaceInsideParensStyle::Compact,
        };
        test::<SpaceInsideParens>()
            .with_options(&opts)
            .expect_no_offenses(indoc! {r#"
            def foo
              a_request(
                :post,
                endpoint
              ).with(
                headers: {}
              )
            end
        "#});
    }
}
