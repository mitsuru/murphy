//! `Layout/MultilineArrayLineBreaks` — each item in a multi-line array literal
//! must start on a separate line.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/MultilineArrayLineBreaks
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Ports `on_array` + the shared `MultilineElementLineBreaks` mixin's
//!   `check_line_breaks(node, node.children, ignore_last:)`. For an array
//!   the children are exactly the elements, so the port iterates
//!   `node.children` via `check_element_line_breaks`. The mixin reports an
//!   offense (one per offending element) on each element that does not start
//!   on a line strictly after the previous kept element's last line, except
//!   the first; `all_on_same_line?` short-circuits single-line arrays.
//!
//!   `AllowMultilineFinalElement` is honoured: with the default `false`, a
//!   multi-line trailing element forces the whole array multi-line; with
//!   `true`, only the elements' start lines are compared so a trailing
//!   element that merely spans lines is not flagged. Autocorrect inserts a
//!   newline before each offending element (RuboCop's
//!   `EmptyLineCorrector.insert_before`).
//! ```
//!
//! ## Matched shapes
//!
//! `array` nodes spanning more than one physical line where two or more
//! elements share a line.

use crate::cops::util::check_element_line_breaks;
use murphy_plugin_api::{CopOptions, Cx, NodeId, cop};

const MSG: &str = "Each item in a multi-line array must start on a separate line.";

/// Stateless unit struct (ADR 0035).
#[derive(Default)]
pub struct MultilineArrayLineBreaks;

/// Options for [`MultilineArrayLineBreaks`]. Matches RuboCop's key.
#[derive(CopOptions)]
pub struct MultilineArrayLineBreaksOptions {
    #[option(
        name = "AllowMultilineFinalElement",
        default = false,
        description = "Allow the final element to span multiple lines without forcing each item onto its own line."
    )]
    pub allow_multiline_final_element: bool,
}

#[cop(
    name = "Layout/MultilineArrayLineBreaks",
    description = "Checks that each item in a multi-line array literal starts on a separate line.",
    default_severity = "warning",
    // RuboCop ships this cop `Enabled: false` (opt-in); the bundled
    // `default.yml` disables it too. This fallback keeps every config path
    // faithful.
    default_enabled = false,
    options = MultilineArrayLineBreaksOptions,
)]
impl MultilineArrayLineBreaks {
    #[on_node(kind = "array")]
    fn check_array(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<MultilineArrayLineBreaksOptions>();
        // RuboCop passes `node.children`; for an array these are the elements.
        check_element_line_breaks(
            cx,
            cx.array_elements(node),
            opts.allow_multiline_final_element,
            MSG,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{MultilineArrayLineBreaks, MultilineArrayLineBreaksOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    fn allow_final() -> MultilineArrayLineBreaksOptions {
        MultilineArrayLineBreaksOptions {
            allow_multiline_final_element: true,
        }
    }

    #[test]
    fn flags_items_sharing_a_line() {
        test::<MultilineArrayLineBreaks>().expect_offense(indoc! {"
            [
              a, b,
                 ^ Each item in a multi-line array must start on a separate line.
              c
            ]
        "});
    }

    #[test]
    fn accepts_each_item_on_own_line() {
        test::<MultilineArrayLineBreaks>().expect_no_offenses(indoc! {"
            [
              a,
              b,
              c
            ]
        "});
    }

    #[test]
    fn accepts_single_line_array() {
        test::<MultilineArrayLineBreaks>().expect_no_offenses("[a, b, c]\n");
    }

    #[test]
    fn accepts_empty_array() {
        test::<MultilineArrayLineBreaks>().expect_no_offenses("[]\n");
    }

    #[test]
    fn accepts_multiline_nested_element_on_own_line() {
        test::<MultilineArrayLineBreaks>().expect_no_offenses(indoc! {"
            [
              a,
              b,
              foo(
                bar
              )
            ]
        "});
    }

    #[test]
    fn corrects_items_sharing_a_line() {
        test::<MultilineArrayLineBreaks>().expect_correction(
            indoc! {"
                [
                  a, b,
                     ^ Each item in a multi-line array must start on a separate line.
                  c
                ]
            "},
            "[\n  a, \nb,\n  c\n]\n",
        );
    }

    // AllowMultilineFinalElement: false (default) flags a trailing multi-line
    // element that shares the opening line with earlier elements. Both `b` and
    // `foo(...)` share line 1 with the kept first element `a`, so both are
    // flagged (RuboCop's `last_seen_line` stays at `a`'s last line).
    #[test]
    fn default_flags_multiline_final_element() {
        test::<MultilineArrayLineBreaks>().expect_offense(indoc! {"
            [a, b, foo(
                ^ Each item in a multi-line array must start on a separate line.
                   ^^^^ Each item in a multi-line array must start on a separate line.
              bar
            )]
        "});
    }

    // AllowMultilineFinalElement: true accepts the same shape — only the
    // elements' start lines are compared, and all start on line 1.
    #[test]
    fn allow_final_accepts_multiline_final_element() {
        test::<MultilineArrayLineBreaks>()
            .with_options(&allow_final())
            .expect_no_offenses(indoc! {"
                [a, b, foo(
                  bar
                )]
            "});
    }
}

murphy_plugin_api::submit_cop!(MultilineArrayLineBreaks);
