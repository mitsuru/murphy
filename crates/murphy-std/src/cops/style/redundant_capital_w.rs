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

use super::string_literals::double_quotes_required;

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

        // If any child's source requires double-quote processing (e.g. \n, \t),
        // then %W may be needed over %w.
        let child_src = cx.raw_source(cx.range(child));
        if double_quotes_required(child_src) {
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
        // \n is a double-quote escape: double_quotes_required? returns true.
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
}

murphy_plugin_api::submit_cop!(RedundantCapitalW);
