//! `Layout/SpaceBeforeBlockBraces` — checks that a brace block's opening `{`
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/SpaceBeforeBlockBraces
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues:
//!   - murphy-vwjm
//! notes: >
//!   Ports RuboCop's `on_block` (and `on_numblock`/`on_itblock` aliases).
//!   `EnforcedStyle` (space|no_space) governs non-empty braces;
//!   `EnforcedStyleForEmptyBraces` (space|no_space) governs `{}`. Brace
//!   blocks only — `do`/`end` blocks are skipped (RuboCop's `keywords?`
//!   guard). The `{` is located via the token stream, searching from the
//!   block's call end so a brace argument (`foo({}) { }`) is not mistaken
//!   for the block opener.
//!
//!   REMAINING GAP (murphy-vwjm): `conflict_with_block_delimiters?` is NOT
//!   ported. RuboCop suppresses the offense when `Style/BlockDelimiters` is
//!   `line_count_based`, this cop's style is `no_space`, and the block is
//!   multiline — it reads another cop's config. Murphy's single-surface
//!   plugin ABI does not expose cross-cop config to a cop body, so that
//!   suppression is a documented divergence (no bypass per the
//!   single-surface boundary). The `config_to_allow_offenses`
//!   auto-gen-config machinery is intentionally omitted (it is not linting).
//!
//!   CLOSED (murphy-vwjm): stabby-lambda literals (`->{ }` / `-> { }`) are
//!   now flagged. They parse as a `Block` over `(lambda)`, and although
//!   Prism tokenizes the lambda-begin `{` as `SourceTokenKind::Other` (not
//!   `LeftBrace`), `find_block_braces` falls back to locating the `{` by
//!   source byte for `cx.is_lambda_literal` blocks, matching RuboCop.
//! ```
//!
//! has (style `space`) or does not have (style `no_space`) a space before it.
//! Mirrors RuboCop's same-named cop: `foo {}` vs `foo{}`.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, Range, SourceTokenKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SpaceBeforeBlockBraces;

#[derive(CopOptions)]
pub struct SpaceBeforeBlockBracesOptions {
    #[option(
        name = "EnforcedStyle",
        default = "space",
        description = "Spacing before the `{` of a non-empty brace block."
    )]
    pub enforced_style: BraceSpaceStyle,

    #[option(
        name = "EnforcedStyleForEmptyBraces",
        default = "space",
        description = "Spacing before the `{` of an empty brace block (`{}`)."
    )]
    pub enforced_style_for_empty_braces: BraceSpaceStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum BraceSpaceStyle {
    #[option(value = "space")]
    Space,
    #[option(value = "no_space")]
    NoSpace,
}

const MISSING_MSG: &str = "Space missing to the left of {.";
const DETECTED_MSG: &str = "Space detected to the left of {.";

#[cop(
    name = "Layout/SpaceBeforeBlockBraces",
    description = "Checks that the left block brace has or doesn't have space before it.",
    default_severity = "warning",
    default_enabled = true,
    options = SpaceBeforeBlockBracesOptions,
)]
impl SpaceBeforeBlockBraces {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let opts = cx.options_or_default::<SpaceBeforeBlockBracesOptions>();

    // Locate this block's opening brace. RuboCop's `keywords?` guard skips
    // `do`/`end` blocks; `find_left_brace` returns `None` for those.
    let Some((left_brace, right_brace)) = find_block_braces(node, cx) else {
        return;
    };

    // Empty braces: `{}` with no body and adjacent `{`/`}`. Mirrors RuboCop's
    // `empty_braces?` (`loc.begin.end_pos == loc.end.begin_pos`).
    let empty = left_brace.end == right_brace.start;
    let style = if empty {
        opts.enforced_style_for_empty_braces
    } else {
        opts.enforced_style
    };

    // The gap before the `{`: from the previous token's end to the brace start.
    let gap_start = prev_token_end(cx, left_brace.start);
    if gap_start > left_brace.start {
        return;
    }
    let gap = Range {
        start: gap_start,
        end: left_brace.start,
    };
    let gap_src = cx.raw_source(gap);

    // A newline in the gap means the `{` is on its own line — RuboCop's
    // `range_with_surrounding_space` does not reach across line breaks for
    // this cop's intent; treat it as "has space" so `no_space` does not fire.
    let has_space = !gap_src.is_empty();
    let crosses_line = gap_src.bytes().any(|b| b == b'\n' || b == b'\r');

    match style {
        BraceSpaceStyle::Space => {
            if !has_space {
                cx.emit_offense(left_brace, MISSING_MSG, None);
                cx.emit_edit(
                    Range {
                        start: left_brace.start,
                        end: left_brace.start,
                    },
                    " ",
                );
            }
        }
        BraceSpaceStyle::NoSpace => {
            if has_space && !crosses_line {
                cx.emit_offense(gap, DETECTED_MSG, None);
                cx.emit_edit(gap, "");
            }
        }
    }
}

/// Find this block's opening `{` and closing `}` ranges. Returns `None` for a
/// `do`/`end` block (no braces — RuboCop's `keywords?` guard) and for a block
/// that does not actually end in a brace.
///
/// The block's closing `}` is the `RightBrace` token ending at the block's
/// `node_end`; its matching `{` is found by scanning backwards with a brace
/// depth counter. This correctly skips a brace *argument* (`foo({}) { }`),
/// because that argument's braces are fully inside the call, not the block's
/// own delimiters.
fn find_block_braces(node: NodeId, cx: &Cx<'_>) -> Option<(Range, Range)> {
    let node_range = cx.range(node);
    let toks = cx.sorted_tokens();

    // The block's `}` is the last `RightBrace` token, ending at node_end.
    let end_idx = toks.partition_point(|t| t.range.end < node_range.end);
    let close = toks.get(end_idx)?;
    if close.kind != SourceTokenKind::RightBrace || close.range.end != node_range.end {
        // do/end block (ends in `end`) or some other shape — not our concern.
        return None;
    }

    // Scan backwards from the `}` matching brace depth to find the `{`.
    let mut depth: i32 = 1;
    for tok in toks[..end_idx].iter().rev() {
        if tok.range.start < node_range.start {
            break;
        }
        match tok.kind {
            SourceTokenKind::RightBrace => depth += 1,
            SourceTokenKind::LeftBrace => {
                depth -= 1;
                if depth == 0 {
                    return Some((tok.range, close.range));
                }
            }
            _ => {}
        }
    }

    // No `LeftBrace` token matched, yet the block ends in `}`. This is a
    // stabby-lambda literal (`->{ }` / `-> (x) { }`): Prism tokenizes the
    // lambda-begin `{` as `SourceTokenKind::Other`, not `LeftBrace`, so the
    // depth scan above never sees it. RuboCop flags lambda-literal brace
    // spacing the same as any brace block (only `do`/`end` keyword blocks are
    // skipped), so locate the `{` by source byte for this shape.
    if cx.is_lambda_literal(node) {
        return lambda_literal_brace(node, close.range, cx);
    }
    None
}

/// Locate the lambda-begin `{` of a stabby-lambda literal by source byte.
///
/// The `{` follows the `->` and an optional argument list (`->(x) { }` /
/// `-> (x) { }`). The empty-arg `Args` node spans the whole block expression,
/// so it cannot bound the search; instead scan the token stream from the node
/// start, skip the leading `->`, skip a parenthesised argument list (tracking
/// `(`/`)` depth so a default-value brace inside it is not mistaken for the
/// body opener), then take the first `{`. The matching `}` is the already-
/// located `close`.
fn lambda_literal_brace(node: NodeId, close: Range, cx: &Cx<'_>) -> Option<(Range, Range)> {
    let node_range = cx.range(node);
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < node_range.start);

    let mut paren_depth: i32 = 0;
    for tok in toks[idx..]
        .iter()
        .take_while(|t| t.range.start < close.start)
    {
        match tok.kind {
            SourceTokenKind::LeftParen => paren_depth += 1,
            // Guard the decrement so a stray `)` (malformed input) cannot drive
            // the depth negative and make a legitimate body `{` invisible to
            // the `paren_depth == 0` check below.
            SourceTokenKind::RightParen if paren_depth > 0 => paren_depth -= 1,
            _ => {
                let s = tok.range.start as usize;
                if paren_depth == 0 && s < source.len() && source[s] == b'{' {
                    return Some((
                        Range {
                            start: tok.range.start,
                            end: tok.range.start + 1,
                        },
                        close,
                    ));
                }
            }
        }
    }
    None
}

/// The end offset of the token immediately before `offset`, or `0`.
fn prev_token_end(cx: &Cx<'_>, offset: u32) -> u32 {
    cx.token_before(offset).map(|t| t.range.end).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{BraceSpaceStyle, SpaceBeforeBlockBraces, SpaceBeforeBlockBracesOptions};
    use murphy_plugin_api::test_support::{indoc, run_cop_with_edits, test};

    fn no_space_opts() -> SpaceBeforeBlockBracesOptions {
        SpaceBeforeBlockBracesOptions {
            enforced_style: BraceSpaceStyle::NoSpace,
            enforced_style_for_empty_braces: BraceSpaceStyle::NoSpace,
        }
    }

    // ── default (space) style ──────────────────────────────────────────────

    #[test]
    fn options_defaults_match_rubocop() {
        let d = SpaceBeforeBlockBracesOptions::default();
        assert_eq!(d.enforced_style, BraceSpaceStyle::Space);
        assert_eq!(d.enforced_style_for_empty_braces, BraceSpaceStyle::Space);
    }

    #[test]
    fn space_style_flags_missing_space() {
        let result = run_cop_with_edits::<SpaceBeforeBlockBraces>("foo{ bar }\n");
        assert_eq!(
            result.offenses.len(),
            1,
            "expected 1 offense, got {:?}",
            result.offenses
        );
        assert_eq!(result.offenses[0].message, "Space missing to the left of {.");
        assert_eq!(result.edits[0].replacement, " ");
    }

    #[test]
    fn space_style_corrects_missing_space() {
        test::<SpaceBeforeBlockBraces>().expect_correction(
            indoc! {r#"
                foo{ bar }
                   ^ Space missing to the left of {.
            "#},
            "foo { bar }\n",
        );
    }

    #[test]
    fn space_style_accepts_space() {
        test::<SpaceBeforeBlockBraces>().expect_no_offenses("foo { bar }\n");
    }

    #[test]
    fn space_style_accepts_space_with_block_args() {
        test::<SpaceBeforeBlockBraces>().expect_no_offenses("foo { |x| bar(x) }\n");
    }

    #[test]
    fn space_style_flags_missing_space_with_block_args() {
        test::<SpaceBeforeBlockBraces>().expect_offense(indoc! {r#"
            foo{ |x| bar(x) }
               ^ Space missing to the left of {.
        "#});
    }

    // ── do/end blocks are skipped ──────────────────────────────────────────

    #[test]
    fn ignores_do_end_blocks() {
        test::<SpaceBeforeBlockBraces>().expect_no_offenses(indoc! {r#"
            foo do
              bar
            end
        "#});
    }

    // ── hash-literal argument is not the block brace ───────────────────────

    #[test]
    fn does_not_confuse_brace_argument_with_block_brace() {
        test::<SpaceBeforeBlockBraces>().expect_no_offenses("foo({}) { x }\n");
    }

    #[test]
    fn flags_missing_space_even_with_brace_argument() {
        test::<SpaceBeforeBlockBraces>().expect_offense(indoc! {r#"
            foo({}){ x }
                   ^ Space missing to the left of {.
        "#});
    }

    // ── empty braces (EnforcedStyleForEmptyBraces) ─────────────────────────

    #[test]
    fn empty_braces_space_style_flags_missing_space() {
        test::<SpaceBeforeBlockBraces>().expect_offense(indoc! {r#"
            foo{}
               ^ Space missing to the left of {.
        "#});
    }

    #[test]
    fn empty_braces_space_style_accepts_space() {
        test::<SpaceBeforeBlockBraces>().expect_no_offenses("foo {}\n");
    }

    // ── no_space style ─────────────────────────────────────────────────────

    #[test]
    fn no_space_style_flags_space() {
        test::<SpaceBeforeBlockBraces>()
            .with_options(&no_space_opts())
            .expect_correction(
                indoc! {r#"
                    foo { bar }
                       ^ Space detected to the left of {.
                "#},
                "foo{ bar }\n",
            );
    }

    #[test]
    fn no_space_style_accepts_no_space() {
        test::<SpaceBeforeBlockBraces>()
            .with_options(&no_space_opts())
            .expect_no_offenses("foo{ bar }\n");
    }

    #[test]
    fn no_space_style_empty_braces_flags_space() {
        test::<SpaceBeforeBlockBraces>()
            .with_options(&no_space_opts())
            .expect_offense(indoc! {r#"
                foo {}
                   ^ Space detected to the left of {.
            "#});
    }

    #[test]
    fn no_space_style_empty_braces_accepts_no_space() {
        test::<SpaceBeforeBlockBraces>()
            .with_options(&no_space_opts())
            .expect_no_offenses("foo{}\n");
    }

    // ── lambda forms ───────────────────────────────────────────────────────

    #[test]
    fn flags_missing_space_for_lambda_method_form() {
        // `lambda { }` (method form) — the `{` is a real `LeftBrace`.
        test::<SpaceBeforeBlockBraces>().expect_offense(indoc! {r#"
            lambda{ x }
                  ^ Space missing to the left of {.
        "#});
    }

    #[test]
    fn flags_missing_space_for_stabby_lambda_literal() {
        // Stabby lambda `->{ }` — the lambda-begin `{` is tokenized as `Other`,
        // not `LeftBrace`, so it is located by source byte. RuboCop flags the
        // missing space before `{` exactly as for any brace block.
        test::<SpaceBeforeBlockBraces>().expect_correction(
            indoc! {r#"
                ->{ x }
                  ^ Space missing to the left of {.
            "#},
            "-> { x }\n",
        );
    }

    #[test]
    fn accepts_well_spaced_stabby_lambda_literal() {
        test::<SpaceBeforeBlockBraces>().expect_no_offenses("-> { x }\n");
    }

    #[test]
    fn flags_missing_space_for_stabby_lambda_with_args() {
        // The argument list (`->(x)`) is skipped when locating the body `{`.
        test::<SpaceBeforeBlockBraces>().expect_correction(
            indoc! {r#"
                ->(x){ x }
                     ^ Space missing to the left of {.
            "#},
            "->(x) { x }\n",
        );
    }

    #[test]
    fn accepts_well_spaced_stabby_lambda_with_args() {
        test::<SpaceBeforeBlockBraces>().expect_no_offenses("->(x) { x }\n");
    }

    #[test]
    fn no_space_style_flags_space_for_stabby_lambda_literal() {
        test::<SpaceBeforeBlockBraces>()
            .with_options(&no_space_opts())
            .expect_correction(
                indoc! {r#"
                    -> { x }
                      ^ Space detected to the left of {.
                "#},
                "->{ x }\n",
            );
    }

    // ── idempotence ────────────────────────────────────────────────────────

    #[test]
    fn space_style_leaves_clean_program() {
        test::<SpaceBeforeBlockBraces>()
            .expect_no_corrections("foo { bar }\nbaz {}\nqux { |y| y }\n");
    }
}
murphy_plugin_api::submit_cop!(SpaceBeforeBlockBraces);
