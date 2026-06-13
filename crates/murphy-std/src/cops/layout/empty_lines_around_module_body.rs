//! `Layout/EmptyLinesAroundModuleBody` — checks empty lines at the top/bottom
//! of a module body against the configured `EnforcedStyle`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/EmptyLinesAroundModuleBody
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Full port of RuboCop's `EmptyLinesAroundBody` mixin (`KIND = 'module'`,
//!   `on_module`) across all four `SupportedStyles`:
//!     * `no_empty_lines` (default): flag a blank line at the beginning/end.
//!     * `empty_lines`: flag a *missing* blank line at the beginning/end.
//!     * `empty_lines_except_namespace`: `no_empty_lines` when the body is a
//!       single namespace child (a lone `class`/`module`); else `empty_lines`.
//!     * `empty_lines_special`: namespace bodies use `no_empty_lines`; otherwise
//!       the beginning requires a blank only when its first child is an
//!       `empty_line_required?` node (def/class/module/bare access modifier),
//!       with a deferred "Empty line missing before first <type> definition"
//!       offense when an interior required child lacks a preceding blank; the
//!       ending always requires a blank.
//!
//!   `valid_body_style?` is honoured: an empty (nil) body is skipped for every
//!   style except `no_empty_lines`. Single-line modules are skipped.
//!
//!   Each boundary fires independently, so `module Foo\n\nend` emits two
//!   `no_empty_lines` offenses (matching RuboCop). Autocorrect removes the full
//!   run of consecutive blank lines at each boundary (deduped when both
//!   boundaries hit the same run) for `no_empty_lines`, and inserts a single
//!   blank line for `empty_lines`.
//!
//!   ABI note: `NodeLoc` exposes only `expression`/`name` ranges, so the cop
//!   works line-based off the module node's `expression` range, exactly as
//!   RuboCop's mixin does (`node.source_range.first_line`/`last_line`).
//! ```

use crate::cops::util::{nth_line_start, physical_lines, PhysicalLine};
use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

#[derive(Default)]
pub struct EmptyLinesAroundModuleBody;

#[derive(CopOptions)]
pub struct EmptyLinesAroundModuleBodyOptions {
    #[option(
        name = "EnforcedStyle",
        default = "no_empty_lines",
        description = "Whether to require, forbid, or conditionally require blank lines around the module body."
    )]
    pub enforced_style: ModuleBodyStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum ModuleBodyStyle {
    /// Forbid blank lines at the module body boundaries (default).
    #[option(value = "no_empty_lines")]
    NoEmptyLines,
    /// Require blank lines at the module body boundaries.
    #[option(value = "empty_lines")]
    EmptyLines,
    /// `empty_lines`, except a single-namespace body forbids them.
    #[option(value = "empty_lines_except_namespace")]
    EmptyLinesExceptNamespace,
    /// Special first/last handling driven by the first child's kind.
    #[option(value = "empty_lines_special")]
    EmptyLinesSpecial,
}

const KIND: &str = "module";

#[cop(
    name = "Layout/EmptyLinesAroundModuleBody",
    description = "Keeps track of empty lines around module bodies.",
    default_severity = "warning",
    default_enabled = true,
    options = EmptyLinesAroundModuleBodyOptions
)]
impl EmptyLinesAroundModuleBody {
    #[on_node(kind = "module")]
    fn check_module(&self, node: NodeId, cx: &Cx<'_>) {
        let style = cx
            .options_or_default::<EmptyLinesAroundModuleBodyOptions>()
            .enforced_style;
        let body = module_body(node, cx);
        check(node, body, style, cx);
    }
}

/// `NodeKind::Module { body, .. }` accessor (no Cx helper exists for module
/// bodies; this mirrors `indentation_width.rs`'s `class_or_module_body`).
fn module_body(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    match *cx.kind(node) {
        NodeKind::Module { body, .. } => body.get(),
        _ => None,
    }
}

/// RuboCop's `check(node, body)`.
fn check(node: NodeId, body: Option<NodeId>, style: ModuleBodyStyle, cx: &Cx<'_>) {
    // `return if valid_body_style?(body)` — a nil body is enforced only for
    // `no_empty_lines`.
    if body.is_none() && style != ModuleBodyStyle::NoEmptyLines {
        return;
    }
    // `return if node.single_line?`
    if cx.is_single_line(node) {
        return;
    }

    let range = cx.range(node);
    // 1-based physical line numbers of the module node's source range.
    let first_line = line_1based(range.start, cx);
    let last_line = line_1based(range.end.saturating_sub(1).max(range.start), cx);
    let lines = physical_lines(cx.source());

    match style {
        ModuleBodyStyle::EmptyLinesExceptNamespace => {
            if is_namespace_one_child(body, cx) {
                check_both(BoundaryStyle::NoEmptyLines, first_line, last_line, &lines, cx);
            } else {
                check_both(BoundaryStyle::EmptyLines, first_line, last_line, &lines, cx);
            }
        }
        ModuleBodyStyle::EmptyLinesSpecial => {
            check_empty_lines_special(body, first_line, last_line, &lines, cx);
        }
        ModuleBodyStyle::NoEmptyLines => {
            check_both(BoundaryStyle::NoEmptyLines, first_line, last_line, &lines, cx);
        }
        ModuleBodyStyle::EmptyLines => {
            check_both(BoundaryStyle::EmptyLines, first_line, last_line, &lines, cx);
        }
    }
}

/// The two terminal styles a boundary can be checked against.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BoundaryStyle {
    NoEmptyLines,
    EmptyLines,
}

/// `check_both(style, first_line, last_line)` — beginning and ending share the
/// same boundary style. (RuboCop's `beginning_only`/`ending_only` variants are
/// unused by the module-body cop.)
fn check_both(
    style: BoundaryStyle,
    first_line: usize,
    last_line: usize,
    lines: &[PhysicalLine],
    cx: &Cx<'_>,
) {
    // Track the blank-run removal already scheduled so the two boundaries do
    // not emit overlapping edits when they resolve to the same run (the
    // nil-body `module Foo\n\nend` case).
    let mut emitted_removal: Option<Range> = None;
    check_beginning(style, first_line, lines, cx, &mut emitted_removal);
    check_ending(style, last_line, lines, cx, &mut emitted_removal);
}

/// `check_beginning` → `check_source(style, first_line, 'beginning')`.
fn check_beginning(
    style: BoundaryStyle,
    first_line: usize,
    lines: &[PhysicalLine],
    cx: &Cx<'_>,
    emitted_removal: &mut Option<Range>,
) {
    check_source(style, first_line, "beginning", lines, cx, emitted_removal);
}

/// `check_ending` → `check_source(style, last_line - 2, 'end')`.
fn check_ending(
    style: BoundaryStyle,
    last_line: usize,
    lines: &[PhysicalLine],
    cx: &Cx<'_>,
    emitted_removal: &mut Option<Range>,
) {
    let Some(line_no) = last_line.checked_sub(2) else {
        return;
    };
    check_source(style, line_no, "end", lines, cx, emitted_removal);
}

/// `check_source(style, line_no, desc)` — `line_no` is the 0-based index into
/// `processed_source.lines`.
fn check_source(
    style: BoundaryStyle,
    line_no: usize,
    desc: &str,
    lines: &[PhysicalLine],
    cx: &Cx<'_>,
    emitted_removal: &mut Option<Range>,
) {
    let Some(&line) = lines.get(line_no) else {
        return;
    };
    match style {
        BoundaryStyle::NoEmptyLines => {
            // `check_line(style, line_no, MSG_EXTRA, &:empty?)`
            if line.blank {
                emit_extra(line_no, desc, lines, cx, emitted_removal);
            }
        }
        BoundaryStyle::EmptyLines => {
            // `check_line(style, line_no, MSG_MISSING) { |line| !line.empty? }`
            if !line.blank {
                emit_missing(line_no, desc, cx);
            }
        }
    }
}

/// `no_empty_lines` offense + blank-run-removing autocorrect. The offense
/// always fires (each boundary is independent, matching RuboCop); the
/// removal edit is skipped when it overlaps a run already scheduled by the
/// other boundary, to keep edits non-overlapping.
fn emit_extra(
    line_no: usize,
    desc: &str,
    lines: &[PhysicalLine],
    cx: &Cx<'_>,
    emitted_removal: &mut Option<Range>,
) {
    let line = lines[line_no];
    let dir = if desc == "end" {
        BlankRunDirection::Up
    } else {
        BlankRunDirection::Down
    };
    let range = blank_run_range(lines, line_no, dir);
    cx.emit_offense(
        Range {
            start: line.start,
            end: line.end,
        },
        &format!("Extra empty line detected at {KIND} body {desc}."),
        None,
    );
    let overlaps = emitted_removal.is_some_and(|e| range.start < e.end && e.start < range.end);
    if !overlaps {
        cx.emit_edit(range, "");
        *emitted_removal = Some(range);
    }
}

/// `empty_lines` offense + blank-line-inserting autocorrect.
///
/// RuboCop reports `source_range(buffer, line + offset, 0)` where `offset` is
/// `2` for the end boundary and `1` for the beginning. That is the line at
/// which the missing blank should be inserted (0-based `line + offset - 1`).
fn emit_missing(line_no: usize, desc: &str, cx: &Cx<'_>) {
    let insert_line = if desc == "end" {
        // 1-based `line + 2` → 0-based `line + 1`.
        line_no + 1
    } else {
        // 1-based `line + 1` → 0-based `line`.
        line_no
    };
    let insert_at = nth_line_start(cx, insert_line as u32)
        .unwrap_or_else(|| cx.source().len() as u32);
    cx.emit_offense(
        Range {
            start: insert_at,
            end: insert_at,
        },
        &format!("Empty line missing at {KIND} body {desc}."),
        None,
    );
    cx.emit_edit(
        Range {
            start: insert_at,
            end: insert_at,
        },
        "\n",
    );
}

/// `check_empty_lines_special(body, first_line, last_line)`.
fn check_empty_lines_special(
    body: Option<NodeId>,
    first_line: usize,
    last_line: usize,
    lines: &[PhysicalLine],
    cx: &Cx<'_>,
) {
    // `return unless body`
    let Some(body) = body else {
        return;
    };
    if is_namespace_one_child(Some(body), cx) {
        check_both(BoundaryStyle::NoEmptyLines, first_line, last_line, lines, cx);
        return;
    }
    // The beginning and ending boundaries here are checked under different
    // styles whose edits cannot coincide, but the shared dedup tracker is
    // threaded for signature parity.
    let mut emitted_removal: Option<Range> = None;
    if first_child_requires_empty_line(body, cx) {
        check_beginning(
            BoundaryStyle::EmptyLines,
            first_line,
            lines,
            cx,
            &mut emitted_removal,
        );
    } else {
        check_beginning(
            BoundaryStyle::NoEmptyLines,
            first_line,
            lines,
            cx,
            &mut emitted_removal,
        );
        check_deferred_empty_line(body, lines, cx);
    }
    check_ending(
        BoundaryStyle::EmptyLines,
        last_line,
        lines,
        cx,
        &mut emitted_removal,
    );
}

/// `check_deferred_empty_line(body)` — the first interior child that requires an
/// empty line before it must be preceded by a blank (ignoring comments).
fn check_deferred_empty_line(body: NodeId, lines: &[PhysicalLine], cx: &Cx<'_>) {
    let Some(child) = first_empty_line_required_child(body, cx) else {
        return;
    };
    // `line = previous_line_ignoring_comments(node.first_line)` — 0-based.
    let child_first_line = line_1based(cx.range(child).start, cx);
    let line = previous_line_ignoring_comments(child_first_line, lines, cx);
    // `return if processed_source[line].empty?`
    if lines.get(line).is_some_and(|l| l.blank) {
        return;
    }
    // `range = source_range(buffer, line + 2, 0)` → insert at 0-based `line + 1`.
    let insert_line = line + 1;
    let insert_at = nth_line_start(cx, insert_line as u32)
        .unwrap_or_else(|| cx.source().len() as u32);
    let ty = node_type_name(child, cx);
    cx.emit_offense(
        Range {
            start: insert_at,
            end: insert_at,
        },
        &format!("Empty line missing before first {ty} definition"),
        None,
    );
    cx.emit_edit(
        Range {
            start: insert_at,
            end: insert_at,
        },
        "\n",
    );
}

/// `namespace?(body, with_one_child: true)` — true when `body` is a single
/// `class`/`module` (not a `begin` wrapping multiple children).
fn is_namespace_one_child(body: Option<NodeId>, cx: &Cx<'_>) -> bool {
    let Some(body) = body else {
        return false;
    };
    if let NodeKind::Begin(_) = cx.kind(body) {
        // `return false if with_one_child`.
        false
    } else {
        is_constant_definition(body, cx)
    }
}

/// `constant_definition?` — `{class module}`.
fn is_constant_definition(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        *cx.kind(node),
        NodeKind::Class { .. } | NodeKind::Module { .. }
    )
}

/// `empty_line_required?` — `{any_def class module (send nil? {:private :protected :public})}`.
fn is_empty_line_required(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.is_any_def_type(node)
        || is_constant_definition(node, cx)
        || (cx.call_receiver(node).get().is_none()
            && matches!(
                cx.method_name(node),
                Some("private" | "protected" | "public")
            ))
}

/// `first_child_requires_empty_line?(body)`.
fn first_child_requires_empty_line(body: NodeId, cx: &Cx<'_>) -> bool {
    match cx.kind(body) {
        NodeKind::Begin(list) => cx
            .list(*list)
            .first()
            .is_some_and(|&c| is_empty_line_required(c, cx)),
        _ => is_empty_line_required(body, cx),
    }
}

/// `first_empty_line_required_child(body)`.
fn first_empty_line_required_child(body: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    match cx.kind(body) {
        NodeKind::Begin(list) => cx
            .list(*list)
            .iter()
            .copied()
            .find(|&c| is_empty_line_required(c, cx)),
        _ => is_empty_line_required(body, cx).then_some(body),
    }
}

/// `previous_line_ignoring_comments(send_line)` — `send_line` is 1-based.
/// Scans `(send_line - 2).downto(0)` for the first non-comment 0-based line,
/// returning 0 if none.
fn previous_line_ignoring_comments(send_line: usize, lines: &[PhysicalLine], cx: &Cx<'_>) -> usize {
    let Some(mut line) = send_line.checked_sub(2) else {
        return 0;
    };
    loop {
        if !line_is_comment_strict(lines, line, cx) {
            return line;
        }
        if line == 0 {
            return 0;
        }
        line -= 1;
    }
}

/// `comment_line?(processed_source[line])` — `/\A\s*#/` for a 0-based line.
fn line_is_comment_strict(lines: &[PhysicalLine], line: usize, cx: &Cx<'_>) -> bool {
    let Some(&pl) = lines.get(line) else {
        return false;
    };
    let content_end = pl.end.saturating_sub(1).max(pl.start);
    let text = cx.raw_source(Range {
        start: pl.start,
        end: content_end,
    });
    let trimmed = text.trim_start();
    trimmed.starts_with('#')
}

/// `node.type` for the deferred message (`def`/`defs`/`class`/`module`/`send`).
fn node_type_name(node: NodeId, cx: &Cx<'_>) -> &'static str {
    match *cx.kind(node) {
        NodeKind::Def { .. } => "def",
        NodeKind::Defs { .. } => "defs",
        NodeKind::Class { .. } => "class",
        NodeKind::Module { .. } => "module",
        _ => "send",
    }
}

#[derive(Clone, Copy)]
enum BlankRunDirection {
    Down,
    Up,
}

/// The byte range covering the maximal run of consecutive blank lines that
/// includes `idx`, scanning toward EOF (`Down`) or BOF (`Up`).
fn blank_run_range(lines: &[PhysicalLine], idx: usize, dir: BlankRunDirection) -> Range {
    let mut lo = idx;
    let mut hi = idx;
    match dir {
        BlankRunDirection::Down => {
            while hi + 1 < lines.len() && lines[hi + 1].blank {
                hi += 1;
            }
        }
        BlankRunDirection::Up => {
            while lo > 0 && lines[lo - 1].blank {
                lo -= 1;
            }
        }
    }
    Range {
        start: lines[lo].start,
        end: lines[hi].end,
    }
}

/// 1-based physical line of `offset`.
fn line_1based(offset: u32, cx: &Cx<'_>) -> usize {
    crate::cops::util::line_of(offset, cx) as usize + 1
}

murphy_plugin_api::submit_cop!(EmptyLinesAroundModuleBody);

#[cfg(test)]
mod tests {
    use super::{
        EmptyLinesAroundModuleBody, EmptyLinesAroundModuleBodyOptions, ModuleBodyStyle,
    };
    use murphy_plugin_api::test_support::{
        run_cop, run_cop_with_edits, run_cop_with_options, run_cop_with_options_and_edits, test,
        CapturedEdit,
    };

    fn apply(source: &str, edits: &[CapturedEdit]) -> String {
        let mut sorted: Vec<&CapturedEdit> = edits.iter().collect();
        sorted.sort_by_key(|e| e.range.start);
        let mut out = String::new();
        let mut cursor = 0usize;
        for e in sorted {
            out.push_str(&source[cursor..e.range.start as usize]);
            out.push_str(&e.replacement);
            cursor = e.range.end as usize;
        }
        out.push_str(&source[cursor..]);
        out
    }

    fn opts(style: ModuleBodyStyle) -> EmptyLinesAroundModuleBodyOptions {
        EmptyLinesAroundModuleBodyOptions {
            enforced_style: style,
        }
    }

    // --- default style: no_empty_lines ---

    #[test]
    fn accepts_no_empty_lines() {
        test::<EmptyLinesAroundModuleBody>().expect_no_offenses("module Foo\n  x = 1\nend\n");
    }

    #[test]
    fn accepts_single_line_module() {
        test::<EmptyLinesAroundModuleBody>().expect_no_offenses("module Foo; end\n");
    }

    #[test]
    fn detects_crlf_blank_line_at_module_body_beginning() {
        let run = run_cop::<EmptyLinesAroundModuleBody>("module Foo\r\n\r\n  x = 1\r\nend\r\n");
        assert_eq!(run.len(), 1);
        assert!(run[0].message.contains("beginning"));
    }

    #[test]
    fn accepts_comment_after_opener() {
        test::<EmptyLinesAroundModuleBody>()
            .expect_no_offenses("module Foo\n  # a comment\n  x = 1\nend\n");
    }

    #[test]
    fn flags_empty_line_at_beginning() {
        let src = "module Foo\n\n  x = 1\nend\n";
        let offenses = run_cop::<EmptyLinesAroundModuleBody>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Extra empty line detected at module body beginning."
        );
    }

    #[test]
    fn flags_empty_line_at_end() {
        let src = "module Foo\n  x = 1\n\nend\n";
        let offenses = run_cop::<EmptyLinesAroundModuleBody>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Extra empty line detected at module body end."
        );
    }

    #[test]
    fn flags_both_beginning_and_end() {
        let src = "module Foo\n\n  x = 1\n\nend\n";
        let offenses = run_cop::<EmptyLinesAroundModuleBody>(src);
        assert_eq!(offenses.len(), 2, "got {offenses:?}");
    }

    #[test]
    fn flags_nil_body_single_blank_twice() {
        let src = "module Foo\n\nend\n";
        let offenses = run_cop::<EmptyLinesAroundModuleBody>(src);
        assert_eq!(offenses.len(), 2, "got {offenses:?}");
    }

    #[test]
    fn corrects_beginning() {
        let src = "module Foo\n\n  x = 1\nend\n";
        let run = run_cop_with_edits::<EmptyLinesAroundModuleBody>(src);
        assert_eq!(apply(src, &run.edits), "module Foo\n  x = 1\nend\n");
    }

    #[test]
    fn corrects_end() {
        let src = "module Foo\n  x = 1\n\nend\n";
        let run = run_cop_with_edits::<EmptyLinesAroundModuleBody>(src);
        assert_eq!(apply(src, &run.edits), "module Foo\n  x = 1\nend\n");
    }

    #[test]
    fn corrects_multiple_blank_lines_at_beginning() {
        let src = "module Foo\n\n\n\n  x = 1\nend\n";
        let run = run_cop_with_edits::<EmptyLinesAroundModuleBody>(src);
        assert_eq!(apply(src, &run.edits), "module Foo\n  x = 1\nend\n");
    }

    #[test]
    fn corrects_nil_body_without_overlap() {
        let src = "module Foo\n\nend\n";
        let run = run_cop_with_edits::<EmptyLinesAroundModuleBody>(src);
        assert_eq!(apply(src, &run.edits), "module Foo\nend\n");
    }

    #[test]
    fn correction_is_idempotent() {
        let src = "module Foo\n\n  x = 1\n\nend\n";
        let run = run_cop_with_edits::<EmptyLinesAroundModuleBody>(src);
        let fixed = apply(src, &run.edits);
        assert!(
            run_cop::<EmptyLinesAroundModuleBody>(&fixed).is_empty(),
            "not idempotent: {fixed:?}"
        );
    }

    // --- empty_lines style ---

    #[test]
    fn empty_lines_accepts_blanks_present() {
        let offenses = run_cop_with_options::<EmptyLinesAroundModuleBody>(
            "module Foo\n\n  x = 1\n\nend\n",
            &opts(ModuleBodyStyle::EmptyLines),
        );
        assert!(offenses.is_empty(), "got {offenses:?}");
    }

    #[test]
    fn empty_lines_flags_missing_both() {
        let offenses = run_cop_with_options::<EmptyLinesAroundModuleBody>(
            "module Foo\n  x = 1\nend\n",
            &opts(ModuleBodyStyle::EmptyLines),
        );
        assert_eq!(offenses.len(), 2, "got {offenses:?}");
        assert!(offenses.iter().any(|o| o.message
            == "Empty line missing at module body beginning."));
        assert!(offenses.iter().any(|o| o.message
            == "Empty line missing at module body end."));
    }

    #[test]
    fn empty_lines_skips_nil_body() {
        // `valid_body_style?` — empty body not enforced for `empty_lines`.
        let offenses = run_cop_with_options::<EmptyLinesAroundModuleBody>(
            "module Foo\nend\n",
            &opts(ModuleBodyStyle::EmptyLines),
        );
        assert!(offenses.is_empty(), "got {offenses:?}");
    }

    #[test]
    fn empty_lines_corrects_missing() {
        let src = "module Foo\n  x = 1\nend\n";
        let run = run_cop_with_options_and_edits::<EmptyLinesAroundModuleBody>(
            src,
            &opts(ModuleBodyStyle::EmptyLines),
        );
        assert_eq!(apply(src, &run.edits), "module Foo\n\n  x = 1\n\nend\n");
    }

    #[test]
    fn empty_lines_correction_is_idempotent() {
        let src = "module Foo\n  x = 1\nend\n";
        let run = run_cop_with_options_and_edits::<EmptyLinesAroundModuleBody>(
            src,
            &opts(ModuleBodyStyle::EmptyLines),
        );
        let fixed = apply(src, &run.edits);
        let again = run_cop_with_options::<EmptyLinesAroundModuleBody>(
            &fixed,
            &opts(ModuleBodyStyle::EmptyLines),
        );
        assert!(again.is_empty(), "not idempotent: {fixed:?} -> {again:?}");
    }

    // --- empty_lines_except_namespace style ---

    #[test]
    fn except_namespace_forbids_blanks_for_namespace_body() {
        // Single nested module → namespace → no_empty_lines.
        let offenses = run_cop_with_options::<EmptyLinesAroundModuleBody>(
            "module Foo\n\n  module Bar\n  end\n\nend\n",
            &opts(ModuleBodyStyle::EmptyLinesExceptNamespace),
        );
        assert_eq!(offenses.len(), 2, "got {offenses:?}");
        assert!(offenses.iter().all(|o| o.message.contains("Extra empty line")));
    }

    #[test]
    fn except_namespace_requires_blanks_for_non_namespace_body() {
        // A non-namespace body (a statement) → empty_lines.
        let offenses = run_cop_with_options::<EmptyLinesAroundModuleBody>(
            "module Foo\n  x = 1\nend\n",
            &opts(ModuleBodyStyle::EmptyLinesExceptNamespace),
        );
        assert_eq!(offenses.len(), 2, "got {offenses:?}");
        assert!(offenses.iter().all(|o| o.message.contains("Empty line missing")));
    }

    #[test]
    fn except_namespace_accepts_namespace_without_blanks() {
        test_with_opts(
            "module Foo\n  module Bar\n  end\nend\n",
            ModuleBodyStyle::EmptyLinesExceptNamespace,
        );
    }

    // --- empty_lines_special style ---

    #[test]
    fn special_requires_blank_before_first_def() {
        // First child is a def → requires a blank at the beginning.
        let offenses = run_cop_with_options::<EmptyLinesAroundModuleBody>(
            "module Foo\n  def a; end\n\nend\n",
            &opts(ModuleBodyStyle::EmptyLinesSpecial),
        );
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert!(offenses[0].message.contains("beginning"));
    }

    #[test]
    fn special_accepts_blank_before_first_def_and_at_end() {
        test_with_opts(
            "module Foo\n\n  def a; end\n\nend\n",
            ModuleBodyStyle::EmptyLinesSpecial,
        );
    }

    #[test]
    fn special_forbids_blank_at_beginning_for_non_required_first_child() {
        // First child is a statement (not def/class/module/modifier) → beginning
        // must NOT have a blank.
        let offenses = run_cop_with_options::<EmptyLinesAroundModuleBody>(
            "module Foo\n\n  x = 1\n\nend\n",
            &opts(ModuleBodyStyle::EmptyLinesSpecial),
        );
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert!(offenses[0].message.contains("beginning"));
        assert!(offenses[0].message.contains("Extra empty line"));
    }

    #[test]
    fn special_deferred_blank_before_interior_def() {
        // Non-required first child, then a def with no preceding blank → deferred.
        let offenses = run_cop_with_options::<EmptyLinesAroundModuleBody>(
            "module Foo\n  x = 1\n  def a; end\n\nend\n",
            &opts(ModuleBodyStyle::EmptyLinesSpecial),
        );
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Empty line missing before first def definition"
        );
    }

    #[test]
    fn special_skips_nil_body() {
        let offenses = run_cop_with_options::<EmptyLinesAroundModuleBody>(
            "module Foo\nend\n",
            &opts(ModuleBodyStyle::EmptyLinesSpecial),
        );
        assert!(offenses.is_empty(), "got {offenses:?}");
    }

    /// Parity pin (roborev #386): the deferred-empty-line check uses RuboCop's
    /// `previous_line_ignoring_comments`, which skips ONLY comment lines (not
    /// blanks) and returns the first non-comment line scanning upward. When a
    /// blank line sits directly above the def's leading comment, that blank IS
    /// the returned line, so `processed_source[line].empty?` is true and NO
    /// offense fires — the blank-before-comment is accepted.
    ///
    /// Source (1-based):
    ///   1 module Foo
    ///   2   x = 1
    ///   3   (blank)        <- returned by previous_line_ignoring_comments(5)
    ///   4   # comment
    ///   5   def a; end
    ///   6   (blank)
    ///   7 end
    /// `def a` at line 5 → scan 3.downto(0): line 3 (0-based) is the comment →
    /// skip → line 2 (0-based, the blank) → return 2 → blank → no offense.
    #[test]
    fn special_accepts_blank_before_leading_comment_of_def() {
        let offenses = run_cop_with_options::<EmptyLinesAroundModuleBody>(
            "module Foo\n  x = 1\n\n  # comment for method\n  def a; end\n\nend\n",
            &opts(ModuleBodyStyle::EmptyLinesSpecial),
        );
        assert!(offenses.is_empty(), "got {offenses:?}");
    }

    fn test_with_opts(src: &str, style: ModuleBodyStyle) {
        let offenses = run_cop_with_options::<EmptyLinesAroundModuleBody>(src, &opts(style));
        assert!(offenses.is_empty(), "expected no offenses, got {offenses:?}");
    }
}
