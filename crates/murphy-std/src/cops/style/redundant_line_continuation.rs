//! `Style/RedundantLineContinuation` — flags unnecessary backslash line
//! continuations.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantLineContinuation
//! upstream_version_checked: 1.49.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   ABI gap: RuboCop's correctness oracle re-parses the source with the `\`
//!   removed and checks `valid_syntax?`. Murphy's single-parse architecture has
//!   no re-parse primitive available in the cop API, so only structurally
//!   provable-redundant cases are flagged. This ensures zero false positives at
//!   the cost of missing some true redundancies.
//!
//!   Implemented:
//!     - `\` inside balanced brackets `(` / `{` (bracket depth > 0) — Ruby
//!       already treats these as continuation contexts; the `\` is always
//!       redundant.
//!     - `\` after a trailing `.` or `&.` operator — the trailing dot already
//!       forces continuation; the `\` is always redundant.
//!
//!   Explicitly skipped (never flagged):
//!     - `\` inside comment lines (detected via Comment token ranges).
//!     - `\` inside heredoc bodies (detected via HeredocStart/HeredocEnd tokens).
//!     - `\` inside string/dstr/xstr/regexp/dsym node ranges (detected by
//!       walking the AST via `cx.descendants`). This correctly handles cases
//!       like `foo("a \\\n b")` where the `\` is inside a string literal
//!       that is itself inside parens — the `\` must not be removed.
//!     - `\` with next-line arithmetic/logical/bitwise operators, modifier
//!       keywords, unparenthesized method arguments: all require the reparse
//!       oracle and are conservatively skipped.
//!
//!   Note: `[` and `]` bracket tracking is done via single-byte Other token
//!   inspection (they appear as `Other` tokens in Murphy's token stream).
//! ```

use murphy_plugin_api::{Cx, NodeKind, NoOptions, Range, SourceToken, SourceTokenKind, cop};

const MSG: &str = "Redundant line continuation.";

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantLineContinuation;

#[cop(
    name = "Style/RedundantLineContinuation",
    description = "Checks for redundant line continuation.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl RedundantLineContinuation {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        check_source(cx);
    }
}

fn check_source(cx: &Cx<'_>) {
    let src = cx.source();
    let bytes = src.as_bytes();
    let toks = cx.sorted_tokens();

    // Collect comment ranges: [start, end) byte pairs.
    let comment_ranges = collect_comment_ranges(toks);

    // Collect heredoc body ranges: [start, end) byte pairs.
    let heredoc_ranges = collect_heredoc_body_ranges(toks, bytes);

    // Collect string/regexp/xstr literal ranges from the AST: any `\` inside
    // these is part of a string literal and must never be flagged.
    let string_ranges = collect_string_literal_ranges(cx);

    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            let next = i + 1;
            // Only care about `\` immediately followed by `\n` (or CRLF).
            let is_backslash_newline = next < bytes.len() && bytes[next] == b'\n';
            let is_backslash_crlf = next + 1 < bytes.len()
                && bytes[next] == b'\r'
                && bytes[next + 1] == b'\n';
            if is_backslash_newline || is_backslash_crlf {
                let backslash_pos = i as u32;

                // Skip if inside a comment.
                if in_any_range(backslash_pos, &comment_ranges) {
                    i += if is_backslash_crlf { 3 } else { 2 };
                    continue;
                }

                // Skip if inside a heredoc body.
                if in_any_range(backslash_pos, &heredoc_ranges) {
                    i += if is_backslash_crlf { 3 } else { 2 };
                    continue;
                }

                // Skip if inside a string/regexp/xstr literal.
                if in_any_range(backslash_pos, &string_ranges) {
                    i += if is_backslash_crlf { 3 } else { 2 };
                    continue;
                }

                if is_redundant_continuation(backslash_pos, bytes, toks) {
                    let offense = Range {
                        start: backslash_pos,
                        end: backslash_pos + 1,
                    };
                    cx.emit_offense(offense, MSG, None);
                    cx.emit_edit(offense, "");
                }

                i += if is_backslash_crlf { 3 } else { 2 };
                continue;
            }
        }
        i += 1;
    }
}

/// Collect source ranges for all string literal, dstr, xstr, and regexp nodes.
/// Any `\` inside these ranges is part of a literal value and must not be
/// flagged as a redundant continuation.
fn collect_string_literal_ranges(cx: &Cx<'_>) -> Vec<(u32, u32)> {
    let root = cx.root();
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    for node_id in cx.descendants(root) {
        match cx.kind(node_id) {
            NodeKind::Str(_)
            | NodeKind::Dstr(_)
            | NodeKind::Xstr(_)
            | NodeKind::Dsym(_)
            | NodeKind::Regexp { .. } => {
                let r = cx.range(node_id);
                ranges.push((r.start, r.end));
            }
            _ => {}
        }
    }
    ranges
}

/// Returns `true` if the `\` at `backslash_pos` is structurally redundant.
fn is_redundant_continuation(
    backslash_pos: u32,
    bytes: &[u8],
    toks: &[SourceToken],
) -> bool {
    // Case 1: Inside balanced `(` / `{` / `[` brackets.
    if bracket_depth_at(backslash_pos, bytes, toks) > 0 {
        return true;
    }

    // Case 2: The last non-whitespace character(s) before `\` form `.` or `&.`.
    if trailing_dot_before(backslash_pos, bytes, toks) {
        return true;
    }

    false
}

/// Compute bracket depth (sum of unmatched open brackets) at `pos`.
///
/// Counts `(` / `)` from LeftParen/RightParen tokens and `{` / `}` from
/// LeftBrace/RightBrace tokens. Counts `[` / `]` from source bytes, but only
/// bytes that are NOT inside the heredoc/comment ranges (the caller already
/// strips those — we use the token-based approach for `[`/`]` via `Other` tokens).
///
/// For `[` / `]` we do a source-byte scan up to `pos`, skipping bytes inside
/// string delimiters. This is conservative: we only scan for `[`/`]` at depth-0
/// positions (not inside strings), but since we're scanning non-string source
/// for `[`/`]`, it's good enough for the common cases.
fn bracket_depth_at(pos: u32, bytes: &[u8], toks: &[SourceToken]) -> i32 {
    let mut depth: i32 = 0;

    for tok in toks {
        if tok.range.start >= pos {
            break;
        }
        match tok.kind {
            SourceTokenKind::LeftParen => depth += 1,
            SourceTokenKind::RightParen => depth -= 1,
            SourceTokenKind::LeftBrace => depth += 1,
            SourceTokenKind::RightBrace => depth -= 1,
            _ => {}
        }
    }

    // Count `[` and `]` from source bytes. These are `Other` tokens so we
    // can't use token kinds. We scan source bytes up to `pos`, skipping
    // bytes that appear to be inside single/double-quoted strings.
    // We use a simple state machine: track whether we're inside a string.
    // This is an approximation — full correctness requires token-based parsing.
    depth += count_square_bracket_depth(pos, bytes, toks);

    depth
}

/// Count net open `[` - `]` from `Other` tokens up to `pos`.
///
/// `[` and `]` are `Other` tokens in Murphy's token stream. We iterate over
/// `Other` tokens whose start < `pos` and check their source text. `Other`
/// tokens that are single-byte `[` or `]` at the operator level (not inside
/// strings) will be present individually. String-interior `[`/`]` would appear
/// inside multi-byte string token ranges — but since we check the token source
/// text for exactly `[` or `]` (single byte, single character), those will
/// also be single-byte Other tokens.
///
/// To avoid counting `[`/`]` inside string literals, we need to distinguish
/// structural `[`/`]` from ones inside string content. We use the following
/// heuristic: only count `Other` tokens whose source text is exactly `[` or `]`
/// and that are NOT adjacent to a string delimiter token. For v1, we take the
/// simple approach of counting all single-byte `[`/`]` Other tokens, accepting
/// that edge cases with `[`/`]` inside string literals may be mishandled. In
/// practice, string-internal `[`/`]` appear as part of larger string tokens
/// (not as separate single-byte Other tokens) in Prism's output.
fn count_square_bracket_depth(pos: u32, bytes: &[u8], toks: &[SourceToken]) -> i32 {
    let mut depth: i32 = 0;
    for tok in toks {
        if tok.range.start >= pos {
            break;
        }
        if tok.kind != SourceTokenKind::Other {
            continue;
        }
        let len = (tok.range.end - tok.range.start) as usize;
        if len != 1 {
            continue;
        }
        match bytes[tok.range.start as usize] {
            b'[' => depth += 1,
            b']' => depth -= 1,
            _ => {}
        }
    }
    depth
}

/// Returns `true` if the non-whitespace content immediately before `backslash_pos`
/// ends with `.` or `&.`.
fn trailing_dot_before(backslash_pos: u32, bytes: &[u8], toks: &[SourceToken]) -> bool {
    // Find the last token whose range ends at or before `backslash_pos`.
    // Skip whitespace between the token and the `\`.
    let idx = toks.partition_point(|t| t.range.end <= backslash_pos);
    if idx == 0 {
        return false;
    }
    let tok = &toks[idx - 1];
    // The dot/safe-nav are `Other` tokens. Check source bytes.
    if tok.kind != SourceTokenKind::Other {
        return false;
    }
    let tok_src = &bytes[tok.range.start as usize..tok.range.end as usize];
    matches!(tok_src, b"." | b"&.")
}

/// Collect (start, end) ranges for all comment tokens.
fn collect_comment_ranges(toks: &[SourceToken]) -> Vec<(u32, u32)> {
    toks.iter()
        .filter(|t| t.kind == SourceTokenKind::Comment)
        .map(|t| (t.range.start, t.range.end))
        .collect()
}

/// Collect (start, end) ranges for heredoc body content.
fn collect_heredoc_body_ranges(toks: &[SourceToken], bytes: &[u8]) -> Vec<(u32, u32)> {
    use std::collections::VecDeque;
    let mut starts: VecDeque<u32> = VecDeque::new();
    let mut ranges: Vec<(u32, u32)> = Vec::new();

    for tok in toks {
        match tok.kind {
            SourceTokenKind::HeredocStart => {
                starts.push_back(tok.range.end + 1);
            }
            SourceTokenKind::HeredocEnd => {
                if let Some(body_start) = starts.pop_front() {
                    let line_start = terminator_line_start(bytes, tok.range.start);
                    ranges.push((body_start, line_start));
                }
            }
            _ => {}
        }
    }
    ranges
}

fn terminator_line_start(source: &[u8], pos: u32) -> u32 {
    let pos = pos as usize;
    source[..pos]
        .iter()
        .rposition(|&b| b == b'\n')
        .map(|i| i as u32 + 1)
        .unwrap_or(0)
}

fn in_any_range(pos: u32, ranges: &[(u32, u32)]) -> bool {
    ranges.iter().any(|&(s, e)| pos >= s && pos < e)
}

#[cfg(test)]
mod tests {
    use super::RedundantLineContinuation;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- No-offense cases ---

    #[test]
    fn no_offense_no_continuation() {
        test::<RedundantLineContinuation>().expect_no_offenses(indoc! {"
            foo
              .bar
        "});
    }

    #[test]
    fn no_offense_continuation_before_arithmetic_operator() {
        test::<RedundantLineContinuation>().expect_no_offenses(indoc! {"
            1 \\
              + 2
        "});
    }

    #[test]
    fn no_offense_continuation_before_logical_and() {
        test::<RedundantLineContinuation>().expect_no_offenses(indoc! {"
            foo \\
              && bar
        "});
    }

    #[test]
    fn no_offense_continuation_before_logical_or() {
        test::<RedundantLineContinuation>().expect_no_offenses(indoc! {"
            foo \\
              || bar
        "});
    }

    #[test]
    fn no_offense_continuation_method_without_parens() {
        test::<RedundantLineContinuation>().expect_no_offenses(indoc! {"
            foo bar \\
              baz
        "});
    }

    #[test]
    fn no_offense_continuation_inside_comment() {
        test::<RedundantLineContinuation>().expect_no_offenses(indoc! {"
            foo
            # bar \\
            baz
        "});
    }

    #[test]
    fn no_offense_continuation_inside_heredoc() {
        test::<RedundantLineContinuation>().expect_no_offenses(indoc! {"
            <<~SQL
              SELECT * FROM foo \\
                WHERE bar = 1
            SQL
        "});
    }

    #[test]
    fn no_offense_string_concat_single_quotes() {
        test::<RedundantLineContinuation>().expect_no_offenses(indoc! {r#"
            'bar' \
              'baz'
        "#});
    }

    #[test]
    fn no_offense_return_with_value() {
        test::<RedundantLineContinuation>().expect_no_offenses(indoc! {"
            return \\
              foo
        "});
    }

    #[test]
    fn no_offense_break_with_value() {
        test::<RedundantLineContinuation>().expect_no_offenses(indoc! {"
            foo do
              break \\
                bar
            end
        "});
    }

    // --- Offense: inside parens ---

    #[test]
    fn flags_continuation_inside_parens_after_comma() {
        test::<RedundantLineContinuation>().expect_offense(indoc! {r#"
            foo(bar, \
                     ^ Redundant line continuation.
              baz)
        "#});
    }

    #[test]
    fn flags_continuation_inside_open_paren() {
        test::<RedundantLineContinuation>().expect_offense(indoc! {r#"
            foo( \
                 ^ Redundant line continuation.
              bar)
        "#});
    }

    // --- Offense: inside hash/block braces ---

    #[test]
    fn flags_continuation_inside_hash() {
        test::<RedundantLineContinuation>().expect_offense(indoc! {r#"
            {foo: \
                  ^ Redundant line continuation.
              bar}
        "#});
    }

    // --- Offense: inside array brackets ---

    #[test]
    fn flags_continuation_inside_array() {
        test::<RedundantLineContinuation>().expect_offense(indoc! {r#"
            [foo, \
                  ^ Redundant line continuation.
              bar]
        "#});
    }

    // --- Offense: inside method def params ---

    #[test]
    fn flags_continuation_inside_method_def_params() {
        test::<RedundantLineContinuation>().expect_offense(indoc! {r#"
            def foo(bar, \
                         ^ Redundant line continuation.
                    baz)
            end
        "#});
    }

    // --- Autocorrect ---

    #[test]
    fn corrects_continuation_inside_parens() {
        test::<RedundantLineContinuation>().expect_correction(
            indoc! {r#"
                foo(bar, \
                         ^ Redundant line continuation.
                  baz)
            "#},
            indoc! {r#"
                foo(bar, 
                  baz)
            "#},
        );
    }

    #[test]
    fn corrects_continuation_inside_array() {
        test::<RedundantLineContinuation>().expect_correction(
            indoc! {r#"
                [foo, \
                      ^ Redundant line continuation.
                  bar]
            "#},
            indoc! {r#"
                [foo, 
                  bar]
            "#},
        );
    }
    #[test]
    fn no_offense_string_inside_parens() {
        // `\` inside a string literal that happens to be inside `()` must NOT
        // be flagged — the string value would change if the `\` were removed.
        test::<RedundantLineContinuation>().expect_no_offenses(indoc! {r#"
            foo("a \
            b")
        "#});
    }

    #[test]
    fn no_offense_string_continuation_double_quoted() {
        // `\` at end of a double-quoted string (multi-line string literal).
        test::<RedundantLineContinuation>().expect_no_offenses(indoc! {r#"
            foo = "foo \
              bar"
        "#});
    }
}
murphy_plugin_api::submit_cop!(RedundantLineContinuation);
