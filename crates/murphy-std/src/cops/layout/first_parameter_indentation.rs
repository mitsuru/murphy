//! `Layout/FirstParameterIndentation` — checks the indentation of the first
//! parameter in a method definition.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/FirstParameterIndentation
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues:
//!   - murphy-4u8u
//! notes: >
//!   Mirrors RuboCop's `on_def`/`on_defs` via the `MultilineElementIndentation`
//!   mixin's `check_first`. A method definition whose parameter list opens with
//!   `(` and whose first parameter starts on a *later* line than the `(` is
//!   checked: the first parameter's column must equal an expected column
//!   derived from the `EnforcedStyle`:
//!     * `consistent` (default): the column of the first non-whitespace
//!       character on the line containing `(`, plus the indentation width.
//!     * `align_parentheses` (`brace_alignment_style`): the `(` column plus
//!       the indentation width.
//!   When the actual column differs (RuboCop's non-zero `@column_delta`), an
//!   offense is emitted at the first parameter and an autocorrect rewrites the
//!   first parameter line's leading whitespace to the expected column.
//!   `configured_indentation_width` matches RuboCop: this cop's own
//!   `IndentationWidth` override is honoured, and when unset the width falls
//!   back to the run-wide resolved `Layout/IndentationWidth.Width` via
//!   `cx.indentation_width()` (default 2) — murphy-kke2.
//!   Known gaps versus RuboCop:
//!   - The ambiguous/correct-style bookkeeping (`ambiguous_style_detected`,
//!     `SupportedStyles` auto-detection) is not modelled; only the active
//!     `EnforcedStyle` is enforced.
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, Range, cop};

/// Stateless unit struct (ADR 0035 const-metadata cop pattern).
#[derive(Default)]
pub struct FirstParameterIndentation;

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum FirstParameterIndentationStyle {
    /// First parameter indented one step past the start of the `(` line.
    #[option(value = "consistent")]
    Consistent,
    /// First parameter indented one step past the `(` column.
    #[option(value = "align_parentheses")]
    AlignParentheses,
}

#[derive(CopOptions)]
pub struct FirstParameterIndentationOptions {
    #[option(
        name = "EnforcedStyle",
        default = "consistent",
        description = "Where the first parameter should be indented relative to."
    )]
    pub enforced_style: FirstParameterIndentationStyle,
    // `Option<i64>` so the bundled default `IndentationWidth: ~` (JSON null) and
    // an unset key both decode to `None`, which falls back to the run-wide
    // resolved `Layout/IndentationWidth.Width` via `cx.indentation_width()`.
    #[option(
        name = "IndentationWidth",
        description = "Indentation width in spaces (null/unset falls back to Layout/IndentationWidth's Width, default 2)."
    )]
    pub indentation_width: Option<i64>,
}

#[cop(
    name = "Layout/FirstParameterIndentation",
    description = "Use the configured number of spaces to indent the first parameter of a multi-line method definition.",
    default_severity = "warning",
    default_enabled = true,
    options = FirstParameterIndentationOptions,
)]
impl FirstParameterIndentation {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // `return if node.arguments.empty?` and `return if loc.begin.nil?`.
    let Some(first_param) = first_parameter(node, cx) else {
        return;
    };
    // RuboCop's `def_node.arguments.loc.begin` — the param-list `(`. The
    // generic `cx.loc(node).begin()` helper is `Send`-shaped (it expects `(`
    // immediately after `name.end`), so for `def` we locate the first
    // `LeftParen` token between the method name and the first parameter.
    let Some(left_paren) = left_paren_range(node, first_param, cx) else {
        return;
    };

    let src = cx.source();
    let bytes = src.as_bytes();

    // `return if same_line?(first_elem, left_parenthesis)`.
    let first_start = cx.range(first_param).start as usize;
    let paren_line_start = line_start(bytes, left_paren.start as usize);
    let first_line_start = line_start(bytes, first_start);
    if paren_line_start == first_line_start {
        return;
    }

    // `actual_column = first.source_range.column` (0-based, char count).
    let actual_column = column_of(src, first_start);

    let opts = cx.options_or_default::<FirstParameterIndentationOptions>();
    // `configured_indentation_width`: this cop's own `IndentationWidth` override,
    // else the run-wide resolved `Layout/IndentationWidth.Width` (murphy-kke2).
    let indentation_width = opts
        .indentation_width
        .unwrap_or(cx.indentation_width())
        .max(0) as usize;
    let base_column = match opts.enforced_style {
        // `brace_alignment_style` → `left_brace.column`.
        FirstParameterIndentationStyle::AlignParentheses => column_of(src, left_paren.start as usize),
        // default `consistent` → `left_brace.source_line =~ /\S/`: the column of
        // the first non-whitespace character on the `(` line.
        FirstParameterIndentationStyle::Consistent => {
            first_non_whitespace_column(bytes, paren_line_start)
        }
    };

    // `expected_column = indent_base_column + configured_indentation_width + offset`
    // (offset is always 0 for `on_def`).
    let expected_column = base_column + indentation_width;

    // `@column_delta = expected_column - actual_column`; offense iff non-zero.
    if expected_column == actual_column {
        return;
    }

    let base_description = match opts.enforced_style {
        FirstParameterIndentationStyle::AlignParentheses => "the position of the opening parenthesis",
        FirstParameterIndentationStyle::Consistent => {
            "the start of the line where the left parenthesis is"
        }
    };
    let msg = format!(
        "Use {indentation_width} spaces for indentation in method args, relative to {base_description}."
    );
    cx.emit_offense(cx.range(first_param), &msg, None);

    // Autocorrect: rewrite the leading whitespace of the first parameter's line
    // to `expected_column` spaces (RuboCop's `AlignmentCorrector` applies the
    // same `@column_delta` shift to the line).
    let leading_ws_end = first_non_whitespace_byte(bytes, first_line_start);
    let leading = Range {
        start: first_line_start as u32,
        end: leading_ws_end as u32,
    };
    let replacement = " ".repeat(expected_column);
    cx.emit_edit(leading, &replacement);
}

/// First parameter of a `def`/`defs`, or `None` if the param list is empty.
fn first_parameter(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let args = cx.def_arguments(node).get()?;
    cx.children(args).first().copied()
}

/// The `(` of a `def`/`defs` parameter list, or `None` for a paren-less
/// definition (`def foo a, b`). Scans for the first `LeftParen` token that lies
/// between the method-name end and the first parameter's start; a paren-less
/// `def` has no such token (its first parameter follows whitespace, not `(`).
fn left_paren_range(node: NodeId, first_param: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let name = cx.node(node).loc.name;
    let search_from = if name == Range::ZERO {
        cx.range(node).start
    } else {
        name.end
    };
    let first_start = cx.range(first_param).start;
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < search_from);
    toks[idx..]
        .iter()
        .take_while(|t| t.range.start < first_start)
        .find(|t| t.kind == murphy_plugin_api::SourceTokenKind::LeftParen)
        .map(|t| t.range)
}

/// Byte index of the start of the line containing `offset`.
fn line_start(bytes: &[u8], offset: usize) -> usize {
    bytes[..offset]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |p| p + 1)
}

/// 0-based column (char count) of `offset` within its source line.
fn column_of(src: &str, offset: usize) -> usize {
    let start = line_start(src.as_bytes(), offset);
    src[start..offset].chars().count()
}

/// 0-based column of the first non-whitespace character on the line starting at
/// `line_start`. RuboCop's `source_line =~ /\S/`.
fn first_non_whitespace_column(bytes: &[u8], line_start: usize) -> usize {
    let mut col = 0;
    let mut i = line_start;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        col += 1;
        i += 1;
    }
    col
}

/// Byte index of the first non-whitespace character on the line starting at
/// `line_start` (used to delimit the leading-whitespace edit range).
fn first_non_whitespace_byte(bytes: &[u8], line_start: usize) -> usize {
    let mut i = line_start;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::{FirstParameterIndentation, FirstParameterIndentationOptions, FirstParameterIndentationStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_unindented_first_param_consistent() {
        test::<FirstParameterIndentation>().expect_correction(
            indoc! {r#"
                def some_method(
                first_param,
                ^^^^^^^^^^^ Use 2 spaces for indentation in method args, relative to the start of the line where the left parenthesis is.
                second_param)
                  123
                end
            "#},
            "def some_method(\n  first_param,\nsecond_param)\n  123\nend\n",
        );
    }

    #[test]
    fn accepts_correct_consistent_indentation() {
        test::<FirstParameterIndentation>().expect_no_offenses(indoc! {r#"
            def some_method(
              first_param,
            second_param)
              123
            end
        "#});
    }

    /// Cross-cop fallback (murphy-kke2): this cop now reads the run-wide
    /// resolved `Layout/IndentationWidth.Width` (and its own `IndentationWidth`
    /// override). At width 4 the first parameter indented 4 (base column 0) is
    /// accepted; under the old hardcoded 2 it was flagged.
    #[test]
    fn falls_back_to_layout_indentation_width() {
        test::<FirstParameterIndentation>()
            .with_indentation_width(4)
            .expect_no_offenses(indoc! {r#"
                def some_method(
                    first_param,
                second_param)
                  123
                end
            "#});
    }

    #[test]
    fn accepts_first_param_on_paren_line() {
        test::<FirstParameterIndentation>().expect_no_offenses(indoc! {r#"
            def some_method(first_param,
              second_param)
              123
            end
        "#});
    }

    #[test]
    fn accepts_no_parameters() {
        test::<FirstParameterIndentation>().expect_no_offenses(indoc! {r#"
            def some_method
              123
            end
        "#});
    }

    #[test]
    fn accepts_no_parens() {
        test::<FirstParameterIndentation>().expect_no_offenses(indoc! {r#"
            def some_method first_param,
              second_param
              123
            end
        "#});
    }

    #[test]
    fn flags_with_align_parentheses_style() {
        let opts = FirstParameterIndentationOptions {
            enforced_style: FirstParameterIndentationStyle::AlignParentheses,
            indentation_width: None,
        };
        // `(` is at column 15, so expected column is 15 + 2 = 17.
        test::<FirstParameterIndentation>().with_options(&opts).expect_correction(
            indoc! {r#"
                def some_method(
                  first_param,
                  ^^^^^^^^^^^ Use 2 spaces for indentation in method args, relative to the position of the opening parenthesis.
                second_param)
                  123
                end
            "#},
            "def some_method(\n                 first_param,\nsecond_param)\n  123\nend\n",
        );
    }

    #[test]
    fn accepts_correct_align_parentheses() {
        let opts = FirstParameterIndentationOptions {
            enforced_style: FirstParameterIndentationStyle::AlignParentheses,
            indentation_width: None,
        };
        test::<FirstParameterIndentation>()
            .with_options(&opts)
            .expect_no_offenses(indoc! {r#"
                def some_method(
                                 first_param,
                second_param)
                  123
                end
            "#});
    }

    #[test]
    fn flags_singleton_def() {
        test::<FirstParameterIndentation>().expect_correction(
            indoc! {r#"
                def self.some_method(
                first_param,
                ^^^^^^^^^^^ Use 2 spaces for indentation in method args, relative to the start of the line where the left parenthesis is.
                second_param)
                  123
                end
            "#},
            "def self.some_method(\n  first_param,\nsecond_param)\n  123\nend\n",
        );
    }
}

murphy_plugin_api::submit_cop!(FirstParameterIndentation);
