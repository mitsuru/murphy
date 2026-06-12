//! `Layout/LineContinuationLeadingSpace` — in a string broken over multiple
//! lines with a backslash, keep the spacing on the trailing edge of the
//! previous line (default) rather than the leading edge of the next line.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/LineContinuationLeadingSpace
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Direct port of RuboCop's `on_dstr`. Visits each `dstr`; bails unless its
//!   source contains a backslash. Walks consecutive raw source-line pairs of
//!   the node, accumulating `end_of_first_line` (the byte offset of the end of
//!   line one / start of line two) by adding each line one's byte length BEFORE
//!   the continuation check, exactly as upstream does. A line is a continuation
//!   when it ends with `\\\n` AND no child node both covers that line and is
//!   itself multiline. With `EnforcedStyle: trailing` (default) the cop flags
//!   leading spaces after the next line's opening quote (`\A\s*['"]\s+`) and
//!   moves them to just before the previous line's closing quote (before its
//!   `['"]\s*\\\n` tail); message "Move leading spaces to the end of the
//!   previous line." With `EnforcedStyle: leading` it flags trailing spaces
//!   before the previous line's continuation (`\s+['"]\s*\\\n`) and moves them
//!   to the start of the next line's string; message "Move trailing spaces to
//!   the start of the next line." Autocorrect is two non-overlapping edits: a
//!   removal of the offending run plus an insertion at the destination.
//!   `Enabled: pending` upstream, so `default_enabled = false`. RuboCop measures
//!   match lengths in characters; Murphy uses byte lengths — identical for
//!   ASCII whitespace/quotes, which is all these patterns match.
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, Range, cop};

#[derive(Default)]
pub struct LineContinuationLeadingSpace;

#[derive(CopOptions)]
pub struct LineContinuationLeadingSpaceOptions {
    #[option(
        name = "EnforcedStyle",
        default = "trailing",
        description = "Whether to keep continuation spacing on the trailing edge of the previous line or the leading edge of the next line."
    )]
    pub enforced_style: LineContinuationStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum LineContinuationStyle {
    /// Spaces belong on the leading edge of the continuation line.
    #[option(value = "leading")]
    Leading,
    /// Spaces belong on the trailing edge of the previous line (default).
    #[option(value = "trailing")]
    Trailing,
}

const MSG_LEADING: &str = "Move trailing spaces to the start of the next line.";
const MSG_TRAILING: &str = "Move leading spaces to the end of the previous line.";

#[cop(
    name = "Layout/LineContinuationLeadingSpace",
    description = "Use trailing spaces instead of leading spaces in strings broken over multiple lines.",
    default_severity = "warning",
    default_enabled = false,
    options = LineContinuationLeadingSpaceOptions
)]
impl LineContinuationLeadingSpace {
    #[on_node(kind = "dstr")]
    fn check_dstr(&self, node: NodeId, cx: &Cx<'_>) {
        let range = cx.range(node);
        let node_src = cx.raw_source(range);
        // RuboCop: `return unless node.source.include?('\\')`.
        if !node_src.contains('\\') {
            return;
        }

        let style = cx
            .options_or_default::<LineContinuationLeadingSpaceOptions>()
            .enforced_style;
        let source = cx.source();

        // RuboCop: `end_of_first_line = node.source_range.begin_pos - column`.
        // That is the byte offset of the start of the line the node begins on.
        let node_start = range.start as usize;
        let first_line_start = source[..node_start].rfind('\n').map_or(0, |p| p + 1);
        // `node.first_line` (1-based) — used to track `line_num` per pair.
        let first_line_num = source[..node_start].matches('\n').count() + 1;

        // The raw source lines the node spans, each keeping its trailing `\n`
        // (RuboCop's `processed_source.raw_source.lines[...]`). The slice runs
        // from the node's first line through the line containing its end.
        let node_end = range.end as usize;
        let lines = source_lines(source, first_line_start, node_end);

        // RuboCop: `lines.each_cons(2).with_index(node.first_line) do ...`.
        let mut end_of_first_line = first_line_start;
        for (i, pair) in lines.windows(2).enumerate() {
            let (line_one_start, line_one) = pair[0];
            let (_line_two_start, line_two) = pair[1];
            let line_num = first_line_num + i;

            // RuboCop: `end_of_first_line += raw_line_one.length` — accumulated
            // UNCONDITIONALLY, before the continuation check.
            end_of_first_line += line_one.len();

            // RuboCop: `next unless continuation?(raw_line_one, line_num, node)`.
            if !is_continuation(cx, node, line_one, line_num) {
                continue;
            }

            match style {
                LineContinuationStyle::Leading => self.investigate_leading(
                    cx,
                    line_one,
                    line_two,
                    line_one_start,
                    end_of_first_line,
                ),
                LineContinuationStyle::Trailing => {
                    self.investigate_trailing(cx, line_one, line_two, end_of_first_line)
                }
            }
        }
    }
}

impl LineContinuationLeadingSpace {
    /// RuboCop `investigate_trailing_style`: flag the run of whitespace after
    /// the next line's opening quote, and move it before the previous line's
    /// closing quote.
    fn investigate_trailing(
        &self,
        cx: &Cx<'_>,
        line_one: &str,
        line_two: &str,
        end_of_first_line: usize,
    ) {
        // `TRAILING_STYLE_OFFENSE = /(?<beginning>\A\s*['"])(?<leading_spaces>\s+)/`.
        let Some((beginning_len, spaces)) = match_line_two_leading(line_two) else {
            return;
        };

        // `trailing_offense_range`: begin = end_of_first_line + beginning.len,
        // end = begin + leading_spaces.len.
        let begin_pos = end_of_first_line + beginning_len;
        let end_pos = begin_pos + spaces.len();
        let offense_range = Range {
            start: begin_pos as u32,
            end: end_pos as u32,
        };

        // `insert_pos = end_of_first_line - first_line[LINE_1_ENDING].length`.
        let Some(ending_len) = line_one_ending_len(line_one) else {
            return;
        };
        let insert_pos = (end_of_first_line - ending_len) as u32;

        cx.emit_offense(offense_range, MSG_TRAILING, None);
        self.autocorrect(cx, offense_range, insert_pos, spaces);
    }

    /// RuboCop `investigate_leading_style`: flag the run of whitespace before
    /// the previous line's continuation, and move it to the start of the next
    /// line's string.
    fn investigate_leading(
        &self,
        cx: &Cx<'_>,
        line_one: &str,
        line_two: &str,
        line_one_start: usize,
        end_of_first_line: usize,
    ) {
        // `LEADING_STYLE_OFFENSE = /(?<trailing_spaces>\s+)(?<ending>['"]\s*\\\n)/`.
        let Some((trailing_spaces, ending_len)) = match_line_one_trailing(line_one) else {
            return;
        };

        // `leading_offense_range`: end = end_of_first_line - ending.len,
        // begin = end - trailing_spaces.len.
        let end_p = end_of_first_line - ending_len;
        let begin_p = end_p - trailing_spaces.len();
        // The `trailing_spaces` slice is the run that ends just before the
        // ending; borrow it from `line_one` so it outlives the closure.
        let spaces = &line_one[(begin_p - line_one_start)..(end_p - line_one_start)];
        let offense_range = Range {
            start: begin_p as u32,
            end: end_p as u32,
        };

        // `insert_pos = end_of_first_line + second_line[LINE_2_BEGINNING].length`.
        let Some(beginning_len) = line_two_beginning_len(line_two) else {
            return;
        };
        let insert_pos = (end_of_first_line + beginning_len) as u32;

        cx.emit_offense(offense_range, MSG_LEADING, None);
        self.autocorrect(cx, offense_range, insert_pos, spaces);
    }

    /// RuboCop `autocorrect`: remove the offending run, then insert the same
    /// spaces at the destination. Two non-overlapping edits.
    fn autocorrect(&self, cx: &Cx<'_>, offense_range: Range, insert_pos: u32, spaces: &str) {
        cx.emit_edit(offense_range, "");
        cx.emit_edit(
            Range {
                start: insert_pos,
                end: insert_pos,
            },
            spaces,
        );
    }
}

/// Build `(line_start_offset, line_with_trailing_newline)` for every source
/// line from `first_line_start` through the line containing `until_offset`.
///
/// Mirrors `processed_source.raw_source.lines[node.first_line - 1, count]`:
/// whole source lines (last line may extend past the node), each retaining its
/// trailing `\n`.
fn source_lines(source: &str, first_line_start: usize, until_offset: usize) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut pos = first_line_start;
    while pos < source.len() {
        let nl = source[pos..].find('\n').map(|i| pos + i);
        let line_end = match nl {
            Some(n) => n + 1, // include the `\n`
            None => source.len(),
        };
        out.push((pos, &source[pos..line_end]));
        // Stop once we have consumed the line containing `until_offset`.
        if line_end >= until_offset {
            break;
        }
        pos = line_end;
    }
    out
}

/// RuboCop `continuation?(line, line_num, node)`:
/// `line.end_with?("\\\n")` and no multiline child covers `line_num`.
fn is_continuation(cx: &Cx<'_>, node: NodeId, line: &str, line_num: usize) -> bool {
    if !line.ends_with("\\\n") {
        return false;
    }
    // `node.children.none? { |c| (c.first_line...c.last_line).cover?(line_num) && c.multiline? }`.
    // `(first_line...last_line)` is a half-open range, so a child covers
    // `line_num` iff `first_line <= line_num < last_line`.
    cx.children(node).iter().all(|&child| {
        if !cx.is_multiline(child) {
            return true;
        }
        let cr = cx.range(child);
        let src = cx.source();
        let child_first = src[..cr.start as usize].matches('\n').count() + 1;
        let child_last = src[..cr.end as usize].matches('\n').count() + 1;
        // NOT covered → keep `all` true.
        !(child_first <= line_num && line_num < child_last)
    })
}

/// `TRAILING_STYLE_OFFENSE = /\A\s*['"]\s+/` applied to line two.
///
/// Returns `(beginning_len, leading_spaces)` where `beginning_len` is the byte
/// length of the `\A\s*['"]` prefix and `leading_spaces` is the run of
/// whitespace immediately after the quote (stopping at the line terminator).
fn match_line_two_leading(line_two: &str) -> Option<(usize, &str)> {
    let bytes = line_two.as_bytes();
    let mut i = 0;
    // `\s*` leading whitespace (excluding newlines — a quote must follow).
    while i < bytes.len() && is_space_byte(bytes[i]) {
        i += 1;
    }
    // `['"]` — the opening quote.
    if i >= bytes.len() || (bytes[i] != b'\'' && bytes[i] != b'"') {
        return None;
    }
    let beginning_len = i + 1;
    // `\s+` — one or more whitespace after the quote (stop at `\r`/`\n`).
    let spaces_start = beginning_len;
    let mut j = spaces_start;
    while j < bytes.len() && is_space_byte(bytes[j]) {
        j += 1;
    }
    if j == spaces_start {
        return None;
    }
    Some((beginning_len, &line_two[spaces_start..j]))
}

/// `LINE_2_BEGINNING = /\A\s*['"]/` length on line two.
fn line_two_beginning_len(line_two: &str) -> Option<usize> {
    let bytes = line_two.as_bytes();
    let mut i = 0;
    while i < bytes.len() && is_space_byte(bytes[i]) {
        i += 1;
    }
    if i < bytes.len() && (bytes[i] == b'\'' || bytes[i] == b'"') {
        Some(i + 1)
    } else {
        None
    }
}

/// `LINE_1_ENDING = /['"]\s*\\\n/` length on line one (the closing-quote tail).
fn line_one_ending_len(line_one: &str) -> Option<usize> {
    // Line one ends in `\\\n`; before the `\` there is `['"]\s*`. Find the
    // last quote that is followed only by `\s*\\\n`.
    let trimmed = line_one.strip_suffix("\\\n")?;
    let bytes = trimmed.as_bytes();
    // Strip the `\s*` immediately before the `\`.
    let mut end = bytes.len();
    while end > 0 && is_space_byte(bytes[end - 1]) {
        end -= 1;
    }
    // The byte just before the spaces must be a quote.
    if end == 0 {
        return None;
    }
    let q = bytes[end - 1];
    if q != b'\'' && q != b'"' {
        return None;
    }
    // ending = from the quote through `\\\n`: quote + spaces + `\` + `\n`.
    Some(line_one.len() - (end - 1))
}

/// `LEADING_STYLE_OFFENSE = /(\s+)(['"]\s*\\\n)/` applied to line one.
///
/// Returns `(trailing_spaces, ending_len)` — the whitespace run immediately
/// before the `['"]\s*\\\n` ending and that ending's byte length.
fn match_line_one_trailing(line_one: &str) -> Option<(&str, usize)> {
    let ending_len = line_one_ending_len(line_one)?;
    let ending_start = line_one.len() - ending_len;
    // The ending begins with the quote. The `\s+` run is the whitespace
    // immediately before the quote.
    let before = &line_one[..ending_start];
    let bytes = before.as_bytes();
    let mut start = before.len();
    while start > 0 && is_space_byte(bytes[start - 1]) {
        start -= 1;
    }
    if start == before.len() {
        return None; // no trailing spaces → no offense
    }
    Some((&before[start..], ending_len))
}

/// Ruby `\s` is ASCII; here it matches space/tab/`\r`/form-feed/vtab but NOT
/// `\n` (line boundaries are handled by the line splitter).
fn is_space_byte(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\x0C' | b'\x0B')
}

murphy_plugin_api::submit_cop!(LineContinuationLeadingSpace);

#[cfg(test)]
mod tests {
    use super::{
        LineContinuationLeadingSpace, LineContinuationLeadingSpaceOptions, LineContinuationStyle,
    };
    use murphy_plugin_api::test_support::{
        run_cop, run_cop_with_edits, run_cop_with_options, run_cop_with_options_and_edits,
        CapturedEdit,
    };

    fn leading() -> LineContinuationLeadingSpaceOptions {
        LineContinuationLeadingSpaceOptions {
            enforced_style: LineContinuationStyle::Leading,
        }
    }

    /// Apply all non-overlapping edits to `source` (left to right).
    fn apply(source: &str, edits: &[CapturedEdit]) -> String {
        let mut ordered: Vec<&CapturedEdit> = edits.iter().collect();
        ordered.sort_by_key(|e| e.range.start);
        let mut out = String::with_capacity(source.len());
        let mut last = 0usize;
        for e in ordered {
            assert!(
                e.range.start as usize >= last,
                "overlapping edits: {:?}",
                edits
            );
            out.push_str(&source[last..e.range.start as usize]);
            out.push_str(&e.replacement);
            last = e.range.end as usize;
        }
        out.push_str(&source[last..]);
        out
    }

    // ---------- trailing style (default) ----------

    #[test]
    fn trailing_accepts_clean_continuation() {
        // Spaces are on the previous line's trailing edge — already correct.
        assert!(run_cop::<LineContinuationLeadingSpace>("x = \"foo  \" \\\n\"bar\"\n").is_empty());
    }

    #[test]
    fn trailing_flags_leading_spaces_after_next_quote() {
        let offenses = run_cop::<LineContinuationLeadingSpace>("x = \"foo\" \\\n\"  bar\"\n");
        assert_eq!(offenses.len(), 1);
        assert_eq!(
            offenses[0].message,
            "Move leading spaces to the end of the previous line."
        );
    }

    #[test]
    fn trailing_corrects_moves_spaces_to_previous_line() {
        let src = "x = \"foo\" \\\n\"  bar\"\n";
        let run = run_cop_with_edits::<LineContinuationLeadingSpace>(src);
        assert_eq!(apply(src, &run.edits), "x = \"foo  \" \\\n\"bar\"\n");
    }

    #[test]
    fn trailing_is_idempotent() {
        let src = "x = \"foo\" \\\n\"  bar\"\n";
        let run = run_cop_with_edits::<LineContinuationLeadingSpace>(src);
        let corrected = apply(src, &run.edits);
        let again = run_cop_with_edits::<LineContinuationLeadingSpace>(&corrected);
        assert!(
            again.offenses.is_empty(),
            "second pass still flagged: {:?}",
            again.offenses
        );
    }

    #[test]
    fn trailing_ignores_non_continuation_dstr() {
        // Interpolation, no backslash continuation → no offense.
        assert!(run_cop::<LineContinuationLeadingSpace>("x = \"a#{b}c\"\n").is_empty());
    }

    #[test]
    fn trailing_handles_three_string_chain() {
        // Two continuation boundaries, each with leading spaces after the quote.
        let src = "x = \"a\" \\\n\"  b\" \\\n\"  c\"\n";
        let run = run_cop_with_edits::<LineContinuationLeadingSpace>(src);
        assert_eq!(run.offenses.len(), 2, "expected 2 offenses: {:?}", run.offenses);
        assert_eq!(apply(src, &run.edits), "x = \"a  \" \\\n\"b  \" \\\n\"c\"\n");
    }

    // ---------- leading style ----------

    #[test]
    fn leading_accepts_clean_continuation() {
        // Spaces on the next line's leading edge — correct for `leading`.
        assert!(
            run_cop_with_options::<LineContinuationLeadingSpace>(
                "x = \"foo\" \\\n\"  bar\"\n",
                &leading()
            )
            .is_empty()
        );
    }

    #[test]
    fn leading_flags_trailing_spaces_before_continuation() {
        let offenses = run_cop_with_options::<LineContinuationLeadingSpace>(
            "x = \"foo  \" \\\n\"bar\"\n",
            &leading(),
        );
        assert_eq!(offenses.len(), 1);
        assert_eq!(
            offenses[0].message,
            "Move trailing spaces to the start of the next line."
        );
    }

    #[test]
    fn leading_corrects_moves_spaces_to_next_line() {
        let src = "x = \"foo  \" \\\n\"bar\"\n";
        let run = run_cop_with_options_and_edits::<LineContinuationLeadingSpace>(src, &leading());
        assert_eq!(apply(src, &run.edits), "x = \"foo\" \\\n\"  bar\"\n");
    }

    #[test]
    fn leading_is_idempotent() {
        let src = "x = \"foo  \" \\\n\"bar\"\n";
        let run = run_cop_with_options_and_edits::<LineContinuationLeadingSpace>(src, &leading());
        let corrected = apply(src, &run.edits);
        let again =
            run_cop_with_options_and_edits::<LineContinuationLeadingSpace>(&corrected, &leading());
        assert!(
            again.offenses.is_empty(),
            "second pass still flagged: {:?}",
            again.offenses
        );
    }
}
