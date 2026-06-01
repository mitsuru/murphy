//! `Style/WhenThen` — flags `when x;` in single-line `case`/`when` branches
//! and suggests `when x then` instead.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/WhenThen
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Detects single-line when branches that use `;` as separator and
//!   autocorrects to `then`. Guards: skip multiline, skip nodes already
//!   using `then`, skip when body is absent. Message interpolates the
//!   comma-joined condition sources.
//! ```
//!
//! ## Matched shapes
//!
//! `When` nodes that:
//! - Are single-line (`!is_multiline?`)
//! - Have a body (`node.body` is non-nil)
//! - Use `;` as the separator between conditions and body (not `then`)
//!
//! ## Autocorrect
//!
//! Replaces the `;` token with ` then`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, Range, SourceTokenKind, cop};

const MSG: &str = "Do not use `when %<expression>s;`. Use `when %<expression>s then` instead.";

#[derive(Default)]
pub struct WhenThen;

#[cop(
    name = "Style/WhenThen",
    description = "Use `when x then` instead of `when x;` for one-line cases.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl WhenThen {
    #[on_node(kind = "when")]
    fn check_when(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Skip multiline when branches.
    if cx.is_multiline(node) {
        return;
    }

    // Skip if the body is absent.
    let body = match cx.when_body(node).get() {
        Some(b) => b,
        None => return,
    };

    // Get conditions list.
    let conds = cx.when_conditions(node);
    if conds.is_empty() {
        return;
    }

    // The separator lies in the gap [last_cond.end, body.start).
    // We scan for `;` vs `then` here.
    let last_cond_end = cx.range(*conds.last().unwrap()).end;
    let body_start = cx.range(body).start;

    if last_cond_end >= body_start {
        return;
    }

    // Look for `;` in the gap. If absent, a `then` keyword is the separator.
    let Some(semi_range) = find_semicolon_in_gap(cx, last_cond_end, body_start) else {
        return;
    };

    // Build expression string: comma-joined condition sources.
    let expression: String = conds
        .iter()
        .map(|&c| cx.raw_source(cx.range(c)))
        .collect::<Vec<_>>()
        .join(", ");

    let message = MSG.replace("%<expression>s", &expression);

    cx.emit_offense(semi_range, &message, None);

    // Autocorrect: replace the `;` with ` then`.
    cx.emit_edit(semi_range, " then");
}

/// Scan tokens in `[from, to)` for a `;` token (represented as `Other` kind
/// with source bytes `b";"`). Returns the range of the first one found, or
/// `None` if no `;` is present (meaning a `then` keyword is the separator).
fn find_semicolon_in_gap(cx: &Cx<'_>, from: u32, to: u32) -> Option<Range> {
    if from >= to {
        return None;
    }
    let toks = cx.sorted_tokens();
    let src = cx.source().as_bytes();
    let idx = toks.partition_point(|t| t.range.start < from);
    for tok in &toks[idx..] {
        if tok.range.start >= to {
            break;
        }
        // `;` is an `Other` token.
        if tok.kind == SourceTokenKind::Other
            && &src[tok.range.start as usize..tok.range.end as usize] == b";"
        {
            return Some(tok.range);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::WhenThen;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_when_semicolon_single_condition() {
        test::<WhenThen>().expect_correction(
            indoc! {"
                case foo
                when 1; 'baz'
                      ^ Do not use `when 1;`. Use `when 1 then` instead.
                end
            "},
            "case foo\nwhen 1 then 'baz'\nend\n",
        );
    }

    #[test]
    fn flags_when_semicolon_multiple_conditions() {
        test::<WhenThen>().expect_correction(
            indoc! {"
                case foo
                when 1, 2; 'baz'
                         ^ Do not use `when 1, 2;`. Use `when 1, 2 then` instead.
                end
            "},
            "case foo\nwhen 1, 2 then 'baz'\nend\n",
        );
    }

    #[test]
    fn flags_multiple_when_semicolons() {
        test::<WhenThen>().expect_correction(
            indoc! {"
                case foo
                when 1; 'baz'
                      ^ Do not use `when 1;`. Use `when 1 then` instead.
                when 2; 'bar'
                      ^ Do not use `when 2;`. Use `when 2 then` instead.
                end
            "},
            "case foo\nwhen 1 then 'baz'\nwhen 2 then 'bar'\nend\n",
        );
    }

    #[test]
    fn accepts_when_then() {
        test::<WhenThen>().expect_no_offenses(indoc! {"
            case foo
            when 1 then 'baz'
            when 2 then 'bar'
            end
        "});
    }

    #[test]
    fn accepts_multiline_when() {
        test::<WhenThen>().expect_no_offenses(indoc! {"
            case foo
            when 1
              'baz'
            end
        "});
    }

    #[test]
    fn accepts_when_without_body() {
        // Empty when body — no separator to flag.
        test::<WhenThen>().expect_no_offenses(indoc! {"
            case foo
            when 1
            end
        "});
    }
}

murphy_plugin_api::submit_cop!(WhenThen);
