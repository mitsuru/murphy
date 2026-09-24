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
//! gap_issues: []
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
//!   `cx.indentation_width()` (default 2) — murphy-kke2 (own override wins
//!   over the cross-cop width; explicit `0` is honoured) — murphy-4u8u.
//!   Ambiguous-style bookkeeping (`MultilineElementIndentation#check_first` /
//!   `detected_styles_for_column`, `ambiguous_style_detected` /
//!   `correct_style_detected` via `ConfigurableEnforcedStyle`) is modelled as
//!   the pure `detected_styles` helper below: for this cop `left_parenthesis`
//!   is always `nil` and `offset` always `0`, so a `consistent` hit also
//!   reports `:special_inside_parentheses` (the mixin pushes it whenever
//!   `left_parenthesis` is nil) and an `align_parentheses` hit reports only
//!   itself — exactly as upstream computes. The *global* side of that
//!   bookkeeping (`DetectedStyle` intersection across files,
//!   `config_to_allow_offenses` for `--auto-gen-config`) only feeds
//!   style-inference diagnostics and never changes which offenses fire, so it
//!   is intentionally not persisted: Murphy is stateless per-file with no
//!   `DisabledConfigFormatter` (same precedent as
//!   `Layout/FirstArrayElementIndentation`, `Layout/CaseIndentation`, and
//!   `Layout/SpaceAroundEqualsInParameterDefault`); only the active
//!   `EnforcedStyle` is enforced.
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

    // RuboCop's `check_first` bookkeeping (`detected_styles` /
    // `ambiguous_style_detected` / `correct_style_detected`): model which
    // styles the actual column satisfies. The result only feeds
    // `--auto-gen-config` style inference and never changes whether an
    // offense fires, so it is computed and intentionally discarded (Murphy is
    // stateless per-file; no `DisabledConfigFormatter`). Kept as a named
    // binding so the modelling is visible and unit-testable.
    let _detected = detected_styles(
        actual_column,
        indentation_width,
        first_non_whitespace_column(bytes, paren_line_start),
        column_of(src, left_paren.start as usize),
    );

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

/// Which `EnforcedStyle` values the actual column satisfies — RuboCop's
/// `detected_styles_for_column` symbols for this cop. `SpecialInsideParentheses`
/// is not in this cop's `SupportedStyles` (`[consistent, align_parentheses]`)
/// but the shared mixin still reports it when `left_parenthesis` is nil, so it
/// is modelled here for fidelity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DetectedStyle {
    Consistent,
    SpecialInsideParentheses,
    AlignParentheses,
}

/// RuboCop's `MultilineElementIndentation#detected_styles_for_column`,
/// specialized for `Layout/FirstParameterIndentation`: `check` always calls
/// `check_first(first, left_brace = `(`, left_parenthesis = nil, offset = 0)`,
/// so `base_column = actual_column - configured_indentation_width` and only
/// the `left_brace` (`(`) column and its line's first-non-whitespace column
/// participate. Mirrors upstream exactly:
/// - `base == start_of_line` pushes `:consistent` *and*
///   `:special_inside_parentheses` (the `unless left_parenthesis` branch —
///   `left_parenthesis` is always nil here);
/// - the `left_parenthesis.column + 1` branch never fires (nil);
/// - `base == left_brace.column` pushes `:align_parentheses`
///   (`brace_alignment_style`).
fn detected_styles(
    actual_column: usize,
    indentation_width: usize,
    start_of_line_column: usize,
    paren_column: usize,
) -> Vec<DetectedStyle> {
    let base = actual_column as i64 - indentation_width as i64;
    let mut styles = Vec::new();
    if base == start_of_line_column as i64 {
        styles.push(DetectedStyle::Consistent);
        // `styles << :special_inside_parentheses unless left_parenthesis` —
        // `left_parenthesis` is always nil for this cop.
        styles.push(DetectedStyle::SpecialInsideParentheses);
    }
    if base == paren_column as i64 {
        styles.push(DetectedStyle::AlignParentheses);
    }
    styles
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

    /// Own `IndentationWidth` wins over the run-wide
    /// `Layout/IndentationWidth.Width` (murphy-4u8u): own 2 beats run-wide 4,
    /// so 4-space indent is flagged with a 2-space message.
    #[test]
    fn own_override_wins_over_cross_cop_width() {
        let opts = FirstParameterIndentationOptions {
            enforced_style: FirstParameterIndentationStyle::Consistent,
            indentation_width: Some(2),
        };
        test::<FirstParameterIndentation>()
            .with_options(&opts)
            .with_indentation_width(4)
            .expect_correction(
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

    /// Explicit `IndentationWidth: 0` is honoured (not treated as unset):
    /// base column 0 + 0 = 0, so a first parameter at column 0 is accepted.
    #[test]
    fn honors_zero_indentation_width() {
        let opts = FirstParameterIndentationOptions {
            enforced_style: FirstParameterIndentationStyle::Consistent,
            indentation_width: Some(0),
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

    /// `detected_styles` models RuboCop's `detected_styles_for_column` for this
    /// cop (`left_parenthesis` is always nil, offset always 0): a `consistent`
    /// hit is ambiguous with `:special_inside_parentheses` (the mixin pushes
    /// it whenever `left_parenthesis` is nil), exactly as upstream does.
    #[test]
    fn detected_styles_consistent_is_ambiguous() {
        // `def some_method(`: `(` at column 15, start-of-line column 0.
        // actual 2 with width 2 -> base 0 -> consistent (+ special_inside).
        let styles = super::detected_styles(2, 2, 0, 15);
        assert!(styles.contains(&super::DetectedStyle::Consistent));
        assert!(styles.contains(&super::DetectedStyle::SpecialInsideParentheses));
        assert!(!styles.contains(&super::DetectedStyle::AlignParentheses));
    }

    #[test]
    fn detected_styles_align_parentheses() {
        // actual 17 with width 2 -> base 15 -> align_parentheses only.
        let styles = super::detected_styles(17, 2, 0, 15);
        assert_eq!(styles, vec![super::DetectedStyle::AlignParentheses]);
    }

    #[test]
    fn detected_styles_no_match() {
        // actual 0 with width 2 -> base -2 matches neither base.
        let styles = super::detected_styles(0, 2, 0, 15);
        assert!(styles.is_empty());
    }
}

murphy_plugin_api::submit_cop!(FirstParameterIndentation);
