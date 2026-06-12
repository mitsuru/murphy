//! `Layout/LineLength` — checks that line length does not exceed the
//! configured limit.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/LineLength
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Ports the detection half of `Layout/LineLength` + `LineLengthHelp`:
//!   per-line `line_length = line.chars.count + indentation_difference(line)`,
//!   where `indentation_difference` adds `(tab_width - 1)` per leading tab.
//!   Line terminators (`\n`, `\r`) are excluded from the count. A line is an
//!   offense when its length exceeds `Max` (default 120) and no exemption
//!   applies.
//!
//!   Default-on exemptions implemented:
//!   - shebang: the first line starting with `#!`.
//!   - AllowHeredoc (default true): lines whose bytes fall inside a heredoc
//!     body are exempt. Heredoc bodies are collected from
//!     `HeredocStart`/`HeredocEnd` token pairs (the per-delimiter allowlist
//!     form of `AllowHeredoc` is not yet supported — only the boolean).
//!   - AllowCopDirectives (default true): a line carrying a `# rubocop:`
//!     directive *comment* (a real `Comment` token, not a `"# rubocop:"`
//!     substring inside a string literal) is measured without the directive;
//!     only the length up to the directive is checked.
//!   - AllowQualifiedName (default true): a line is exempt when a fully
//!     qualified constant name (`Foo::Bar::Baz`) starts before column `Max`
//!     and runs to the end of the line.
//!   - AllowedPatterns (default `[]`): a line matching any configured regex is
//!     exempt.
//!
//!   Gaps:
//!   - AllowURI (default true): RuboCop builds a URI regex from Ruby's URI
//!     parser and validates with `URI.parse`; that grammar cannot be faithfully
//!     reproduced in Rust. Murphy uses a pragmatic `https?://…`-to-end-of-line
//!     matcher (and the configured `URISchemes`); it is approximate.
//!   - Tab width is hardcoded to 2 (RuboCop reads `Layout/IndentationStyle`'s
//!     `IndentationWidth`; cross-cop config is not read, matching the
//!     `Layout/ParameterAlignment` precedent). Only affects tab-indented lines.
//!   - Autocorrect (line breaking via `CheckLineBreakable` / `SplitStrings`) is
//!     not implemented — the detect-only port ships without it, matching the
//!     precedent set by `Layout/MultilineMethodDefinitionBraceLayout`.
//!   - AllowRBSInlineAnnotation is not supported (default false → no effect).
//!   - Offense range: the highlighted range runs to the end of the line
//!     content. RuboCop's `check_directive_line` ends the range just before the
//!     directive comment; Murphy includes it. Cosmetic for a detect-only port
//!     (the line still fires on the right lines).
//! ```

use murphy_plugin_api::{CopOptions, Cx, Range, SourceTokenKind, cop, regex::Regex};
use std::sync::LazyLock;

/// RuboCop's `qualified_name_regexp`, compiled once.
static QUALIFIED_NAME_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:[A-Z][A-Za-z0-9_]*::)+[A-Za-z_][A-Za-z0-9_]*\b")
        .expect("qualified-name pattern is a valid regex")
});

/// Stateless unit struct (ADR 0035).
#[derive(Default)]
pub struct LineLength;

/// Options for [`LineLength`]. Keys match RuboCop verbatim.
#[derive(CopOptions)]
pub struct LineLengthOptions {
    #[option(
        name = "Max",
        default = 120,
        description = "Maximum allowed line length in columns."
    )]
    pub max: i64,
    #[option(
        name = "AllowHeredoc",
        default = true,
        description = "When true, lines inside heredoc bodies are exempt."
    )]
    pub allow_heredoc: bool,
    #[option(
        name = "AllowURI",
        default = true,
        description = "When true, a line ending in a long URI is exempt."
    )]
    pub allow_uri: bool,
    #[option(
        name = "AllowQualifiedName",
        default = true,
        description = "When true, a line ending in a long fully qualified name is exempt."
    )]
    pub allow_qualified_name: bool,
    #[option(
        name = "AllowCopDirectives",
        default = true,
        description = "When true, a trailing cop directive is excluded from the measured length."
    )]
    pub allow_cop_directives: bool,
    #[option(
        name = "URISchemes",
        default = ["http", "https"],
        description = "URI schemes recognised by the AllowURI exemption."
    )]
    pub uri_schemes: Vec<String>,
    #[option(
        name = "AllowedPatterns",
        default = [],
        description = "Lines matching any of these regular expressions are exempt."
    )]
    pub allowed_patterns: Vec<String>,
}

const MSG_PREFIX: &str = "Line is too long.";

#[cop(
    name = "Layout/LineLength",
    description = "Checks that line length does not exceed the configured limit.",
    default_severity = "warning",
    default_enabled = true,
    options = LineLengthOptions,
)]
impl LineLength {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<LineLengthOptions>();
        let max = if opts.max > 0 { opts.max as usize } else { 120 };
        let src = cx.source();
        let bytes = src.as_bytes();

        let heredoc_ranges = if opts.allow_heredoc {
            heredoc_body_ranges(cx)
        } else {
            Vec::new()
        };
        let comment_ranges = if opts.allow_cop_directives {
            comment_ranges(cx)
        } else {
            Vec::new()
        };
        let patterns = compile_patterns(&opts.allowed_patterns);

        let mut line_start = 0usize;
        let mut line_index = 0usize;
        let mut i = 0usize;
        while i <= bytes.len() {
            let at_end = i == bytes.len();
            if at_end || bytes[i] == b'\n' {
                if line_start < i || at_end {
                    // Line content is `[line_start, content_end)`, excluding a
                    // trailing `\r` (CRLF) and the `\n` itself.
                    let mut content_end = i;
                    if content_end > line_start && bytes[content_end - 1] == b'\r' {
                        content_end -= 1;
                    }
                    if content_end > line_start || (at_end && line_start < bytes.len()) {
                        check_line(
                            cx,
                            src,
                            line_start,
                            content_end,
                            line_index,
                            max,
                            &opts,
                            &heredoc_ranges,
                            &comment_ranges,
                            &patterns,
                        );
                    }
                }
                if at_end {
                    break;
                }
                line_start = i + 1;
                line_index += 1;
            }
            i += 1;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn check_line(
    cx: &Cx<'_>,
    src: &str,
    line_start: usize,
    content_end: usize,
    line_index: usize,
    max: usize,
    opts: &LineLengthOptions,
    heredoc_ranges: &[(u32, u32)],
    comment_ranges: &[(u32, u32)],
    patterns: &[Regex],
) {
    let line = &src[line_start..content_end];
    let length = line_length(line);
    // RuboCop: `return if line_length(line) <= max`.
    if length <= max {
        return;
    }

    // `allowed_line?`: AllowedPatterns match, shebang, or permitted heredoc.
    if patterns.iter().any(|re| re.is_match(line)) {
        return;
    }
    if line_index == 0 && line.starts_with("#!") {
        return;
    }
    if opts.allow_heredoc && line_in_heredoc(line_start as u32, content_end as u32, heredoc_ranges) {
        return;
    }

    // AllowCopDirectives: measure the line without a trailing directive. The
    // directive must be a real comment (a `Comment` token on this line whose
    // text is a `# rubocop:` directive), so a `"# rubocop:"` substring inside a
    // string literal is not mistaken for one.
    if let Some(directive_at) = opts
        .allow_cop_directives
        .then(|| directive_start(src, line_start, content_end, comment_ranges))
        .flatten()
    {
        let directive_col = directive_at - line_start;
        let trimmed = line[..directive_col].trim_end();
        let trimmed_len = line_length(trimmed);
        if trimmed_len <= max {
            return;
        }
        register_offense(cx, src, line_start, content_end, max, line, trimmed_len);
        return;
    }

    // AllowURI / AllowQualifiedName: exempt when the match starts before `max`
    // and runs to the end of the line.
    if opts.allow_uri && uri_exempts(line, max, &opts.uri_schemes) {
        return;
    }
    if opts.allow_qualified_name && qualified_name_exempts(line, max) {
        return;
    }

    register_offense(cx, src, line_start, content_end, max, line, length);
}

fn register_offense(
    cx: &Cx<'_>,
    src: &str,
    line_start: usize,
    content_end: usize,
    max: usize,
    line: &str,
    length: usize,
) {
    let message = format!("{MSG_PREFIX} [{length}/{max}]");
    // `highlight_start = [max - indentation_difference, 0].max`, a column.
    let highlight_col = max.saturating_sub(indentation_difference(line));
    let start = col_to_byte(src, line_start, content_end, highlight_col);
    let range = Range {
        start: start as u32,
        end: content_end as u32,
    };
    cx.emit_offense(range, &message, None);
}

/// RuboCop `line_length`: visible columns of a line — character count plus the
/// indentation difference from leading tabs.
fn line_length(line: &str) -> usize {
    line.chars().count() + indentation_difference(line)
}

/// RuboCop `indentation_difference`: `leading_tab_count * (tab_width - 1)`.
/// Tab width is hardcoded to 2 (see parity notes), so this is the count of
/// leading tabs.
fn indentation_difference(line: &str) -> usize {
    const TAB_WIDTH: usize = 2;
    let leading_tabs = line.chars().take_while(|&c| c == '\t').count();
    leading_tabs * (TAB_WIDTH - 1)
}

/// Map a visible column to a byte offset on the line `[line_start, content_end)`.
/// Accounts for leading tabs expanding to `tab_width` columns.
fn col_to_byte(src: &str, line_start: usize, content_end: usize, target_col: usize) -> usize {
    const TAB_WIDTH: usize = 2;
    let line = &src[line_start..content_end];
    let mut col = 0usize;
    for (byte_off, ch) in line.char_indices() {
        if col >= target_col {
            return line_start + byte_off;
        }
        col += if ch == '\t' { TAB_WIDTH } else { 1 };
    }
    content_end
}

/// True when the line's bytes fall inside any heredoc body range.
fn line_in_heredoc(line_start: u32, content_end: u32, ranges: &[(u32, u32)]) -> bool {
    // Exempt the line if any heredoc body covers (part of) it. RuboCop exempts
    // the source lines that are heredoc-body lines.
    ranges
        .iter()
        .any(|&(start, end)| line_start >= start && content_end <= end)
}

/// The absolute byte offset where a `# rubocop:` directive comment begins on
/// the line `[line_start, content_end)`, if any. Mirrors RuboCop's
/// `directive_on_source_line?`: the directive must be a real comment, so only a
/// `Comment` token whose body is a `rubocop:` directive counts — a
/// `"# rubocop:"` substring inside a string literal is ignored.
fn directive_start(
    src: &str,
    line_start: usize,
    content_end: usize,
    comment_ranges: &[(u32, u32)],
) -> Option<usize> {
    for &(c_start, c_end) in comment_ranges {
        let c_start = c_start as usize;
        // Only comments that begin on this line.
        if c_start < line_start || c_start >= content_end {
            continue;
        }
        let c_end = (c_end as usize).min(src.len());
        let comment = &src[c_start..c_end];
        // `# rubocop:` — whitespace tolerant after the `#`.
        let is_directive = comment
            .strip_prefix('#')
            .is_some_and(|body| body.trim_start().starts_with("rubocop:"));
        if is_directive {
            return Some(c_start);
        }
    }
    None
}

/// Collect `Comment` token byte ranges.
fn comment_ranges(cx: &Cx<'_>) -> Vec<(u32, u32)> {
    cx.sorted_tokens()
        .iter()
        .filter(|t| t.kind == SourceTokenKind::Comment)
        .map(|t| (t.range.start, t.range.end))
        .collect()
}

/// AllowURI exemption: a `scheme://…` URI starts before column `max` and runs
/// to the end of the line. Approximate (see parity notes).
fn uri_exempts(line: &str, max: usize, schemes: &[String]) -> bool {
    for scheme in schemes {
        let needle = format!("{scheme}://");
        // Scan EVERY occurrence of the scheme on the line, not just the first:
        // a short URI earlier on the line must not mask a long URI that runs to
        // the line's end.
        for (pos, _) in line.match_indices(&needle) {
            // The URI must start before column `max` and run to the end of the
            // line (its tail contains no whitespace).
            let start_col = line[..pos].chars().count();
            let tail = &line[pos..];
            if start_col < max && !tail.chars().any(|c| c.is_whitespace()) {
                return true;
            }
        }
    }
    false
}

/// AllowQualifiedName exemption: a fully qualified constant name
/// (`Foo::Bar::Baz`) starts before column `max` and runs to the end of the
/// line. Ports RuboCop's `qualified_name_regexp`.
fn qualified_name_exempts(line: &str, max: usize) -> bool {
    // The last match decides, mirroring `match_qualified_names(...).last`.
    if let Some(m) = QUALIFIED_NAME_RE.find_iter(line).last() {
        let start_col = line[..m.start()].chars().count();
        let end_col = line[..m.end()].chars().count();
        return start_col < max && end_col == line.chars().count();
    }
    false
}

/// Compile the configured `AllowedPatterns`. Invalid patterns are skipped.
fn compile_patterns(patterns: &[String]) -> Vec<Regex> {
    patterns.iter().filter_map(|p| Regex::new(p).ok()).collect()
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

#[cfg(test)]
mod tests {
    use super::{LineLength, LineLengthOptions};
    use murphy_plugin_api::test_support::{run_cop, run_cop_with_options};

    fn opts_max(max: i64) -> LineLengthOptions {
        LineLengthOptions {
            max,
            allow_heredoc: true,
            allow_uri: true,
            allow_qualified_name: true,
            allow_cop_directives: true,
            uri_schemes: vec!["http".into(), "https".into()],
            allowed_patterns: vec![],
        }
    }

    /// A line of exactly `n` 'a' characters plus a trailing newline.
    fn line_of(n: usize) -> String {
        format!("{}\n", "a".repeat(n))
    }

    // boundary ------------------------------------------------------------

    #[test]
    fn accepts_line_at_max() {
        // 120 chars, Max 120 → no offense.
        assert!(run_cop::<LineLength>(&line_of(120)).is_empty());
    }

    #[test]
    fn flags_line_over_max() {
        let offenses = run_cop::<LineLength>(&line_of(121));
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].message, "Line is too long. [121/120]");
    }

    #[test]
    fn boundary_without_trailing_newline() {
        // No final newline: 120 clean, 121 flagged.
        assert!(run_cop::<LineLength>(&"a".repeat(120)).is_empty());
        assert_eq!(run_cop::<LineLength>(&"a".repeat(121)).len(), 1);
    }

    #[test]
    fn crlf_terminator_excluded_from_count() {
        // 120 'a' + CRLF → still 120 columns, clean.
        assert!(run_cop::<LineLength>(&format!("{}\r\n", "a".repeat(120))).is_empty());
    }

    #[test]
    fn custom_max() {
        assert!(run_cop_with_options::<LineLength>(&line_of(10), &opts_max(10)).is_empty());
        assert_eq!(
            run_cop_with_options::<LineLength>(&line_of(11), &opts_max(10)).len(),
            1
        );
    }

    // tab indentation -----------------------------------------------------

    #[test]
    fn tab_counts_as_two_columns() {
        // One leading tab + 119 'a' = 1*2 + 119 = 121 columns → flagged.
        let src = format!("\t{}\n", "a".repeat(119));
        assert_eq!(run_cop::<LineLength>(&src).len(), 1);
        // One leading tab + 118 'a' = 120 columns → clean.
        let ok = format!("\t{}\n", "a".repeat(118));
        assert!(run_cop::<LineLength>(&ok).is_empty());
    }

    // exemptions ----------------------------------------------------------

    #[test]
    fn shebang_exempt_on_first_line() {
        let src = format!("#!{}\n", "a".repeat(121));
        assert!(run_cop::<LineLength>(&src).is_empty());
    }

    #[test]
    fn shebang_not_exempt_after_first_line() {
        let src = format!("x = 1\n#!{}\n", "a".repeat(121));
        assert_eq!(run_cop::<LineLength>(&src).len(), 1);
    }

    #[test]
    fn heredoc_body_exempt_by_default() {
        let long = "a".repeat(140);
        let src = format!("x = <<~RUBY\n  {long}\nRUBY\n");
        assert!(run_cop::<LineLength>(&src).is_empty());
    }

    #[test]
    fn heredoc_body_flagged_when_disabled() {
        let long = "a".repeat(140);
        let src = format!("x = <<~RUBY\n  {long}\nRUBY\n");
        let mut o = opts_max(120);
        o.allow_heredoc = false;
        assert_eq!(run_cop_with_options::<LineLength>(&src, &o).len(), 1);
    }

    #[test]
    fn cop_directive_excluded_from_length() {
        // Code is short; only the trailing rubocop directive pushes it over.
        let code = format!("x = {}", "1".repeat(110));
        let src = format!("{code} # rubocop:disable Layout/LineLength\n");
        assert!(run_cop::<LineLength>(&src).is_empty());
    }

    #[test]
    fn cop_directive_still_flags_long_code() {
        // The code itself is over the limit even without the directive.
        let code = format!("x = {}", "1".repeat(130));
        let src = format!("{code} # rubocop:disable Layout/LineLength\n");
        assert_eq!(run_cop::<LineLength>(&src).len(), 1);
    }

    #[test]
    fn directive_inside_string_literal_is_not_a_directive() {
        // A `# rubocop:` substring inside a string literal must NOT be treated
        // as a trailing cop directive — the line still fires.
        let filler = "x".repeat(110);
        let src = format!("x = \"{filler} # rubocop:disable Foo\"\n");
        assert_eq!(run_cop::<LineLength>(&src).len(), 1);
    }

    #[test]
    fn qualified_name_exempt() {
        // A long qualified constant name at the end of the line is exempt.
        let prefix = "x = ";
        let name_part = "Foo::".repeat(40);
        let src = format!("{prefix}{name_part}Bar\n");
        assert!(run_cop::<LineLength>(&src).is_empty());
    }

    #[test]
    fn allowed_pattern_exempt() {
        let long = "a".repeat(130);
        let src = format!("# SKIP {long}\n");
        let mut o = opts_max(120);
        o.allowed_patterns = vec!["SKIP".into()];
        assert!(run_cop_with_options::<LineLength>(&src, &o).is_empty());
    }

    #[test]
    fn uri_exempt_by_default() {
        let url = format!("https://example.com/{}", "a".repeat(130));
        let src = format!("# see {url}\n");
        assert!(run_cop::<LineLength>(&src).is_empty());
    }

    #[test]
    fn uri_exempt_with_earlier_short_uri() {
        // A short URI earlier on the line must not mask the long trailing URI.
        let long = "a".repeat(130);
        let src = format!("# see http://x and https://example.com/{long}\n");
        assert!(run_cop::<LineLength>(&src).is_empty());
    }

    #[test]
    fn short_lines_clean() {
        assert!(run_cop::<LineLength>("x = 1\ny = 2\n").is_empty());
    }
}

murphy_plugin_api::submit_cop!(LineLength);
