//! `Style/RedundantPercentQ` — flags `%q`/`%Q` when plain quotes would do.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantPercentQ
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Ports all three branches of RuboCop's `allowed_percent_q?`:
//!     1. %q/%Q containing both ' and " -> allowed (interpolated_quotes? guard).
//!     2. %q containing #{...} AND ' -> allowed (acceptable_q? guard).
//!     3. %q with \X non-backslash escapes -> allowed (escaped_non_backslash guard).
//!   acceptable_capital_q? is fully ported:
//!     %Q with " AND (interpolation OR double_quotes_required?) -> allowed.
//!   Autocorrect mirrors RuboCop: use " when %Q[no-double-quote] or body has ',
//!   otherwise use '.
//!   str segments inside dstr are skipped (parent-is-dstr guard, parity with
//!   RuboCop's on_str return-unless-string_literal? guard).
//! ```
//!
//! ## Detection logic
//!
//! A `%q`/`%Q` literal is redundant when:
//! - **`%q`**: the body does not contain both `'` and `"`, does not rely on
//!   `#{...}`-escaping, and has no `\X` non-backslash escape sequences.
//! - **`%Q`**: the body does not contain `"`, or (it contains `"` but has no
//!   interpolation and `double_quotes_required?` is false).
//!
//! ## Autocorrect
//!
//! Replace the opening `%q`/`%Q{delimiter}` with `'` or `"`, and the matching
//! closing delimiter with the same quote.
//!
//! Delimiter selection (mirrors RuboCop):
//! - Use `"` when: source starts with `%Q` and contains no `"`, OR source contains `'`.
//! - Use `'` otherwise.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

use super::string_literals::double_quotes_required;

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantPercentQ;

const DYNAMIC_EXTRA: &str = ", or for dynamic strings that contain double quotes";

#[cop(
    name = "Style/RedundantPercentQ",
    description = "Checks for %q/%Q when single quotes or double quotes would do.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RedundantPercentQ {
    #[on_node(kind = "str")]
    fn check_str(&self, node: NodeId, cx: &Cx<'_>) {
        // Skip str segments that are children of a dstr — the dstr handler
        // covers those (mirrors RuboCop's on_str guard that checks string_literal?
        // by verifying loc.begin is present on the *top-level* node).
        if cx.parent(node).get().is_some_and(|p| matches!(cx.kind(p), NodeKind::Dstr(_) | NodeKind::Dsym(_))) {
            return;
        }
        check(node, cx);
    }

    #[on_node(kind = "dstr")]
    fn check_dstr(&self, node: NodeId, cx: &Cx<'_>) {
        // Skip dstr segments that are children of another dstr (nested interpolation
        // segments) — only check top-level dstr nodes.
        if cx.parent(node).get().is_some_and(|p| matches!(cx.kind(p), NodeKind::Dstr(_) | NodeKind::Dsym(_))) {
            return;
        }
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let src = cx.raw_source(cx.range(node));

    // Only interested in %q and %Q literals.
    let is_percent_q = src.starts_with("%q");
    let is_percent_capital_q = src.starts_with("%Q");
    if !is_percent_q && !is_percent_capital_q {
        return;
    }

    // If the source contains both single and double quotes, the %q/%Q form
    // is justified (interpolated_quotes? guard).
    if src.contains('\'') && src.contains('"') {
        return;
    }

    // Check whether the %q/%Q is acceptable (not redundant).
    if is_percent_q && acceptable_q(src) {
        return;
    }
    if is_percent_capital_q && acceptable_capital_q(src) {
        return;
    }

    // Emit offense. Message mirrors RuboCop's MSG format.
    let q_type = if is_percent_q { "%q" } else { "%Q" };
    let extra = if is_percent_capital_q {
        DYNAMIC_EXTRA
    } else {
        ""
    };
    let message = format!(
        "Use `{q_type}` only for strings that contain both single quotes and double quotes{extra}."
    );

    let node_range = cx.range(node);
    cx.emit_offense(node_range, &message, None);

    // Autocorrect: determine target delimiter.
    // RuboCop: use `"` if source matches `%Q[^"]+` (starts with %Q and has no ")
    //          OR if source contains `'`. Otherwise use `'`.
    let use_double_quote = (is_percent_capital_q && !src.contains('"')) || src.contains('\'');
    let target_quote = if use_double_quote { '"' } else { '\'' };

    // The opening delimiter is `%q<x>` or `%Q<x>` — always 3 bytes.
    // The closing delimiter is the matching bracket/character — always 1 byte at the end.
    let open_range = Range {
        start: node_range.start,
        end: node_range.start + 3,
    };
    let close_range = Range {
        start: node_range.end - 1,
        end: node_range.end,
    };
    let q_str = target_quote.to_string();
    cx.emit_edit(open_range, &q_str);
    cx.emit_edit(close_range, &q_str);
}

/// `%q` is acceptable (NOT redundant) when:
/// - The body contains `#{...}` interpolation-like syntax AND a single quote
///   (converting to double-quoted would activate interpolation).
/// - The body has a `\X` escape where X is not `\` (non-backslash escape).
///
/// Mirrors RuboCop's `acceptable_q?`.
fn acceptable_q(src: &str) -> bool {
    // STRING_INTERPOLATION_REGEXP = /#\{.+\}/.freeze
    if has_interpolation_syntax(src) && src.contains('\'') {
        return true;
    }
    // ESCAPED_NON_BACKSLASH = /\\[^\\]/.freeze
    has_escaped_non_backslash(src)
}

/// `%Q` is acceptable (NOT redundant) when:
/// - The body contains `"` AND (has interpolation OR double_quotes_required?).
///
/// Mirrors RuboCop's `acceptable_capital_q?`.
fn acceptable_capital_q(src: &str) -> bool {
    src.contains('"') && (has_interpolation_syntax(src) || double_quotes_required(src))
}

/// True if `src` contains a `#{...}` interpolation-like sequence.
fn has_interpolation_syntax(src: &str) -> bool {
    let bytes = src.as_bytes();
    let mut i = 0;
    while i + 2 < bytes.len() {
        if bytes[i] == b'#' && bytes[i + 1] == b'{' {
            // Look for the closing `}`.
            if bytes[i + 2..].iter().position(|&b| b == b'}').is_some_and(|close| close > 0) {
                return true;
            }
        }
        i += 1;
    }
    false
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
            // Skip the pair `\\` (escaped backslash) — it does NOT count.
            i += 2;
            continue;
        }
        i += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- Offense cases ---

    #[test]
    fn flags_percent_q_plain_string() {
        test::<RedundantPercentQ>().expect_offense(indoc! {r#"
            name = %q(Bruce Wayne)
                   ^^^^^^^^^^^^^^^ Use `%q` only for strings that contain both single quotes and double quotes.
        "#});
    }

    #[test]
    fn flags_percent_capital_q_plain_string() {
        test::<RedundantPercentQ>().expect_offense(indoc! {r#"
            name = %Q(Bruce Wayne)
                   ^^^^^^^^^^^^^^^ Use `%Q` only for strings that contain both single quotes and double quotes, or for dynamic strings that contain double quotes.
        "#});
    }

    #[test]
    fn flags_percent_q_with_only_single_quote() {
        // %q(8 o'clock) — has ' but not " -> offense; autocorrect to "
        test::<RedundantPercentQ>().expect_offense(indoc! {r#"
            time = %q(8 o'clock)
                   ^^^^^^^^^^^^^ Use `%q` only for strings that contain both single quotes and double quotes.
        "#});
    }

    // --- No-offense cases ---

    #[test]
    fn no_offense_percent_q_with_both_quotes() {
        // Contains both ' and " — justified.
        test::<RedundantPercentQ>().expect_no_offenses("x = %q(it's a \"test\")\n");
    }

    #[test]
    fn no_offense_percent_q_with_interpolation_and_single_quote() {
        // %q(#{foo}'s) — has #{} AND ' -> acceptable_q returns true
        test::<RedundantPercentQ>().expect_no_offenses("x = %q(#{foo}'s)\n");
    }

    #[test]
    fn no_offense_percent_q_with_escaped_non_backslash() {
        // has \; (non-backslash escape) -> acceptable_q returns true
        test::<RedundantPercentQ>().expect_no_offenses("x = %q(foo\\;bar)\n");
    }

    #[test]
    fn no_offense_percent_capital_q_with_double_quote_and_interpolation() {
        // %Q(say "hi #{name}") — has " and #{} -> acceptable_capital_q returns true
        test::<RedundantPercentQ>().expect_no_offenses("x = %Q(say \"hi #{name}\")\n");
    }

    #[test]
    fn no_offense_for_plain_double_quoted_string() {
        test::<RedundantPercentQ>().expect_no_offenses("x = \"hello\"\n");
    }

    #[test]
    fn no_offense_for_plain_single_quoted_string() {
        test::<RedundantPercentQ>().expect_no_offenses("x = 'hello'\n");
    }

    // --- Autocorrect cases ---

    #[test]
    fn autocorrects_percent_q_plain_to_single_quote() {
        // No ' or " in body -> use '
        test::<RedundantPercentQ>().expect_correction(
            indoc! {r#"
                %q(Bruce Wayne)
                ^^^^^^^^^^^^^^^ Use `%q` only for strings that contain both single quotes and double quotes.
            "#},
            "'Bruce Wayne'\n",
        );
    }

    #[test]
    fn autocorrects_percent_q_with_single_quote_body_to_double_quote() {
        // Has ' -> use "
        test::<RedundantPercentQ>().expect_correction(
            indoc! {r#"
                %q(8 o'clock)
                ^^^^^^^^^^^^^ Use `%q` only for strings that contain both single quotes and double quotes.
            "#},
            "\"8 o'clock\"\n",
        );
    }

    #[test]
    fn autocorrects_percent_capital_q_plain_to_double_quote() {
        // %Q with no " -> use_double_quote = true -> "
        test::<RedundantPercentQ>().expect_correction(
            indoc! {r#"
                %Q(Bruce Wayne)
                ^^^^^^^^^^^^^^^ Use `%Q` only for strings that contain both single quotes and double quotes, or for dynamic strings that contain double quotes.
            "#},
            "\"Bruce Wayne\"\n",
        );
    }

    #[test]
    fn flags_percent_capital_q_interpolated_without_double_quote() {
        // %Q(hello #{name}) — dstr, has interpolation but no " -> not acceptable_capital_q
        test::<RedundantPercentQ>().expect_offense(indoc! {r#"
            x = %Q(hello #{name})
                ^^^^^^^^^^^^^^^^^ Use `%Q` only for strings that contain both single quotes and double quotes, or for dynamic strings that contain double quotes.
        "#});
    }
}

murphy_plugin_api::submit_cop!(RedundantPercentQ);
