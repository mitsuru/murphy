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
//!   `|`, and `tSTRING_DEND`; Murphy's token stream only models `)` and `}`
//!   as distinct kinds (`RightParen` / `RightBrace`), so `]` and `|` are
//!   matched by source byte while the interpolation-end `}` (`Other` `"}"`)
//!   is always exempt. A regular `}` (`RightBrace`) requires a space under
//!   `Layout/SpaceInsideHashLiteralBraces`'s default (`space`), so `{ a,}`
//!   flags by default, exactly matching RuboCop's default config.
//!
//!   Cross-cop `space_required_before?` (murphy-je5n, murphy-ilrx, option 2,
//!   no ABI bump): RuboCop exempts a comma directly before `}` when
//!   `Layout/SpaceInsideHashLiteralBraces` is `EnforcedStyle: no_space`
//!   (`space_style_before_rcurly` / `space_forbidden_before_rcurly?`). The host
//!   bakes that sibling style into this cop's `options_json` as
//!   `SpaceInsideHashLiteralBracesEnforcedStyle` (`config.cop_options_json`
//!   contract); the cop reads it via `SpaceAfterCommaOptions` and exempts a
//!   regular `}` only under `no_space` (`compact` still requires a space,
//!   matching `style == 'no_space'`). Default (`{}` when unconfigured) is
//!   `space`, preserving the default-config behavior.
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, Range, SourceToken, SourceTokenKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SpaceAfterComma;

/// Host-baked sibling style for [`SpaceAfterComma`].
///
/// The host resolves `Layout/SpaceInsideHashLiteralBraces.EnforcedStyle`
/// (`for_cop`, fallback `"space"`) and bakes it into this cop's
/// `options_json` as `SpaceInsideHashLiteralBracesEnforcedStyle` (murphy-ilrx,
/// option 2 — no `CxRaw` change, no ABI bump). Only `no_space` exempts a
/// comma directly before a regular `}` (`compact` still requires a space,
/// matching RuboCop's `style == 'no_space'`).
#[derive(CopOptions)]
pub struct SpaceAfterCommaOptions {
    #[option(
        name = "SpaceInsideHashLiteralBracesEnforcedStyle",
        default = "space",
        description = "Baked Layout/SpaceInsideHashLiteralBraces EnforcedStyle; only no_space exempts `,}`."
    )]
    pub space_inside_hash_literal_braces_enforced_style: HashBraceSiblingStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum HashBraceSiblingStyle {
    #[option(value = "space")]
    Space,
    #[option(value = "no_space")]
    NoSpace,
    #[option(value = "compact")]
    Compact,
}

#[cop(
    name = "Layout/SpaceAfterComma",
    description = "Use spaces after commas.",
    default_severity = "warning",
    default_enabled = true,
    options = SpaceAfterCommaOptions,
)]
impl SpaceAfterComma {
    #[on_new_investigation]
    fn investigate(&self, cx: &Cx<'_>) {
        // Host-baked sibling style (murphy-ilrx, option 2): only `no_space`
        // exempts a comma directly before a regular `}`.
        let sibling_no_space = cx
            .options_or_default::<SpaceAfterCommaOptions>()
            .space_inside_hash_literal_braces_enforced_style
            == HashBraceSiblingStyle::NoSpace;
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
                check_comma_pair(cx, comma, tok, sibling_no_space);
            }

            if tok.kind == SourceTokenKind::Comma {
                prev_comma = Some(tok);
            }
        }
    }
}

fn check_comma_pair(cx: &Cx<'_>, comma: SourceToken, next: SourceToken, sibling_no_space: bool) {
    // RuboCop's `kind`: the token after a comma must not be a `;`.
    if cx.raw_source(next.range) == ";" {
        return;
    }
    // `space_missing?`: same line AND directly adjacent (zero gap).
    if comma.range.end != next.range.start {
        return;
    }
    // `space_required_before?`: `allowed_type?` (`)`, `]`, `|`,
    // `tSTRING_DEND`) plus the sibling-gated rcurly exemption (murphy-ilrx).
    // A regular `}` (`RightBrace`) is exempt only under `no_space`; the
    // interpolation-end `}` (`Other` `"}"`) is always exempt.
    if is_allowed_after_comma(cx, next, sibling_no_space) {
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

/// Mirror of RuboCop `space_required_before?`: `allowed_type?` plus the
/// sibling-gated rcurly exemption. A space is not required when the following
/// token is `)`, `]`, `|`, or the interpolation-end `}` (`tSTRING_DEND`,
/// `Other` `"}"`). A regular `}` (`RightBrace`) is exempt only when the
/// baked `SpaceInsideHashLiteralBraces` style is `no_space`.
fn is_allowed_after_comma(cx: &Cx<'_>, next: SourceToken, sibling_no_space: bool) -> bool {
    if next.kind == SourceTokenKind::RightParen {
        return true;
    }
    if next.kind == SourceTokenKind::RightBrace {
        return sibling_no_space;
    }
    if next.kind == SourceTokenKind::Other && cx.raw_source(next.range) == "}" {
        return true;
    }
    matches!(cx.raw_source(next.range), "]" | "|")
}

murphy_plugin_api::submit_cop!(SpaceAfterComma);

#[cfg(test)]
mod tests {
    use super::{HashBraceSiblingStyle, SpaceAfterComma, SpaceAfterCommaOptions};
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

    // ── murphy-ilrx: host-baked Layout/SpaceInsideHashLiteralBraces.EnforcedStyle ──

    #[test]
    fn options_default_is_space() {
        let d = SpaceAfterCommaOptions::default();
        assert_eq!(
            d.space_inside_hash_literal_braces_enforced_style,
            HashBraceSiblingStyle::Space
        );
    }

    #[test]
    fn accepts_comma_before_closing_brace_under_no_space_sibling_style() {
        // RuboCop `space_forbidden_before_rcurly?`: `,}` is exempt when the
        // sibling cop is `EnforcedStyle: no_space` (host-baked).
        test::<SpaceAfterComma>()
            .with_options(&SpaceAfterCommaOptions {
                space_inside_hash_literal_braces_enforced_style: HashBraceSiblingStyle::NoSpace,
            })
            .expect_no_offenses("{ foo: bar,}\n");
    }

    #[test]
    fn flags_comma_before_closing_brace_under_explicit_space_style() {
        // Explicit `space` behaves like the default: `,}` is flagged.
        test::<SpaceAfterComma>()
            .with_options(&SpaceAfterCommaOptions {
                space_inside_hash_literal_braces_enforced_style: HashBraceSiblingStyle::Space,
            })
            .expect_correction(
                indoc! {r#"
                    { foo: bar,}
                              ^ Space missing after comma.
                "#},
                "{ foo: bar, }\n",
            );
    }

    #[test]
    fn flags_comma_before_closing_brace_under_compact_style() {
        // `compact` still requires a space (`style == 'no_space'` is false).
        test::<SpaceAfterComma>()
            .with_options(&SpaceAfterCommaOptions {
                space_inside_hash_literal_braces_enforced_style: HashBraceSiblingStyle::Compact,
            })
            .expect_offense(indoc! {r#"
                { foo: bar,}
                          ^ Space missing after comma.
            "#});
    }

    #[test]
    fn no_space_style_still_flags_ordinary_missing_space() {
        // The sibling gate only affects a regular `}`; `,2` still fires.
        test::<SpaceAfterComma>()
            .with_options(&SpaceAfterCommaOptions {
                space_inside_hash_literal_braces_enforced_style: HashBraceSiblingStyle::NoSpace,
            })
            .expect_offense(indoc! {r#"
                [1,2]
                  ^ Space missing after comma.
            "#});
    }
}
