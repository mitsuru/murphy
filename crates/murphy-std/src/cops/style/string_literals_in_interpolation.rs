//! `Style/StringLiteralsInInterpolation` — enforce quote style inside interpolations.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/StringLiteralsInInterpolation
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   RuboCop overrides `on_regexp(node); end` (a no-op) because StringHelp's
//!   default `on_regexp` would otherwise dispatch on regexp node strings.
//!   Murphy's uniform `str` dispatch via `#[on_node(kind = "str")]` already
//!   reaches regexp/dsym/dstr interpolation strings identically -- no override
//!   is needed.
//!
//!   Inside-interpolation detection: a `str` node is inside an interpolation
//!   `#{...}` if and only if it has a Begin ancestor whose parent is a
//!   `Dstr`, `Dsym`, `Xstr`, or `Regexp` node. Plain `begin...end` blocks
//!   are also NodeKind::Begin but their parent is never an interpolation
//!   container, so they are correctly excluded.
//!
//!   Quote helpers (parse_quote_form, double_quotes_required, single_quotes_required,
//!   safe_swap) are shared with Style/StringLiterals to avoid drift.
//! ```
//!
//! Subscribes to `NodeKind::Str` (plain string literal). Each `str` node is
//! checked for whether it lives inside a string interpolation `#{...}`, and
//! if so, whether its quote style matches the configured preference.
//!
//! ## Option (`EnforcedStyle`)
//!
//! Declared via the shared `StringLiteralsOptions`. Default `single_quotes` matches RuboCop.

use murphy_plugin_api::{Cx, NodeId, NodeKind, cop};

use super::string_literals::{
    EnforcedStyle, QuoteStyle, StringLiteralsOptions, double_quotes_required, parse_quote_form,
    safe_swap, single_quotes_required,
};

/// Stateless unit struct.
#[derive(Default)]
pub struct StringLiteralsInInterpolation;

#[cop(
    name = "Style/StringLiteralsInInterpolation",
    description = "Checks if uses of quotes inside expressions in interpolated strings match the configured preference.",
    default_severity = "warning",
    default_enabled = true,
    options = StringLiteralsOptions,
)]
impl StringLiteralsInInterpolation {
    #[on_node(kind = "str")]
    fn check_str(&self, node: NodeId, cx: &Cx<'_>) {
        // Only check strings that are inside an interpolation #{...}.
        if !inside_interpolation(node, cx) {
            return;
        }

        let opts = cx.options_or_default::<StringLiteralsOptions>();
        let prefer_single = opts.enforced_style == EnforcedStyle::SingleQuotes;

        let range = cx.range(node);
        let src = cx.raw_source(range);
        let Some((actual, body)) = parse_quote_form(src) else {
            return;
        };

        let preferred = if prefer_single {
            QuoteStyle::Single
        } else {
            QuoteStyle::Double
        };
        if actual == preferred {
            return;
        }

        // Apply the same double_quotes_required / single_quotes_required guards
        // as Style/StringLiterals to avoid false positives.
        if prefer_single && actual == QuoteStyle::Double && double_quotes_required(src) {
            return;
        }
        if !prefer_single && actual == QuoteStyle::Single && single_quotes_required(src) {
            return;
        }

        let (message, replacement) = match preferred {
            QuoteStyle::Single => (
                "Prefer single-quoted strings inside interpolations.",
                safe_swap(body, b'\'', b'"').map(|s| format!("'{s}'")),
            ),
            QuoteStyle::Double => (
                "Prefer double-quoted strings inside interpolations.",
                safe_swap(body, b'"', b'\'').map(|s| format!("\"{s}\"")),
            ),
        };

        cx.emit_offense(range, message, None);
        if let Some(text) = replacement {
            cx.emit_edit(range, &text);
        }
    }
}

/// Returns `true` when `node` is a `str` inside a string interpolation `#{...}`.
///
/// A `str` is inside an interpolation iff it has a `Begin` ancestor whose
/// parent is a `Dstr`, `Dsym`, `Xstr`, or `Regexp` node.
fn inside_interpolation(node: NodeId, cx: &Cx<'_>) -> bool {
    for ancestor in cx.ancestors(node) {
        if !matches!(*cx.kind(ancestor), NodeKind::Begin(_)) {
            continue;
        }
        // Check that the Begin's parent is an interpolation container.
        let Some(container) = cx.parent(ancestor).get() else {
            continue;
        };
        if matches!(
            *cx.kind(container),
            NodeKind::Dstr(_) | NodeKind::Dsym(_) | NodeKind::Xstr(_) | NodeKind::Regexp { .. }
        ) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- offense detection (single_quotes mode, default) ---

    #[test]
    fn flags_double_quoted_string_inside_dstr_interpolation() {
        // Ruby: "Tests #{success ? "PASS" : "FAIL"}"
        test::<StringLiteralsInInterpolation>().expect_offense(indoc! {r#"
            x = "Tests #{success ? "PASS" : "FAIL"}"
                                   ^^^^^^ Prefer single-quoted strings inside interpolations.
                                            ^^^^^^ Prefer single-quoted strings inside interpolations.
        "#});
    }

    #[test]
    fn no_offense_for_single_quoted_inside_interpolation() {
        // Ruby: "Tests #{success ? 'PASS' : 'FAIL'}"
        test::<StringLiteralsInInterpolation>().expect_no_offenses(
            r#"x = "Tests #{success ? 'PASS' : 'FAIL'}"
"#,
        );
    }

    #[test]
    fn no_offense_for_str_outside_interpolation() {
        // Plain double-quoted string not inside any interpolation.
        test::<StringLiteralsInInterpolation>().expect_no_offenses("x = \"hello\"\n");
    }

    #[test]
    fn no_offense_for_plain_single_quoted_not_in_interpolation() {
        test::<StringLiteralsInInterpolation>().expect_no_offenses("x = 'hello'\n");
    }

    // --- double_quotes mode ---

    #[test]
    fn double_quotes_mode_flags_single_quoted_inside_interpolation() {
        // Ruby: "Tests #{success ? 'PASS' : 'FAIL'}"
        test::<StringLiteralsInInterpolation>()
            .with_options(&StringLiteralsOptions {
                enforced_style: EnforcedStyle::DoubleQuotes,
            })
            .expect_offense(indoc! {r#"
                x = "Tests #{success ? 'PASS' : 'FAIL'}"
                                       ^^^^^^ Prefer double-quoted strings inside interpolations.
                                                ^^^^^^ Prefer double-quoted strings inside interpolations.
            "#});
    }

    #[test]
    fn double_quotes_mode_no_offense_for_double_quoted_inside_interpolation() {
        // Ruby: "Tests #{success ? "PASS" : "FAIL"}"
        test::<StringLiteralsInInterpolation>()
            .with_options(&StringLiteralsOptions {
                enforced_style: EnforcedStyle::DoubleQuotes,
            })
            .expect_no_offenses(
                r#"x = "Tests #{success ? "PASS" : "FAIL"}"
"#,
            );
    }

    // --- autocorrect ---

    #[test]
    fn corrects_double_quoted_to_single_quoted_inside_interpolation() {
        test::<StringLiteralsInInterpolation>().expect_correction(
            indoc! {r#"
                x = "a #{b ? "yes" : "no"}"
                             ^^^^^ Prefer single-quoted strings inside interpolations.
                                     ^^^^ Prefer single-quoted strings inside interpolations.
            "#},
            "x = \"a \x23{b ? 'yes' : 'no'}\"\n",
        );
    }

    // --- regexp interpolation ---

    #[test]
    fn flags_double_quoted_inside_regexp_interpolation() {
        test::<StringLiteralsInInterpolation>().expect_offense(indoc! {r#"
            x = /prefix #{a ? "b" : "c"}/
                              ^^^ Prefer single-quoted strings inside interpolations.
                                    ^^^ Prefer single-quoted strings inside interpolations.
        "#});
    }

    // --- dsym interpolation ---

    #[test]
    fn flags_double_quoted_inside_dsym_interpolation() {
        test::<StringLiteralsInInterpolation>().expect_offense(indoc! {r#"
            x = :"prefix #{a ? "b" : "c"}"
                               ^^^ Prefer single-quoted strings inside interpolations.
                                     ^^^ Prefer single-quoted strings inside interpolations.
        "#});
    }

    // --- config keys match RuboCop ---

    #[test]
    fn enforced_style_single_quotes_from_config_json() {
        use murphy_plugin_api::CopOptions;
        let opts =
            StringLiteralsOptions::from_config_json(br#"{"EnforcedStyle": "single_quotes"}"#)
                .expect("valid config");
        assert_eq!(opts.enforced_style, EnforcedStyle::SingleQuotes);
    }

    #[test]
    fn enforced_style_double_quotes_from_config_json() {
        use murphy_plugin_api::CopOptions;
        let opts =
            StringLiteralsOptions::from_config_json(br#"{"EnforcedStyle": "double_quotes"}"#)
                .expect("valid config");
        assert_eq!(opts.enforced_style, EnforcedStyle::DoubleQuotes);
    }
}
murphy_plugin_api::submit_cop!(StringLiteralsInInterpolation);
