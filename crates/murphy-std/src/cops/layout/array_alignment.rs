//! `Layout/ArrayAlignment` — the elements of a multi-line array literal must be
//! aligned.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/ArrayAlignment
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Ports `on_array` plus the `Alignment#check_alignment` /
//!   `each_bad_alignment` spine. Fires on every array node with two or more
//!   children whose parent is not a `masgn` (RuboCop's
//!   `return if node.parent&.masgn_type?`). Each element that *begins its own
//!   line* must sit at the base column; misaligned ones are flagged.
//!
//!   - with_first_element (default): base column = the first element's display
//!     column.
//!   - with_fixed_indentation: base column = the indentation of the array's
//!     anchor line (the `[` line when bracketed, otherwise the parent's line)
//!     plus the configured indentation width (default 2).
//!
//!   Columns use `.chars().count()` from the line start so multi-byte source
//!   aligns by visible column, matching RuboCop's `display_column` (modulo full
//!   Unicode east-asian-width handling, a known minor gap shared with the other
//!   alignment cops).
//!
//!   Autocorrect: not implemented (v1 gap). RuboCop shifts each misaligned
//!   element to the base column via `AlignmentCorrector`, which also re-indents
//!   continuation lines of multi-line elements — a shape `reindent_line` does
//!   not model. The detect-only port matches the `Layout/ParameterAlignment`
//!   precedent and ships without it.
//! ```
//!
//! ## Matched shapes
//!
//! `array` nodes with `children.size >= 2` and a non-`masgn` parent where a
//! later element begins its own line at a column other than the base column.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

const ALIGN_ELEMENTS_MSG: &str =
    "Align the elements of an array literal if they span more than one line.";
const FIXED_INDENT_MSG: &str = "Use one level of indentation for elements \
    following the first line of a multi-line array.";

/// Stateless unit struct (ADR 0035).
#[derive(Default)]
pub struct ArrayAlignment;

/// Options for [`ArrayAlignment`]. `EnforcedStyle` matches RuboCop verbatim;
/// the default is `with_first_element`. `IndentationWidth` overrides the
/// indentation width used by `with_fixed_indentation` (default 2, mirroring
/// `Layout/IndentationWidth`).
#[derive(CopOptions)]
pub struct ArrayAlignmentOptions {
    #[option(
        name = "EnforcedStyle",
        default = "with_first_element",
        description = "How to align elements following the first line of a multi-line array."
    )]
    pub enforced_style: ArrayAlignmentStyle,
    // `Option<i64>` (not `i64`) so the bundled default `IndentationWidth: ~`
    // (which merges to JSON `null`) decodes to `None` instead of erroring the
    // whole option struct and silently discarding the user's `EnforcedStyle`.
    #[option(
        name = "IndentationWidth",
        description = "Indentation width for `with_fixed_indentation` (null/unset falls back to RuboCop's default of 2)."
    )]
    pub indentation_width: Option<i64>,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum ArrayAlignmentStyle {
    /// Align with the first element's column.
    #[option(value = "with_first_element")]
    WithFirstElement,
    /// Indent one level past the array's anchor line.
    #[option(value = "with_fixed_indentation")]
    WithFixedIndentation,
}

#[cop(
    name = "Layout/ArrayAlignment",
    description = "Align the elements of a multi-line array literal.",
    default_severity = "warning",
    default_enabled = true,
    options = ArrayAlignmentOptions,
)]
impl ArrayAlignment {
    #[on_node(kind = "array")]
    fn check_array(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// Visible column (0-based, char count) of a byte offset within its line.
fn display_column(offset: u32, src: &str) -> usize {
    let line_start = src[..offset as usize].rfind('\n').map_or(0, |p| p + 1);
    src[line_start..offset as usize].chars().count()
}

/// Returns true when `offset` is the first non-whitespace byte on its line.
fn begins_its_line(offset: u32, src: &str) -> bool {
    let bytes = src.as_bytes();
    let line_start = src[..offset as usize].rfind('\n').map_or(0, |p| p + 1);
    bytes[line_start..offset as usize]
        .iter()
        .all(|&b| b == b' ' || b == b'\t')
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let opts = cx.options_or_default::<ArrayAlignmentOptions>();

    let elements = cx.array_elements(node);
    // RuboCop: `return if node.children.size < 2`.
    if elements.len() < 2 {
        return;
    }
    // RuboCop: `return if node.parent&.masgn_type?`.
    if let Some(parent) = cx.parent(node).get()
        && matches!(cx.kind(parent), NodeKind::Masgn { .. })
    {
        return;
    }

    let src = cx.source();
    let fixed = opts.enforced_style == ArrayAlignmentStyle::WithFixedIndentation;

    // Base column: first element's display column (with_first_element), or the
    // anchor line's indentation + indentation width (with_fixed_indentation).
    let base_column = if fixed {
        let anchor = anchor_line_offset(node, cx);
        first_non_ws_column(anchor, src) + indentation_width(&opts)
    } else {
        display_column(cx.range(elements[0]).start, src)
    };

    let msg = if fixed {
        FIXED_INDENT_MSG
    } else {
        ALIGN_ELEMENTS_MSG
    };

    // Each element that begins its own line must sit at `base_column`.
    for &element in elements {
        let start = cx.range(element).start;
        if !begins_its_line(start, src) {
            continue;
        }
        if display_column(start, src) != base_column {
            cx.emit_offense(offending_range(element, cx), msg, None);
        }
    }
}

/// Configured indentation width for `with_fixed_indentation` (null/non-positive
/// → default 2).
fn indentation_width(opts: &ArrayAlignmentOptions) -> usize {
    opts.indentation_width.filter(|&w| w > 0).map_or(2, |w| w as usize)
}

/// RuboCop's `target_method_lineno`: the array's `[` line when bracketed,
/// otherwise the parent's line. We return a byte offset on that line so the
/// caller can compute its indentation column.
fn anchor_line_offset(node: NodeId, cx: &Cx<'_>) -> u32 {
    if is_bracketed(node, cx) {
        cx.range(node).start
    } else if let Some(parent) = cx.parent(node).get() {
        cx.range(parent).start
    } else {
        cx.range(node).start
    }
}

/// RuboCop's `ArrayNode#bracketed?` (`square_brackets? || percent_literal?`):
/// true when the array has an opening delimiter — `[`, or any percent literal
/// (`%w[…]`, `%i(…)`, …). For an array, `loc.begin` is always either `[` or
/// starts with `%`, so `bracketed?` is exactly "has a begin delimiter". A
/// bracketless array (`return 1, 2`) begins at its first element, so
/// `node.start < first_element.start` is the faithful test — and it correctly
/// rejects a bracketless array whose first element is itself a percent literal
/// (`return %w[a], 2`), unlike a `starts_with('%')` source check. Murphy does
/// not populate a begin-delimiter loc for array nodes, hence the positional test.
fn is_bracketed(node: NodeId, cx: &Cx<'_>) -> bool {
    match cx.array_elements(node).first() {
        Some(&first) => cx.range(node).start < cx.range(first).start,
        None => true,
    }
}

/// Column of the first non-whitespace char on the line containing `offset`.
fn first_non_ws_column(offset: u32, src: &str) -> usize {
    let off = offset as usize;
    let line_start = src[..off].rfind('\n').map_or(0, |p| p + 1);
    let line_end = src[line_start..]
        .find('\n')
        .map_or(src.len(), |p| line_start + p);
    src[line_start..line_end]
        .chars()
        .position(|c| !c.is_whitespace())
        .unwrap_or(0)
}

/// Highlight the offending element, trimmed to its first line.
fn offending_range(element: NodeId, cx: &Cx<'_>) -> Range {
    let r = cx.range(element);
    let src = cx.source().as_bytes();
    let line_end = src[r.start as usize..r.end as usize]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(r.end, |pos| r.start + pos as u32);
    Range {
        start: r.start,
        end: line_end,
    }
}

#[cfg(test)]
mod tests {
    use super::{ArrayAlignment, ArrayAlignmentOptions, ArrayAlignmentStyle};
    use murphy_plugin_api::CopOptions;
    use murphy_plugin_api::test_support::{indoc, test};

    fn fixed() -> ArrayAlignmentOptions {
        ArrayAlignmentOptions {
            enforced_style: ArrayAlignmentStyle::WithFixedIndentation,
            indentation_width: None,
        }
    }

    /// Regression (Codex #384): bundled default `IndentationWidth: ~` → JSON
    /// `null`. It must decode (as `Option<i64>`) rather than erroring the struct
    /// and discarding the user's `EnforcedStyle`.
    #[test]
    fn null_indentation_width_preserves_other_keys() {
        let opts = <ArrayAlignmentOptions as CopOptions>::from_config_json(
            br#"{"EnforcedStyle":"with_fixed_indentation","IndentationWidth":null}"#,
        )
        .expect("null IndentationWidth must decode, not discard the struct");
        assert!(opts.enforced_style == ArrayAlignmentStyle::WithFixedIndentation);
    }

    /// Parity pin (Codex #384): RuboCop's `ArrayNode#bracketed?`
    /// (`square_brackets? || percent_literal?`) treats `%w[…]`/`%i[…]` as
    /// bracketed, so `with_fixed_indentation` anchors to the percent array's own
    /// line (here indent 2 + one level = 4), not the enclosing `foo(` line.
    #[test]
    fn fixed_treats_percent_array_as_bracketed() {
        test::<ArrayAlignment>()
            .with_options(&fixed())
            .expect_no_offenses(indoc! {"
                foo(
                  %w[one
                    two]
                )
            "});
    }

    /// Companion: a *bracketless* array (`return 1, 2`) has no opening delimiter,
    /// so `node.start == first_element.start` and it anchors to the parent line
    /// (the `return` line, indent 2 + one level = 4). Pins the false direction of
    /// the `is_bracketed` rewrite.
    #[test]
    fn fixed_bracketless_array_anchors_to_parent_line() {
        test::<ArrayAlignment>()
            .with_options(&fixed())
            .expect_no_offenses(indoc! {"
                def f
                  return 1,
                    2
                end
            "});
    }

    // with_first_element (default) ----------------------------------------

    #[test]
    fn accepts_aligned_with_first_element() {
        test::<ArrayAlignment>().expect_no_offenses(indoc! {"
            array = [1, 2, 3,
                     4, 5, 6]
        "});
    }

    #[test]
    fn accepts_each_on_own_line_aligned() {
        test::<ArrayAlignment>().expect_no_offenses(indoc! {"
            array = ['run',
                     'forrest',
                     'run']
        "});
    }

    #[test]
    fn flags_misaligned_second_line() {
        test::<ArrayAlignment>().expect_offense(indoc! {"
            array = [1, 2, 3,
              4, 5, 6]
              ^ Align the elements of an array literal if they span more than one line.
        "});
    }

    #[test]
    fn flags_each_misaligned_line() {
        test::<ArrayAlignment>().expect_offense(indoc! {"
            array = ['run',
                 'forrest',
                 ^^^^^^^^^ Align the elements of an array literal if they span more than one line.
                 'run']
                 ^^^^^ Align the elements of an array literal if they span more than one line.
        "});
    }

    #[test]
    fn accepts_single_line_array() {
        test::<ArrayAlignment>().expect_no_offenses(indoc! {"
            array = [1, 2, 3]
        "});
    }

    #[test]
    fn accepts_single_element() {
        test::<ArrayAlignment>().expect_no_offenses(indoc! {"
            array = [
              1
            ]
        "});
    }

    #[test]
    fn ignores_multiple_assignment_rhs() {
        test::<ArrayAlignment>().expect_no_offenses(indoc! {"
            a, b = 1,
              2
        "});
    }

    // with_fixed_indentation ----------------------------------------------

    #[test]
    fn fixed_accepts_one_level_indentation() {
        test::<ArrayAlignment>()
            .with_options(&fixed())
            .expect_no_offenses(indoc! {"
                array = [1, 2, 3,
                  4, 5, 6]
            "});
    }

    #[test]
    fn fixed_flags_aligned_with_first_element() {
        test::<ArrayAlignment>()
            .with_options(&fixed())
            .expect_offense(indoc! {"
                array = [1, 2, 3,
                         4, 5, 6]
                         ^ Use one level of indentation for elements following the first line of a multi-line array.
            "});
    }
}

murphy_plugin_api::submit_cop!(ArrayAlignment);
