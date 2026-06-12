//! `Layout/FirstHashElementLineBreak` — requires a line break before the
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/FirstHashElementLineBreak
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Ports `on_hash` + the shared `FirstElementLineBreak` mixin's
//!   `check_children_line_break`. Fires only for brace-delimited hashes
//!   (`node.loc.begin`) whose first element shares the opening `{` line in
//!   a multi-line hash. All element children — `pair` and `kwsplat` alike —
//!   participate in the line comparison (RuboCop passes `node.children`).
//!   `AllowMultilineFinalElement` is honoured. Autocorrect inserts a newline
//!   before the first element.
//! ```
//!
//! first element of a multi-line hash literal. Mirrors RuboCop's same-named
//! cop.

use crate::cops::util::check_children_line_break;
use murphy_plugin_api::{CopOptions, Cx, NodeId, cop};

const MSG: &str = "Add a line break before the first element of a multi-line hash.";

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct FirstHashElementLineBreak;

/// Options for [`FirstHashElementLineBreak`]. Matches RuboCop's key.
#[derive(CopOptions)]
pub struct FirstHashElementLineBreakOptions {
    #[option(
        name = "AllowMultilineFinalElement",
        default = false,
        description = "Allow the final element to span multiple lines without a leading line break."
    )]
    pub allow_multiline_final_element: bool,
}

#[cop(
    name = "Layout/FirstHashElementLineBreak",
    description = "Checks for a line break before the first element in a multi-line hash.",
    default_severity = "warning",
    // RuboCop ships this cop `Enabled: false` (opt-in). The `default.yml`
    // layer also disables it; this fallback keeps every config path faithful.
    default_enabled = false,
    options = FirstHashElementLineBreakOptions,
)]
impl FirstHashElementLineBreak {
    #[on_node(kind = "hash")]
    fn check_hash(&self, node: NodeId, cx: &Cx<'_>) {
        // RuboCop: `check_children_line_break(node, node.children) if node.loc.begin`.
        // A braced hash's source starts with `{`; brace-less (kwarg) hashes do not.
        if !cx.raw_source(cx.range(node)).starts_with('{') {
            return;
        }
        let opts = cx.options_or_default::<FirstHashElementLineBreakOptions>();
        check_children_line_break(
            cx,
            cx.range(node).start,
            cx.children(node).as_slice(),
            opts.allow_multiline_final_element,
            MSG,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{FirstHashElementLineBreak, FirstHashElementLineBreakOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_first_element_on_opening_line() {
        test::<FirstHashElementLineBreak>().expect_offense(indoc! {r#"
            { a: 1,
              ^^^^ Add a line break before the first element of a multi-line hash.
              b: 2 }
        "#});
    }

    #[test]
    fn corrects_first_element_on_opening_line() {
        test::<FirstHashElementLineBreak>().expect_correction(
            indoc! {r#"
                { a: 1,
                  ^^^^ Add a line break before the first element of a multi-line hash.
                  b: 2 }
            "#},
            "{ \na: 1,\n  b: 2 }\n",
        );
    }

    #[test]
    fn accepts_first_element_on_own_line() {
        test::<FirstHashElementLineBreak>().expect_no_offenses(indoc! {r#"
            {
              a: 1,
              b: 2 }
        "#});
    }

    #[test]
    fn accepts_single_line_hash() {
        test::<FirstHashElementLineBreak>().expect_no_offenses("{ a: 1, b: 2 }\n");
    }

    #[test]
    fn ignores_braceless_hash_argument() {
        // `foo(a: 1,\n  b: 2)` is a brace-less kwargs hash — not this cop's
        // concern (RuboCop gates on `node.loc.begin`).
        test::<FirstHashElementLineBreak>().expect_no_offenses(indoc! {r#"
            foo(a: 1,
              b: 2)
        "#});
    }

    #[test]
    fn flags_kwsplat_first_element() {
        test::<FirstHashElementLineBreak>().expect_offense(indoc! {r#"
            { **opts,
              ^^^^^^ Add a line break before the first element of a multi-line hash.
              b: 2 }
        "#});
    }

    #[test]
    fn accepts_multiline_final_element_when_allowed() {
        test::<FirstHashElementLineBreak>()
            .with_options(&FirstHashElementLineBreakOptions {
                allow_multiline_final_element: true,
            })
            .expect_no_offenses(indoc! {r#"
                { a: 1, b: {
                  c: 2
                } }
            "#});
    }

    #[test]
    fn flags_multiline_final_element_by_default() {
        test::<FirstHashElementLineBreak>().expect_offense(indoc! {r#"
            { a: 1, b: {
              ^^^^ Add a line break before the first element of a multi-line hash.
              c: 2
            } }
        "#});
    }
}
murphy_plugin_api::submit_cop!(FirstHashElementLineBreak);
