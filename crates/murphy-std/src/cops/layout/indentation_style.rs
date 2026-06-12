//! `Layout/IndentationStyle` — consistent indentation either with tabs only
//! or spaces only.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/IndentationStyle
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's `on_new_investigation` line scan. For
//!   `EnforcedStyle: spaces` (default) a leading run of optional spaces
//!   followed by one or more tabs (`/\A\s*\t+/`) is flagged ("Tab detected
//!   in indentation."); for `EnforcedStyle: tabs` a leading run of optional
//!   whitespace followed by one or more spaces (`/\A\s* +/`) is flagged
//!   ("Space detected in indentation."). Offenses whose range is fully
//!   contained in a `str`/`dstr` node range are skipped, matching RuboCop's
//!   `string_literal_ranges` exemption (this covers heredoc bodies and
//!   multiline string literals, where leading tabs/spaces are content, not
//!   indentation). Autocorrect: for `spaces`, each leading tab becomes
//!   `IndentationWidth` spaces; for `tabs`, each `IndentationWidth`-wide run
//!   of leading spaces becomes one tab.
//!   `IndentationWidth` defaults to 2 (Murphy cannot read the sibling
//!   `Layout/IndentationWidth: Width` value across the single-surface ABI,
//!   so the RuboCop default of 2 is applied directly when the option is
//!   unset).
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeKind, Range, SourceTokenKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct IndentationStyle;

#[derive(CopOptions)]
pub struct IndentationStyleOptions {
    #[option(
        name = "EnforcedStyle",
        default = "spaces",
        description = "Whether indentation must use spaces only or tabs only."
    )]
    pub enforced_style: IndentationStyleKind,
    #[option(
        name = "IndentationWidth",
        default = 2,
        description = "Number of spaces that replace each tab (and vice versa) during autocorrection."
    )]
    pub indentation_width: i64,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum IndentationStyleKind {
    #[option(value = "spaces")]
    Spaces,
    #[option(value = "tabs")]
    Tabs,
}

#[cop(
    name = "Layout/IndentationStyle",
    description = "Consistent indentation either with tabs only or spaces only.",
    default_severity = "warning",
    default_enabled = true,
    options = IndentationStyleOptions,
)]
impl IndentationStyle {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<IndentationStyleOptions>();
        let bytes = cx.source().as_bytes();
        // Lazily computed only when an offense candidate is found, mirroring
        // RuboCop's "perform costly calculation only when needed".
        let mut str_ranges: Option<Vec<Range>> = None;

        let mut line_start = 0usize;
        while line_start < bytes.len() {
            let line_end = bytes[line_start..]
                .iter()
                .position(|&b| b == b'\n')
                .map(|i| line_start + i)
                .unwrap_or(bytes.len());

            if let Some((off_start, off_end)) =
                find_offense(&bytes[line_start..line_end], opts.enforced_style)
            {
                let range = Range {
                    start: (line_start + off_start) as u32,
                    end: (line_start + off_end) as u32,
                };
                let ranges = str_ranges.get_or_insert_with(|| string_literal_ranges(cx));
                if !in_string_literal(ranges, range) {
                    let message = match opts.enforced_style {
                        IndentationStyleKind::Spaces => "Tab detected in indentation.",
                        IndentationStyleKind::Tabs => "Space detected in indentation.",
                    };
                    cx.emit_offense(range, message, None);
                    emit_autocorrect(cx, range, &opts);
                }
            }

            line_start = line_end + 1;
        }
    }
}

/// Mirror RuboCop's `find_offense`: returns the byte span (relative to the
/// line start) of the leading whitespace run that violates the style.
///
/// - `spaces`: `/\A\s*\t+/` — optional leading whitespace, then one or more
///   tabs. The span covers from the line start to the end of the tab run.
/// - `tabs`: `/\A\s* +/` — optional leading whitespace, then one or more
///   spaces. The span covers from the line start to the end of the space run.
///
/// `\s` matches space, tab, `\r`, `\x0c`, `\x0b` — but since we operate on a
/// single line (no `\n`), the meaningful members are space, tab, `\r`, form
/// feed and vertical tab.
fn find_offense(line: &[u8], style: IndentationStyleKind) -> Option<(usize, usize)> {
    let (target, others): (u8, fn(u8) -> bool) = match style {
        // `\A\s*\t+`: leading `\s*` may include tabs, but the regex is
        // greedy and backtracks so the match ends on the last tab of the
        // first maximal tab run reachable through leading whitespace. In
        // practice `\s*` consumes spaces/tabs/etc up to (and including) a
        // trailing tab run; the match ends at the last consecutive tab.
        IndentationStyleKind::Spaces => (b'\t', is_ws),
        IndentationStyleKind::Tabs => (b' ', is_ws),
    };

    // Walk the leading whitespace. The regex `\A\s*<target>+` matches iff the
    // leading whitespace run contains at least one `target`; the match ends at
    // the last `target` in the contiguous leading-whitespace run.
    let mut i = 0;
    let mut last_target_end: Option<usize> = None;
    while i < line.len() && others(line[i]) {
        if line[i] == target {
            last_target_end = Some(i + 1);
        }
        i += 1;
    }
    last_target_end.map(|end| (0, end))
}

/// `\s` membership for a single line's bytes (no `\n`, which would terminate
/// the line). Space, tab, carriage return, form feed, vertical tab.
fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\x0c' | b'\x0b')
}

/// Collect ranges of all `str` / `dstr` nodes plus all heredoc body ranges.
/// A leading-whitespace offense whose range is fully contained in one of
/// these is exempt: the tabs/spaces are string content, not indentation.
/// Mirrors RuboCop's `string_literal_ranges` (which exempts both string
/// literal interiors and heredoc bodies via `loc.heredoc_body`).
///
/// Heredoc bodies need a token-based pass because, in Murphy's AST, a
/// heredoc `str`/`dstr` node's range covers the opener token (`<<~RUBY`),
/// not the multiline body that follows.
fn string_literal_ranges(cx: &Cx<'_>) -> Vec<Range> {
    let mut ranges: Vec<Range> = cx
        .descendants(cx.root())
        .into_iter()
        .filter(|&id| matches!(cx.kind(id), NodeKind::Str(_) | NodeKind::Dstr(_)))
        .map(|id| cx.range(id))
        .collect();
    ranges.extend(heredoc_body_ranges(cx));
    ranges
}

/// Collect `(body_start, body_end)` ranges for every heredoc body, FIFO-matched
/// so multiple heredocs opened on one line pair openers and terminators in
/// source order (same approach as `Layout/TrailingWhitespace`).
fn heredoc_body_ranges(cx: &Cx<'_>) -> Vec<Range> {
    use std::collections::VecDeque;
    let source = cx.source().as_bytes();
    let mut starts: VecDeque<u32> = VecDeque::new();
    let mut ranges: Vec<Range> = Vec::new();
    for tok in cx.sorted_tokens() {
        match tok.kind {
            SourceTokenKind::HeredocStart => {
                // +1 skips the `\n` ending the opener line.
                starts.push_back(tok.range.end + 1);
            }
            SourceTokenKind::HeredocEnd => {
                if let Some(body_start) = starts.pop_front() {
                    let term_line_start = source[..tok.range.start as usize]
                        .iter()
                        .rposition(|&b| b == b'\n')
                        .map(|i| i as u32 + 1)
                        .unwrap_or(0);
                    ranges.push(Range {
                        start: body_start,
                        end: term_line_start,
                    });
                }
            }
            _ => {}
        }
    }
    ranges
}

/// True when `offense` is fully contained within any string literal range.
fn in_string_literal(ranges: &[Range], offense: Range) -> bool {
    ranges
        .iter()
        .any(|r| r.start <= offense.start && offense.end <= r.end)
}

/// Emit the autocorrect edit for the flagged leading whitespace run.
///
/// - `spaces`: replace each tab with `IndentationWidth` spaces, leaving any
///   leading spaces untouched.
/// - `tabs`: replace each `IndentationWidth`-wide run of spaces with one tab,
///   leaving leftover spaces (a partial run) and any tabs untouched.
fn emit_autocorrect(cx: &Cx<'_>, range: Range, opts: &IndentationStyleOptions) {
    let width = opts.indentation_width.max(1) as usize;
    let original = cx.raw_source(range);
    let replacement: String = match opts.enforced_style {
        IndentationStyleKind::Spaces => {
            let spaces = " ".repeat(width);
            original
                .chars()
                .map(|c| if c == '\t' { spaces.clone() } else { c.to_string() })
                .collect()
        }
        IndentationStyleKind::Tabs => spaces_to_tabs(original, width),
    };
    cx.emit_edit(range, &replacement);
}

/// Replace each maximal run of `width` leading spaces with one tab. Any
/// remaining spaces shorter than `width` are preserved; tabs pass through.
fn spaces_to_tabs(s: &str, width: usize) -> String {
    let mut out = String::with_capacity(s.len());
    let mut run = 0usize;
    for c in s.chars() {
        if c == ' ' {
            run += 1;
            if run == width {
                out.push('\t');
                run = 0;
            }
        } else {
            // Flush any partial space run, then pass the character through.
            for _ in 0..run {
                out.push(' ');
            }
            run = 0;
            out.push(c);
        }
    }
    for _ in 0..run {
        out.push(' ');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{IndentationStyle, IndentationStyleKind, IndentationStyleOptions};
    use murphy_plugin_api::test_support::test;

    fn tabs_style() -> IndentationStyleOptions {
        IndentationStyleOptions {
            enforced_style: IndentationStyleKind::Tabs,
            indentation_width: 2,
        }
    }

    // ── EnforcedStyle: spaces (default) ───────────────────────────────────────

    #[test]
    fn flags_leading_tab() {
        // "\tx = 0\n": the leading tab (byte 0) is flagged.
        test::<IndentationStyle>()
            .expect_offense("\tx = 0\n^ Tab detected in indentation.\n");
    }

    #[test]
    fn corrects_leading_tab_to_two_spaces() {
        test::<IndentationStyle>().expect_correction(
            "\tx = 0\n^ Tab detected in indentation.\n",
            "  x = 0\n",
        );
    }

    #[test]
    fn accepts_space_indentation() {
        test::<IndentationStyle>().expect_no_offenses("  x = 0\n");
    }

    #[test]
    fn accepts_no_indentation() {
        test::<IndentationStyle>().expect_no_offenses("x = 0\n");
    }

    #[test]
    fn flags_tab_after_leading_space() {
        // " \tx\n": `\A\s*\t+` matches " \t" (1 space + 1 tab), 2 chars.
        test::<IndentationStyle>()
            .expect_offense(" \tx = 0\n^^ Tab detected in indentation.\n");
    }

    #[test]
    fn skips_tab_inside_string_literal() {
        // The tab is inside a multiline string literal's content, not
        // indentation — exempt.
        test::<IndentationStyle>().expect_no_offenses("x = \"a\n\tb\"\n");
    }

    #[test]
    fn skips_tab_inside_heredoc_body() {
        // Heredoc body lines are part of the str/dstr node range — exempt.
        test::<IndentationStyle>().expect_no_offenses("x = <<~RUBY\n\thello\nRUBY\n");
    }

    // ── EnforcedStyle: tabs ───────────────────────────────────────────────────

    #[test]
    fn flags_leading_space_when_tabs_enforced() {
        // "  x = 0\n": `\A\s* +` matches both leading spaces.
        test::<IndentationStyle>()
            .with_options(&tabs_style())
            .expect_offense("  x = 0\n^^ Space detected in indentation.\n");
    }

    #[test]
    fn accepts_tab_indentation_when_tabs_enforced() {
        test::<IndentationStyle>()
            .with_options(&tabs_style())
            .expect_no_offenses("\tx = 0\n");
    }

    #[test]
    fn corrects_two_spaces_to_one_tab_when_tabs_enforced() {
        test::<IndentationStyle>().with_options(&tabs_style()).expect_correction(
            "  x = 0\n^^ Space detected in indentation.\n",
            "\tx = 0\n",
        );
    }
}

murphy_plugin_api::submit_cop!(IndentationStyle);
