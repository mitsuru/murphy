//! `Layout/SpaceBeforeSemicolon` — flags a semicolon (`;`) preceded by
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/SpaceBeforeSemicolon
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues:
//!   - murphy-4qhr
//! notes: >
//!   Mirrors RuboCop's `SpaceBeforePunctuation` mixin (`each_missing_space`):
//!   flags the whitespace gap when a semicolon is preceded by space on the
//!   same line and removes it. Murphy has no dedicated `Semicolon`
//!   `SourceTokenKind`, so the token is matched by source bytes (`;`).
//!   murphy-4qhr: RuboCop's `space_required_after?` also exempts a `{`
//!   preceding the punctuation when `Layout/SpaceInsideBlockBraces`'s
//!   `EnforcedStyle` is `space` (the default); Murphy cannot read another
//!   cop's config from inside a cop (single-surface ABI boundary), so it
//!   unconditionally exempts a `{` directly before the semicolon (the
//!   default-style behavior).
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
}

murphy_plugin_api::submit_cop!(SpaceBeforeSemicolon);
