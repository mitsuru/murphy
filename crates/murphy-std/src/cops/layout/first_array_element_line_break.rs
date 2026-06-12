//! `Layout/FirstArrayElementLineBreak` — requires a line break before the
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/FirstArrayElementLineBreak
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Ports `on_array` + the shared `FirstElementLineBreak` mixin's
//!   `check_children_line_break`. Fires when a multi-line array has its
//!   first element on the same line as the opening `[`. Skips bracket-less
//!   arrays unless an assignment ends the preceding line (RuboCop's
//!   `assignment_on_same_line?`). `AllowImplicitArrayLiterals` and
//!   `AllowMultilineFinalElement` options are honoured. Autocorrect inserts
//!   a newline before the first element.
//! ```
//!
//! first element of a multi-line array literal. Mirrors RuboCop's same-named
//! cop.

use crate::cops::util::check_children_line_break;
use murphy_plugin_api::{CopOptions, Cx, NodeId, cop};

const MSG: &str = "Add a line break before the first element of a multi-line array.";

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct FirstArrayElementLineBreak;

/// Options for [`FirstArrayElementLineBreak`]. Both keys match RuboCop.
#[derive(CopOptions)]
pub struct FirstArrayElementLineBreakOptions {
    #[option(
        name = "AllowMultilineFinalElement",
        default = false,
        description = "Allow the final element to span multiple lines without a leading line break."
    )]
    pub allow_multiline_final_element: bool,
    #[option(
        name = "AllowImplicitArrayLiterals",
        default = false,
        description = "Allow implicit (bracket-less) array literals to skip the line break."
    )]
    pub allow_implicit_array_literals: bool,
}

#[cop(
    name = "Layout/FirstArrayElementLineBreak",
    description = "Checks for a line break before the first element in a multi-line array.",
    default_severity = "warning",
    // RuboCop ships this cop `Enabled: false` (opt-in). The `default.yml`
    // layer also disables it; this fallback keeps every config path faithful.
    default_enabled = false,
    options = FirstArrayElementLineBreakOptions,
)]
impl FirstArrayElementLineBreak {
    #[on_node(kind = "array")]
    fn check_array(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<FirstArrayElementLineBreakOptions>();

        let bracketed = cx.is_bracketed(node);
        // RuboCop: `return if !node.loc.begin && !assignment_on_same_line?`.
        if !bracketed && !assignment_on_same_line(node, cx) {
            return;
        }
        // RuboCop: `return if allow_implicit_array_brackets? && !node.bracketed?`.
        if opts.allow_implicit_array_literals && !bracketed {
            return;
        }

        check_children_line_break(
            cx,
            cx.range(node).start,
            cx.array_elements(node),
            opts.allow_multiline_final_element,
            MSG,
        );
    }
}

/// RuboCop's `assignment_on_same_line?`: the source preceding the array on
/// its opening line ends with `=` (possibly surrounded by whitespace), as in
/// `a =\n  b, c`. Ported with byte slicing so multi-byte text is safe.
fn assignment_on_same_line(node: NodeId, cx: &Cx<'_>) -> bool {
    let start = cx.range(node).start as usize;
    let src = cx.source().as_bytes();
    let line_start = src[..start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |i| i + 1);
    let prefix = &src[line_start..start];
    // Equivalent to /\s*=\s*$/ — trailing whitespace then `=`.
    matches!(
        prefix.iter().rev().find(|&&b| b != b' ' && b != b'\t'),
        Some(&b'=')
    )
}

#[cfg(test)]
mod tests {
    use super::{FirstArrayElementLineBreak, FirstArrayElementLineBreakOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_first_element_on_opening_line() {
        test::<FirstArrayElementLineBreak>().expect_offense(indoc! {r#"
            [ :a,
              ^^ Add a line break before the first element of a multi-line array.
              :b ]
        "#});
    }

    #[test]
    fn corrects_first_element_on_opening_line() {
        test::<FirstArrayElementLineBreak>().expect_correction(
            indoc! {r#"
                [ :a,
                  ^^ Add a line break before the first element of a multi-line array.
                  :b ]
            "#},
            "[ \n:a,\n  :b ]\n",
        );
    }

    #[test]
    fn accepts_first_element_on_own_line() {
        test::<FirstArrayElementLineBreak>().expect_no_offenses(indoc! {r#"
            [
              :a,
              :b ]
        "#});
    }

    #[test]
    fn accepts_single_line_array() {
        test::<FirstArrayElementLineBreak>().expect_no_offenses("[:a, :b]\n");
    }

    #[test]
    fn accepts_implicit_array_without_assignment() {
        // Bracket-less array not on an assignment line: not this cop's concern.
        test::<FirstArrayElementLineBreak>().expect_no_offenses(indoc! {r#"
            foo :a,
              :b
        "#});
    }

    #[test]
    fn flags_implicit_array_on_assignment_line() {
        test::<FirstArrayElementLineBreak>().expect_offense(indoc! {r#"
            a = :b,
                ^^ Add a line break before the first element of a multi-line array.
              :c
        "#});
    }

    #[test]
    fn accepts_multiline_final_element_when_allowed() {
        // First element `a` shares the opening line, but the array only spans
        // multiple lines because its final element's `{ … }` body does.
        // `AllowMultilineFinalElement` ignores that final span → no offense.
        test::<FirstArrayElementLineBreak>()
            .with_options(&FirstArrayElementLineBreakOptions {
                allow_multiline_final_element: true,
                allow_implicit_array_literals: false,
            })
            .expect_no_offenses(indoc! {r#"
                [a, {
                  b: 1
                }]
            "#});
    }

    #[test]
    fn flags_multiline_final_element_by_default() {
        test::<FirstArrayElementLineBreak>().expect_offense(indoc! {r#"
            [a, {
             ^ Add a line break before the first element of a multi-line array.
              b: 1
            }]
        "#});
    }

    #[test]
    fn accepts_implicit_array_literal_when_allowed() {
        test::<FirstArrayElementLineBreak>()
            .with_options(&FirstArrayElementLineBreakOptions {
                allow_multiline_final_element: false,
                allow_implicit_array_literals: true,
            })
            .expect_no_offenses(indoc! {r#"
                a = :b,
                  :c
            "#});
    }
}
murphy_plugin_api::submit_cop!(FirstArrayElementLineBreak);
