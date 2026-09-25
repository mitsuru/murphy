//! `Layout/SpaceAfterComma` — require a space after every comma.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/SpaceAfterComma
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Token-stream port of RuboCop's `SpaceAfterPunctuation` mixin specialized
//!   for commas. Fires only when a `,` is immediately followed (zero gap, same
//!   line) by a non-allowed token. RuboCop's `allowed_type?` exempts `)`, `]`,
//!   `|`, and `tSTRING_DEND` (interpolation-end `}`, an `Other "}"` token in
//!   Murphy, always exempt); a regular `}` (`RightBrace`, `tRCURLY`) additionally
//!   needs the sibling `Layout/SpaceInsideHashLiteralBraces.EnforcedStyle ==
//!   "no_space"` check (`space_forbidden_before_rcurly?`). The sibling style is
//!   read dynamically via `Cx::hash_literal_braces_space()` (murphy-ilrx,
//!   murphy-bgd8 pattern: host resolves `for_cop` into `AllCopsContext` and
//!   threads it into `CxRaw` without a numeric ABI bump; `compact` counts as
//!   space-required, only `no_space` exempts).
//! ```

use murphy_plugin_api::{Cx, NoOptions, Range, SourceToken, SourceTokenKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SpaceAfterComma;

#[cop(
    name = "Layout/SpaceAfterComma",
    description = "Use spaces after commas.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl SpaceAfterComma {
    #[on_new_investigation]
    fn investigate(&self, cx: &Cx<'_>) {
        // RuboCop iterates the Parser token stream pairwise. Murphy's stream
        // additionally carries Newline/IgnoredNewline/Comment tokens that the
        // Parser stream omits, so skip those when looking for the token that
        // follows a comma; otherwise `,\n` and `,#comment` would be treated as
        // "comma directly followed by something" and mis-fire. A single
        // "previous significant comma" cursor avoids allocating a filtered Vec.
        let mut prev_comma: Option<SourceToken> = None;
        for &tok in cx.sorted_tokens() {
            if matches!(
                tok.kind,
                SourceTokenKind::Newline
                    | SourceTokenKind::IgnoredNewline
                    | SourceTokenKind::Comment
            ) {
                // Insignificant tokens neither start nor satisfy a comma pair.
                continue;
            }

            if let Some(comma) = prev_comma.take() {
                check_comma_pair(cx, comma, tok);
            }

            if tok.kind == SourceTokenKind::Comma {
                prev_comma = Some(tok);
            }
        }
    }
}

fn check_comma_pair(cx: &Cx<'_>, comma: SourceToken, next: SourceToken) {
    // RuboCop's `kind`: the token after a comma must not be a `;`.
    if cx.raw_source(next.range) == ";" {
        return;
    }
    // `space_missing?`: same line AND directly adjacent (zero gap).
    if comma.range.end != next.range.start {
        return;
    }
    // `space_required_before?`: `!(allowed_type? || (right_curly && no_space))`.
    // `allowed_type?` (`tRPAREN`/`tRBRACK`/`tPIPE`/`tSTRING_DEND`) is handled by
    // `is_allowed_after_comma` (the `tSTRING_DEND` interpolation-end `}` is an
    // `Other "}"` token and is always exempt). A regular `}` (`RightBrace`,
    // `tRCURLY`) additionally needs the sibling
    // `Layout/SpaceInsideHashLiteralBraces.EnforcedStyle == "no_space"` check:
    // read dynamically via `Cx::hash_literal_braces_space()` (murphy-ilrx,
    // murphy-bgd8 pattern).
    if is_allowed_after_comma(cx, next) {
        return;
    }
    if next.kind == SourceTokenKind::RightBrace
        && cx.space_forbidden_before_rcurly_for_comma()
    {
        return;
    }

    cx.emit_offense(comma.range, "Space missing after comma.", None);
    cx.emit_edit(
        Range {
            start: comma.range.end,
            end: comma.range.end,
        },
        " ",
    );
}

/// Mirror of RuboCop `SpaceAfterPunctuation#allowed_type?`: a space is not
/// required when the following token is `)`, `]`, `|`, or string-interpolation
/// end `}` (`tSTRING_DEND`, an `Other "}"` token in Murphy). A regular
/// hash/block `}` (`RightBrace`, `tRCURLY`) is NOT in `allowed_type?` — it is
/// gated by the sibling `no_space` style in `check_comma_pair`.
fn is_allowed_after_comma(cx: &Cx<'_>, next: SourceToken) -> bool {
    if next.kind == SourceTokenKind::RightParen {
        return true;
    }
    if next.kind == SourceTokenKind::Other && cx.raw_source(next.range) == "}" {
        return true;
    }
    matches!(cx.raw_source(next.range), "]" | "|")
}

murphy_plugin_api::submit_cop!(SpaceAfterComma);

#[cfg(test)]
mod tests {
    use super::SpaceAfterComma;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_missing_space_after_comma_in_array() {
        test::<SpaceAfterComma>().expect_correction(
            indoc! {r#"
                [1,2]
                  ^ Space missing after comma.
            "#},
            "[1, 2]\n",
        );
    }

    #[test]
    fn flags_missing_space_after_comma_in_call() {
        test::<SpaceAfterComma>().expect_correction(
            indoc! {r#"
                foo(1,2)
                     ^ Space missing after comma.
            "#},
            "foo(1, 2)\n",
        );
    }

    #[test]
    fn accepts_space_after_comma() {
        test::<SpaceAfterComma>().expect_no_offenses("[1, 2]\nfoo(1, 2)\n");
    }

    #[test]
    fn accepts_comma_before_closing_paren() {
        // Trailing comma directly before `)` is exempt (allowed_type?).
        test::<SpaceAfterComma>().expect_no_offenses("foo(1,)\n");
    }

    #[test]
    fn accepts_comma_before_closing_bracket() {
        test::<SpaceAfterComma>().expect_no_offenses("[1,]\n");
    }

    #[test]
    fn accepts_comma_before_newline() {
        // Multiline trailing comma must not fire.
        test::<SpaceAfterComma>().expect_no_offenses("[\n  1,\n  2,\n]\n");
    }

    #[test]
    fn flags_multiple_commas() {
        test::<SpaceAfterComma>().expect_correction(
            indoc! {r#"
                [1,2,3]
                  ^ Space missing after comma.
                    ^ Space missing after comma.
            "#},
            "[1, 2, 3]\n",
        );
    }

    #[test]
    fn flags_missing_space_after_comma_in_hash() {
        test::<SpaceAfterComma>().expect_correction(
            indoc! {r#"
                { a: 1,b: 2 }
                      ^ Space missing after comma.
            "#},
            "{ a: 1, b: 2 }\n",
        );
    }

    // ── RuboCop spec parity ───────────────────────────────────────────────────

    /// RuboCop parity: block parameter commas — `each { |s,t| }`.
    #[test]
    fn flags_missing_space_after_comma_in_block_args() {
        test::<SpaceAfterComma>().expect_correction(
            indoc! {r#"
                each { |s,t| }
                         ^ Space missing after comma.
            "#},
            "each { |s, t| }\n",
        );
    }

    /// RuboCop parity: array index commas — `formats[0,1]`.
    #[test]
    fn flags_missing_space_after_comma_in_index() {
        test::<SpaceAfterComma>().expect_correction(
            indoc! {r#"
                formats[0,1]
                         ^ Space missing after comma.
            "#},
            "formats[0, 1]\n",
        );
    }

    /// RuboCop parity: trailing comma before `]` in an index is exempt.
    #[test]
    fn accepts_trailing_comma_in_index() {
        test::<SpaceAfterComma>().expect_no_offenses("formats[0,]\n");
    }

    /// RuboCop parity: trailing comma before `|` in block args is exempt.
    #[test]
    fn accepts_trailing_comma_in_block_args() {
        test::<SpaceAfterComma>().expect_no_offenses("each { |s, t,| }\n");
    }

    /// RuboCop parity (default `space` config): a comma directly before `}`
    /// requires a space, since `Layout/SpaceInsideHashLiteralBraces` defaults to
    /// `space`. `{ foo: bar,}` → `{ foo: bar, }`.
    #[test]
    fn flags_comma_before_closing_brace() {
        test::<SpaceAfterComma>().expect_correction(
            indoc! {r#"
                { foo: bar,}
                          ^ Space missing after comma.
            "#},
            "{ foo: bar, }\n",
        );
    }

    /// RuboCop parity: a properly spaced trailing comma before `}` is accepted.
    #[test]
    fn accepts_spaced_trailing_comma_before_brace() {
        test::<SpaceAfterComma>().expect_no_offenses("{ foo: bar, }\n");
    }

    // ── murphy-ilrx: dynamic Layout/SpaceInsideHashLiteralBraces.EnforcedStyle ──

    #[test]
    fn accepts_comma_before_brace_under_no_space_sibling_style() {
        // `EnforcedStyle: no_space` sibling style: `,}` needs no space
        // (`space_forbidden_before_rcurly?`), so no offense.
        test::<SpaceAfterComma>()
            .with_hash_literal_braces_space(false)
            .expect_no_offenses("{ foo: bar,}\n");
    }

    #[test]
    fn flags_comma_before_brace_with_explicit_space_sibling_style() {
        // Explicit `space` sibling style: `,}` requires the space.
        test::<SpaceAfterComma>()
            .with_hash_literal_braces_space(true)
            .expect_correction(
                indoc! {r#"
                    { foo: bar,}
                              ^ Space missing after comma.
                "#},
                "{ foo: bar, }\n",
            );
    }

    #[test]
    fn accepts_spaced_trailing_comma_before_brace_under_no_space_style() {
        // Spaced `, }` is accepted in both sibling styles.
        test::<SpaceAfterComma>()
            .with_hash_literal_braces_space(false)
            .expect_no_offenses("{ foo: bar, }\n");
    }

    #[test]
    fn flags_normal_missing_space_regardless_of_sibling_style() {
        // Non-`}` gaps flag in both sibling styles — the flag only gates the
        // `}`-exemption.
        test::<SpaceAfterComma>()
            .with_hash_literal_braces_space(false)
            .expect_correction(
                indoc! {r#"
                    [1,2]
                      ^ Space missing after comma.
                "#},
                "[1, 2]\n",
            );
    }
}
