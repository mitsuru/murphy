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
//!   sits between two non-space characters (the left one not being a bare
//!   backslash, allowing escaped spaces `\ ` before the run) down to a single
//!   space. Only `%i/%I/%w/%W` literals (which map to Murphy `Array` nodes)
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
    // Opener is `%` + type char + one delimiter char, e.g. `%w[`.
    // The delimiter character may be multibyte-safe ASCII (`[`, `(`, `{`, `<`,
    // `|`, `/`, etc.); take the third char's byte length.
    let mut chars = src.char_indices();
    chars.next()?; // `%`
    chars.next()?; // type char (`w`/`i`/`W`/`I`)
    let (delim_idx, _) = chars.next()?; // opening delimiter
    let opener_len = delim_idx + 1; // bytes up to and including the delimiter
    let total_len = src.len();
    if total_len < opener_len + 1 {
        return None;
    }
    // Closing delimiter is the final byte (ASCII for all percent delimiters).
    Some(Range {
        start: node_range.start + opener_len as u32,
        end: node_range.end - 1,
    })
}

/// Find byte ranges (relative to `contents`) of runs of 2+ spaces that are
/// bounded on the left by a non-space, non-backslash character (allowing
/// `\ ` escaped-space sequences immediately before the run) and on the right
/// by a non-space character. Mirrors RuboCop's
/// `(?:[\S&&[^\\]](?:\\ )*)( {2,})(?=\S)`.
fn multiple_space_runs(contents: &str) -> Vec<(usize, usize)> {
    let bytes = contents.as_bytes();
    let mut runs = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b' ' {
            i += 1;
            continue;
        }
        // Found the start of a space run. Measure it.
        let run_start = i;
        while i < bytes.len() && bytes[i] == b' ' {
            i += 1;
        }
        let run_end = i;
        // Need 2+ spaces.
        if run_end - run_start < 2 {
            continue;
        }
        // Must be followed by a non-space (i.e. not trailing the contents).
        if run_end >= bytes.len() {
            continue;
        }
        // Must be preceded by a non-space, non-backslash character. A bare
        // trailing backslash means the spaces are escaped (`\ `), which the
        // regex skips over via `(?:\\ )*`.
        if run_start == 0 || bytes[run_start - 1] == b'\\' {
            continue;
        }
        runs.push((run_start, run_end));
    }
    runs
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
        test::<SpaceInsideArrayPercentLiteral>().expect_no_offenses("%w(a\\  b)\n");
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
}
murphy_plugin_api::submit_cop!(SpaceInsideArrayPercentLiteral);
