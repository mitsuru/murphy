//! `Layout/SpaceInsidePercentLiteralDelimiters` — flags unnecessary spaces
//! immediately inside the delimiters of `%i`/`%I`/`%w`/`%W`/`%x` literals.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/SpaceInsidePercentLiteralDelimiters
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop: subscribe to array (`%i`/`%I`/`%w`/`%W`) and xstr
//!   (`%x`) nodes; restrict to those `%`-literal prefixes. Two offense paths:
//!   blank-only bodies (`%w( )` → flag/remove the whole whitespace body) and
//!   single-line unnecessary leading/trailing spaces (`BEGIN_REGEX = /\A( +)/`,
//!   `END_REGEX = /(?<!\\)( +)\z/`). The trailing path honours the lookbehind,
//!   so an escaped trailing space (`%w(... c\ )`) is preserved while a
//!   following unescaped space is removed. Multiline literals are accepted, and
//!   `%r`/`%q`/`%s`/`%Q` are out of scope (matching RuboCop). Only literal
//!   space characters (`' '`) are flagged, not tabs.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, Range, cop};

const MSG: &str = "Do not use spaces inside percent literal delimiters.";

#[derive(Default)]
pub struct SpaceInsidePercentLiteralDelimiters;

#[cop(
    name = "Layout/SpaceInsidePercentLiteralDelimiters",
    description = "Checks for unnecessary spaces inside percent literal delimiters.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl SpaceInsidePercentLiteralDelimiters {
    #[on_node(kind = "array")]
    fn check_array(&self, node: NodeId, cx: &Cx<'_>) {
        process(node, cx, &["%i", "%I", "%w", "%W"]);
    }

    #[on_node(kind = "xstr")]
    fn check_xstr(&self, node: NodeId, cx: &Cx<'_>) {
        process(node, cx, &["%x"]);
    }
}

fn process(node: NodeId, cx: &Cx<'_>, allowed: &[&str]) {
    let node_range = cx.range(node);
    let src = cx.raw_source(node_range);
    // The opening delimiter is `%` + flag-letter + 1 delimiter char (3 bytes
    // for every `%i`/`%I`/`%w`/`%W`/`%x` form). Require that 2-char prefix to
    // be in scope, and a closing delimiter byte at the tail.
    if src.len() < 4 {
        return;
    }
    let Some(prefix) = src.get(..2) else {
        return;
    };
    if !allowed.contains(&prefix) {
        return;
    }

    // body_range: between the opening delimiter (`%` + flag + open char) and
    // the closing delimiter (1 char). Compute boundaries on char boundaries
    // rather than fixed byte offsets so a multi-byte delimiter never slices
    // into the middle of a UTF-8 character (which would panic `raw_source`).
    // The opening is the first three chars; the close is the final char.
    let body_start_rel = src.char_indices().nth(3).map_or(src.len(), |(idx, _)| idx);
    let Some((body_end_rel, _)) = src.char_indices().next_back() else {
        return;
    };
    if body_start_rel >= body_end_rel {
        return;
    }
    let body_start = node_range.start + body_start_rel as u32;
    let body_end = node_range.start + body_end_rel as u32;
    let body = cx.raw_source(Range {
        start: body_start,
        end: body_end,
    });

    // --- blank-only path -----------------------------------------------------
    // RuboCop: `return if range.source.empty? || !range.source.strip.empty?`.
    if !body.is_empty() && body.trim().is_empty() {
        let range = Range {
            start: body_start,
            end: body_end,
        };
        cx.emit_offense(range, MSG, None);
        cx.emit_edit(range, "");
        return;
    }

    // --- unnecessary leading/trailing path (single-line only) ---------------
    if body.contains('\n') || body.contains('\r') {
        return;
    }
    let body_bytes = body.as_bytes();

    // BEGIN_REGEX: /\A( +)/ — leading run of literal spaces.
    let lead = body_bytes.iter().take_while(|&&b| b == b' ').count();
    if lead > 0 {
        let range = Range {
            start: body_start,
            end: body_start + lead as u32,
        };
        cx.emit_offense(range, MSG, None);
        cx.emit_edit(range, "");
    }

    // END_REGEX: /(?<!\\)( +)\z/ — trailing run of literal spaces whose first
    // matched space is not immediately preceded by a backslash.
    if let Some((trail_start, trail_end)) = trailing_space_match(body_bytes) {
        let range = Range {
            start: body_start + trail_start as u32,
            end: body_start + trail_end as u32,
        };
        cx.emit_offense(range, MSG, None);
        cx.emit_edit(range, "");
    }
}

/// Computes the byte span (relative to body start) flagged by RuboCop's
/// `END_REGEX = /(?<!\\)( +)\z/`: the trailing run of spaces, excluding a
/// leading space that is immediately preceded by a backslash (escaped). Returns
/// `None` when there is no qualifying trailing space.
fn trailing_space_match(body: &[u8]) -> Option<(usize, usize)> {
    let end = body.len();
    // Count the trailing run of literal spaces.
    let mut start = end;
    while start > 0 && body[start - 1] == b' ' {
        start -= 1;
    }
    if start == end {
        return None; // no trailing spaces
    }
    // `(?<!\\)` lookbehind: the first matched space must not be preceded by a
    // backslash. If it is, shrink the match by one (drop the escaped space).
    if start > 0 && body[start - 1] == b'\\' {
        start += 1;
        if start >= end {
            return None; // the only trailing space was escaped
        }
    }
    Some((start, end))
}

murphy_plugin_api::submit_cop!(SpaceInsidePercentLiteralDelimiters);

#[cfg(test)]
mod tests {
    use super::SpaceInsidePercentLiteralDelimiters as Cop;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn corrects_leading_and_trailing_spaces() {
        test::<Cop>().expect_correction(
            indoc! {r#"
                %w( 1 2  )
                       ^^ Do not use spaces inside percent literal delimiters.
                   ^ Do not use spaces inside percent literal delimiters.
            "#},
            "%w(1 2)\n",
        );
    }

    #[test]
    fn corrects_leading_space() {
        test::<Cop>().expect_correction(
            indoc! {r#"
                %w( 1 2)
                   ^ Do not use spaces inside percent literal delimiters.
            "#},
            "%w(1 2)\n",
        );
    }

    #[test]
    fn corrects_trailing_space() {
        test::<Cop>().expect_correction(
            indoc! {r#"
                %w(1 2 )
                      ^ Do not use spaces inside percent literal delimiters.
            "#},
            "%w(1 2)\n",
        );
    }

    #[test]
    fn corrects_escaped_and_other_spaces() {
        // ` \ a b c\  ` — leading space flagged; the final unescaped trailing
        // space flagged; the escaped `\ ` is preserved.
        test::<Cop>().expect_correction(
            indoc! {r#"
                %w( \ a b c\  )
                             ^ Do not use spaces inside percent literal delimiters.
                   ^ Do not use spaces inside percent literal delimiters.
            "#},
            "%w(\\ a b c\\ )\n",
        );
    }

    #[test]
    fn corrects_blank_single_space() {
        test::<Cop>().expect_correction(
            indoc! {r#"
                %w( )
                   ^ Do not use spaces inside percent literal delimiters.
            "#},
            "%w()\n",
        );
    }

    #[test]
    fn corrects_blank_multiple_spaces() {
        test::<Cop>().expect_correction(
            indoc! {r#"
                %w(  )
                   ^^ Do not use spaces inside percent literal delimiters.
            "#},
            "%w()\n",
        );
    }

    #[test]
    fn accepts_clean_literals() {
        test::<Cop>()
            .expect_no_offenses("%w(a b c)\n")
            .expect_no_offenses("%i(a b c)\n")
            .expect_no_offenses("%x(a b c)\n")
            .expect_no_offenses("%w(\\ a b c\\ )\n")
            .expect_no_offenses("%w(a  b  c)\n");
    }

    #[test]
    fn accepts_multiline_literals() {
        test::<Cop>().expect_no_offenses(indoc! {r#"
            %w(
              a
              b
              c
            )
        "#});
    }

    #[test]
    fn ignores_regexp_and_string_percent_literals() {
        test::<Cop>()
            .expect_no_offenses("%r( foo )\n")
            .expect_no_offenses("%q( foo )\n")
            .expect_no_offenses("%( foo )\n");
    }

    #[test]
    fn handles_multibyte_body_without_panicking() {
        // Multi-byte characters in the body must not confuse the char-boundary
        // body computation (a fixed `+3`/`-1` byte slice could panic here).
        test::<Cop>()
            .expect_no_offenses("%w(あ いう)\n")
            .expect_correction(
                indoc! {r#"
                    %w( あ いう )
                            ^ Do not use spaces inside percent literal delimiters.
                       ^ Do not use spaces inside percent literal delimiters.
                "#},
                "%w(あ いう)\n",
            );
    }
}
