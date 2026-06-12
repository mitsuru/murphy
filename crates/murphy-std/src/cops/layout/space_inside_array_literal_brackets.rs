//! `Layout/SpaceInsideArrayLiteralBrackets` — checks the spacing immediately
//! inside square-bracket array literals `[ ]`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/SpaceInsideArrayLiteralBrackets
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues:
//!   - murphy-26l9
//! notes: >
//!   Node-driven (on_array) like RuboCop. Percent-literal arrays (`%w[]`) are
//!   skipped via `is_square_brackets` so this cop and ArrayPercentLiteral never
//!   both fire. Implements EnforcedStyle no_space(default)/space and
//!   EnforcedStyleForEmptyBrackets no_space(default)/space, plus the multiline
//!   exemptions (a bracket adjacent to a newline is not flagged: RuboCop's
//!   `next_to_newline?` / `end_has_own_line?`). The `compact` style and the
//!   multi-dimensional collapse it implies are NOT yet ported (tracked in the
//!   follow-up gap issue) — compact falls back to the `space` behaviour, which
//!   is the closest single-style approximation. Comment-after-`[` handling
//!   (RuboCop's `next_to_comment?`) is covered for the no_space start side.
//! ```

use murphy_plugin_api::{
    CopOptionEnum, CopOptions, Cx, NodeId, Range, SourceToken, SourceTokenKind, cop,
};

#[derive(Default)]
pub struct SpaceInsideArrayLiteralBrackets;

#[derive(CopOptions)]
pub struct SpaceInsideArrayLiteralBracketsOptions {
    #[option(
        name = "EnforcedStyle",
        default = "no_space",
        description = "Array bracket spacing style."
    )]
    pub enforced_style: ArrayBracketStyle,
    #[option(
        name = "EnforcedStyleForEmptyBrackets",
        default = "no_space",
        description = "Spacing style for empty array brackets."
    )]
    pub empty_style: EmptyArrayBracketStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum ArrayBracketStyle {
    #[option(value = "no_space")]
    NoSpace,
    #[option(value = "space")]
    Space,
    #[option(value = "compact")]
    Compact,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum EmptyArrayBracketStyle {
    #[option(value = "no_space")]
    NoSpace,
    #[option(value = "space")]
    Space,
}

#[cop(
    name = "Layout/SpaceInsideArrayLiteralBrackets",
    description = "Check spacing inside array literal brackets.",
    default_severity = "warning",
    default_enabled = true,
    options = SpaceInsideArrayLiteralBracketsOptions,
)]
impl SpaceInsideArrayLiteralBrackets {
    #[on_node(kind = "array")]
    fn check_array(
        &self,
        node: NodeId,
        cx: &Cx<'_>,
        options: &SpaceInsideArrayLiteralBracketsOptions,
    ) {
        // Only `[ ]`-delimited arrays; `%w[]` belongs to ArrayPercentLiteral.
        if !cx.is_square_brackets(node) {
            return;
        }

        // `[` and `]` are `SourceTokenKind::Other`, so locate them by position:
        // a square-bracket array's range starts at `[` and ends just past `]`.
        let node_range = cx.range(node);
        let src = cx.raw_source(node_range);
        if !src.starts_with('[') || !src.ends_with(']') || node_range.end <= node_range.start {
            return;
        }
        let tokens = cx.tokens_in(node_range);
        let left = SourceToken {
            range: Range {
                start: node_range.start,
                end: node_range.start + 1,
            },
            kind: SourceTokenKind::Other,
        };
        let right = SourceToken {
            range: Range {
                start: node_range.end - 1,
                end: node_range.end,
            },
            kind: SourceTokenKind::Other,
        };
        if right.range.start < left.range.end {
            return;
        }

        let interior = Range {
            start: left.range.end,
            end: right.range.start,
        };
        let inner = cx.raw_source(interior);

        // Empty brackets (`[]` / `[ ]`).
        if inner.bytes().all(|b| b.is_ascii_whitespace()) {
            empty_offenses(cx, options, left, right, interior);
            return;
        }

        let single_line = is_single_line(cx, left, right);
        // Multiline exemptions: a bracket directly bounded by a newline is not
        // an inline spacing issue (RuboCop's next_to_newline? / end_has_own_line?).
        let start_ok = !single_line && bracket_followed_by_newline(cx, left, tokens);
        let end_ok = !single_line && bracket_preceded_by_newline(cx, right);

        match options.enforced_style {
            ArrayBracketStyle::NoSpace => {
                // RuboCop exempts the start side when a comment follows `[`.
                let start_ok = start_ok || bracket_followed_by_comment(left, tokens);
                no_space_offenses(cx, left, right, start_ok, end_ok);
            }
            // `compact` is not fully ported; approximate with `space`.
            ArrayBracketStyle::Space | ArrayBracketStyle::Compact => {
                space_offenses(cx, left, right, start_ok, end_ok);
            }
        }
    }
}

fn empty_offenses(
    cx: &Cx<'_>,
    options: &SpaceInsideArrayLiteralBracketsOptions,
    left: SourceToken,
    right: SourceToken,
    interior: Range,
) {
    let has_space = interior.start < interior.end;
    match options.empty_style {
        EmptyArrayBracketStyle::Space => {
            // Require exactly one space: `[ ]`.
            let is_single_space = cx.raw_source(interior) == " ";
            if !is_single_space {
                let range = Range {
                    start: left.range.start,
                    end: right.range.end,
                };
                cx.emit_offense(range, "Use space inside empty array brackets.", None);
                cx.emit_edit(interior, " ");
            }
        }
        EmptyArrayBracketStyle::NoSpace => {
            if has_space {
                let range = Range {
                    start: left.range.start,
                    end: right.range.end,
                };
                cx.emit_offense(range, "Do not use space inside empty array brackets.", None);
                cx.emit_edit(interior, "");
            }
        }
    }
}

/// `no_space`: flag and remove a space immediately after `[` (unless start_ok)
/// and immediately before `]` (unless end_ok).
fn no_space_offenses(
    cx: &Cx<'_>,
    left: SourceToken,
    right: SourceToken,
    start_ok: bool,
    end_ok: bool,
) {
    if !start_ok {
        let range = space_after(cx, left.range.end);
        if range.start < range.end {
            cx.emit_offense(range, "Do not use space inside array brackets.", None);
            cx.emit_edit(range, "");
        }
    }
    if !end_ok {
        let range = space_before(cx, right.range.start);
        if range.start < range.end {
            cx.emit_offense(range, "Do not use space inside array brackets.", None);
            cx.emit_edit(range, "");
        }
    }
}

/// `space`: flag and insert a missing space after `[` (unless start_ok) and
/// before `]` (unless end_ok).
fn space_offenses(
    cx: &Cx<'_>,
    left: SourceToken,
    right: SourceToken,
    start_ok: bool,
    end_ok: bool,
) {
    if !start_ok && !has_space_at(cx, left.range.end) {
        let at = left.range.end;
        let range = Range { start: at, end: at };
        cx.emit_offense(range, "Use space inside array brackets.", None);
        cx.emit_edit(range, " ");
    }
    if !end_ok && !has_space_before(cx, right.range.start) {
        let at = right.range.start;
        let range = Range { start: at, end: at };
        cx.emit_offense(range, "Use space inside array brackets.", None);
        cx.emit_edit(range, " ");
    }
}

fn space_after(cx: &Cx<'_>, from: u32) -> Range {
    let src = cx.source().as_bytes();
    let mut end = from as usize;
    while src.get(end).is_some_and(|&b| b == b' ' || b == b'\t') {
        end += 1;
    }
    Range {
        start: from,
        end: end as u32,
    }
}

fn space_before(cx: &Cx<'_>, to: u32) -> Range {
    let src = cx.source().as_bytes();
    let mut start = to as usize;
    while start > 0 && src.get(start - 1).is_some_and(|&b| b == b' ' || b == b'\t') {
        start -= 1;
    }
    Range {
        start: start as u32,
        end: to,
    }
}

fn has_space_at(cx: &Cx<'_>, offset: u32) -> bool {
    cx.source()
        .as_bytes()
        .get(offset as usize)
        .is_some_and(|&b| b == b' ' || b == b'\t')
}

fn has_space_before(cx: &Cx<'_>, offset: u32) -> bool {
    offset > 0
        && cx
            .source()
            .as_bytes()
            .get(offset as usize - 1)
            .is_some_and(|&b| b == b' ' || b == b'\t')
}

fn is_single_line(cx: &Cx<'_>, left: SourceToken, right: SourceToken) -> bool {
    !cx.raw_source(Range {
        start: left.range.start,
        end: right.range.end,
    })
    .bytes()
    .any(|b| b == b'\n')
}

/// The first token strictly after `[` is on a different line (RuboCop's
/// `next_to_newline?`): the opening bracket starts a multiline body.
fn bracket_followed_by_newline(cx: &Cx<'_>, left: SourceToken, tokens: &[SourceToken]) -> bool {
    let Some(next) = tokens
        .iter()
        .find(|t| t.range.start >= left.range.end)
    else {
        return false;
    };
    cx.raw_source(Range {
        start: left.range.end,
        end: next.range.start,
    })
    .bytes()
    .any(|b| b == b'\n')
}

/// The closing `]` has only whitespace before it back to the line start
/// (RuboCop's `end_has_own_line?`): the bracket sits on its own line.
fn bracket_preceded_by_newline(cx: &Cx<'_>, right: SourceToken) -> bool {
    let src = cx.source().as_bytes();
    let mut i = right.range.start as usize;
    while i > 0 {
        match src[i - 1] {
            b' ' | b'\t' => i -= 1,
            b'\n' => return true,
            _ => return false,
        }
    }
    true
}

/// A comment token immediately follows `[` (RuboCop's `next_to_comment?`).
fn bracket_followed_by_comment(left: SourceToken, tokens: &[SourceToken]) -> bool {
    tokens
        .iter()
        .find(|t| t.range.start >= left.range.end)
        .is_some_and(|t| t.kind == SourceTokenKind::Comment)
}

#[cfg(test)]
mod tests {
    use super::{
        ArrayBracketStyle, EmptyArrayBracketStyle, SpaceInsideArrayLiteralBrackets,
        SpaceInsideArrayLiteralBracketsOptions,
    };
    use murphy_plugin_api::test_support::{indoc, run_cop_with_options_and_edits, test};

    // ── default (no_space) style ────────────────────────────────────────────

    #[test]
    fn no_space_accepts_tight_array() {
        test::<SpaceInsideArrayLiteralBrackets>().expect_no_offenses("a = [1, 2, 3]\n");
    }

    #[test]
    fn no_space_flags_leading_and_trailing_space() {
        test::<SpaceInsideArrayLiteralBrackets>().expect_correction(
            indoc! {r#"
                a = [ 1, 2 ]
                     ^ Do not use space inside array brackets.
                          ^ Do not use space inside array brackets.
            "#},
            "a = [1, 2]\n",
        );
    }

    #[test]
    fn no_space_flags_only_leading() {
        test::<SpaceInsideArrayLiteralBrackets>().expect_correction(
            indoc! {r#"
                a = [ 1, 2]
                     ^ Do not use space inside array brackets.
            "#},
            "a = [1, 2]\n",
        );
    }

    #[test]
    fn no_space_accepts_multiline_array() {
        test::<SpaceInsideArrayLiteralBrackets>().expect_no_offenses(indoc! {r#"
            a = [
              1,
              2,
            ]
        "#});
    }

    // ── space style ─────────────────────────────────────────────────────────

    #[test]
    fn space_style_accepts_spaced_array() {
        let opts = SpaceInsideArrayLiteralBracketsOptions {
            enforced_style: ArrayBracketStyle::Space,
            empty_style: EmptyArrayBracketStyle::NoSpace,
        };
        test::<SpaceInsideArrayLiteralBrackets>()
            .with_options(&opts)
            .expect_no_offenses("a = [ 1, 2, 3 ]\n");
    }

    #[test]
    fn space_style_flags_missing_space() {
        let opts = SpaceInsideArrayLiteralBracketsOptions {
            enforced_style: ArrayBracketStyle::Space,
            empty_style: EmptyArrayBracketStyle::NoSpace,
        };
        let result =
            run_cop_with_options_and_edits::<SpaceInsideArrayLiteralBrackets>("a = [1, 2]\n", &opts);
        assert_eq!(result.offenses.len(), 2, "offenses: {:?}", result.offenses);
        assert!(
            result
                .offenses
                .iter()
                .all(|o| o.message == "Use space inside array brackets."),
            "offenses: {:?}",
            result.offenses
        );
    }

    // ── empty brackets ──────────────────────────────────────────────────────

    #[test]
    fn empty_no_space_accepts_tight_empty() {
        test::<SpaceInsideArrayLiteralBrackets>().expect_no_offenses("a = []\n");
    }

    #[test]
    fn empty_no_space_flags_spaced_empty() {
        test::<SpaceInsideArrayLiteralBrackets>().expect_correction(
            indoc! {r#"
                a = [ ]
                    ^^^ Do not use space inside empty array brackets.
            "#},
            "a = []\n",
        );
    }

    #[test]
    fn empty_space_style_flags_tight_empty() {
        let opts = SpaceInsideArrayLiteralBracketsOptions {
            enforced_style: ArrayBracketStyle::NoSpace,
            empty_style: EmptyArrayBracketStyle::Space,
        };
        test::<SpaceInsideArrayLiteralBrackets>()
            .with_options(&opts)
            .expect_correction(
                indoc! {r#"
                    a = []
                        ^^ Use space inside empty array brackets.
                "#},
                "a = [ ]\n",
            );
    }

    // ── cross-cop: must NOT fire on percent-literal arrays or index calls ────

    #[test]
    fn does_not_flag_percent_literal() {
        // `%w[ ]` belongs to SpaceInsideArrayPercentLiteral, not this cop.
        test::<SpaceInsideArrayLiteralBrackets>().expect_no_offenses("a = %w[foo bar]\n");
    }

    #[test]
    fn does_not_flag_index_access() {
        // `foo[1]` is an index `Send`, not an array literal.
        test::<SpaceInsideArrayLiteralBrackets>().expect_no_offenses("foo[1]\n");
    }
}
murphy_plugin_api::submit_cop!(SpaceInsideArrayLiteralBrackets);
