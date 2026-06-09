//! `Style/TrailingCommaInArrayLiteral` — checks for trailing comma in array
//! literals.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/TrailingCommaInArrayLiteral
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Supports EnforcedStyleForMultiline values "no_comma" (default), "comma",
//!   "consistent_comma", and "diff_comma" for square-bracket array literals.
//!   Single-line arrays never permit a trailing comma. Multiline comma-adding
//!   styles insert a comma after the last element when required.
//!   Percent-literal arrays (%w/%i/%W/%I) are skipped (no trailing comma
//!   syntax).
//!   Heredoc-as-last-element edge case is not handled (conservative skip not
//!   implemented; acceptable gap for v1).
//! ```
//!
//! ## Matched shapes
//!
//! Array literals written with `[...]` delimiters whose last element is
//! followed by a comma before the closing `]`.
//!
//! Honors `EnforcedStyleForMultiline` for square-bracket arrays.
//!
//! ## Autocorrect
//!
//! Deletes or inserts the trailing comma token (surgical single-edit change).

use murphy_plugin_api::{
    cop, CopOptionEnum, CopOptions, Cx, NodeId, Range, SourceToken, SourceTokenKind,
};

const MSG: &str = "Avoid comma after the last item of an array.";

/// Stateless unit struct.
#[derive(Default)]
pub struct TrailingCommaInArrayLiteral;

#[derive(CopOptions)]
pub struct TrailingCommaInArrayLiteralOptions {
    #[option(
        name = "EnforcedStyleForMultiline",
        default = "no_comma",
        description = "Controls when trailing commas are required or forbidden in multiline array literals."
    )]
    pub enforced_style_for_multiline: TrailingCommaStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrailingCommaStyle {
    #[option(value = "no_comma")]
    #[default]
    NoComma,
    #[option(value = "comma")]
    Comma,
    #[option(value = "consistent_comma")]
    ConsistentComma,
    #[option(value = "diff_comma")]
    DiffComma,
}

#[cop(
    name = "Style/TrailingCommaInArrayLiteral",
    description = "Checks for trailing comma in array literals.",
    default_severity = "warning",
    default_enabled = true,
    options = TrailingCommaInArrayLiteralOptions,
)]
impl TrailingCommaInArrayLiteral {
    #[on_node(kind = "array")]
    fn check_array(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Only process square-bracket array literals; skip %w/%W/%i/%I.
    if !cx.is_square_brackets(node) {
        return;
    }

    // Array must have at least one element to have a trailing comma.
    let elements = cx.array_elements(node);
    if elements.is_empty() {
        return;
    }

    let last_elem = *elements.last().unwrap();
    let last_elem_end = cx.range(last_elem).end;
    let array_end = cx.range(node).end;

    let Some(close_tok) = find_closing_bracket(cx, last_elem_end, array_end) else {
        return;
    };

    // Search for a Comma token in [last_elem_end, array_end).
    let comma_range = find_trailing_comma(cx, last_elem_end, close_tok.range.start);
    let opts = cx.options_or_default::<TrailingCommaInArrayLiteralOptions>();

    if let Some(comma_range) = comma_range {
        if !cx.is_single_line(node)
            && should_have_comma(
                opts.enforced_style_for_multiline,
                node,
                last_elem,
                close_tok,
                cx,
            )
        {
            return;
        }

        cx.emit_offense(comma_range, MSG, None);

        // Autocorrect: delete the trailing comma.
        cx.emit_edit(comma_range, "");
        return;
    }

    if cx.is_single_line(node)
        || !should_have_comma(
            opts.enforced_style_for_multiline,
            node,
            last_elem,
            close_tok,
            cx,
        )
    {
        return;
    }

    let msg = "Put a comma after the last item of a multiline array.";
    let insert = Range {
        start: last_elem_end,
        end: last_elem_end,
    };
    cx.emit_offense(insert, msg, None);
    cx.emit_edit(insert, ",");
}

/// Find a trailing comma token in the source range `[from, to)`.
/// Returns `None` if no comma is found.
fn find_trailing_comma(cx: &Cx<'_>, from: u32, to: u32) -> Option<Range> {
    cx.tokens_in(Range {
        start: from,
        end: to,
    })
    .iter()
    .find(|tok| tok.kind == SourceTokenKind::Comma)
    .map(|tok| tok.range)
}

fn find_closing_bracket(cx: &Cx<'_>, from: u32, to: u32) -> Option<SourceToken> {
    let source = cx.source().as_bytes();
    cx.tokens_in(Range {
        start: from,
        end: to,
    })
    .iter()
    .find(|tok| {
        tok.kind == SourceTokenKind::Other
            && &source[tok.range.start as usize..tok.range.end as usize] == b"]"
    })
    .copied()
}

fn should_have_comma(
    style: TrailingCommaStyle,
    array: NodeId,
    last_elem: NodeId,
    close_tok: SourceToken,
    cx: &Cx<'_>,
) -> bool {
    match style {
        TrailingCommaStyle::NoComma => false,
        TrailingCommaStyle::Comma => elements_and_close_on_separate_lines(array, close_tok, cx),
        TrailingCommaStyle::ConsistentComma => cx.is_multiline(array),
        TrailingCommaStyle::DiffComma => {
            has_newline_between(cx.range(last_elem).end, close_tok.range.start, cx)
        }
    }
}

fn elements_and_close_on_separate_lines(
    array: NodeId,
    close_tok: SourceToken,
    cx: &Cx<'_>,
) -> bool {
    let elements = cx.array_elements(array);
    if elements.is_empty() || !cx.is_multiline(array) {
        return false;
    }

    let Some(open_tok) =
        find_opening_bracket(cx, cx.range(array).start, cx.range(elements[0]).start)
    else {
        return false;
    };
    if !has_newline_between(open_tok.range.end, cx.range(elements[0]).start, cx) {
        return false;
    }

    for window in elements.windows(2) {
        if !has_newline_between(cx.range(window[0]).end, cx.range(window[1]).start, cx) {
            return false;
        }
    }

    let last_elem = *elements.last().expect("non-empty array elements");
    has_newline_between(cx.range(last_elem).end, close_tok.range.start, cx)
}

fn find_opening_bracket(cx: &Cx<'_>, from: u32, to: u32) -> Option<SourceToken> {
    let source = cx.source().as_bytes();
    cx.tokens_in(Range {
        start: from,
        end: to,
    })
    .iter()
    .find(|tok| {
        tok.kind == SourceTokenKind::Other
            && &source[tok.range.start as usize..tok.range.end as usize] == b"["
    })
    .copied()
}

fn has_newline_between(from: u32, to: u32, cx: &Cx<'_>) -> bool {
    if from >= to {
        return false;
    }
    cx.source().as_bytes()[from as usize..to as usize].contains(&b'\n')
}

#[cfg(test)]
mod tests {
    use super::*;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- no offense ---

    #[test]
    fn no_offense_empty_array() {
        test::<TrailingCommaInArrayLiteral>().expect_no_offenses("x = []\n");
    }

    #[test]
    fn no_offense_single_element_no_comma() {
        test::<TrailingCommaInArrayLiteral>().expect_no_offenses("x = [1]\n");
    }

    #[test]
    fn no_offense_multi_element_no_trailing_comma() {
        test::<TrailingCommaInArrayLiteral>().expect_no_offenses("x = [1, 2, 3]\n");
    }

    #[test]
    fn no_offense_multiline_no_trailing_comma() {
        test::<TrailingCommaInArrayLiteral>().expect_no_offenses(indoc! {"
            x = [
              1,
              2,
              3
            ]
        "});
    }

    #[test]
    fn no_offense_percent_w_array() {
        test::<TrailingCommaInArrayLiteral>().expect_no_offenses("x = %w[foo bar]\n");
    }

    #[test]
    fn no_offense_percent_i_array() {
        test::<TrailingCommaInArrayLiteral>().expect_no_offenses("x = %i[foo bar]\n");
    }

    #[test]
    fn comma_style_accepts_multiline_trailing_comma() {
        test::<TrailingCommaInArrayLiteral>()
            .with_options(&TrailingCommaInArrayLiteralOptions {
                enforced_style_for_multiline: TrailingCommaStyle::Comma,
            })
            .expect_no_offenses(indoc! {"
                x = [
                  1,
                  2,
                ]
            "});
    }

    #[test]
    fn comma_style_does_not_require_comma_for_nested_single_element_on_opener_line() {
        test::<TrailingCommaInArrayLiteral>()
            .with_options(&TrailingCommaInArrayLiteralOptions {
                enforced_style_for_multiline: TrailingCommaStyle::Comma,
            })
            .expect_no_offenses(indoc! {"
                value = [[{
                  'name' => ['Author 1'],
                }]]
            "});
    }

    #[test]
    fn comma_style_does_not_require_comma_when_close_bracket_shares_last_element_line() {
        test::<TrailingCommaInArrayLiteral>()
            .with_options(&TrailingCommaInArrayLiteralOptions {
                enforced_style_for_multiline: TrailingCommaStyle::Comma,
            })
            .expect_no_offenses(indoc! {"
                x = [
                  method_call(
                    1)]
            "});
    }

    // --- offense: single-line trailing comma ---

    #[test]
    fn flags_single_line_trailing_comma() {
        // Comma is at index 12 in "x = [1, 2, 3,]"
        test::<TrailingCommaInArrayLiteral>().expect_offense(indoc! {r#"
            x = [1, 2, 3,]
                        ^ Avoid comma after the last item of an array.
        "#});
    }

    #[test]
    fn flags_single_element_trailing_comma() {
        // Comma is at index 6 in "x = [1,]"
        test::<TrailingCommaInArrayLiteral>().expect_offense(indoc! {r#"
            x = [1,]
                  ^ Avoid comma after the last item of an array.
        "#});
    }

    // --- offense: multiline trailing comma ---

    #[test]
    fn flags_multiline_trailing_comma() {
        // Comma is at index 3 in "  3,"
        test::<TrailingCommaInArrayLiteral>().expect_offense(indoc! {r#"
            x = [
              1,
              2,
              3,
               ^ Avoid comma after the last item of an array.
            ]
        "#});
    }

    // --- autocorrect ---

    #[test]
    fn corrects_single_line_trailing_comma() {
        test::<TrailingCommaInArrayLiteral>().expect_correction(
            indoc! {r#"
                x = [1, 2, 3,]
                            ^ Avoid comma after the last item of an array.
            "#},
            "x = [1, 2, 3]\n",
        );
    }

    #[test]
    fn corrects_multiline_trailing_comma() {
        test::<TrailingCommaInArrayLiteral>().expect_correction(
            indoc! {r#"
                x = [
                  1,
                  2,
                  3,
                   ^ Avoid comma after the last item of an array.
                ]
            "#},
            "x = [\n  1,\n  2,\n  3\n]\n",
        );
    }

    #[test]
    fn corrects_single_element_trailing_comma() {
        test::<TrailingCommaInArrayLiteral>().expect_correction(
            indoc! {r#"
                x = [1,]
                      ^ Avoid comma after the last item of an array.
            "#},
            "x = [1]\n",
        );
    }
}

murphy_plugin_api::submit_cop!(TrailingCommaInArrayLiteral);
