//! `Layout/LineContinuationSpacing` — the backslash of a line continuation
//! must be separated from preceding text by exactly one space (default) or
//! zero spaces.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/LineContinuationSpacing
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Direct port of `on_new_investigation` + `investigate` +
//!   `find_offensive_spacing` + `autocorrect`. Scans every source line except
//!   those at/after the last token's line. For each line ending in `\` (the
//!   continuation), measures the whitespace run `w` immediately before it:
//!
//!   - `space` style (default): flag when `w == 0` (range = the `\`) or
//!     `w >= 2` (range = the `w` spaces + `\`); accept `w == 1`. Correction
//!     ` \`. Message "Use one space in front of backslash."
//!   - `no_space` style: flag when `w >= 1` (range = the `w` spaces + `\`).
//!     Correction `\`. Message "Use zero spaces in front of backslash."
//!
//!   Backslashes inside ignored ranges are not flagged: comments, heredoc
//!   bodies, single `str` literals, `regexp`/`xstr` literals, percent-literal
//!   arrays, and interpolated/delimited `dstr` literals (those with a
//!   non-`str` child). Bare string-concatenation `dstr`s (`'a' \ 'b'`, only
//!   `str` children) are NOT ignored — their continuation `\` is checkable,
//!   matching RuboCop's `loc?(:begin)` discriminator.
//!
//!   `Enabled: pending` upstream → `default_enabled = false`. RuboCop measures
//!   in characters; Murphy uses byte offsets — identical for the ASCII
//!   whitespace/backslash these patterns match.
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeKind, Range, SourceTokenKind, cop};

/// Stateless unit struct (ADR 0035).
#[derive(Default)]
pub struct LineContinuationSpacing;

#[derive(CopOptions)]
pub struct LineContinuationSpacingOptions {
    #[option(
        name = "EnforcedStyle",
        default = "space",
        description = "Whether the line-continuation backslash is preceded by one space or zero spaces."
    )]
    pub enforced_style: ContinuationSpacingStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum ContinuationSpacingStyle {
    /// Exactly one space before the backslash (default).
    #[option(value = "space")]
    Space,
    /// No space before the backslash.
    #[option(value = "no_space")]
    NoSpace,
}

const MSG_SPACE: &str = "Use one space in front of backslash.";
const MSG_NO_SPACE: &str = "Use zero spaces in front of backslash.";

#[cop(
    name = "Layout/LineContinuationSpacing",
    description = "Checks the spacing in front of backslash in line continuations.",
    default_severity = "warning",
    default_enabled = false,
    options = LineContinuationSpacingOptions
)]
impl LineContinuationSpacing {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let src = cx.source();
        // RuboCop: `return unless processed_source.raw_source.include?('\\')`.
        if !src.contains('\\') {
            return;
        }

        let style = cx
            .options_or_default::<LineContinuationSpacingOptions>()
            .enforced_style;

        let ignored = collect_ignored_ranges(cx);
        let bytes = src.as_bytes();

        // RuboCop skips lines at/after the last token's line (`last_line`).
        // A continuation `\` is always followed by content, so the genuine
        // last line of the file can never be a continuation; scanning every
        // line and requiring a trailing `\` before a `\n` is equivalent.
        let mut line_start = 0usize;
        let mut i = 0usize;
        while i < bytes.len() {
            if bytes[i] == b'\n' {
                investigate(cx, bytes, line_start, i, style, &ignored);
                line_start = i + 1;
            }
            i += 1;
        }
    }
}

/// RuboCop `investigate`: if the line ends with a continuation `\`, measure the
/// whitespace run before it and flag per style.
fn investigate(
    cx: &Cx<'_>,
    bytes: &[u8],
    line_start: usize,
    nl: usize,
    style: ContinuationSpacingStyle,
    ignored: &[(u32, u32)],
) {
    // The line content is `[line_start, nl)`; a continuation requires the last
    // content byte to be `\`. Handle a trailing `\r` (CRLF) before the `\n`.
    let mut content_end = nl;
    if content_end > line_start && bytes[content_end - 1] == b'\r' {
        content_end -= 1;
    }
    if content_end == line_start || bytes[content_end - 1] != b'\\' {
        return;
    }
    let backslash = content_end - 1;

    // Whitespace run immediately before the backslash.
    let mut w_start = backslash;
    while w_start > line_start && is_space_byte(bytes[w_start - 1]) {
        w_start -= 1;
    }
    let w = backslash - w_start;

    // Determine the offense range + correction per RuboCop's regexes.
    let (range_start, message, correction) = match style {
        // `space`: `((?<!\s)|\s{2,})\\$` → flag w == 0 or w >= 2.
        ContinuationSpacingStyle::Space => {
            if w == 1 {
                return;
            }
            (w_start, MSG_SPACE, " \\")
        }
        // `no_space`: `\s+\\$` → flag w >= 1.
        ContinuationSpacingStyle::NoSpace => {
            if w == 0 {
                return;
            }
            (w_start, MSG_NO_SPACE, "\\")
        }
    };

    let range = Range {
        start: range_start as u32,
        end: content_end as u32,
    };

    // RuboCop `ignore_range?`: skip when the offense range is contained in an
    // ignored literal / comment range.
    if range_ignored(range, ignored) {
        return;
    }

    cx.emit_offense(range, message, None);
    cx.emit_edit(range, correction);
}

/// True when `range` is fully contained in any ignored range.
fn range_ignored(range: Range, ignored: &[(u32, u32)]) -> bool {
    ignored
        .iter()
        .any(|&(start, end)| start <= range.start && range.end <= end)
}

/// `\s` excluding `\n` — Ruby's `\s` is ASCII space/tab/`\r`/`\f`/`\v`.
fn is_space_byte(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\x0C' | b'\x0B')
}

/// Collect byte ranges in which a continuation `\` must be ignored: comments,
/// heredoc bodies, and string / regexp / xstr / percent-array / delimited-dstr
/// literals. Mirrors RuboCop's `ignored_ranges`.
fn collect_ignored_ranges(cx: &Cx<'_>) -> Vec<(u32, u32)> {
    let mut ranges: Vec<(u32, u32)> = Vec::new();

    // Comments (`Comment` tokens).
    for tok in cx.sorted_tokens() {
        if tok.kind == SourceTokenKind::Comment {
            ranges.push((tok.range.start, tok.range.end));
        }
    }

    // Heredoc bodies (FIFO over HeredocStart/HeredocEnd token pairs).
    ranges.extend(heredoc_body_ranges(cx));

    // Literal node ranges.
    for &id in cx.descendants(cx.root()).iter() {
        match cx.kind(id) {
            // `str` — a single quoted string. Its quoted range encloses any
            // internal `\`.
            NodeKind::Str(_) | NodeKind::Xstr(_) | NodeKind::Regexp { .. } => {
                let r = cx.range(id);
                ranges.push((r.start, r.end));
            }
            // `dstr` — only the delimited/interpolated form (a non-`str` child
            // is present) is ignored wholesale. A bare concatenation `dstr`
            // has only `str`/`dstr` children and its continuation `\` stays
            // checkable.
            NodeKind::Dstr(list) => {
                let children = cx.list(*list);
                let is_delimited = children
                    .iter()
                    .any(|&c| !matches!(cx.kind(c), NodeKind::Str(_) | NodeKind::Dstr(_)));
                if is_delimited {
                    let r = cx.range(id);
                    ranges.push((r.start, r.end));
                }
            }
            // `array` — only percent literals (`%w[…]`, `%i(…)`) are ignored.
            NodeKind::Array(_) if cx.is_percent_literal(id) => {
                let r = cx.range(id);
                ranges.push((r.start, r.end));
            }
            _ => {}
        }
    }

    ranges
}

/// FIFO heredoc-body byte ranges (body start..terminator line start).
fn heredoc_body_ranges(cx: &Cx<'_>) -> Vec<(u32, u32)> {
    use std::collections::VecDeque;
    let source = cx.source().as_bytes();
    let mut starts: VecDeque<u32> = VecDeque::new();
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    for tok in cx.sorted_tokens() {
        match tok.kind {
            SourceTokenKind::HeredocStart => starts.push_back(tok.range.end + 1),
            SourceTokenKind::HeredocEnd => {
                if let Some(body_start) = starts.pop_front() {
                    let term_line_start = source[..tok.range.start as usize]
                        .iter()
                        .rposition(|&b| b == b'\n')
                        .map_or(0, |i| i + 1) as u32;
                    ranges.push((body_start, term_line_start));
                }
            }
            _ => {}
        }
    }
    ranges
}

murphy_plugin_api::submit_cop!(LineContinuationSpacing);

#[cfg(test)]
mod tests {
    use super::{
        ContinuationSpacingStyle, LineContinuationSpacing, LineContinuationSpacingOptions,
    };
    use murphy_plugin_api::test_support::{run_cop, run_cop_with_edits, run_cop_with_options};

    fn no_space() -> LineContinuationSpacingOptions {
        LineContinuationSpacingOptions {
            enforced_style: ContinuationSpacingStyle::NoSpace,
        }
    }

    // ---------- space style (default) ----------

    #[test]
    fn space_accepts_single_space() {
        assert!(run_cop::<LineContinuationSpacing>("'a' \\\n'b'\n").is_empty());
    }

    #[test]
    fn space_flags_no_space() {
        let offenses = run_cop::<LineContinuationSpacing>("'a'\\\n'b'\n");
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].message, "Use one space in front of backslash.");
    }

    #[test]
    fn space_flags_two_spaces() {
        let offenses = run_cop::<LineContinuationSpacing>("'a'  \\\n'b'\n");
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].message, "Use one space in front of backslash.");
    }

    #[test]
    fn space_corrects_no_space_to_one() {
        let run = run_cop_with_edits::<LineContinuationSpacing>("'a'\\\n'b'\n");
        assert_eq!(apply("'a'\\\n'b'\n", &run.edits), "'a' \\\n'b'\n");
    }

    #[test]
    fn space_corrects_two_spaces_to_one() {
        let run = run_cop_with_edits::<LineContinuationSpacing>("'a'  \\\n'b'\n");
        assert_eq!(apply("'a'  \\\n'b'\n", &run.edits), "'a' \\\n'b'\n");
    }

    // ---------- no_space style ----------

    #[test]
    fn no_space_accepts_no_space() {
        assert!(
            run_cop_with_options::<LineContinuationSpacing>("'a'\\\n'b'\n", &no_space()).is_empty()
        );
    }

    #[test]
    fn no_space_flags_one_space() {
        let offenses =
            run_cop_with_options::<LineContinuationSpacing>("'a' \\\n'b'\n", &no_space());
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].message, "Use zero spaces in front of backslash.");
    }

    // ---------- ignored ranges ----------

    #[test]
    fn ignores_backslash_inside_comment() {
        // A `\` ending a comment line is not a continuation offense.
        assert!(run_cop::<LineContinuationSpacing>("x = 1 # foo  \\\ny = 2\n").is_empty());
    }

    #[test]
    fn ignores_backslash_inside_heredoc() {
        let src = "x = <<~RUBY\n  a  \\\n  b\nRUBY\n";
        assert!(run_cop::<LineContinuationSpacing>(src).is_empty());
    }

    #[test]
    fn flags_each_continuation_in_chain() {
        // Two bad continuations (no space).
        let offenses = run_cop::<LineContinuationSpacing>("'a'\\\n'b'\\\n'c'\n");
        assert_eq!(offenses.len(), 2);
    }

    #[test]
    fn ignores_non_continuation_line() {
        assert!(run_cop::<LineContinuationSpacing>("x = 1\ny = 2\n").is_empty());
    }

    /// Apply all non-overlapping edits to `source` (left to right).
    fn apply(source: &str, edits: &[murphy_plugin_api::test_support::CapturedEdit]) -> String {
        let mut ordered: Vec<_> = edits.iter().collect();
        ordered.sort_by_key(|e| e.range.start);
        let mut out = String::with_capacity(source.len());
        let mut last = 0usize;
        for e in ordered {
            out.push_str(&source[last..e.range.start as usize]);
            out.push_str(&e.replacement);
            last = e.range.end as usize;
        }
        out.push_str(&source[last..]);
        out
    }
}
