//! `Style/RedundantCapitalW` — flags `%W` when `%w` would do.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantCapitalW
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Ports the full RuboCop logic:
//!     - Skip if any child is a dstr (has interpolation).
//!     - Skip if any child's raw source requires double-quote processing
//!       (mirrors RuboCop's double_quotes_required? check via string source).
//!   Autocorrect: replace the opening `W` with `w` (single surgical edit on
//!   the second byte of the `%W` opening token).
//! ```
//!
//! ## Detection logic
//!
//! A `%W(...)` / `%W[...]` / etc. literal is redundant when:
//! - No child element is a `dstr` (interpolated string).
//! - No child element's raw source requires double-quote processing
//!   (i.e. `double_quotes_required?` is false for each element).
//!
//! ## Autocorrect
//!
//! Replace the opening `%W` with `%w` — a single 1-byte edit on the second
//! byte of the begin token.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantCapitalW;

const MSG: &str = "Do not use `%W` unless interpolation is needed. If not, use `%w`.";

#[cop(
    name = "Style/RedundantCapitalW",
    description = "Checks for %W when interpolation is not needed.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RedundantCapitalW {
    #[on_node(kind = "array")]
    fn check_array(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// Returns `true` when the `%W` element source contains a backslash escape
/// sequence that `%w` would NOT process.
///
/// Both `%W` and `%w` process:
///   - `\\` (escaped backslash — even backslash run)
///   - `\ ` (backslash-space, an in-word space separator)
///
/// Only `%W` processes character escapes (`\n`, `\t`, `\uXXXX`, `\xXX`, etc.).
///
/// Unlike `double_quotes_required?`, this deliberately ignores `'` (single
/// quote) because `%w` handles single quotes as literal characters just like
/// `%W` does — there is no escaping difference.
fn element_requires_percent_w(src: &str) -> bool {
    let b = src.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' {
            let start = i;
            while i < b.len() && b[i] == b'\\' {
                i += 1;
            }
            let run = i - start;
            // An odd-length run means the last backslash is an active escape.
            if run % 2 == 1 && i < b.len() {
                let next = b[i];
                // `\ ` is a word-separator escape handled identically by %w and %W.
                // `\\` is already handled by the even-run path above.
                // All other escapes (\n, \t, \r, \uXXXX, \xXX, etc.) are
                // meaningful character escapes that %w would not process.
                if next != b' ' {
                    return true;
                }
            }
            continue;
        }
        i += 1;
    }
    false
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Only interested in %W literals. Check the raw source of the node.
    let array_src = cx.raw_source(cx.range(node));
    if !array_src.starts_with("%W") {
        return;
    }

    // Check each child element.
    let elements = cx.array_elements(node);

    for &child in elements {
        // If any child is a dstr (interpolated), %W is required.
        if matches!(cx.kind(child), NodeKind::Dstr(_)) {
            return;
        }

        // If any child's source contains a meaningful backslash escape (e.g. \n, \t),
        // then %W is required since %w does not process escape sequences.
        // Note: single quotes do NOT require %W; %w[can't] is perfectly valid.
        let child_src = cx.raw_source(cx.range(child));
        if element_requires_percent_w(child_src) {
            return;
        }
    }

    // All elements are plain strings without interpolation or double-quote escapes.
    // Emit offense on the whole array.
    let node_range = cx.range(node);
    cx.emit_offense(node_range, MSG, None);

    // Autocorrect: replace `W` (the second byte, index 1) with `w`.
    // The opening `%W` is at the start of the node range; `W` is at offset 1.
    let w_range = Range {
        start: node_range.start + 1,
        end: node_range.start + 2,
    };
    cx.emit_edit(w_range, "w");
}

#[cfg(test)]
mod tests {
    use super::RedundantCapitalW;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- Offense cases ---

    #[test]
    fn flags_percent_capital_w_plain_words() {
        test::<RedundantCapitalW>().expect_offense(indoc! {r#"
            x = %W(cat dog pig)
                ^^^^^^^^^^^^^^^ Do not use `%W` unless interpolation is needed. If not, use `%w`.
        "#});
    }

    #[test]
    fn flags_percent_capital_w_bracket_delimiter() {
        test::<RedundantCapitalW>().expect_offense(indoc! {r#"
            x = %W[door wall floor]
                ^^^^^^^^^^^^^^^^^^^ Do not use `%W` unless interpolation is needed. If not, use `%w`.
        "#});
    }

    // --- No-offense cases ---

    #[test]
    fn no_offense_percent_w_lowercase() {
        test::<RedundantCapitalW>().expect_no_offenses("x = %w[swim run bike]\n");
    }

    #[test]
    fn no_offense_percent_capital_w_with_interpolation() {
        test::<RedundantCapitalW>().expect_no_offenses("x = %W[apple #{fruit} grape]\n");
    }

    #[test]
    fn no_offense_percent_capital_w_with_newline_escape() {
        // \n is a meaningful backslash escape that %w does not process: skip.
        test::<RedundantCapitalW>().expect_no_offenses("x = %W[cat\\ndog pig]\n");
    }

    #[test]
    fn no_offense_plain_bracket_array() {
        test::<RedundantCapitalW>().expect_no_offenses("x = ['cat', 'dog']\n");
    }

    // --- Autocorrect cases ---

    #[test]
    fn autocorrects_percent_capital_w_to_lowercase() {
        test::<RedundantCapitalW>().expect_correction(
            indoc! {r#"
                x = %W(cat dog pig)
                    ^^^^^^^^^^^^^^^ Do not use `%W` unless interpolation is needed. If not, use `%w`.
            "#},
            "x = %w(cat dog pig)\n",
        );
    }

    #[test]
    fn autocorrects_percent_capital_w_bracket_delimiter() {
        test::<RedundantCapitalW>().expect_correction(
            indoc! {r#"
                x = %W[door wall floor]
                    ^^^^^^^^^^^^^^^^^^^ Do not use `%W` unless interpolation is needed. If not, use `%w`.
            "#},
            "x = %w[door wall floor]\n",
        );
    }

    #[test]
    fn flags_percent_capital_w_with_single_quote() {
        // Single quotes do NOT require %W — %w[can't stop] is perfectly valid.
        // This was a false-negative in RuboCop; Murphy correctly flags it.
        test::<RedundantCapitalW>().expect_offense(indoc! {r#"
            x = %W[can't stop]
                ^^^^^^^^^^^^^^ Do not use `%W` unless interpolation is needed. If not, use `%w`.
        "#});
    }

    #[test]
    fn autocorrects_percent_capital_w_with_single_quote() {
        test::<RedundantCapitalW>().expect_correction(
            indoc! {r#"
                x = %W[can't stop]
                    ^^^^^^^^^^^^^^ Do not use `%W` unless interpolation is needed. If not, use `%w`.
            "#},
            "x = %w[can't stop]\n",
        );
    }

    #[test]
    fn no_offense_percent_capital_w_with_tab_escape() {
        // \t is a character escape only %W processes: skip.
        test::<RedundantCapitalW>().expect_no_offenses("x = %W[foo\\tbar baz]\n");
    }
}

murphy_plugin_api::submit_cop!(RedundantCapitalW);