//! `Layout/SpaceAfterComma` — require a space after every comma.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/SpaceAfterComma
//! upstream_version_checked: master
//! status: partial
//! gap_issues:
//!   - murphy-je5n
//! notes: >
//!   Token-stream port of RuboCop's `SpaceAfterPunctuation` mixin specialized
//!   for commas. Fires only when a `,` is immediately followed (zero gap, same
//!   line) by a non-allowed token. RuboCop's `allowed_type?` exempts `)`, `]`,
//!   `|`, and `tSTRING_DEND`; Murphy's token stream only models `)` as a
//!   distinct kind (`RightParen`), so `]` and `|` are matched by source byte.
//!   The `}` rcurly case follows `Layout/SpaceInsideHashLiteralBraces`'s default
//!   (`space`), under which a space *is* required before `}` — so `{ a,}` flags
//!   by default, exactly matching RuboCop's default config. The cross-cop
//!   `no_space` exemption for `}` is not wired (no cross-cop config access,
//!   tracked as murphy-je5n); this is the only intentional gap and matches
//!   RuboCop only under default config.
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
    // `space_required_before?`: skip closers `)`, `]`, `|`. `tSTRING_DEND`
    // (string interpolation end `}`) is `Other` in Murphy; a `}` here is either
    // a hash/brace close (which RuboCop's default config *does* require a space
    // before) or interpolation end. We do not special-case `}`; under default
    // config this matches RuboCop.
    if is_allowed_after_comma(cx, next) {
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
/// required when the following token is `)`, `]`, or `|`.
fn is_allowed_after_comma(cx: &Cx<'_>, next: SourceToken) -> bool {
    if next.kind == SourceTokenKind::RightParen {
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
}
