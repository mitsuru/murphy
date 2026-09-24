//! `Layout/SpaceBeforeComma` — flags a comma (`,`) preceded by whitespace
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/SpaceBeforeComma
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's `SpaceBeforePunctuation` mixin (`each_missing_space`):
//!   flags the whitespace gap when a comma is preceded by space on the same
//!   line and removes it. RuboCop's `space_required_after?` exempts a `{`
//!   preceding the comma when `Layout/SpaceInsideBlockBraces`'s
//!   `EnforcedStyle` is `space` (the default); the sibling style is read
//!   dynamically via `Cx::block_braces_space()` (murphy-4qhr, murphy-bgd8
//!   pattern: host resolves `for_cop` into `AllCopsContext` and threads it
//!   into `CxRaw` without a numeric ABI bump).
//! ```
//!
//! and autocorrects by removing that whitespace. Mirrors RuboCop's
//! same-named cop.

use crate::cops::layout::space_before_punctuation::check_space_before_punctuation;
use murphy_plugin_api::{Cx, NoOptions, SourceToken, SourceTokenKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SpaceBeforeComma;

#[cop(
    name = "Layout/SpaceBeforeComma",
    description = "Flag a comma preceded by whitespace.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl SpaceBeforeComma {
    #[on_new_investigation]
    fn investigate(&self, cx: &Cx<'_>) {
        check_space_before_punctuation(cx, is_comma, "comma");
    }
}

fn is_comma(_cx: &Cx<'_>, token: SourceToken) -> bool {
    token.kind == SourceTokenKind::Comma
}

#[cfg(test)]
mod tests {
    use super::SpaceBeforeComma;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_space_before_comma_in_array() {
        test::<SpaceBeforeComma>().expect_correction(
            indoc! {r#"
                [1 , 2 , 3]
                  ^ Space found before comma.
                      ^ Space found before comma.
            "#},
            "[1, 2, 3]\n",
        );
    }

    #[test]
    fn flags_space_before_comma_in_call() {
        test::<SpaceBeforeComma>().expect_correction(
            indoc! {r#"
                a(1 , 2)
                   ^ Space found before comma.
            "#},
            "a(1, 2)\n",
        );
    }

    #[test]
    fn flags_space_before_comma_in_block_args() {
        test::<SpaceBeforeComma>().expect_correction(
            indoc! {r#"
                each { |a , b| }
                         ^ Space found before comma.
            "#},
            "each { |a, b| }\n",
        );
    }

    #[test]
    fn accepts_no_space_before_comma() {
        test::<SpaceBeforeComma>().expect_no_offenses("[1, 2, 3]\na(1, 2)\neach { |a, b| }\n");
    }

    #[test]
    fn accepts_leading_comma_at_line_start() {
        // A comma at the start of a (continuation) line is preceded by a
        // newline, not inline space, so the same-line guard exempts it.
        test::<SpaceBeforeComma>().expect_no_offenses("[1\n, 2\n, 3]\n");
    }

    #[test]
    fn flags_multiple_spaces_before_comma() {
        test::<SpaceBeforeComma>().expect_correction(
            indoc! {r#"
                [1   , 2]
                  ^^^ Space found before comma.
            "#},
            "[1, 2]\n",
        );
    }

    // ── murphy-4qhr: dynamic Layout/SpaceInsideBlockBraces.EnforcedStyle ──

    #[test]
    fn accepts_space_after_lcurly_under_space_style() {
        // Default context is `space` (RuboCop default): `{ ,` is exempt via
        // `space_required_after?` — the space is required by the sibling cop.
        test::<SpaceBeforeComma>().expect_no_offenses("{ , }\n");
    }

    #[test]
    fn accepts_space_after_lcurly_with_explicit_space_style() {
        test::<SpaceBeforeComma>()
            .with_block_braces_space(true)
            .expect_no_offenses("{ , }\n");
    }

    #[test]
    fn flags_space_after_lcurly_under_no_space_style() {
        // `no_space` sibling style: the `{ ,` gap is NOT exempt and is flagged.
        test::<SpaceBeforeComma>()
            .with_block_braces_space(false)
            .expect_correction(
                indoc! {r#"
                    { , }
                     ^ Space found before comma.
                "#},
                "{, }\n",
            );
    }

    #[test]
    fn flags_normal_space_before_comma_regardless_of_sibling_style() {
        // Non-`{` gaps flag in both sibling styles — the flag only gates the
        // `{`-exemption.
        test::<SpaceBeforeComma>()
            .with_block_braces_space(false)
            .expect_correction(
                indoc! {r#"
                    [1 , 2]
                      ^ Space found before comma.
                "#},
                "[1, 2]\n",
            );
    }
}

murphy_plugin_api::submit_cop!(SpaceBeforeComma);
