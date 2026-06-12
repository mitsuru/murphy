//! `Layout/HeredocIndentation` — checks the indentation of heredoc bodies.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/HeredocIndentation
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's `on_heredoc`. Heredocs are matched token-side via
//!   FIFO `HeredocStart`/`HeredocEnd` pairs because Murphy's AST hides the
//!   opener sigil (`<<~` vs `<<-` vs `<<`); the `HeredocStart` token source
//!   carries the full opener so the sigil and base line are read from it.
//!   For squiggly (`<<~`) heredocs the body indent must equal
//!   `base_indent + IndentationWidth`; otherwise WIDTH_MSG fires and the body
//!   plus terminator are re-indented (`adjust_squiggly`). For `<<-`/`<<`
//!   heredocs an offense fires only when the body indent level is zero
//!   (TYPE_MSG), and the autocorrect converts the opener sigil to `<<~`
//!   (`adjust_minus`) — the production fixpoint loop then re-indents on a
//!   later pass. Empty/whitespace-only bodies are skipped.
//!   Gaps (documented, not bypassed):
//!     - `IndentationWidth` defaults to 2 (Murphy cannot read the sibling
//!       `Layout/IndentationWidth: Width` value across the single-surface
//!       ABI, so the RuboCop default of 2 is applied when the option is
//!       unset).
//!     - `line_too_long?` is treated as always-false. RuboCop short-circuits
//!       it whenever `Layout/LineLength: AllowHeredoc` is true (the default),
//!       so this matches default behaviour; the `AllowHeredoc: false` edge is
//!       not modelled (cross-cop config is unreadable across the ABI).
//!     - `heredoc_squish?` (ActiveSupport `.squish` on a non-zero-indent
//!       `<<-`/`<<` body) is treated as always-false. ActiveSupport
//!       extensions are off by default, so this matches default behaviour.

use murphy_plugin_api::{CopOptions, Cx, Range, SourceTokenKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct HeredocIndentation;

#[derive(CopOptions)]
pub struct HeredocIndentationOptions {
    #[option(
        name = "IndentationWidth",
        default = 2,
        description = "Number of spaces required for heredoc body indentation."
    )]
    pub indentation_width: i64,
}

#[cop(
    name = "Layout/HeredocIndentation",
    description = "Checks the indentation of the here document bodies.",
    default_severity = "warning",
    default_enabled = true,
    options = HeredocIndentationOptions,
)]
impl HeredocIndentation {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<HeredocIndentationOptions>();
        let width = opts.indentation_width.max(0) as usize;
        let source = cx.source();
        let bytes = source.as_bytes();

        for heredoc in collect_heredocs(cx) {
            let body = &source[heredoc.body.start as usize..heredoc.body.end as usize];
            // `body.strip.empty?` — skip empty / whitespace-only bodies.
            if body.trim().is_empty() {
                continue;
            }

            let body_indent_level = indent_level(body);
            let base_indent = base_indent_level(bytes, heredoc.opener_start);

            // The offense highlight is RuboCop's `loc.heredoc_body`, which spans
            // every body line. Murphy's offense model is one offense per line, so
            // the highlight narrows to the first body line (column 0 to the line's
            // last char, leading whitespace included — matching RuboCop's
            // caret convention). The *autocorrect* still operates on the full
            // body range. This first-line narrowing is the documented parity gap.
            let offense_range = first_body_line_range(bytes, heredoc.body);

            match heredoc.indent_type {
                IndentType::Squiggly => {
                    let expected = base_indent + width;
                    if expected == body_indent_level {
                        continue;
                    }
                    let message =
                        format!("Use {width} spaces for indentation in a heredoc.");
                    cx.emit_offense(offense_range, &message, None);
                    adjust_squiggly(cx, &heredoc, body, body_indent_level, base_indent, width);
                }
                IndentType::Minus | IndentType::Plain => {
                    // Offense only when the body has no indentation
                    // (`heredoc_squish?` is treated as always-false).
                    if body_indent_level != 0 {
                        continue;
                    }
                    let current = heredoc.indent_type.current_indent_type();
                    let message = format!(
                        "Use {width} spaces for indentation in a heredoc by using `<<~` instead of `{current}`."
                    );
                    cx.emit_offense(offense_range, &message, None);
                    adjust_minus(cx, &heredoc);
                }
            }
        }
    }
}

/// Heredoc opener sigil class.
#[derive(Clone, Copy, PartialEq, Eq)]
enum IndentType {
    /// `<<~` (squiggly).
    Squiggly,
    /// `<<-` (dash).
    Minus,
    /// `<<` (plain).
    Plain,
}

impl IndentType {
    /// The `current_indent_type` string RuboCop embeds in TYPE_MSG.
    fn current_indent_type(self) -> &'static str {
        match self {
            IndentType::Minus => "<<-",
            IndentType::Plain => "<<",
            IndentType::Squiggly => "<<~",
        }
    }
}

/// A single heredoc, resolved from its `HeredocStart`/`HeredocEnd` token pair.
struct Heredoc {
    /// Byte offset of the opener's first `<`.
    opener_start: u32,
    /// Source range of the opener token (`<<~RUBY`, `<<-RUBY`, `<<RUBY`).
    opener: Range,
    /// Source range of the body: from the byte after the opener line's `\n`
    /// to the start of the terminator line.
    body: Range,
    /// Source range of the terminator line (its leading whitespace + label).
    end_line: Range,
    indent_type: IndentType,
}

/// Pair `HeredocStart`/`HeredocEnd` tokens FIFO (Ruby reads heredoc bodies in
/// opener order) and resolve each into a [`Heredoc`].
///
/// Body starts cannot be derived from the opener token alone. When several
/// heredocs share one opener line — `foo(<<~A, <<~B)` — every opener token
/// ends on that same line, yet the bodies **stack**: `A`'s body runs from the
/// byte after the opener line's `\n` to `A`'s terminator, and `B`'s body
/// starts only after `A`'s terminator line. So body start is resolved at
/// `HeredocEnd`-time with a running `cursor` that chains past each consumed
/// terminator: `body_start = max(cursor, opener_line_end)`. The `max` covers
/// both cases with no explicit "is-stacked?" branch — the opener-line newline
/// wins for the first/lone heredoc on a line, the cursor wins for every
/// stacked follower.
fn collect_heredocs(cx: &Cx<'_>) -> Vec<Heredoc> {
    use std::collections::VecDeque;
    let source = cx.source().as_bytes();
    // Queue of (opener_start, opener_range, indent_type).
    let mut pending: VecDeque<(u32, Range, IndentType)> = VecDeque::new();
    let mut out: Vec<Heredoc> = Vec::new();
    // First unconsumed body byte — advances past each terminator line so that
    // stacked heredocs on one opener line do not overlap.
    let mut cursor: u32 = 0;

    for tok in cx.sorted_tokens() {
        match tok.kind {
            SourceTokenKind::HeredocStart => {
                let opener = cx.raw_source(tok.range);
                let indent_type = classify_opener(opener);
                pending.push_back((tok.range.start, tok.range, indent_type));
            }
            SourceTokenKind::HeredocEnd => {
                if let Some((opener_start, opener, indent_type)) = pending.pop_front() {
                    // Byte after the `\n` that ends the opener's source line.
                    // Forward-scan from the opener token end so the result is
                    // correct regardless of where the opener token stops
                    // (single heredoc: token ends at the label; stacked: token
                    // ends mid-line before the next opener / args).
                    let opener_line_end = next_line_start(source, opener.end);
                    let body_start = cursor.max(opener_line_end).min(source.len() as u32);
                    let term_line_start = line_start(source, tok.range.start);
                    // Advance the cursor to the first byte after the terminator
                    // line so the next stacked heredoc's body begins there. Scan
                    // forward from the label *start* (`tok.range.start`): the
                    // first `\n` found ends the terminator's own line. Scanning
                    // from `tok.range.end` would skip a line, because the
                    // `HeredocEnd` token already spans the terminator's newline.
                    cursor = next_line_start(source, tok.range.start);
                    out.push(Heredoc {
                        opener_start,
                        opener,
                        body: Range {
                            start: body_start.min(term_line_start),
                            end: term_line_start,
                        },
                        end_line: Range {
                            start: term_line_start,
                            end: tok.range.end,
                        },
                        indent_type,
                    });
                }
            }
            _ => {}
        }
    }
    out
}

/// Determine the opener sigil from the `HeredocStart` token text. RuboCop:
/// `node.source[/^<<([~-])/, 1]` — `~` → squiggly, `-` → minus, else plain.
fn classify_opener(opener: &str) -> IndentType {
    let after = opener.strip_prefix("<<").unwrap_or(opener);
    match after.chars().next() {
        Some('~') => IndentType::Squiggly,
        Some('-') => IndentType::Minus,
        _ => IndentType::Plain,
    }
}

/// RuboCop `indent_level(str)`: minimum, over all lines containing a
/// non-whitespace character, of the column of that line's first
/// non-whitespace char. `\r`/`\n` do not count as `\S`. Returns 0 when no
/// line has non-whitespace content (caller has already excluded that case).
fn indent_level(s: &str) -> usize {
    s.lines()
        .filter_map(|line| line.find(|c: char| !c.is_whitespace()))
        .min()
        .unwrap_or(0)
}

/// RuboCop `base_indent_level(node)`: the indent level of the line on which
/// the heredoc opener appears.
fn base_indent_level(bytes: &[u8], opener_start: u32) -> usize {
    let line_start = line_start(bytes, opener_start) as usize;
    let mut col = 0usize;
    let mut i = line_start;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        col += 1;
        i += 1;
    }
    col
}

/// Narrow the full body range to its first line (excluding the line's `\n`).
/// Murphy emits one offense per line, so the heredoc-body highlight is
/// reported on the first body line only; the caret span runs from column 0
/// of that line to its last character (leading whitespace included), matching
/// RuboCop's caret convention.
fn first_body_line_range(bytes: &[u8], body: Range) -> Range {
    let start = body.start;
    let first_nl = bytes[start as usize..body.end as usize]
        .iter()
        .position(|&b| b == b'\n')
        .map(|i| start + i as u32)
        .unwrap_or(body.end);
    Range {
        start,
        end: first_nl,
    }
}

/// Byte offset of the first byte on the line containing `pos`.
fn line_start(bytes: &[u8], pos: u32) -> u32 {
    bytes[..pos as usize]
        .iter()
        .rposition(|&b| b == b'\n')
        .map(|i| i as u32 + 1)
        .unwrap_or(0)
}

/// Byte offset of the first byte on the line *after* the one containing `pos`.
/// Forward-scans from `pos` for the next `\n` and returns the byte after it; if
/// there is no further `\n` (the line runs to EOF), returns the source length.
fn next_line_start(bytes: &[u8], pos: u32) -> u32 {
    let pos = (pos as usize).min(bytes.len());
    bytes[pos..]
        .iter()
        .position(|&b| b == b'\n')
        .map(|i| (pos + i + 1) as u32)
        .unwrap_or(bytes.len() as u32)
}

/// `adjust_squiggly`: re-indent each body line from `body_indent_level` to
/// `base_indent + width`, and re-indent the terminator to `base_indent` when
/// it is currently under-indented. Mirrors RuboCop's `indented_body` /
/// `indented_end`.
fn adjust_squiggly(
    cx: &Cx<'_>,
    heredoc: &Heredoc,
    body: &str,
    body_indent_level: usize,
    base_indent: usize,
    width: usize,
) {
    let correct = base_indent + width;
    cx.emit_edit(heredoc.body, &reindent_body(body, body_indent_level, correct));

    // `indented_end`: only re-indent when the terminator is under-indented.
    let end_src = cx.raw_source(heredoc.end_line);
    let end_indent = indent_level(end_src);
    if end_indent < base_indent {
        let mut replacement = " ".repeat(base_indent);
        replacement.push_str(end_src.trim_start_matches([' ', '\t']));
        cx.emit_edit(heredoc.end_line, &replacement);
    }
}

/// `body.gsub(/^[^\S\r\n]{body_indent_level}/, ' ' * correct)` — for each body
/// line, replace exactly `body_indent_level` leading horizontal-whitespace
/// characters with `correct` spaces. Lines shorter than `body_indent_level`
/// leading whitespace (e.g. blank lines) are untouched, matching the regex
/// which only matches when the full run is present.
fn reindent_body(body: &str, body_indent_level: usize, correct: usize) -> String {
    let new_indent = " ".repeat(correct);
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while !rest.is_empty() {
        // Split off one line including its trailing `\n` (if any).
        let line_end = rest.find('\n').map(|i| i + 1).unwrap_or(rest.len());
        let (line, tail) = rest.split_at(line_end);

        // Count leading horizontal whitespace ([^\S\r\n] = space/tab/FF/VT).
        let leading: usize = line
            .chars()
            .take_while(|&c| c == ' ' || c == '\t' || c == '\x0c' || c == '\x0b')
            .take(body_indent_level)
            .count();
        if leading == body_indent_level {
            out.push_str(&new_indent);
            out.push_str(&line[byte_len(line, body_indent_level)..]);
        } else {
            out.push_str(line);
        }
        rest = tail;
    }
    out
}

/// Byte length of the first `n` horizontal-whitespace chars of `line`
/// (each such char is a single ASCII byte, so this equals `n` when the run
/// is all ASCII whitespace — which it is by construction).
fn byte_len(line: &str, n: usize) -> usize {
    line.char_indices()
        .nth(n)
        .map(|(i, _)| i)
        .unwrap_or(line.len())
}

/// `adjust_minus`: rewrite the opener sigil `<<` / `<<-` to `<<~`.
fn adjust_minus(cx: &Cx<'_>, heredoc: &Heredoc) {
    let opener = cx.raw_source(heredoc.opener);
    // `heredoc_beginning.sub(/<<-?/, '<<~')` — replace the leading `<<` or
    // `<<-` with `<<~`, leaving the label intact.
    let label = opener
        .strip_prefix("<<-")
        .or_else(|| opener.strip_prefix("<<"))
        .unwrap_or(opener);
    let replacement = format!("<<~{label}");
    cx.emit_edit(heredoc.opener, &replacement);
}

#[cfg(test)]
mod tests {
    use super::HeredocIndentation;
    use murphy_plugin_api::test_support::test;

    // ── Squiggly heredocs (`<<~`) ─────────────────────────────────────────────

    #[test]
    fn accepts_correctly_indented_squiggly_heredoc() {
        test::<HeredocIndentation>().expect_no_offenses("x = <<~RUBY\n  hello\nRUBY\n");
    }

    #[test]
    fn flags_under_indented_squiggly_body() {
        // body "hello\n" has indent 0; expected base(0)+2 = 2. The offense
        // highlights the first body line (column 0 → last char).
        test::<HeredocIndentation>().expect_offense(
            "x = <<~RUBY\nhello\n^^^^^ Use 2 spaces for indentation in a heredoc.\nRUBY\n",
        );
    }

    #[test]
    fn corrects_under_indented_squiggly_body() {
        test::<HeredocIndentation>().expect_correction(
            "x = <<~RUBY\nhello\n^^^^^ Use 2 spaces for indentation in a heredoc.\nRUBY\n",
            "x = <<~RUBY\n  hello\nRUBY\n",
        );
    }

    #[test]
    fn flags_over_indented_squiggly_body() {
        // body "    hello\n" indent 4; expected 2 → offense; re-indent to 2.
        // Carets cover the leading whitespace too (4 spaces + "hello" = 9).
        test::<HeredocIndentation>().expect_correction(
            "x = <<~RUBY\n    hello\n^^^^^^^^^ Use 2 spaces for indentation in a heredoc.\nRUBY\n",
            "x = <<~RUBY\n  hello\nRUBY\n",
        );
    }

    #[test]
    fn corrects_multiline_squiggly_body() {
        // Two under-indented body lines: both re-indent from 0 to 2.
        // The offense is anchored to the first body line only.
        test::<HeredocIndentation>().expect_correction(
            "x = <<~RUBY\nhello\n^^^^^ Use 2 spaces for indentation in a heredoc.\nworld\nRUBY\n",
            "x = <<~RUBY\n  hello\n  world\nRUBY\n",
        );
    }

    #[test]
    fn idempotent_on_correctly_indented_multiline_body() {
        test::<HeredocIndentation>()
            .expect_no_offenses("x = <<~RUBY\n  hello\n  world\nRUBY\n");
    }

    #[test]
    fn skips_empty_squiggly_heredoc() {
        test::<HeredocIndentation>().expect_no_offenses("x = <<~RUBY\nRUBY\n");
    }

    // ── Dash / plain heredocs (`<<-`, `<<`) ───────────────────────────────────

    #[test]
    fn flags_zero_indent_dash_heredoc() {
        test::<HeredocIndentation>().expect_offense(
            "x = <<-RUBY\nhello\n^^^^^ Use 2 spaces for indentation in a heredoc by using `<<~` instead of `<<-`.\nRUBY\n",
        );
    }

    #[test]
    fn corrects_dash_heredoc_to_squiggly() {
        // adjust_minus only rewrites the sigil; the production fixpoint loop
        // re-indents on a later pass.
        test::<HeredocIndentation>().expect_correction(
            "x = <<-RUBY\nhello\n^^^^^ Use 2 spaces for indentation in a heredoc by using `<<~` instead of `<<-`.\nRUBY\n",
            "x = <<~RUBY\nhello\nRUBY\n",
        );
    }

    #[test]
    fn flags_zero_indent_plain_heredoc() {
        test::<HeredocIndentation>().expect_offense(
            "x = <<RUBY\nhello\n^^^^^ Use 2 spaces for indentation in a heredoc by using `<<~` instead of `<<`.\nRUBY\n",
        );
    }

    #[test]
    fn accepts_indented_dash_heredoc_body() {
        // A `<<-` heredoc whose body is already indented is not flagged
        // (offense only when body_indent_level is zero).
        test::<HeredocIndentation>().expect_no_offenses("x = <<-RUBY\n  hello\n  RUBY\n");
    }

    // ── Multiple heredocs on one opener line ──────────────────────────────────
    //
    // Regression for the body-range bug where each heredoc body was taken as
    // `opener_token.end + 1 .. terminator_line_start`. For stacked openers
    // (`foo(<<~A, <<~B)`), that swallowed the rest of the opener line plus the
    // sibling heredoc's body, producing destructive autocorrects. Bodies must
    // stack: `A` runs to its own terminator, `B` starts after `A`'s.

    #[test]
    fn accepts_correctly_indented_stacked_squiggly_heredocs() {
        test::<HeredocIndentation>()
            .expect_no_offenses("foo(<<~A, <<~B)\n  a\nA\n  b\nB\n");
    }

    #[test]
    fn corrects_each_stacked_squiggly_body_independently() {
        // Both bodies are under-indented; each is re-indented to 2 spaces
        // without touching the opener line or the sibling heredoc. The offense
        // is anchored to each body's first line.
        test::<HeredocIndentation>().expect_correction(
            "foo(<<~A, <<~B)\na\n^ Use 2 spaces for indentation in a heredoc.\nA\nb\n^ Use 2 spaces for indentation in a heredoc.\nB\n",
            "foo(<<~A, <<~B)\n  a\nA\n  b\nB\n",
        );
    }

    #[test]
    fn handles_empty_first_stacked_heredoc_body() {
        // `A` has an empty body (skipped); `B` is already correctly indented.
        // The cursor must still chain past `A`'s terminator so `B`'s body is
        // located correctly.
        test::<HeredocIndentation>().expect_no_offenses("foo(<<~A, <<~B)\nA\n  b\nB\n");
    }
}

murphy_plugin_api::submit_cop!(HeredocIndentation);
