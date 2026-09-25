//! `Layout/SpaceAfterSemicolon` — flags a semicolon (`;`) that is not
//! followed by whitespace and autocorrects by inserting a single space.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/SpaceAfterSemicolon
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's `SpaceAfterPunctuation` mixin for the semicolon token:
//!   an offense fires when `;` is immediately followed (same line, adjacent
//!   column) by a non-allowed token, with the autocorrect inserting one space.
//!   Allowed following tokens (no offense) match RuboCop's `allowed_type?`
//!   (`tRPAREN`/`tRBRACK`/`tPIPE`/`tSTRING_DEND`): `)`, `]`, `|`, and the
//!   string-interpolation-end `}` of `#{...}`. A regular hash/block `}` is NOT
//!   in `allowed_type?`, so `;}` IS flagged under RuboCop's default
//!   (`SpaceInsideBlockBraces: space`). A run of semicolons (`;;`) is accepted
//!   between the pair, mirroring `semicolon_sequence?`.
//!
//!   Cross-cop `space_required_before?` (murphy-rwi3, murphy-ilrx, option 2,
//!   no ABI bump): RuboCop suppresses the regular-`}` offense when
//!   `Layout/SpaceInsideBlockBraces` is `EnforcedStyle: no_space`
//!   (`space_style_before_rcurly` / `space_forbidden_before_rcurly?`). The host
//!   bakes that sibling style into this cop's `options_json` as
//!   `SpaceInsideBlockBracesEnforcedStyle` (`config.cop_options_json`
//!   contract); the cop reads it via `SpaceAfterSemicolonOptions` and exempts
//!   a regular `}` (`RightBrace`) only under `no_space`. Default (`{}` when
//!   unconfigured) is `space`, preserving the default-config behavior.
//! ```
//!
//! ## Matched shape
//!
//! - `x = 1;y = 2` — `;` directly abutting the next token on the same line.
//!
//! ## Accepted (no offense)
//!
//! - `x = 1; y = 2` — already followed by a space.
//! - `x = 1;` at end of line — next token is a newline (different line).
//! - `;;` — consecutive semicolons (`semicolon_sequence?`).
//! - `;)`, `;]`, `;|` — next token is a closing paren / bracket / pipe.
//! - `";#{x;}"` — next token is the interpolation-end `}` (`tSTRING_DEND`).
//!   A *regular* hash/block `}` (`;}`) IS flagged under the default
//!   (`SpaceInsideBlockBraces: space`) and accepted only when that sibling
//!   cop is `EnforcedStyle: no_space` (host-baked, murphy-ilrx).
//!
//! ## Autocorrect
//!
//! Inserts a single space immediately after the semicolon.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, Range, SourceToken, SourceTokenKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SpaceAfterSemicolon;

/// Host-baked sibling style for [`SpaceAfterSemicolon`].
///
/// The host resolves `Layout/SpaceInsideBlockBraces.EnforcedStyle`
/// (`for_cop`, fallback `"space"`) and bakes it into this cop's
/// `options_json` as `SpaceInsideBlockBracesEnforcedStyle` (murphy-ilrx,
/// option 2 — no `CxRaw` change, no ABI bump). `space` (default) requires a
/// space before a regular `}` (`;}` is flagged); `no_space` exempts it.
#[derive(CopOptions)]
pub struct SpaceAfterSemicolonOptions {
    #[option(
        name = "SpaceInsideBlockBracesEnforcedStyle",
        default = "space",
        description = "Baked Layout/SpaceInsideBlockBraces EnforcedStyle; no_space exempts `;}`."
    )]
    pub space_inside_block_braces_enforced_style: BlockBracesSiblingStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlockBracesSiblingStyle {
    #[option(value = "space")]
    Space,
    #[option(value = "no_space")]
    NoSpace,
}

#[cop(
    name = "Layout/SpaceAfterSemicolon",
    description = "Flag a semicolon that is not followed by a space.",
    default_severity = "warning",
    default_enabled = true,
    options = SpaceAfterSemicolonOptions,
)]
impl SpaceAfterSemicolon {
    #[on_new_investigation]
    fn investigate(&self, cx: &Cx<'_>) {
        let sibling_no_space = cx
            .options_or_default::<SpaceAfterSemicolonOptions>()
            .space_inside_block_braces_enforced_style
            == BlockBracesSiblingStyle::NoSpace;
        let toks = cx.sorted_tokens();
        // A `;` that is the entire content of a string/symbol literal is an
        // `Other` `b";"` token but not a separator; skip those (see
        // `string_literal_content_ranges`).
        let literal_ranges = crate::cops::util::string_literal_content_ranges(cx);
        for pair in toks.windows(2) {
            let token1 = pair[0];
            let token2 = pair[1];

            // token1 must be a `;` (a `SourceTokenKind::Other` whose source
            // text is exactly `;`).
            if !is_semicolon(cx, token1) {
                continue;
            }

            if crate::cops::util::offset_within_any(token1.range.start, &literal_ranges) {
                continue;
            }

            // `semicolon_sequence?` — a run of semicolons is accepted.
            if is_semicolon(cx, token2) {
                continue;
            }

            // Different line → `same_line?` is false in RuboCop. The next
            // token being a (ignored) newline means the `;` ends the line.
            if matches!(
                token2.kind,
                SourceTokenKind::Newline | SourceTokenKind::IgnoredNewline
            ) {
                continue;
            }

            // `space_missing?` — adjacency means no whitespace between them.
            if token1.range.end != token2.range.start {
                continue;
            }

            // `space_required_before?` — closing delimiters / pipe are allowed
            // to abut a semicolon; a regular `}` is additionally allowed when
            // the baked sibling style is `no_space` (murphy-ilrx).
            if allowed_following(cx, token2, sibling_no_space) {
                continue;
            }

            cx.emit_offense(token1.range, "Space missing after semicolon.", None);
            cx.emit_edit(
                Range {
                    start: token1.range.end,
                    end: token1.range.end,
                },
                " ",
            );
        }
    }
}

/// `true` when `token` is a `;` token (`Other` kind, source text `;`).
fn is_semicolon(cx: &Cx<'_>, token: SourceToken) -> bool {
    token.kind == SourceTokenKind::Other && cx.raw_source(token.range) == ";"
}

/// Mirrors RuboCop's `space_required_before?`: `allowed_type?`
/// (`%i[tRPAREN tRBRACK tPIPE tSTRING_DEND]`) plus the sibling-gated rcurly
/// exemption (`token.right_curly_brace? && space_forbidden_before_rcurly?`).
/// A following `)`, `]`, `|`, or string-interpolation-end `}` (`#{...}`) is
/// always allowed. A *regular* hash/block `}` (`RightBrace`) is allowed only
/// when the baked `SpaceInsideBlockBraces` style is `no_space`; under the
/// default (`space`) `;}` IS flagged. The interpolation-end `}` tokenizes as
/// `Other` (per token-api.md), so the `Other => "}"` arm catches exactly
/// that case while `RightBrace` goes through the sibling gate.
fn allowed_following(cx: &Cx<'_>, token: SourceToken, sibling_no_space: bool) -> bool {
    match token.kind {
        SourceTokenKind::RightParen => true,
        SourceTokenKind::RightBrace => sibling_no_space,
        SourceTokenKind::Other => {
            matches!(cx.raw_source(token.range), "]" | "|" | "}")
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BlockBracesSiblingStyle, SpaceAfterSemicolon, SpaceAfterSemicolonOptions,
    };
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_and_corrects_missing_space_after_semicolon() {
        test::<SpaceAfterSemicolon>()
            .expect_offense(indoc! {r#"
                x = 1;y = 2
                     ^ Space missing after semicolon.
            "#})
            .expect_correction(
                indoc! {r#"
                    x = 1;y = 2
                         ^ Space missing after semicolon.
                "#},
                "x = 1; y = 2\n",
            );
    }

    #[test]
    fn accepts_semicolon_followed_by_space() {
        test::<SpaceAfterSemicolon>().expect_no_offenses("x = 1; y = 2\n");
    }

    #[test]
    fn ignores_semicolon_that_is_string_literal_content() {
        // `';'` / `';'` inside `join` are literal text, not statement
        // separators — the lexer emits no `tSEMI` there.
        test::<SpaceAfterSemicolon>().expect_no_offenses("x = ';'\n");
        test::<SpaceAfterSemicolon>().expect_no_offenses("y = [1].join(';')\n");
        test::<SpaceAfterSemicolon>().expect_no_offenses("z = \"a=#{b};#{c}\"\n");
    }

    #[test]
    fn ignores_semicolon_that_is_symbol_literal_content() {
        // `:';'` is a symbol whose content is `;`; `string_literal_content_ranges`
        // covers `Sym`, so no missing-space-after-semicolon offense fires.
        test::<SpaceAfterSemicolon>().expect_no_offenses("x = :';'\n");
    }

    #[test]
    fn ignores_semicolon_when_whole_file_is_a_literal() {
        // The whole program is a bare `;`-bearing literal (the root node itself
        // is the `Str`/`Sym`), so its content range must still be covered.
        test::<SpaceAfterSemicolon>().expect_no_offenses("';'\n");
        test::<SpaceAfterSemicolon>().expect_no_offenses(":';'\n");
    }

    #[test]
    fn accepts_semicolon_at_end_of_line() {
        test::<SpaceAfterSemicolon>().expect_no_offenses("x = 1;\ny = 2\n");
    }

    #[test]
    fn accepts_consecutive_semicolons() {
        // `semicolon_sequence?` — `;;` is accepted; only the trailing one is a
        // real offense candidate. Here the run ends the line so no offense.
        test::<SpaceAfterSemicolon>().expect_no_offenses("x = 1;;\n");
    }

    #[test]
    fn flags_only_last_in_run_when_followed_by_code() {
        test::<SpaceAfterSemicolon>().expect_offense(indoc! {r#"
            x = 1;;y = 2
                  ^ Space missing after semicolon.
        "#});
    }

    #[test]
    fn accepts_semicolon_before_closing_paren() {
        test::<SpaceAfterSemicolon>().expect_no_offenses("foo(1;)\n");
    }

    #[test]
    fn flags_semicolon_before_regular_brace() {
        // A regular hash/block `}` is NOT in RuboCop's `allowed_type?`; under
        // the default `SpaceInsideBlockBraces: space` style, `;}` is flagged.
        test::<SpaceAfterSemicolon>().expect_offense(indoc! {r#"
            foo { x = 1;}
                       ^ Space missing after semicolon.
        "#});
    }

    #[test]
    fn accepts_semicolon_before_interpolation_end_brace() {
        // The interpolation-end `}` (`tSTRING_DEND`) IS in `allowed_type?`.
        test::<SpaceAfterSemicolon>().expect_no_offenses("\"a#{x;}\"\n");
    }

    #[test]
    fn accepts_semicolon_before_closing_bracket() {
        test::<SpaceAfterSemicolon>().expect_no_offenses("[1;]\n");
    }

    #[test]
    fn flags_multiple_semicolons_on_one_line() {
        test::<SpaceAfterSemicolon>().expect_offense(indoc! {r#"
            a = 1;b = 2;c = 3
                 ^ Space missing after semicolon.
                       ^ Space missing after semicolon.
        "#});
    }

    #[test]
    fn corrects_multiple_semicolons_idempotently() {
        test::<SpaceAfterSemicolon>().expect_correction(
            indoc! {r#"
                a = 1;b = 2;c = 3
                     ^ Space missing after semicolon.
                           ^ Space missing after semicolon.
            "#},
            "a = 1; b = 2; c = 3\n",
        );
    }

    #[test]
    fn leaves_clean_program_without_corrections() {
        test::<SpaceAfterSemicolon>().expect_no_corrections("a = 1; b = 2\n");
    }

    // ── murphy-ilrx: host-baked Layout/SpaceInsideBlockBraces.EnforcedStyle ──

    #[test]
    fn options_default_is_space() {
        let d = SpaceAfterSemicolonOptions::default();
        assert_eq!(
            d.space_inside_block_braces_enforced_style,
            BlockBracesSiblingStyle::Space
        );
    }

    #[test]
    fn accepts_semicolon_before_regular_brace_under_no_space_sibling_style() {
        // RuboCop `space_forbidden_before_rcurly?`: `;}` is exempt when the
        // sibling cop is `EnforcedStyle: no_space` (host-baked).
        test::<SpaceAfterSemicolon>()
            .with_options(&SpaceAfterSemicolonOptions {
                space_inside_block_braces_enforced_style: BlockBracesSiblingStyle::NoSpace,
            })
            .expect_no_offenses("foo { x = 1;}\n");
    }

    #[test]
    fn flags_semicolon_before_regular_brace_under_explicit_space_style() {
        // Explicit `space` behaves like the default: `;}` is flagged.
        test::<SpaceAfterSemicolon>()
            .with_options(&SpaceAfterSemicolonOptions {
                space_inside_block_braces_enforced_style: BlockBracesSiblingStyle::Space,
            })
            .expect_offense(indoc! {r#"
                foo { x = 1;}
                           ^ Space missing after semicolon.
            "#});
    }

    #[test]
    fn no_space_style_still_flags_ordinary_missing_space() {
        // The sibling gate only affects a regular `}`; `;y` still fires.
        test::<SpaceAfterSemicolon>()
            .with_options(&SpaceAfterSemicolonOptions {
                space_inside_block_braces_enforced_style: BlockBracesSiblingStyle::NoSpace,
            })
            .expect_offense(indoc! {r#"
                x = 1;y = 2
                     ^ Space missing after semicolon.
            "#});
    }
}

murphy_plugin_api::submit_cop!(SpaceAfterSemicolon);
