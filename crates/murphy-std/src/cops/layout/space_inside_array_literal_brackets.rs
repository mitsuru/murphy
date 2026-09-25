//! `Layout/SpaceInsideArrayLiteralBrackets` — checks the spacing immediately
//! inside square-bracket array literals `[ ]`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/SpaceInsideArrayLiteralBrackets
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Node-driven (on_array) like RuboCop. Percent-literal arrays (`%w[]`) are
//!   skipped via `is_square_brackets` so this cop and ArrayPercentLiteral never
//!   both fire. Implements EnforcedStyle no_space(default)/space/compact and
//!   EnforcedStyleForEmptyBrackets no_space(default)/space, plus the multiline
//!   exemptions (a bracket adjacent to a newline is not flagged: RuboCop's
//!   `next_to_newline?` / `end_has_own_line?`). The `compact` style collapses
//!   successive nested brackets (RuboCop's `qualifies_for_compact?` /
//!   `multi_dimensional_array?` / `compact_offenses` / `compact_corrections`):
//!   a `[` whose next non-whitespace char is `[` (or a `]` whose previous
//!   non-whitespace char is `]`) must touch with no gap — including newlines,
//!   which are removed — otherwise `compact` requires spaces like `space`.
//!   Empty-bracket handling (`EnforcedStyleForEmptyBrackets`) applies under
//!   every style, matching RuboCop. Comment-after-`[` handling (RuboCop's
//!   `next_to_comment?`) is covered for the no_space start side.
//! ```

use murphy_plugin_api::{
    cop, CopOptionEnum, CopOptions, Cx, NodeId, Range, SourceToken, SourceTokenKind,
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
            ArrayBracketStyle::Space => {
                space_offenses(cx, left, right, start_ok, end_ok);
            }
            ArrayBracketStyle::Compact => {
                compact_offenses(cx, left, right, start_ok, end_ok);
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

/// `compact`: normally require a space like `space`, except successive nested
/// brackets collapse (RuboCop's `compact_offenses`).
///
/// - Left: if the next non-whitespace char after `[` is `[`
///   (`multi_dimensional_array?` with `side: :left`), any gap — spaces or
///   newlines — is an offense to remove (`qualifies_for_compact?` +
///   `compact_offense` + `compact_corrections`). Otherwise require a space
///   unless `start_ok` (multiline newline exemption).
/// - Right: mirrored — if the previous non-whitespace char before `]` is `]`,
///   remove the gap; otherwise require a space unless `end_ok`.
///
/// Whitespace scanning is byte-based: spaces are invisible to RuboCop's token
/// stream and newlines are explicitly skipped, so "next non-whitespace is
/// `[`/`]`" matches `multi_dimensional_array?`. A `#` comment breaks the
/// collapse (it is non-whitespace and not a bracket), matching RuboCop where
/// the comment token sits between the brackets.
fn compact_offenses(
    cx: &Cx<'_>,
    left: SourceToken,
    right: SourceToken,
    start_ok: bool,
    end_ok: bool,
) {
    let src = cx.source().as_bytes();
    let left_multidim = next_non_ws_is(src, left.range.end, right.range.start, b'[');
    let right_multidim = prev_non_ws_is(src, right.range.start, left.range.end, b']');

    if left_multidim {
        // Collapse: remove the whole whitespace run including newlines
        // (RuboCop's `compact` with `include_newlines: true`).
        let gap_end = ws_run_end(src, left.range.end, right.range.start);
        if gap_end > left.range.end {
            // Offense range mirrors RuboCop's `side_space_range` without
            // newlines: the spaces/tabs prefix only. When the gap starts with
            // a newline there is no spaces prefix, so the offense is
            // zero-width at the bracket edge (RuboCop's `^{}`).
            let off_end = spaces_run_end(src, left.range.end, right.range.start);
            if off_end > left.range.end {
                let range = Range {
                    start: left.range.end,
                    end: off_end,
                };
                cx.emit_offense(range, "Do not use space inside array brackets.", None);
            } else {
                let at = left.range.end;
                let range = Range { start: at, end: at };
                cx.emit_offense(range, "Do not use space inside array brackets.", None);
            }
            cx.emit_edit(
                Range {
                    start: left.range.end,
                    end: gap_end,
                },
                "",
            );
        }
    } else if !start_ok && !has_space_at(cx, left.range.end) {
        let at = left.range.end;
        let range = Range { start: at, end: at };
        cx.emit_offense(range, "Use space inside array brackets.", None);
        cx.emit_edit(range, " ");
    }

    if right_multidim {
        let gap_start = ws_run_start(src, right.range.start, left.range.end);
        if gap_start < right.range.start {
            let off_start = spaces_run_start(src, right.range.start, left.range.end);
            if off_start < right.range.start {
                let range = Range {
                    start: off_start,
                    end: right.range.start,
                };
                cx.emit_offense(range, "Do not use space inside array brackets.", None);
            } else {
                let at = right.range.start;
                let range = Range { start: at, end: at };
                cx.emit_offense(range, "Do not use space inside array brackets.", None);
            }
            cx.emit_edit(
                Range {
                    start: gap_start,
                    end: right.range.start,
                },
                "",
            );
        }
    } else if !end_ok && !has_space_before(cx, right.range.start) {
        let at = right.range.start;
        let range = Range { start: at, end: at };
        cx.emit_offense(range, "Use space inside array brackets.", None);
        cx.emit_edit(range, " ");
    }
}

/// True when the next non-whitespace byte in `[from, ceil)` is `want`.
/// Whitespace is `is_ascii_whitespace` plus vertical tab (Ruby `\s`).
fn next_non_ws_is(src: &[u8], from: u32, ceil: u32, want: u8) -> bool {
    let mut i = from as usize;
    let ceil = ceil as usize;
    while i < ceil && is_ws(src.get(i).copied().unwrap_or(b'x')) {
        i += 1;
    }
    i < ceil && src.get(i) == Some(&want)
}

/// True when the previous non-whitespace byte in `[floor, to)` is `want`.
fn prev_non_ws_is(src: &[u8], to: u32, floor: u32, want: u8) -> bool {
    let mut i = to as usize;
    let floor = floor as usize;
    while i > floor && is_ws(src.get(i - 1).copied().unwrap_or(b'x')) {
        i -= 1;
    }
    i > floor && src.get(i - 1) == Some(&want)
}

fn is_ws(b: u8) -> bool {
    b.is_ascii_whitespace() || b == 0x0B
}

/// End of the full whitespace run (spaces, tabs, newlines) starting at `from`.
fn ws_run_end(src: &[u8], from: u32, ceil: u32) -> u32 {
    let mut end = from as usize;
    let ceil = ceil as usize;
    while end < ceil && end < src.len() && is_ws(src[end]) {
        end += 1;
    }
    end as u32
}

/// Start of the full whitespace run ending at `to`.
fn ws_run_start(src: &[u8], to: u32, floor: u32) -> u32 {
    let mut start = to as usize;
    let floor = floor as usize;
    while start > floor && start > 0 && is_ws(src[start - 1]) {
        start -= 1;
    }
    start as u32
}

/// End of the spaces/tabs-only prefix starting at `from` (stops at newline).
fn spaces_run_end(src: &[u8], from: u32, ceil: u32) -> u32 {
    let mut end = from as usize;
    let ceil = ceil as usize;
    while end < ceil && src.get(end).is_some_and(|&b| b == b' ' || b == b'\t') {
        end += 1;
    }
    end as u32
}

/// Start of the spaces/tabs-only suffix ending at `to` (stops at newline).
fn spaces_run_start(src: &[u8], to: u32, floor: u32) -> u32 {
    let mut start = to as usize;
    let floor = floor as usize;
    while start > floor && src.get(start - 1).is_some_and(|&b| b == b' ' || b == b'\t') {
        start -= 1;
    }
    start as u32
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
    let Some(next) = tokens.iter().find(|t| t.range.start >= left.range.end) else {
        return false;
    };
    // When the next token IS the newline itself (`[\n`), the gap to it is
    // empty but the bracket still starts a multiline body (RuboCop's
    // `next_to_newline?` checks the next token's line, not the gap).
    if matches!(
        next.kind,
        SourceTokenKind::Newline | SourceTokenKind::IgnoredNewline
    ) {
        return true;
    }
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
        let result = run_cop_with_options_and_edits::<SpaceInsideArrayLiteralBrackets>(
            "a = [1, 2]\n",
            &opts,
        );
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

    // ── compact style ───────────────────────────────────────────────────────

    fn compact_opts() -> SpaceInsideArrayLiteralBracketsOptions {
        SpaceInsideArrayLiteralBracketsOptions {
            enforced_style: ArrayBracketStyle::Compact,
            empty_style: EmptyArrayBracketStyle::NoSpace,
        }
    }

    fn apply_edits(src: &str, edits: &[murphy_plugin_api::test_support::CapturedEdit]) -> String {
        let mut ordered: Vec<_> = edits.iter().collect();
        ordered.sort_by_key(|b| std::cmp::Reverse(b.range.start));
        let mut out = src.to_owned();
        for e in ordered {
            out.replace_range(e.range.start as usize..e.range.end as usize, &e.replacement);
        }
        out
    }

    #[test]
    fn compact_style_requires_spaces_for_plain_array() {
        // Like `space`: `[1, 2]` needs spaces on both sides.
        let opts = compact_opts();
        let src = "a = [1, 2]\n";
        let result = run_cop_with_options_and_edits::<SpaceInsideArrayLiteralBrackets>(src, &opts);
        assert_eq!(result.offenses.len(), 2, "offenses: {:?}", result.offenses);
        assert!(
            result
                .offenses
                .iter()
                .all(|o| o.message == "Use space inside array brackets."),
            "offenses: {:?}",
            result.offenses
        );
        assert_eq!(apply_edits(src, &result.edits), "a = [ 1, 2 ]\n");
    }

    #[test]
    fn compact_style_accepts_spaced_array() {
        test::<SpaceInsideArrayLiteralBrackets>()
            .with_options(&compact_opts())
            .expect_no_offenses("a = [ 1, 2, 3 ]\n");
    }

    #[test]
    fn compact_style_accepts_valid_2d_array() {
        // RuboCop: `[ 1, [ 2,3,4 ], [ 5,6,7 ]]` — outer collapses on the right.
        test::<SpaceInsideArrayLiteralBrackets>()
            .with_options(&compact_opts())
            .expect_no_offenses("a = [ 1, [ 2, 3 ], [ 4, 5 ]]\n");
    }

    #[test]
    fn compact_style_accepts_valid_3d_array() {
        test::<SpaceInsideArrayLiteralBrackets>()
            .with_options(&compact_opts())
            .expect_no_offenses("a = [[ 2, 3, [ 4 ]]]\n");
    }

    #[test]
    fn compact_style_accepts_valid_4d_array() {
        test::<SpaceInsideArrayLiteralBrackets>()
            .with_options(&compact_opts())
            .expect_no_offenses("a = [[[[ boom ]]]]\n");
    }

    #[test]
    fn compact_style_collapses_space_between_closing_brackets() {
        let opts = compact_opts();
        let src = "a = [ 1, [ 2, 3 ], [ 4, 5 ] ]\n";
        let result = run_cop_with_options_and_edits::<SpaceInsideArrayLiteralBrackets>(src, &opts);
        assert!(
            result
                .offenses
                .iter()
                .any(|o| o.message == "Do not use space inside array brackets."),
            "expected collapse offense, got {:?}",
            result.offenses
        );
        assert_eq!(
            apply_edits(src, &result.edits),
            "a = [ 1, [ 2, 3 ], [ 4, 5 ]]\n"
        );
    }

    #[test]
    fn compact_style_collapses_space_between_opening_brackets() {
        let opts = compact_opts();
        let src = "a = [ [ 2, 3 ], [ 4, 5 ], 6 ]\n";
        let result = run_cop_with_options_and_edits::<SpaceInsideArrayLiteralBrackets>(src, &opts);
        assert!(
            result
                .offenses
                .iter()
                .any(|o| o.message == "Do not use space inside array brackets."),
            "expected collapse offense, got {:?}",
            result.offenses
        );
        assert_eq!(
            apply_edits(src, &result.edits),
            "a = [[ 2, 3 ], [ 4, 5 ], 6 ]\n"
        );
    }

    #[test]
    fn compact_style_accepts_multiline_collapsed() {
        test::<SpaceInsideArrayLiteralBrackets>()
            .with_options(&compact_opts())
            .expect_no_offenses(indoc! {r#"
                array = [[ a ],
                  [ b, c ]]
            "#});
    }

    #[test]
    fn compact_style_collapses_space_after_outer_left() {
        // `multiline = [ [ 1, ...` → `[[ 1, ...`
        let opts = compact_opts();
        let src = "multiline = [ [ 1, 2 ],\n  [ 3, 4 ]]\n";
        let result = run_cop_with_options_and_edits::<SpaceInsideArrayLiteralBrackets>(src, &opts);
        assert!(
            result
                .offenses
                .iter()
                .any(|o| o.message == "Do not use space inside array brackets."),
            "expected collapse offense, got {:?}",
            result.offenses
        );
        assert_eq!(
            apply_edits(src, &result.edits),
            "multiline = [[ 1, 2 ],\n  [ 3, 4 ]]\n"
        );
    }

    #[test]
    fn compact_style_collapses_space_before_outer_right() {
        let opts = compact_opts();
        let src = "multiline = [[ 1, 2 ],\n  [ 3, 4 ] ]\n";
        let result = run_cop_with_options_and_edits::<SpaceInsideArrayLiteralBrackets>(src, &opts);
        assert!(
            result
                .offenses
                .iter()
                .any(|o| o.message == "Do not use space inside array brackets."),
            "expected collapse offense, got {:?}",
            result.offenses
        );
        assert_eq!(
            apply_edits(src, &result.edits),
            "multiline = [[ 1, 2 ],\n  [ 3, 4 ]]\n"
        );
    }

    #[test]
    fn compact_style_collapses_newline_after_outer_left() {
        // RuboCop: `multiline = [\n  [ 1, ...` → `[[ 1, ...`
        let opts = compact_opts();
        let src = "multiline = [\n  [ 1, 2 ],\n  [ 3, 4 ]]\n";
        let result = run_cop_with_options_and_edits::<SpaceInsideArrayLiteralBrackets>(src, &opts);
        assert!(
            result
                .offenses
                .iter()
                .any(|o| o.message == "Do not use space inside array brackets."),
            "expected newline-collapse offense, got {:?}",
            result.offenses
        );
        assert_eq!(
            apply_edits(src, &result.edits),
            "multiline = [[ 1, 2 ],\n  [ 3, 4 ]]\n"
        );
    }

    #[test]
    fn compact_style_collapses_newline_before_outer_right() {
        let opts = compact_opts();
        let src = "multiline = [[ 1, 2 ],\n  [ 3, 4 ]\n]\n";
        let result = run_cop_with_options_and_edits::<SpaceInsideArrayLiteralBrackets>(src, &opts);
        assert!(
            result
                .offenses
                .iter()
                .any(|o| o.message == "Do not use space inside array brackets."),
            "expected newline-collapse offense, got {:?}",
            result.offenses
        );
        assert_eq!(
            apply_edits(src, &result.edits),
            "multiline = [[ 1, 2 ],\n  [ 3, 4 ]]\n"
        );
    }

    #[test]
    fn compact_style_corrects_2d_with_extra_spaces() {
        let opts = compact_opts();
        let src = "a = [ [ a, b ], [ 1, 7 ] ]\n";
        let result = run_cop_with_options_and_edits::<SpaceInsideArrayLiteralBrackets>(src, &opts);
        assert_eq!(
            apply_edits(src, &result.edits),
            "a = [[ a, b ], [ 1, 7 ]]\n",
            "offenses: {:?}",
            result.offenses
        );
        assert!(
            result
                .offenses
                .iter()
                .all(|o| o.message == "Do not use space inside array brackets."),
            "offenses: {:?}",
            result.offenses
        );
    }

    #[test]
    fn compact_style_corrects_3d_with_extra_spaces() {
        // RuboCop: `[ [a, b ], [foo, [bar, baz] ] ]` →
        //          `[[ a, b ], [ foo, [ bar, baz ]]]`
        let opts = compact_opts();
        let src = "a = [ [a, b ], [foo, [bar, baz] ] ]\n";
        let result = run_cop_with_options_and_edits::<SpaceInsideArrayLiteralBrackets>(src, &opts);
        assert_eq!(
            apply_edits(src, &result.edits),
            "a = [[ a, b ], [ foo, [ bar, baz ]]]\n",
            "offenses: {:?}",
            result.offenses
        );
    }

    #[test]
    fn compact_style_accepts_plain_multiline() {
        // Non-nested multiline still exempt like `space`.
        test::<SpaceInsideArrayLiteralBrackets>()
            .with_options(&compact_opts())
            .expect_no_offenses(indoc! {r#"
                stuff = [
                  a,
                  b
                ]
            "#});
    }

    #[test]
    fn compact_style_still_governs_empty_brackets() {
        // Empty-bracket handling applies under compact too (RuboCop: compact
        // governs only non-empty arrays). With no_space empty style, `[ ]` is
        // still flagged.
        let opts = SpaceInsideArrayLiteralBracketsOptions {
            enforced_style: ArrayBracketStyle::Compact,
            empty_style: EmptyArrayBracketStyle::NoSpace,
        };
        test::<SpaceInsideArrayLiteralBrackets>()
            .with_options(&opts)
            .expect_correction(
                indoc! {r#"
                    a = [ ]
                        ^^^ Do not use space inside empty array brackets.
                "#},
                "a = []\n",
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
