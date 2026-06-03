//! `Style/PercentQLiterals` — prefer `%q` or `%Q` depending on configured style.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/PercentQLiterals
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Ports both EnforcedStyle modes:
//!     - lower_case_q (default): prefers %q; flags %Q when interpolation is not
//!       needed (i.e. changing to %q would not alter string semantics).
//!     - upper_case_q: always use %Q; flags %q literals.
//!   Subscribes to `str` only (not dstr). %Q(#{x}) is a dstr and is correctly
//!   skipped because it genuinely requires interpolation.
//!   Semantic guard: changing case must not alter string meaning.
//!     - %Q -> %q: blocked when the body contains a non-backslash escape sequence
//!       (\n, \t, etc.) — %q does not process them.
//!     - %q -> %Q: blocked when the body contains #{...}-like interpolation syntax
//!       (converting would activate interpolation, regardless of single-quote presence)
//!       or has a non-backslash escape (converting would activate the escape).
//!   The guard is equivalent to RuboCop's parse-and-compare via acceptable_q? and
//!   acceptable_capital_q? / has_escaped_non_backslash patterns.
//!   Offense range: the 3-byte opening delimiter (%Q( or %q{, etc.).
//!   Autocorrect: single-byte swap at position start+1 (Q <-> q).
//! ```
//!
//! ## Matched shapes
//!
//! - **lower_case_q** (default): `%Q[Mix the foo]` -> `%q[Mix the foo]`
//! - **upper_case_q**: `%q/Mix the foo/` -> `%Q/Mix the foo/`
//!
//! ## Autocorrect
//!
//! Replace the second character (`Q` or `q`) with its counterpart.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct PercentQLiterals;

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PercentQLiteralsStyle {
    #[default]
    #[option(value = "lower_case_q")]
    LowerCaseQ,
    #[option(value = "upper_case_q")]
    UpperCaseQ,
}

#[derive(CopOptions)]
pub struct PercentQLiteralsOptions {
    #[option(
        name = "EnforcedStyle",
        default = "lower_case_q",
        description = "Preferred percent-literal style."
    )]
    pub enforced_style: PercentQLiteralsStyle,
}

const LOWER_CASE_Q_MSG: &str = "Do not use `%Q` unless interpolation is needed. Use `%q`.";
const UPPER_CASE_Q_MSG: &str = "Use `%Q` instead of `%q`.";

#[cop(
    name = "Style/PercentQLiterals",
    description = "Checks if uses of %Q/%q match the configured preference.",
    default_severity = "warning",
    default_enabled = true,
    options = PercentQLiteralsOptions,
)]
impl PercentQLiterals {
    #[on_node(kind = "str")]
    fn check_str(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let node_range = cx.range(node);
    let src = cx.raw_source(node_range);

    let is_percent_upper_q = src.starts_with("%Q");
    let is_percent_lower_q = src.starts_with("%q");
    if !is_percent_upper_q && !is_percent_lower_q {
        return;
    }

    let opts = cx.options_or_default::<PercentQLiteralsOptions>();

    // Check if the literal is already using the correct style.
    let already_correct = match opts.enforced_style {
        PercentQLiteralsStyle::LowerCaseQ => is_percent_lower_q,
        PercentQLiteralsStyle::UpperCaseQ => is_percent_upper_q,
    };
    if already_correct {
        return;
    }

    // Semantic guard: skip offense if changing case would alter the string's meaning.
    // - %Q -> %q: %q doesn't process escape sequences like \n, \t (only \\ and \').
    //   If the body has any non-backslash escape, converting would change semantics.
    // - %q -> %Q: %Q activates interpolation (#{...}) and escape sequences.
    //   If the body has #{...} (regardless of single-quote presence) or has
    //   non-backslash escapes, converting would change the string's value.
    if !safe_to_change_case(src) {
        return;
    }

    let message = match opts.enforced_style {
        PercentQLiteralsStyle::LowerCaseQ => LOWER_CASE_Q_MSG,
        PercentQLiteralsStyle::UpperCaseQ => UPPER_CASE_Q_MSG,
    };

    // Offense range: just the 3-byte opening delimiter (%Q( or %q{, etc.).
    let opener_range = Range {
        start: node_range.start,
        end: node_range.start + 3,
    };
    cx.emit_offense(opener_range, message, None);

    // Autocorrect: swap Q <-> q at position start+1.
    let q_range = Range {
        start: node_range.start + 1,
        end: node_range.start + 2,
    };
    let replacement = if is_percent_upper_q { "q" } else { "Q" };
    cx.emit_edit(q_range, replacement);
}

/// Returns true if it is safe to swap `%Q` <-> `%q` without changing string semantics.
///
/// Safe when:
/// - No `\X` escape where X is not `\` (non-backslash escape).
///   `%q` does not process escape sequences (except `\\` and `\'`), so
///   `%Q(\n)` (newline) != `%q(\n)` (literal `\n`).
/// - For `%q` -> `%Q`: no `#{...}` interpolation syntax.
///   `%q(#{foo})` is a literal string containing `#{foo}`;
///   `%Q(#{foo})` would interpolate `foo`, changing the string's value.
///
/// Note: `\\` (double backslash = escaped backslash) is safe in both forms.
fn safe_to_change_case(src: &str) -> bool {
    if has_escaped_non_backslash(src) {
        return false;
    }
    // %q -> %Q would activate interpolation; block if #{...} syntax is present.
    // Any #{...} in %q is literal text; converting to %Q would evaluate it.
    if src.starts_with("%q") && has_interpolation_syntax(src) {
        return false;
    }
    true
}

/// True if `src` contains a `\X` sequence where X is not `\`.
/// Mirrors RuboCop's `ESCAPED_NON_BACKSLASH = /\\[^\\]/.freeze`.
fn has_escaped_non_backslash(src: &str) -> bool {
    let bytes = src.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'\\' {
            let next = bytes[i + 1];
            if next != b'\\' {
                return true;
            }
            // Skip the pair `\\` (escaped backslash).
            i += 2;
            continue;
        }
        i += 1;
    }
    false
}

/// True if `src` contains a `#{...}` interpolation-like sequence.
///
/// Matches `#{` followed by a closing `}` at any distance (including `#{}` —
/// empty interpolation). The `close > 0` condition was removed: `%q(#{})` is
/// literal text but `%Q(#{})` is valid empty interpolation, so both forms must
/// be treated as interpolation syntax.
fn has_interpolation_syntax(src: &str) -> bool {
    let bytes = src.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'#' && bytes[i + 1] == b'{'
            && bytes[i + 2..].contains(&b'}') {
                return true;
            }
        i += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- Default style (lower_case_q): flag %Q when %q would do ---

    #[test]
    fn flags_percent_upper_q_plain_string() {
        test::<PercentQLiterals>().expect_offense(indoc! {r#"
            name = %Q[Mix the foo into the baz.]
                   ^^^ Do not use `%Q` unless interpolation is needed. Use `%q`.
        "#});
    }

    #[test]
    fn flags_percent_upper_q_with_single_quotes() {
        test::<PercentQLiterals>().expect_offense(indoc! {r#"
            name = %Q(They all said: 'Hooray!')
                   ^^^ Do not use `%Q` unless interpolation is needed. Use `%q`.
        "#});
    }

    #[test]
    fn no_offense_for_percent_lower_q_in_lower_case_mode() {
        test::<PercentQLiterals>().expect_no_offenses("%q[Mix the foo into the baz]\n");
    }

    // --- upper_case_q style: flag %q ---

    #[test]
    fn upper_case_q_flags_percent_lower_q() {
        test::<PercentQLiterals>()
            .with_options(&PercentQLiteralsOptions {
                enforced_style: PercentQLiteralsStyle::UpperCaseQ,
            })
            .expect_offense(indoc! {r#"
                name = %q/Mix the foo into the baz./
                       ^^^ Use `%Q` instead of `%q`.
            "#});
    }

    #[test]
    fn upper_case_q_no_offense_for_percent_upper_q() {
        test::<PercentQLiterals>()
            .with_options(&PercentQLiteralsOptions {
                enforced_style: PercentQLiteralsStyle::UpperCaseQ,
            })
            .expect_no_offenses("%Q/Mix the foo into the baz./\n");
    }

    // --- Semantic guard: skip when changing case would alter meaning ---

    #[test]
    fn no_offense_for_percent_upper_q_with_escape_sequence() {
        // \n in %Q is a newline; in %q it would be literal \n.
        test::<PercentQLiterals>().expect_no_offenses("%Q(hello\\nworld)\n");
    }

    #[test]
    fn no_offense_for_percent_lower_q_with_escape_sequence_when_upper_case_q() {
        // \n in %q is literal \n; in %Q it would be a newline.
        test::<PercentQLiterals>()
            .with_options(&PercentQLiteralsOptions {
                enforced_style: PercentQLiteralsStyle::UpperCaseQ,
            })
            .expect_no_offenses("%q(hello\\nworld)\n");
    }

    #[test]
    fn no_offense_for_percent_lower_q_with_interpolation_syntax_when_upper_case_q() {
        // #{foo} in %q is literal text; converting to %Q would activate interpolation.
        test::<PercentQLiterals>()
            .with_options(&PercentQLiteralsOptions {
                enforced_style: PercentQLiteralsStyle::UpperCaseQ,
            })
            .expect_no_offenses("%q(#{foo})\n");
    }

    #[test]
    fn no_offense_for_percent_lower_q_with_empty_interpolation_when_upper_case_q() {
        // %q(#{}) contains #{} which is literal text; %Q(#{}) is empty interpolation.
        test::<PercentQLiterals>()
            .with_options(&PercentQLiteralsOptions {
                enforced_style: PercentQLiteralsStyle::UpperCaseQ,
            })
            .expect_no_offenses("%q(#{})
");
    }

    #[test]
    fn no_offense_for_percent_lower_q_with_interpolation_no_single_quote() {
        // Even without single-quote, #{...} in %q blocks the conversion to %Q.
        test::<PercentQLiterals>()
            .with_options(&PercentQLiteralsOptions {
                enforced_style: PercentQLiteralsStyle::UpperCaseQ,
            })
            .expect_no_offenses("%q(hello #{world} there)\n");
    }

    // --- Skip non-%q/%Q strings ---

    #[test]
    fn no_offense_for_plain_double_quoted_string() {
        test::<PercentQLiterals>().expect_no_offenses("x = \"hello\"\n");
    }

    #[test]
    fn no_offense_for_plain_single_quoted_string() {
        test::<PercentQLiterals>().expect_no_offenses("x = 'hello'\n");
    }

    // --- Autocorrect ---

    #[test]
    fn autocorrects_percent_upper_q_to_lower_q() {
        test::<PercentQLiterals>().expect_correction(
            indoc! {r#"
                name = %Q[Mix the foo into the baz.]
                       ^^^ Do not use `%Q` unless interpolation is needed. Use `%q`.
            "#},
            "name = %q[Mix the foo into the baz.]\n",
        );
    }

    #[test]
    fn upper_case_q_autocorrects_percent_lower_q_to_upper_q() {
        test::<PercentQLiterals>()
            .with_options(&PercentQLiteralsOptions {
                enforced_style: PercentQLiteralsStyle::UpperCaseQ,
            })
            .expect_correction(
                indoc! {r#"
                    name = %q/Mix the foo/
                           ^^^ Use `%Q` instead of `%q`.
                "#},
                "name = %Q/Mix the foo/\n",
            );
    }
}

murphy_plugin_api::submit_cop!(PercentQLiterals);
