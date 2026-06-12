//! `Layout/SpaceBeforeComma` — flags a comma (`,`) preceded by whitespace
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/SpaceBeforeComma
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues:
//!   - murphy-4qhr
//! notes: >
//!   Mirrors RuboCop's `SpaceBeforePunctuation` mixin (`each_missing_space`):
//!   flags the whitespace gap when a comma is preceded by space on the same
//!   line and removes it. murphy-4qhr: RuboCop's `space_required_after?` also
//!   exempts a `{` preceding the punctuation when `Layout/SpaceInsideBlockBraces`'s
//!   `EnforcedStyle` is `space` (the default); Murphy cannot read another
//!   cop's config from inside a cop (single-surface ABI boundary), so it
//!   unconditionally exempts a `{` directly before the comma (the
//!   default-style behavior). A `,` directly after `{` is a near-impossible
//!   Ruby shape, so the divergence is inert.
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
}

murphy_plugin_api::submit_cop!(SpaceBeforeComma);
