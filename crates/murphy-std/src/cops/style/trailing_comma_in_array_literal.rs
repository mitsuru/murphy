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
//!   EnforcedStyleForMultiline: only "no_comma" (default) is implemented.
//!   The three comma-adding styles ("comma", "consistent_comma",
//!   "diff_comma") are not yet implemented — they require multiline layout
//!   analysis. "no_comma" flags any trailing comma in a square-bracket array
//!   literal and autocorrects by deleting the comma.
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
//! Only fires when `EnforcedStyleForMultiline` is `no_comma` (the default).
//!
//! ## Autocorrect
//!
//! Deletes the trailing comma token (surgical single-edit delete).

use murphy_plugin_api::{Cx, NoOptions, NodeId, Range, SourceTokenKind, cop};

const MSG: &str = "Avoid comma after the last item of an array.";

/// Stateless unit struct.
#[derive(Default)]
pub struct TrailingCommaInArrayLiteral;

#[cop(
    name = "Style/TrailingCommaInArrayLiteral",
    description = "Checks for trailing comma in array literals.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
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

    // Search for a Comma token in [last_elem_end, array_end).
    let Some(comma_range) = find_trailing_comma(cx, last_elem_end, array_end) else {
        return;
    };

    cx.emit_offense(comma_range, MSG, None);

    // Autocorrect: delete the trailing comma.
    cx.emit_edit(comma_range, "");
}

/// Find a trailing comma token in the source range `[from, to)`.
/// Returns `None` if no comma is found.
fn find_trailing_comma(cx: &Cx<'_>, from: u32, to: u32) -> Option<Range> {
    if from >= to {
        return None;
    }
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < from);
    for tok in &toks[idx..] {
        if tok.range.start >= to {
            break;
        }
        if tok.kind == SourceTokenKind::Comma {
            return Some(tok.range);
        }
    }
    None
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
