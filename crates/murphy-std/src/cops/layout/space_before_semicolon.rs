//! `Layout/SpaceBeforeSemicolon` — flags a semicolon (`;`) preceded by
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/SpaceBeforeSemicolon
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's `SpaceBeforePunctuation` mixin (`each_missing_space`):
//!   flags the whitespace gap when a semicolon is preceded by space on the
//!   same line and removes it. Murphy has no dedicated `Semicolon`
//!   `SourceTokenKind`, so the token is matched by source bytes (`;`).
//!   RuboCop's `space_required_after?` exempts a `{` preceding the semicolon
//!   when `Layout/SpaceInsideBlockBraces`'s `EnforcedStyle` is `space` (the
//!   default); the sibling style is read dynamically via
//!   `Cx::block_braces_space()` (murphy-4qhr, murphy-bgd8 pattern: host
//!   resolves `for_cop` into `AllCopsContext` and threads it into `CxRaw`
//!   without a numeric ABI bump).
//! ```
//!
//! whitespace, and autocorrects by removing that whitespace. Mirrors
//! RuboCop's same-named cop.

use crate::cops::layout::space_before_punctuation::check_space_before_punctuation;
use murphy_plugin_api::{Cx, NoOptions, SourceToken, SourceTokenKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SpaceBeforeSemicolon;

#[cop(
    name = "Layout/SpaceBeforeSemicolon",
    description = "Flag a semicolon preceded by whitespace.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl SpaceBeforeSemicolon {
    #[on_new_investigation]
    fn investigate(&self, cx: &Cx<'_>) {
        check_space_before_punctuation(cx, is_semicolon, "semicolon");
    }
}

/// Murphy has no `Semicolon` token kind — `;` tokenizes as `Other`. Match by
/// source bytes, restricting to `Other` first to skip the costly source
/// lookup for the common case.
fn is_semicolon(cx: &Cx<'_>, token: SourceToken) -> bool {
    token.kind == SourceTokenKind::Other && cx.raw_source(token.range) == ";"
}

#[cfg(test)]
mod tests {
    use super::SpaceBeforeSemicolon;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_space_before_semicolon() {
        test::<SpaceBeforeSemicolon>().expect_correction(
            indoc! {r#"
                x = 1 ; y = 2
                     ^ Space found before semicolon.
            "#},
            "x = 1; y = 2\n",
        );
    }

    #[test]
    fn accepts_no_space_before_semicolon() {
        test::<SpaceBeforeSemicolon>().expect_no_offenses("x = 1; y = 2\n");
    }

    #[test]
    fn flags_multiple_spaces_before_semicolon() {
        test::<SpaceBeforeSemicolon>().expect_correction(
            indoc! {r#"
                x = 1   ; y = 2
                     ^^^ Space found before semicolon.
            "#},
            "x = 1; y = 2\n",
        );
    }

    #[test]
    fn accepts_lone_trailing_semicolon() {
        test::<SpaceBeforeSemicolon>().expect_no_offenses("x = 1;\n");
    }

    // ── murphy-4qhr: dynamic Layout/SpaceInsideBlockBraces.EnforcedStyle ──

    #[test]
    fn accepts_space_after_lcurly_under_space_style() {
        // Default context is `space` (RuboCop default): `{ ;` is exempt via
        // `space_required_after?` — the space is required by the sibling cop.
        test::<SpaceBeforeSemicolon>().expect_no_offenses("foo { ; }\n");
    }

    #[test]
    fn accepts_space_after_lcurly_with_explicit_space_style() {
        test::<SpaceBeforeSemicolon>()
            .with_block_braces_space(true)
            .expect_no_offenses("foo { ; }\n");
    }

    #[test]
    fn flags_space_after_lcurly_under_no_space_style() {
        // `no_space` sibling style: the `{ ;` gap is NOT exempt and is flagged.
        test::<SpaceBeforeSemicolon>()
            .with_block_braces_space(false)
            .expect_correction(
                indoc! {r#"
                    foo { ; }
                         ^ Space found before semicolon.
                "#},
                "foo {; }\n",
            );
    }

    #[test]
    fn flags_normal_space_before_semicolon_regardless_of_sibling_style() {
        // Non-`{` gaps flag in both sibling styles — the flag only gates the
        // `{`-exemption.
        test::<SpaceBeforeSemicolon>()
            .with_block_braces_space(false)
            .expect_correction(
                indoc! {r#"
                    x = 1 ; y = 2
                         ^ Space found before semicolon.
                "#},
                "x = 1; y = 2\n",
            );
    }
}

murphy_plugin_api::submit_cop!(SpaceBeforeSemicolon);
