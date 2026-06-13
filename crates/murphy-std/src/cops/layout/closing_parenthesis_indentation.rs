//! `Layout/ClosingParenthesisIndentation` — flags hanging closing parens that
//! are neither aligned with their opening paren nor indented to the expected
//! column.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/ClosingParenthesisIndentation
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: [murphy-bgd8]
//! notes: >
//!   Port of RuboCop's `ClosingParenthesisIndentation`. Handles `send`/`csend`
//!   (call argument lists), `def`/`defs` (parameter lists), and `begin`
//!   (parenthesized grouping `( … )` — Murphy's `Begin` is exactly the
//!   parenthesized form; `begin … end` is the distinct `Kwbegin`, so it is not
//!   matched). The `(`/`)` come from `cx.loc(node).begin()` / `.end()`; the
//!   check only runs when the `)` exists and begins its own line
//!   (`begins_its_line?`). With elements, `expected_column` has three branches:
//!   line-break-after-`(` → `first_arg_indent - indentation_width` (clamped to
//!   0); all elements aligned → `(`'s column; otherwise → first-arg-line
//!   indentation. The hash-first-element special case compares the hash's
//!   child columns. With no elements, the `)` is accepted if its column is the
//!   `(` line's indentation, the `(`'s column, or the node's column; otherwise
//!   it is corrected to the first candidate. Messages mirror upstream:
//!   `Align ) with (.` when the target equals the `(` column, else
//!   `Indent ) to column N (not M)`.
//!
//!   `configured_indentation_width` resolves as RuboCop's
//!   `cop_config['IndentationWidth'] || config.for_cop('Layout/IndentationWidth')['Width'] || 2`.
//!   This cop's **own** `IndentationWidth` override is honoured (in-boundary cop
//!   option); when unset it falls through to the final default `2`.
//!
//!   GAP (murphy-bgd8): the middle term — the cross-cop fallback to
//!   `Layout/IndentationWidth`'s `Width` — is **not** wired. Murphy's
//!   single-surface plugin ABI threads only run-wide `AllCops.*` scalars into a
//!   cop's `CxRaw` (via `AllCopsContext`); it does not expose another cop's
//!   resolved config table to a plugin. Adding `Layout/IndentationWidth.Width`
//!   would require a new run-wide scalar on the wire `CxRaw` (with its offset
//!   assertions) plus host-side resolution in `Config::allcops_context`. That
//!   wire-ABI change is deferred — the boundary is never bypassed. The divergence
//!   is observable only when a user sets `Layout/IndentationWidth: { Width: N }`
//!   with `N != 2` AND leaves this cop's own `IndentationWidth` unset; in that
//!   narrow case Murphy uses 2 where RuboCop would use `N`. Setting this cop's
//!   own `IndentationWidth: N` is the documented workaround.
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, SourceTokenKind, cop};

/// Fallback indentation width when neither this cop's `IndentationWidth` nor
/// (the un-wired) `Layout/IndentationWidth.Width` is set. See the GAP note above.
const DEFAULT_INDENTATION_WIDTH: usize = 2;

/// Stateless unit struct (ADR 0035 const-metadata cop pattern).
#[derive(Default)]
pub struct ClosingParenthesisIndentation;

#[derive(CopOptions)]
pub struct ClosingParenthesisIndentationOptions {
    /// `IndentationWidth: ~` — an optional per-cop override of the indentation
    /// width. When unset (`None`), RuboCop falls back to
    /// `Layout/IndentationWidth.Width` (not wired — see GAP note) and finally to
    /// [`DEFAULT_INDENTATION_WIDTH`].
    #[option(
        name = "IndentationWidth",
        description = "Number of spaces for indentation; overrides Layout/IndentationWidth's Width. Use null to inherit."
    )]
    pub indentation_width: Option<i64>,
}

#[cop(
    name = "Layout/ClosingParenthesisIndentation",
    description = "Checks the indentation of hanging closing parentheses.",
    default_severity = "warning",
    default_enabled = true,
    options = ClosingParenthesisIndentationOptions
)]
impl ClosingParenthesisIndentation {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check_call(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check_call(node, cx);
    }

    #[on_node(kind = "begin")]
    fn check_begin(&self, node: NodeId, cx: &Cx<'_>) {
        // A `Begin` node is the parenthesized grouping `( … )`. The grouping
        // parens are `loc.begin()` / `loc.end()` (token scan from the node's
        // start). An implicit body (`def a; x; y; end`) is also a `Begin` but
        // has no parens, so `begin()` returns ZERO and is skipped.
        let loc = cx.loc(node);
        let children = cx.children(node);
        check(node, &children, loc.begin(), loc.end(), cx);
    }

    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        let Some((left, right)) = def_param_parens(node, cx) else {
            return;
        };
        let params = def_parameters(node, cx);
        check(node, &params, left, right, cx);
    }

    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        let Some((left, right)) = def_param_parens(node, cx) else {
            return;
        };
        let params = def_parameters(node, cx);
        check(node, &params, left, right, cx);
    }
}

/// RuboCop's `configured_indentation_width`:
/// `cop_config['IndentationWidth'] || config.for_cop('Layout/IndentationWidth')['Width'] || 2`.
///
/// The first term — this cop's own `IndentationWidth` override — is honoured,
/// including an explicit `0` (Ruby's `cop_config['IndentationWidth'] || …` is
/// truthy for `0`, so `0` selects width 0, not the fallback). Only an unset
/// override (`None`) inherits the default. The cross-cop
/// `Layout/IndentationWidth.Width` fallback is the un-wired GAP (murphy-bgd8);
/// when this cop's override is unset, we go straight to the final default `2`.
/// A negative override is clamped to `0`.
fn configured_indentation_width(cx: &Cx<'_>) -> usize {
    cx.options_or_default::<ClosingParenthesisIndentationOptions>()
        .indentation_width
        .map_or(DEFAULT_INDENTATION_WIDTH, |w| w.max(0) as usize)
}

/// `on_send` / `on_csend`: only parenthesized calls have argument-list parens
/// (RuboCop's `node.loc.begin` is `nil` otherwise). Operator sends and command
/// calls have no own parens, so they are skipped — this prevents an operator
/// send like `y + z` inside a grouped `( … )` from claiming the grouping parens.
fn check_call(node: NodeId, cx: &Cx<'_>) {
    if !cx.is_parenthesized(node) {
        return;
    }
    let loc = cx.loc(node);
    check(node, cx.call_arguments(node), loc.begin(), loc.end(), cx);
}

/// RuboCop's `check(node, elements)`.
fn check(node: NodeId, elements: &[NodeId], left_paren: Range, right_paren: Range, cx: &Cx<'_>) {
    // `return unless right_paren && begins_its_line?(right_paren)`.
    if left_paren == Range::ZERO || right_paren == Range::ZERO {
        return;
    }
    if !begins_its_line(cx, right_paren.start) {
        return;
    }

    let right_col = column_of(cx, right_paren.start);
    let left_col = column_of(cx, left_paren.start);

    let correct_column = if elements.is_empty() {
        // `check_for_no_elements`.
        let candidates = [
            line_indent_at(cx, left_paren.start),
            left_col,
            column_of(cx, cx.range(node).start),
        ];
        if candidates.contains(&right_col) {
            return;
        }
        candidates[0]
    } else {
        // `check_for_elements`.
        let col = expected_column(cx, left_paren, elements, configured_indentation_width(cx));
        if col == right_col {
            return;
        }
        col
    };

    let message = if correct_column == left_col {
        "Align `)` with `(`.".to_owned()
    } else {
        format!("Indent `)` to column {correct_column} (not {right_col})")
    };
    cx.emit_offense(right_paren, &message, None);

    // Autocorrect: rewrite the `)` line's leading whitespace to `correct_column`
    // spaces. Idempotent.
    let line_start = right_paren.start - right_col as u32;
    cx.emit_edit(
        Range {
            start: line_start,
            end: right_paren.start,
        },
        &" ".repeat(correct_column),
    );
}

/// RuboCop's `expected_column(left_paren, elements)`. `indentation_width` is the
/// resolved `configured_indentation_width`.
fn expected_column(
    cx: &Cx<'_>,
    left_paren: Range,
    elements: &[NodeId],
    indentation_width: usize,
) -> usize {
    let first = elements[0];
    let first_start = cx.range(first).start;

    // `line_break_after_left_paren?` — the first element starts on a later
    // line than `(`. Checked by a newline in the bytes between them, avoiding
    // a from-file-start line-number computation.
    let line_break_after_left_paren = cx.source().as_bytes()
        [left_paren.start as usize..first_start as usize]
        .contains(&b'\n');

    if line_break_after_left_paren {
        line_indent_at(cx, first_start).saturating_sub(indentation_width)
    } else if all_elements_aligned(cx, elements) {
        column_of(cx, left_paren.start)
    } else {
        line_indent_at(cx, first_start)
    }
}

/// RuboCop's `all_elements_aligned?` — every element starts at the same column.
/// When the first element is a hash, its *child* columns are compared instead
/// (a brace-less hash spreads its pairs, and the parens align with the pairs).
fn all_elements_aligned(cx: &Cx<'_>, elements: &[NodeId]) -> bool {
    if matches!(cx.kind(elements[0]), NodeKind::Hash(_)) {
        let children = cx.children(elements[0]);
        let Some((&first, rest)) = children.split_first() else {
            return true;
        };
        let first_col = column_of(cx, cx.range(first).start);
        rest.iter()
            .all(|&child| column_of(cx, cx.range(child).start) == first_col)
    } else {
        let first_col = column_of(cx, cx.range(elements[0]).start);
        elements[1..]
            .iter()
            .all(|&e| column_of(cx, cx.range(e).start) == first_col)
    }
}

/// The parameter nodes of a `def`/`defs` (the `Args` node's children).
fn def_parameters(node: NodeId, cx: &Cx<'_>) -> Vec<NodeId> {
    match cx.def_arguments(node).get() {
        Some(args) => cx.children(args),
        None => Vec::new(),
    }
}

/// The parameter-list `(` / `)` of a `def`/`defs`, found by token scanning.
///
/// Murphy does not record a usable `loc.name` / args sub-range on `def` nodes
/// (both span the whole definition), so `LocRef::begin()` cannot be used. The
/// param-list `(` is the first `LeftParen` token after the method name; its
/// match is found by paren-depth counting. `None` when the def has no
/// parenthesized parameter list (`def foo` / `def foo a, b`).
///
/// For a singleton method with a parenthesized receiver (`def (obj).foo(a)`),
/// the receiver's `(obj)` parens precede the param list, so the scan starts
/// after the receiver's source range — otherwise the receiver parens would be
/// mistaken for the parameter list.
fn def_param_parens(node: NodeId, cx: &Cx<'_>) -> Option<(Range, Range)> {
    let range = cx.range(node);
    // Skip past a parenthesized receiver (`def (obj).foo …`).
    let scan_start = match cx.def_receiver(node).get() {
        Some(recv) => cx.range(recv).end,
        None => range.start,
    };
    let toks = cx.tokens_in(Range {
        start: scan_start,
        end: range.end,
    });
    let open_idx = toks
        .iter()
        .position(|t| t.kind == SourceTokenKind::LeftParen)?;
    let left = toks[open_idx].range;
    let mut depth = 1i32;
    for tok in &toks[open_idx + 1..] {
        match tok.kind {
            SourceTokenKind::LeftParen => depth += 1,
            SourceTokenKind::RightParen => {
                depth -= 1;
                if depth == 0 {
                    return Some((left, tok.range));
                }
            }
            _ => {}
        }
    }
    None
}

/// True when only whitespace precedes `offset` on its line.
fn begins_its_line(cx: &Cx<'_>, offset: u32) -> bool {
    let src = cx.source().as_bytes();
    let mut i = offset as usize;
    while i > 0 {
        match src[i - 1] {
            b' ' | b'\t' => i -= 1,
            b'\n' => return true,
            _ => return false,
        }
    }
    true
}

/// 0-based column (byte offset from line start) of `offset`. Indentation is
/// ASCII, so byte == display column.
fn column_of(cx: &Cx<'_>, offset: u32) -> usize {
    let src = cx.source().as_bytes();
    let off = offset as usize;
    let line_start = src[..off]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |pos| pos + 1);
    off - line_start
}

/// RuboCop's `processed_source.line_indentation(line)` for the line containing
/// `offset` — the count of leading space/tab characters on that line. Computed
/// directly from the line start, avoiding a from-file-start line-number scan.
fn line_indent_at(cx: &Cx<'_>, offset: u32) -> usize {
    let src = cx.source().as_bytes();
    let off = offset as usize;
    let line_start = src[..off]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |pos| pos + 1);
    src[line_start..]
        .iter()
        .take_while(|&&b| b == b' ' || b == b'\t')
        .count()
}

murphy_plugin_api::submit_cop!(ClosingParenthesisIndentation);

#[cfg(test)]
mod tests {
    use super::{ClosingParenthesisIndentation as Cop, ClosingParenthesisIndentationOptions};
    use murphy_plugin_api::test_support::{
        run_cop, run_cop_with_edits, run_cop_with_options, run_cop_with_options_and_edits,
    };

    fn width(w: i64) -> ClosingParenthesisIndentationOptions {
        ClosingParenthesisIndentationOptions {
            indentation_width: Some(w),
        }
    }

    fn apply(source: &str, edits: &[murphy_plugin_api::test_support::CapturedEdit]) -> String {
        let mut out = String::with_capacity(source.len());
        let mut last = 0usize;
        let mut ordered: Vec<_> = edits.iter().collect();
        ordered.sort_by_key(|e| e.range.start);
        for e in ordered {
            out.push_str(&source[last..e.range.start as usize]);
            out.push_str(&e.replacement);
            last = e.range.end as usize;
        }
        out.push_str(&source[last..]);
        out
    }

    // ---- line break after `(`: indent paren to first_arg_indent - width ----

    #[test]
    fn flags_misaligned_indent_after_line_break() {
        let src = "some_method(\n  a\n  )\n";
        let run = run_cop_with_edits::<Cop>(src);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(run.offenses[0].message, "Indent `)` to column 0 (not 2)");
        assert_eq!(apply(src, &run.edits), "some_method(\n  a\n)\n");
    }

    #[test]
    fn accepts_aligned_indent_after_line_break() {
        assert!(run_cop::<Cop>("some_method(\n  a\n)\n").is_empty());
    }

    // ---- no line break: align `)` with `(` ----

    #[test]
    fn flags_unaligned_paren_no_line_break() {
        // `(` is at column 11; `)` at column 0 -> align to column 11.
        let src = "some_method(a\n)\n";
        let run = run_cop_with_edits::<Cop>(src);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(run.offenses[0].message, "Align `)` with `(`.");
        assert_eq!(apply(src, &run.edits), "some_method(a\n           )\n");
    }

    #[test]
    fn accepts_aligned_paren_no_line_break() {
        assert!(run_cop::<Cop>("some_method(a\n           )\n").is_empty());
    }

    // ---- no elements ----

    #[test]
    fn accepts_empty_parens_same_line() {
        assert!(run_cop::<Cop>("some_method()\n").is_empty());
    }

    #[test]
    fn accepts_empty_paren_at_left_edge() {
        // `)` at column 0 == line indentation of the `(` line.
        assert!(run_cop::<Cop>("some_method(\n)\n").is_empty());
    }

    #[test]
    fn accepts_empty_paren_aligned_with_open() {
        // `(` at column 11, `)` at column 11.
        assert!(run_cop::<Cop>("some_method(\n           )\n").is_empty());
    }

    #[test]
    fn flags_empty_paren_misindented() {
        // `)` at column 2, candidates [0, 11, 0] -> correct to 0.
        let src = "some_method(\n  )\n";
        let run = run_cop_with_edits::<Cop>(src);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(run.offenses[0].message, "Indent `)` to column 0 (not 2)");
        assert_eq!(apply(src, &run.edits), "some_method(\n)\n");
    }

    // ---- def parameter list ----

    #[test]
    fn flags_def_param_list_misindent() {
        let src = "def some_method(\n  a\n  )\nend\n";
        let run = run_cop_with_edits::<Cop>(src);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(run.offenses[0].message, "Indent `)` to column 0 (not 2)");
        assert_eq!(apply(src, &run.edits), "def some_method(\n  a\n)\nend\n");
    }

    #[test]
    fn accepts_def_empty_param_list() {
        assert!(run_cop::<Cop>("def some_method()\nend\n").is_empty());
    }

    #[test]
    fn flags_defs_with_parenthesized_receiver() {
        // `def (obj).foo(...)`: the receiver `(obj)` parens precede the param
        // list, so the scan must skip them and check the param-list `)`.
        let src = "def (obj).foo(\n  a\n  )\nend\n";
        let run = run_cop_with_edits::<Cop>(src);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(run.offenses[0].message, "Indent `)` to column 0 (not 2)");
        assert_eq!(apply(src, &run.edits), "def (obj).foo(\n  a\n)\nend\n");
    }

    // ---- grouped expression (begin node) ----

    #[test]
    fn flags_grouped_expression_misindent() {
        let src = "w = x * (\n  y + z\n  )\n";
        let run = run_cop_with_edits::<Cop>(src);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(run.offenses[0].message, "Indent `)` to column 0 (not 2)");
        assert_eq!(apply(src, &run.edits), "w = x * (\n  y + z\n)\n");
    }

    #[test]
    fn accepts_grouped_expression_aligned() {
        assert!(run_cop::<Cop>("w = x * (\n  y + z\n)\n").is_empty());
    }

    #[test]
    fn accepts_implicit_begin_body_no_parens() {
        // `def a; x; y; end` parses to a `Begin` body with no parens — must be
        // skipped (no `(`/`)`).
        assert!(run_cop::<Cop>("def a\n  x\n  y\nend\n").is_empty());
    }

    // ---- safe navigation ----

    #[test]
    fn flags_safe_navigation_call() {
        let src = "receiver&.some_method(\n  a\n  )\n";
        let run = run_cop_with_edits::<Cop>(src);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(run.offenses[0].message, "Indent `)` to column 0 (not 2)");
        assert_eq!(apply(src, &run.edits), "receiver&.some_method(\n  a\n)\n");
    }

    // ---- closing paren not at line start: skipped ----

    #[test]
    fn accepts_closing_paren_not_at_line_start() {
        assert!(run_cop::<Cop>("w = x * (y + z +\n        a)\n").is_empty());
    }

    // ---- aligned-elements branch: `)` aligns with `(` ----

    #[test]
    fn accepts_aligned_arguments_paren_with_open() {
        // Arguments all start at column 11 (aligned), so `)` aligns with `(`.
        let src = "foo = some_method(a\n                 )\n";
        assert!(run_cop::<Cop>(src).is_empty());
    }

    // ---- idempotence ----

    #[test]
    fn autocorrect_is_idempotent() {
        let src = "some_method(\n  a\n  )\n";
        let run = run_cop_with_edits::<Cop>(src);
        let fixed = apply(src, &run.edits);
        let run2 = run_cop_with_edits::<Cop>(&fixed);
        assert!(run2.offenses.is_empty(), "second pass must be clean: {fixed}");
    }

    // ---- IndentationWidth override (own cop config) ----

    /// With `IndentationWidth: 4` and the first arg indented 4, the line-break
    /// branch expects `)` at column `4 - 4 = 0`. Verified against RuboCop 1.86.2.
    #[test]
    fn honors_own_indentation_width_override() {
        let src = "some_method(\n    a\n    )\n";
        let run = run_cop_with_options::<Cop>(src, &width(4));
        assert_eq!(run.len(), 1, "got {run:?}");
        assert_eq!(run[0].message, "Indent `)` to column 0 (not 4)");
    }

    /// With `IndentationWidth: 4`, the same input but `)` already at column 0 is
    /// accepted.
    #[test]
    fn accepts_paren_at_override_expected_column() {
        let src = "some_method(\n    a\n)\n";
        assert!(run_cop_with_options::<Cop>(src, &width(4)).is_empty());
    }

    /// The override is also applied during autocorrect.
    #[test]
    fn corrects_to_override_expected_column() {
        let src = "some_method(\n    a\n    )\n";
        let run = run_cop_with_options_and_edits::<Cop>(src, &width(4));
        assert_eq!(apply(src, &run.edits), "some_method(\n    a\n)\n");
    }

    /// Default (no override) keeps width 2: first arg indented 4, `)` at col 4 ->
    /// expected col 2. Verified against RuboCop 1.86.2.
    #[test]
    fn default_width_is_two() {
        let src = "some_method(\n    a\n    )\n";
        let run = run_cop::<Cop>(src);
        assert_eq!(run.len(), 1, "got {run:?}");
        assert_eq!(run[0].message, "Indent `)` to column 2 (not 4)");
    }

    /// Parity pin (Codex #387): an explicit `IndentationWidth: 0` is honoured
    /// (Ruby's `cop_config['IndentationWidth'] || …` is truthy for `0`), so the
    /// line-break branch expects `)` at `first_arg_indent - 0 = 4`. With `)`
    /// already at column 4 there is no offense — `0` must NOT be treated as
    /// "unset" (which would fall back to width 2 and wrongly flag column 2).
    #[test]
    fn honors_zero_indentation_width() {
        let src = "some_method(\n    a\n    )\n";
        let run = run_cop_with_options::<Cop>(src, &width(0));
        assert!(run.is_empty(), "got {run:?}");
    }
}
