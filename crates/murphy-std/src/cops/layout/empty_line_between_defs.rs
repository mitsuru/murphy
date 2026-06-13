//! `Layout/EmptyLineBetweenDefs` — require blank line(s) between consecutive
//! class / module / method definitions.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/EmptyLineBetweenDefs
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: [murphy-upgm]
//! notes: >
//!   Ports RuboCop's `on_begin`: walk each `begin` body's children in
//!   consecutive pairs; when both members are definition candidates
//!   (`def`/`defs`/`class`/`module`, gated by `EmptyLineBetweenMethodDefs` /
//!   `EmptyLineBetweenClassDefs` / `EmptyLineBetweenModuleDefs`), count the
//!   blank lines between the first def's end line and the second def's start
//!   line. If the count is outside `NumberOfEmptyLines`, flag the second def.
//!   `AllowAdjacentOneLineDefs` (default true) suppresses the offense when both
//!   defs are single-line. Offense location is `keyword..name` of the second
//!   def (RuboCop's `def_location`).
//!
//!   Autocorrect inserts/removes blank lines at the first newline after the
//!   previous def's end (RuboCop's `autocorrect`), handling the same-line
//!   one-liner case by anchoring before the second def instead.
//!
//!   Documented gaps (filed as murphy-upgm):
//!     - `NumberOfEmptyLines` is modelled as a single integer (min == max).
//!       RuboCop also accepts an array `[min, max]`; the array (allowance
//!       range) form is not supported, so `expected_lines` always renders a
//!       fixed count.
//!     - `DefLikeMacros` (treating configured macro calls like defs) is not
//!       supported — only true `def`/`defs`/`class`/`module` are candidates.
//!     - `multiple_blank_lines_groups?` (skip when blank lines are split by a
//!       comment group) is not modelled; such cases still flag, matching the
//!       common single-group layout.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};

/// Stateless unit struct (ADR 0035 const-metadata cop pattern).
#[derive(Default)]
pub struct EmptyLineBetweenDefs;

#[derive(CopOptions)]
pub struct EmptyLineBetweenDefsOptions {
    #[option(
        name = "EmptyLineBetweenMethodDefs",
        default = true,
        description = "Check for empty lines between method definitions."
    )]
    pub method_defs: bool,
    #[option(
        name = "EmptyLineBetweenClassDefs",
        default = true,
        description = "Check for empty lines between class definitions."
    )]
    pub class_defs: bool,
    #[option(
        name = "EmptyLineBetweenModuleDefs",
        default = true,
        description = "Check for empty lines between module definitions."
    )]
    pub module_defs: bool,
    #[option(
        name = "AllowAdjacentOneLineDefs",
        default = true,
        description = "Allow adjacent one-line definitions without a blank line."
    )]
    pub allow_adjacent_one_line_defs: bool,
    #[option(
        name = "NumberOfEmptyLines",
        default = 1,
        description = "Number of empty lines required between definitions."
    )]
    pub number_of_empty_lines: i64,
}

#[cop(
    name = "Layout/EmptyLineBetweenDefs",
    description = "Use empty lines between method, class, and module definitions.",
    default_severity = "warning",
    default_enabled = true,
    options = EmptyLineBetweenDefsOptions
)]
impl EmptyLineBetweenDefs {
    #[on_node(kind = "begin")]
    fn check_begin(&self, node: NodeId, cx: &Cx<'_>, options: &EmptyLineBetweenDefsOptions) {
        let NodeKind::Begin(list) = *cx.kind(node) else {
            return;
        };
        let children = cx.list(list);
        // RuboCop: `node.children.each_cons(2)`.
        for pair in children.windows(2) {
            let prev = pair[0];
            let cur = pair[1];
            if candidate(prev, cx, options) && candidate(cur, cx, options) {
                check_defs(prev, cur, cx, options);
            }
        }
    }
}

/// RuboCop `candidate?`: a method/class/module definition gated by the
/// corresponding enable flag.
fn candidate(node: NodeId, cx: &Cx<'_>, options: &EmptyLineBetweenDefsOptions) -> bool {
    match *cx.kind(node) {
        NodeKind::Def { .. } | NodeKind::Defs { .. } => options.method_defs,
        NodeKind::Class { .. } => options.class_defs,
        NodeKind::Module { .. } => options.module_defs,
        _ => false,
    }
}

/// RuboCop `check_defs`.
fn check_defs(prev: NodeId, cur: NodeId, cx: &Cx<'_>, options: &EmptyLineBetweenDefsOptions) {
    let count = blank_lines_count_between(prev, cur, cx);
    let expected = options.number_of_empty_lines.max(0) as usize;

    // RuboCop: `return if line_count_allowed?(count)` (min..max cover); here
    // min == max == expected.
    if count == expected {
        return;
    }
    // RuboCop: `return if nodes.all?(&:single_line?) && AllowAdjacentOneLineDefs`.
    if options.allow_adjacent_one_line_defs && is_single_line(prev, cx) && is_single_line(cur, cx) {
        return;
    }

    let location = def_location(cur, cx);
    let message = format!(
        "Expected {} between {} definitions; found {count}.",
        expected_lines(expected),
        node_type(cur, cx),
    );
    cx.emit_offense(location, &message, None);
    autocorrect(prev, cur, count, expected, cx);
}

/// RuboCop `def_location`: `loc.keyword.join(loc.name)`. For `def`/`defs`/
/// `class`/`module` the node starts at the keyword, so the range spans from the
/// node start to the end of the definition's name.
fn def_location(node: NodeId, cx: &Cx<'_>) -> Range {
    let loc = cx.loc(node);
    let name_end = loc.name.end;
    let start = cx.range(node).start;
    if name_end > start {
        Range {
            start,
            end: name_end,
        }
    } else {
        // Fallback: keyword token only (e.g. anonymous shape with no name loc).
        let keyword = loc.keyword();
        if keyword != Range::ZERO {
            keyword
        } else {
            Range {
                start,
                end: start,
            }
        }
    }
}

/// RuboCop `node_type`: defs map to `method`; everything else uses its type.
fn node_type(node: NodeId, cx: &Cx<'_>) -> &'static str {
    match *cx.kind(node) {
        NodeKind::Def { .. } | NodeKind::Defs { .. } => "method",
        NodeKind::Class { .. } => "class",
        NodeKind::Module { .. } => "module",
        _ => "definition",
    }
}

/// RuboCop `expected_lines` for the fixed-count case (allowance range is a
/// documented gap).
fn expected_lines(expected: usize) -> String {
    let lines = if expected == 1 { "line" } else { "lines" };
    format!("{expected} empty {lines}")
}

/// True iff the node occupies a single source line.
fn is_single_line(node: NodeId, cx: &Cx<'_>) -> bool {
    let range = cx.range(node);
    let src = cx.source().as_bytes();
    !src[range.start as usize..range.end as usize].contains(&b'\n')
}

/// RuboCop `blank_lines_count_between`: blank lines strictly between the first
/// def's end line and the second def's start line.
fn blank_lines_count_between(prev: NodeId, cur: NodeId, cx: &Cx<'_>) -> usize {
    let src = cx.source().as_bytes();
    // The region between `prev`'s end and `cur`'s start.
    let between_start = cx.range(prev).end as usize;
    let cur_start = cx.range(cur).start as usize;
    if cur_start <= between_start {
        return 0;
    }

    // RuboCop counts whole physical lines that lie strictly between the two
    // definitions and are blank. The first newline after `prev` ends `prev`'s
    // last line; the line containing `cur`'s start is `cur`'s first line.
    // Lines in between are the candidates.
    let region = &src[between_start..cur_start];
    // The slice starts mid-line (right after `prev`'s last token). Skip to the
    // first newline so we begin counting on the line *after* `prev`.
    let Some(first_nl) = region.iter().position(|&b| b == b'\n') else {
        return 0;
    };
    let mut line_start = first_nl + 1;
    let mut count = 0usize;
    while line_start < region.len() {
        let line_end = region[line_start..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(region.len(), |i| line_start + i);
        // The final partial line (no trailing newline before `cur`) is `cur`'s
        // own line and must not be counted.
        if line_end >= region.len() {
            break;
        }
        if region[line_start..line_end]
            .iter()
            .all(|b| b.is_ascii_whitespace())
        {
            count += 1;
        }
        line_start = line_end + 1;
    }
    count
}

/// RuboCop `autocorrect`: anchor at the first newline after `prev`'s end, then
/// remove surplus or insert missing blank lines.
fn autocorrect(prev: NodeId, cur: NodeId, count: usize, expected: usize, cx: &Cx<'_>) {
    let src = cx.source().as_bytes();
    let end_pos = cx.range(prev).end as usize;
    let Some(rel) = src[end_pos..].iter().position(|&b| b == b'\n') else {
        return;
    };
    let mut newline_pos = end_pos + rel;
    let begin_pos = cx.range(cur).start as usize;
    // Same-line one-liners: anchor just before `cur` instead.
    if newline_pos > begin_pos {
        newline_pos = begin_pos.saturating_sub(1);
    }

    if count > expected {
        // Remove `count - expected` newlines starting at `newline_pos`.
        let difference = count - expected;
        let range = Range {
            start: newline_pos as u32,
            end: (newline_pos + difference) as u32,
        };
        cx.emit_edit(range, "");
    } else {
        // Insert `expected - count` newlines after `newline_pos`.
        let difference = expected - count;
        let anchor = Range {
            start: (newline_pos + 1) as u32,
            end: (newline_pos + 1) as u32,
        };
        cx.emit_edit(anchor, &"\n".repeat(difference));
    }
}

murphy_plugin_api::submit_cop!(EmptyLineBetweenDefs);

#[cfg(test)]
mod tests {
    use super::EmptyLineBetweenDefs;
    use murphy_plugin_api::test_support::{indoc, run_cop_with_edits, test};

    fn apply(source: &str, edits: &[murphy_plugin_api::test_support::CapturedEdit]) -> String {
        assert_eq!(edits.len(), 1, "expected exactly one edit");
        let edit = &edits[0];
        let mut out = String::with_capacity(source.len() + edit.replacement.len());
        out.push_str(&source[..edit.range.start as usize]);
        out.push_str(&edit.replacement);
        out.push_str(&source[edit.range.end as usize..]);
        out
    }

    // ── Clean ────────────────────────────────────────────────────────────────

    #[test]
    fn accepts_blank_line_between_methods() {
        test::<EmptyLineBetweenDefs>().expect_no_offenses(indoc! {r#"
            def a
            end

            def b
            end
        "#});
    }

    #[test]
    fn accepts_single_method() {
        test::<EmptyLineBetweenDefs>().expect_no_offenses(indoc! {r#"
            def a
            end
        "#});
    }

    #[test]
    fn accepts_blank_line_between_classes() {
        test::<EmptyLineBetweenDefs>().expect_no_offenses(indoc! {r#"
            class A
            end

            class B
            end
        "#});
    }

    #[test]
    fn accepts_adjacent_one_line_defs() {
        // AllowAdjacentOneLineDefs default true.
        test::<EmptyLineBetweenDefs>().expect_no_offenses(indoc! {r#"
            def a; end
            def b; end
        "#});
    }

    // ── Offenses ─────────────────────────────────────────────────────────────

    #[test]
    fn flags_missing_blank_line_between_methods() {
        let offenses = murphy_plugin_api::test_support::run_cop::<EmptyLineBetweenDefs>(indoc! {r#"
            def a
            end
            def b
            end
        "#});
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Expected 1 empty line between method definitions; found 0."
        );
    }

    #[test]
    fn corrects_missing_blank_line_between_methods() {
        let src = "def a\nend\ndef b\nend\n";
        let run = run_cop_with_edits::<EmptyLineBetweenDefs>(src);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(apply(src, &run.edits), "def a\nend\n\ndef b\nend\n");
    }

    #[test]
    fn flags_missing_blank_line_between_classes() {
        let offenses = murphy_plugin_api::test_support::run_cop::<EmptyLineBetweenDefs>(indoc! {r#"
            class A
            end
            class B
            end
        "#});
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Expected 1 empty line between class definitions; found 0."
        );
    }

    #[test]
    fn flags_missing_blank_line_between_modules() {
        let offenses = murphy_plugin_api::test_support::run_cop::<EmptyLineBetweenDefs>(indoc! {r#"
            module A
            end
            module B
            end
        "#});
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Expected 1 empty line between module definitions; found 0."
        );
    }

    #[test]
    fn flags_too_many_blank_lines() {
        let offenses = murphy_plugin_api::test_support::run_cop::<EmptyLineBetweenDefs>(
            "def a\nend\n\n\ndef b\nend\n",
        );
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Expected 1 empty line between method definitions; found 2."
        );
    }

    #[test]
    fn corrects_too_many_blank_lines() {
        let src = "def a\nend\n\n\ndef b\nend\n";
        let run = run_cop_with_edits::<EmptyLineBetweenDefs>(src);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(apply(src, &run.edits), "def a\nend\n\ndef b\nend\n");
    }
}
