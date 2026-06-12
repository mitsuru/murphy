//! `Layout/SpaceInsideArrayPercentLiteral` — flag multiple spaces between
//! elements inside `%i`/`%I`/`%w`/`%W` array percent literals.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/SpaceInsideArrayPercentLiteral
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's MULTIPLE_SPACES_BETWEEN_ITEMS_REGEX
//!   `(?:[\S&&[^\\]](?:\\ )*)( {2,})(?=\S)`: collapse a run of 2+ spaces that
//!   sits between two non-whitespace characters (the left one not being a bare
//!   backslash, allowing escaped spaces `\ ` before the run) down to a single
//!   space. Both anchors are `\S`, so the indentation between elements of a
//!   multi-line `%w(...)` — a run preceded/followed by a newline — is left
//!   alone. Only `%i/%I/%w/%W` literals (which map to Murphy `Array` nodes)
//!   are checked; the leading/trailing padding of the contents is left to
//!   other cops, matching RuboCop which only collapses interior runs.
//! ```

use murphy_plugin_api::{Cx, NodeId, NoOptions, Range, cop};

#[derive(Default)]
pub struct SpaceInsideArrayPercentLiteral;

const MSG: &str = "Use only a single space inside array percent literal.";

const PERCENT_ARRAY_PREFIXES: &[&str] = &["%i", "%I", "%w", "%W"];

#[cop(
    name = "Layout/SpaceInsideArrayPercentLiteral",
    description = "Flag multiple spaces between elements in array percent literals.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl SpaceInsideArrayPercentLiteral {
    #[on_node(kind = "array")]
    fn check_array(&self, node: NodeId, cx: &Cx<'_>) {
        let node_range = cx.range(node);
        let src = cx.raw_source(node_range);
        if !PERCENT_ARRAY_PREFIXES.iter().any(|p| src.starts_with(p)) {
            return;
        }

        let Some(contents) = contents_range(node_range, src) else {
            return;
        };
        let contents_src = &src[(contents.start - node_range.start) as usize
            ..(contents.end - node_range.start) as usize];

        for (rel_start, rel_end) in multiple_space_runs(contents_src) {
            let range = Range {
                start: contents.start + rel_start as u32,
                end: contents.start + rel_end as u32,
            };
            cx.emit_offense(range, MSG, None);
            cx.emit_edit(range, " ");
        }
    }
}

/// Byte range of the literal's contents — between the opener (`%w[`, `%i(`,
/// etc.) and the closing delimiter. Returns `None` if the shape is degenerate.
fn contents_range(node_range: Range, src: &str) -> Option<Range> {
    // Opener is `%` + type char + one delimiter char, e.g. `%w[`. The delimiter
    // is usually 1-byte ASCII (`[`, `(`, `{`, `<`, `|`, `/`, …) but Ruby permits
    // multi-byte delimiters, so measure it with `len_utf8()` and clamp every
    // computed index to the byte length to stay on UTF-8 boundaries (no panic
    // on slicing).
    let mut chars = src.char_indices();
    chars.next()?; // `%`
    chars.next()?; // type char (`w`/`i`/`W`/`I`)
    let (delim_idx, delim_char) = chars.next()?; // opening delimiter
    let delim_len = delim_char.len_utf8();
    let total_len = src.len();
    let opener_len = (delim_idx + delim_len).min(total_len);
    // Need room for the opener and a matching closing delimiter of equal width.
    if total_len < opener_len + delim_len {
        return None;
    }
    Some(Range {
        start: node_range.start + opener_len as u32,
        end: node_range.end - delim_len as u32,
    })
}

/// Find byte ranges (relative to `contents`) of the collapsible portion of
/// runs of spaces between two non-space characters. Mirrors RuboCop's
/// `(?:[\S&&[^\\]](?:\\ )*)( {2,})(?=\S)`: the run must be preceded by a
/// non-space character and followed by a non-space character, and any single
/// leading escaped space (`\ `) is consumed before measuring the collapsible
/// remainder.
fn multiple_space_runs(contents: &str) -> Vec<(usize, usize)> {
    let bytes = contents.as_bytes();
    let mut runs = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b' ' {
            i += 1;
            continue;
        }
        // Measure the full run of consecutive spaces.
        let run_start = i;
        while i < bytes.len() && bytes[i] == b' ' {
            i += 1;
        }
        let run_end = i;

        // The run must sit between two non-whitespace characters: a leading
        // char (the regex's `[\S&&[^\\]]…` anchor) and a trailing one
        // (`(?=\S)`). Both anchors are `\S`, so a newline/tab on either side —
        // e.g. the indentation between elements of a multi-line `%w(...)` —
        // does NOT anchor the run and must be left alone.
        if run_start == 0 || is_ruby_space(bytes[run_start - 1]) {
            continue;
        }
        if run_end >= bytes.len() || is_ruby_space(bytes[run_end]) {
            continue;
        }

        // If the first space is an escaped space (`\ `), the backslash
        // consumes it via `(?:\\ )*`; the collapsible run starts one space
        // later. The backslash is only an escape when an odd number of
        // backslashes immediately precede the run (an even count is literal
        // backslashes, leaving the space unescaped).
        let collapse_start = if preceding_backslashes(bytes, run_start) % 2 == 1 {
            run_start + 1
        } else {
            run_start
        };

        // 2+ remaining spaces collapse to one.
        if run_end - collapse_start >= 2 {
            runs.push((collapse_start, run_end));
        }
    }
    runs
}

/// Ruby's `\s` character class: `[ \t\r\n\f\v]` (ASCII-only, as RuboCop's
/// regex is not in Unicode mode). Used to test the run's anchors against
/// RuboCop's `\S`, so newline/tab between elements does not count as an anchor.
fn is_ruby_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0C | 0x0B)
}

/// Count the consecutive `\` bytes immediately before `pos`.
fn preceding_backslashes(bytes: &[u8], pos: usize) -> usize {
    let mut count = 0;
    let mut j = pos;
    while j > 0 && bytes[j - 1] == b'\\' {
        count += 1;
        j -= 1;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::SpaceInsideArrayPercentLiteral;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_multiple_spaces_in_percent_w() {
        test::<SpaceInsideArrayPercentLiteral>().expect_correction(
            indoc! {r#"
                %w(a  b)
                    ^^ Use only a single space inside array percent literal.
            "#},
            "%w(a b)\n",
        );
    }

    #[test]
    fn flags_multiple_spaces_in_percent_i() {
        test::<SpaceInsideArrayPercentLiteral>().expect_correction(
            indoc! {r#"
                %i(foo   bar)
                      ^^^ Use only a single space inside array percent literal.
            "#},
            "%i(foo bar)\n",
        );
    }

    #[test]
    fn accepts_single_spaces() {
        test::<SpaceInsideArrayPercentLiteral>().expect_no_offenses("%w(a b c)\n");
    }

    #[test]
    fn accepts_leading_and_trailing_spaces() {
        // RuboCop only collapses interior runs between elements; padding at the
        // edges of the contents is not this cop's concern.
        test::<SpaceInsideArrayPercentLiteral>().expect_no_offenses("%w( a b )\n");
    }

    #[test]
    fn ignores_regular_bracketed_array() {
        test::<SpaceInsideArrayPercentLiteral>().expect_no_offenses("[:foo,  :bar]\n");
    }

    #[test]
    fn flags_multiple_runs() {
        test::<SpaceInsideArrayPercentLiteral>().expect_correction(
            indoc! {r#"
                %w(a  b  c)
                    ^^ Use only a single space inside array percent literal.
                       ^^ Use only a single space inside array percent literal.
            "#},
            "%w(a b c)\n",
        );
    }

    #[test]
    fn accepts_escaped_space() {
        // `\ ` is an escaped space inside the element, not a separator run.
        // `a\  b` = escaped space + a single separator space → no offense.
        test::<SpaceInsideArrayPercentLiteral>().expect_no_offenses("%w(a\\  b)\n");
    }

    #[test]
    fn flags_extra_spaces_after_escaped_space() {
        // `a\   b` = escaped space (`\ `) consumed, then 2 separator spaces
        // remain → collapse those to one (RuboCop's `(?:\\ )*( {2,})`).
        test::<SpaceInsideArrayPercentLiteral>().expect_correction(
            indoc! {r#"
                %w(a\   b)
                      ^^ Use only a single space inside array percent literal.
            "#},
            "%w(a\\  b)\n",
        );
    }

    #[test]
    fn accepts_literal_backslash_before_double_space_is_flagged() {
        // `a\\  b` = a literal backslash (`\\`, even count) then 2 unescaped
        // spaces → the run is collapsible.
        test::<SpaceInsideArrayPercentLiteral>().expect_correction(
            indoc! {r#"
                %w(a\\  b)
                      ^^ Use only a single space inside array percent literal.
            "#},
            "%w(a\\\\ b)\n",
        );
    }

    #[test]
    fn flags_with_square_bracket_delimiter() {
        test::<SpaceInsideArrayPercentLiteral>().expect_correction(
            indoc! {r#"
                %w[a  b]
                    ^^ Use only a single space inside array percent literal.
            "#},
            "%w[a b]\n",
        );
    }

    #[test]
    fn accepts_multiline_word_array() {
        // Elements separated by `\n` + indentation are NOT flagged: RuboCop's
        // anchors `[\S&&[^\\]]` / `(?=\S)` both require a non-whitespace
        // character, and the `\n` before each indent run breaks the leading
        // anchor. Matches RuboCop 1.87.0 (0 offenses).
        test::<SpaceInsideArrayPercentLiteral>().expect_no_offenses(indoc! {r#"
            FILTERS = %w(
              lowercase
              asciifolding
              cjk_width
            )
        "#});
    }

    #[test]
    fn flags_interior_double_space_in_multiline() {
        // A genuine 2-space run between two elements on the same physical line
        // is still flagged inside a multi-line literal; the newline-anchored
        // indentation on the other lines is not.
        test::<SpaceInsideArrayPercentLiteral>().expect_correction(
            indoc! {r#"
                FILTERS = %w(
                  same  line
                      ^^ Use only a single space inside array percent literal.
                  next
                )
            "#},
            indoc! {r#"
                FILTERS = %w(
                  same line
                  next
                )
            "#},
        );
    }
}
murphy_plugin_api::submit_cop!(SpaceInsideArrayPercentLiteral);
