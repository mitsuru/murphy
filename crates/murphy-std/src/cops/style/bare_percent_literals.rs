//! `Style/BarePercentLiterals` — checks if usage of `%()` or `%Q()` matches configuration.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/BarePercentLiterals
//! upstream_version_checked: 1.86.2
//! version_added: "0.25"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Ports both EnforcedStyle modes:
//!     - bare_percent (default): prefers %(...); flags %Q(...) strings.
//!     - percent_q: prefers %Q(...); flags bare %(...) strings (percent
//!       followed by a non-word character).
//!   Subscribes to both str and dstr because %Q(...) and %(...) are
//!   semantically identical (same interpolation and escape handling), so the
//!   swap is always safe — no semantic guard needed.
//!   Heredocs are naturally excluded: their raw source starts with << not %.
//!   %q (lowercase) is never touched by this cop — that is PercentQLiterals.
//!   In percent_q mode, %q, %w, %W, %i, %I, %r, %s, %x are all excluded
//!   because they have word characters after %.
//!   str segments that are children of a dstr node are skipped to avoid
//!   double-flagging.
//! ```
//!
//! ## Matched shapes
//!
//! - **bare_percent** (default): `%Q(He said: "#{greeting}")` -> `%(He said: "#{greeting}")`
//! - **bare_percent** (default): `%Q{She said: 'Hi'}` -> `%{She said: 'Hi'}`
//! - **percent_q**: `%|He said: "#{greeting}"|` -> `%Q|He said: "#{greeting}"|`
//! - **percent_q**: `%/She said: 'Hi'/` -> `%Q/She said: 'Hi'/`
//!
//! ## Why this shape
//!
//! `%()` and `%Q()` are semantically identical: both support interpolation
//! (`#{...}`) and the same escape sequences. The choice between them is purely
//! stylistic. Consistent use of one form makes the codebase easier to read.
//!
//! ## Autocorrect
//!
//! - **bare_percent**: delete the `Q` at position `start+1`.
//! - **percent_q**: insert `Q` after `%` at position `start+1`.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct BarePercentLiterals;

/// Preferred style for bare percent literals.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BarePercentLiteralsStyle {
    #[default]
    #[option(value = "bare_percent")]
    BarePercent,
    #[option(value = "percent_q")]
    PercentQ,
}

#[derive(CopOptions)]
pub struct BarePercentLiteralsOptions {
    #[option(
        name = "EnforcedStyle",
        default = "bare_percent",
        description = "Preferred style for bare percent literals."
    )]
    pub enforced_style: BarePercentLiteralsStyle,
}

const BARE_PERCENT_MSG: &str = "Use `%` instead of `%Q`.";
const PERCENT_Q_MSG: &str = "Use `%Q` instead of `%`.";

#[cop(
    name = "Style/BarePercentLiterals",
    description = "Checks if usage of %() or %Q() matches configuration.",
    default_severity = "warning",
    default_enabled = true,
    options = BarePercentLiteralsOptions,
)]
impl BarePercentLiterals {
    #[on_node(kind = "str")]
    fn check_str(&self, node: NodeId, cx: &Cx<'_>) {
        // Skip str segments that are children of a dstr — the dstr handler
        // covers those (avoids double-flagging the same literal).
        if cx
            .parent(node)
            .get()
            .is_some_and(|p| matches!(cx.kind(p), NodeKind::Dstr(_)))
        {
            return;
        }
        check(node, cx);
    }

    #[on_node(kind = "dstr")]
    fn check_dstr(&self, node: NodeId, cx: &Cx<'_>) {
        // Skip dstr segments nested inside another dstr (interpolation segments).
        if cx
            .parent(node)
            .get()
            .is_some_and(|p| matches!(cx.kind(p), NodeKind::Dstr(_)))
        {
            return;
        }
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let node_range = cx.range(node);
    let src = cx.raw_source(node_range);

    let opts = cx.options_or_default::<BarePercentLiteralsOptions>();

    match opts.enforced_style {
        BarePercentLiteralsStyle::BarePercent => {
            // Flag `%Q(...)` — use `%` instead.
            if !src.starts_with("%Q") {
                return;
            }
            // Offense range: 3 bytes (`%Q(`)
            let opener_range = Range {
                start: node_range.start,
                end: node_range.start + 3,
            };
            cx.emit_offense(opener_range, BARE_PERCENT_MSG, None);
            // Autocorrect: delete the `Q` at position start+1.
            let q_range = Range {
                start: node_range.start + 1,
                end: node_range.start + 2,
            };
            cx.emit_edit(q_range, "");
        }
        BarePercentLiteralsStyle::PercentQ => {
            // Flag bare `%(...)` — bare `%` followed by a non-word character.
            // Excludes: %q, %Q (has word char), %w, %W, %i, %I, %r, %s, %x
            // (all have word characters after %).
            if !src.starts_with('%') {
                return;
            }
            let second = src.as_bytes().get(1).copied().unwrap_or(0);
            // Non-word character: not a letter, digit, or underscore.
            if second == 0 || second.is_ascii_alphanumeric() || second == b'_' {
                return;
            }
            // Offense range: 2 bytes (`%(`)
            let opener_range = Range {
                start: node_range.start,
                end: node_range.start + 2,
            };
            cx.emit_offense(opener_range, PERCENT_Q_MSG, None);
            // Autocorrect: insert `Q` after `%` at position start+1.
            let insert_range = Range {
                start: node_range.start + 1,
                end: node_range.start + 1,
            };
            cx.emit_edit(insert_range, "Q");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- bare_percent mode (default): flag %Q ---

    #[test]
    fn bare_percent_flags_percent_q_with_interpolation() {
        // Use r##"..."## because the string contains "# (terminates r#"..."#).
        test::<BarePercentLiterals>().expect_offense(indoc! {r##"
            %Q(He said: "#{greeting}")
            ^^^ Use `%` instead of `%Q`.
        "##});
    }

    #[test]
    fn bare_percent_flags_percent_q_plain() {
        test::<BarePercentLiterals>().expect_offense(indoc! {r#"
            %Q{She said: 'Hi'}
            ^^^ Use `%` instead of `%Q`.
        "#});
    }

    #[test]
    fn bare_percent_no_offense_for_bare_percent_with_interpolation() {
        test::<BarePercentLiterals>()
            .expect_no_offenses("%(He said: \"#{greeting}\")\n");
    }

    #[test]
    fn bare_percent_no_offense_for_bare_percent_plain() {
        test::<BarePercentLiterals>().expect_no_offenses("%{She said: 'Hi'}\n");
    }

    #[test]
    fn bare_percent_no_offense_for_percent_q_lowercase() {
        // %q is PercentQLiterals' job, not ours.
        test::<BarePercentLiterals>().expect_no_offenses("%q{She said: 'Hi'}\n");
    }

    #[test]
    fn bare_percent_no_offense_for_regular_string() {
        test::<BarePercentLiterals>().expect_no_offenses("\"hello\"\n");
    }

    #[test]
    fn bare_percent_no_offense_for_single_quoted_string() {
        test::<BarePercentLiterals>().expect_no_offenses("'hello'\n");
    }

    #[test]
    fn bare_percent_no_offense_for_percent_w_array() {
        test::<BarePercentLiterals>().expect_no_offenses("%w[foo bar]\n");
    }

    // --- percent_q mode: flag bare % ---

    #[test]
    fn percent_q_flags_bare_percent_with_interpolation() {
        test::<BarePercentLiterals>()
            .with_options(&BarePercentLiteralsOptions {
                enforced_style: BarePercentLiteralsStyle::PercentQ,
            })
            .expect_offense(indoc! {r##"
                %|He said: "#{greeting}"|
                ^^ Use `%Q` instead of `%`.
            "##});
    }

    #[test]
    fn percent_q_flags_bare_percent_plain() {
        test::<BarePercentLiterals>()
            .with_options(&BarePercentLiteralsOptions {
                enforced_style: BarePercentLiteralsStyle::PercentQ,
            })
            .expect_offense(indoc! {r#"
                %/She said: 'Hi'/
                ^^ Use `%Q` instead of `%`.
            "#});
    }

    #[test]
    fn percent_q_no_offense_for_percent_q_with_interpolation() {
        test::<BarePercentLiterals>()
            .with_options(&BarePercentLiteralsOptions {
                enforced_style: BarePercentLiteralsStyle::PercentQ,
            })
            .expect_no_offenses("%Q|He said: \"#{greeting}\"|\n");
    }

    #[test]
    fn percent_q_no_offense_for_percent_q_plain() {
        test::<BarePercentLiterals>()
            .with_options(&BarePercentLiteralsOptions {
                enforced_style: BarePercentLiteralsStyle::PercentQ,
            })
            .expect_no_offenses("%Q/She said: 'Hi'/\n");
    }

    #[test]
    fn percent_q_no_offense_for_percent_q_lowercase() {
        // %q is excluded — has word char after %.
        test::<BarePercentLiterals>()
            .with_options(&BarePercentLiteralsOptions {
                enforced_style: BarePercentLiteralsStyle::PercentQ,
            })
            .expect_no_offenses("%q/She said: 'Hi'/\n");
    }

    #[test]
    fn percent_q_no_offense_for_percent_w_array() {
        // %w has word char after %, excluded.
        test::<BarePercentLiterals>()
            .with_options(&BarePercentLiteralsOptions {
                enforced_style: BarePercentLiteralsStyle::PercentQ,
            })
            .expect_no_offenses("%w[foo bar]\n");
    }

    // --- Autocorrect ---

    #[test]
    fn bare_percent_autocorrects_percent_q_to_bare() {
        test::<BarePercentLiterals>().expect_correction(
            indoc! {r##"
                %Q(He said: "#{greeting}")
                ^^^ Use `%` instead of `%Q`.
            "##},
            "%(He said: \"#{greeting}\")\n",
        );
    }

    #[test]
    fn bare_percent_autocorrects_percent_q_plain() {
        test::<BarePercentLiterals>().expect_correction(
            indoc! {r#"
                %Q{She said: 'Hi'}
                ^^^ Use `%` instead of `%Q`.
            "#},
            "%{She said: 'Hi'}\n",
        );
    }

    #[test]
    fn percent_q_autocorrects_bare_percent_to_percent_q() {
        test::<BarePercentLiterals>()
            .with_options(&BarePercentLiteralsOptions {
                enforced_style: BarePercentLiteralsStyle::PercentQ,
            })
            .expect_correction(
                indoc! {r##"
                    %|He said: "#{greeting}"|
                    ^^ Use `%Q` instead of `%`.
                "##},
                "%Q|He said: \"#{greeting}\"|\n",
            );
    }

    #[test]
    fn percent_q_autocorrects_bare_percent_plain() {
        test::<BarePercentLiterals>()
            .with_options(&BarePercentLiteralsOptions {
                enforced_style: BarePercentLiteralsStyle::PercentQ,
            })
            .expect_correction(
                indoc! {r#"
                    %/She said: 'Hi'/
                    ^^ Use `%Q` instead of `%`.
                "#},
                "%Q/She said: 'Hi'/\n",
            );
    }

    // --- Heredoc guard (naturally excluded) ---

    #[test]
    fn bare_percent_no_offense_for_heredoc() {
        test::<BarePercentLiterals>().expect_no_offenses(indoc! {"
            x = <<~RUBY
              hello
            RUBY
        "});
    }
}

murphy_plugin_api::submit_cop!(BarePercentLiterals);
