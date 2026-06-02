//! `Style/InPatternThen` — flags `in pattern;` in single-line `case`/`in`
//! pattern-matching expressions and suggests `in pattern then` instead.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/InPatternThen
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Detects single-line `in` branches that use `;` as the separator between
//!   the pattern and the body, and autocorrects to ` then`. Guards: skip
//!   multiline, skip nodes without a body, skip when `;` is absent (already
//!   using `then`). The offense is placed on the `;` token. Message
//!   interpolates the pattern source; alternative patterns (`A | B`) are
//!   covered by `cx.raw_source(cx.range(pattern))` which captures the full
//!   `match_alt` tree source verbatim — no recursive reconstruction needed
//!   (simplification over RuboCop's `alternative_pattern_source` helper).
//! ```
//!
//! ## Matched shapes
//!
//! `InPattern` nodes that:
//! - Are single-line (`!is_multiline`)
//! - Have a body
//! - Use `;` as the separator between the pattern (or guard) and the body
//!
//! ## Autocorrect
//!
//! Replaces the `;` token with ` then`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

const MSG: &str = "Do not use `in %<pattern>s;`. Use `in %<pattern>s then` instead.";

#[derive(Default)]
pub struct InPatternThen;

#[cop(
    name = "Style/InPatternThen",
    description = "Use `in pattern then` instead of `in pattern;` for one-line pattern matching.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl InPatternThen {
    #[on_node(kind = "in_pattern")]
    fn check_in_pattern(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Skip multiline in_pattern nodes.
    if cx.is_multiline(node) {
        return;
    }

    // Destructure; skip if no body.
    let (pattern, guard, body) = match *cx.kind(node) {
        NodeKind::InPattern { pattern, guard, body } => (pattern, guard, body),
        _ => return,
    };

    let Some(body_id) = body.get() else {
        return;
    };

    // The separator lies in the gap [anchor_end, body.start).
    // anchor_end is the guard end if a guard is present, else pattern end.
    let anchor_end = if let Some(g) = guard.get() {
        cx.range(g).end
    } else {
        cx.range(pattern).end
    };
    let body_start = cx.range(body_id).start;

    if anchor_end >= body_start {
        return;
    }

    // Look for `;` in the gap. If absent, `then` is the separator (no offense).
    let Some(semi_range) = find_semicolon_in_gap(cx, anchor_end, body_start) else {
        return;
    };

    // Build pattern source string for the message.
    let pattern_source = cx.raw_source(cx.range(pattern));
    let message = MSG.replace("%<pattern>s", pattern_source);

    cx.emit_offense(semi_range, &message, None);

    // Autocorrect: replace `;` with ` then`.
    cx.emit_edit(semi_range, " then");
}

/// Scan tokens in `[from, to)` for a `;` token (`Other` kind with byte `b";"`).
/// Returns the range of the first one found, or `None`.
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
    use super::InPatternThen;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_in_semicolon_simple_pattern() {
        test::<InPatternThen>().expect_correction(
            indoc! {"
                case a
                in b; c
                    ^ Do not use `in b;`. Use `in b then` instead.
                end
            "},
            "case a\nin b then c\nend\n",
        );
    }

    #[test]
    fn flags_in_semicolon_array_pattern() {
        test::<InPatternThen>().expect_correction(
            indoc! {"
                case a
                in b, c, d; e
                          ^ Do not use `in b, c, d;`. Use `in b, c, d then` instead.
                end
            "},
            "case a\nin b, c, d then e\nend\n",
        );
    }

    #[test]
    fn flags_in_semicolon_alternative_pattern() {
        test::<InPatternThen>().expect_correction(
            indoc! {"
                case a
                in 0 | 1 | 2; x
                            ^ Do not use `in 0 | 1 | 2;`. Use `in 0 | 1 | 2 then` instead.
                end
            "},
            "case a\nin 0 | 1 | 2 then x\nend\n",
        );
    }

    #[test]
    fn flags_multiple_in_semicolons() {
        test::<InPatternThen>().expect_correction(
            indoc! {"
                case a
                in b; c
                    ^ Do not use `in b;`. Use `in b then` instead.
                in d; e
                    ^ Do not use `in d;`. Use `in d then` instead.
                end
            "},
            "case a\nin b then c\nin d then e\nend\n",
        );
    }

    #[test]
    fn accepts_in_then() {
        test::<InPatternThen>().expect_no_offenses(indoc! {"
            case a
            in b then c
            end
        "});
    }

    #[test]
    fn accepts_multiline_in_pattern() {
        test::<InPatternThen>().expect_no_offenses(indoc! {"
            case a
            in b
              c
            end
        "});
    }

    #[test]
    fn accepts_in_pattern_without_body() {
        test::<InPatternThen>().expect_no_offenses(indoc! {"
            case condition
            in pattern
            end
        "});
    }

    #[test]
    fn accepts_semicolon_in_body_with_then() {
        // Semicolon separating statements in the body is not flagged.
        test::<InPatternThen>().expect_no_offenses(indoc! {"
            case a
            in b then c; d
            end
        "});
    }
}

murphy_plugin_api::submit_cop!(InPatternThen);
