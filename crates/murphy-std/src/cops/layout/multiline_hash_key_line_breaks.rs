//! `Layout/MultilineHashKeyLineBreaks` — each key in a multi-line hash literal
//! must start on a separate line.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/MultilineHashKeyLineBreaks
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Ports `on_hash` + the shared `MultilineElementLineBreaks` mixin's
//!   `check_line_breaks(node, node.children, ignore_last:)`. Fires only for
//!   brace-delimited hashes (`starts_with_curly_brace?` / `node.loc.begin`),
//!   not kwargs hashes (those belong to
//!   `Layout/MultilineMethodArgumentLineBreaks`). All element children —
//!   `pair` and `kwsplat` alike — participate (RuboCop passes
//!   `node.children`), so the port iterates `cx.children`.
//!
//!   Each child after the first must start on a line strictly after the
//!   previous kept child's last line; otherwise an offense and a
//!   leading-newline autocorrect is emitted. `AllowMultilineFinalElement`
//!   is honoured: with the default `false`, a multi-line trailing element
//!   forces the whole hash multi-line; with `true`, only start lines are
//!   compared.
//! ```
//!
//! ## Matched shapes
//!
//! Brace-delimited `hash` nodes spanning more than one physical line where
//! two or more elements share a line.

use crate::cops::util::check_element_line_breaks;
use murphy_plugin_api::{CopOptions, Cx, NodeId, cop};

const MSG: &str = "Each key in a multi-line hash must start on a separate line.";

/// Stateless unit struct (ADR 0035).
#[derive(Default)]
pub struct MultilineHashKeyLineBreaks;

/// Options for [`MultilineHashKeyLineBreaks`]. Matches RuboCop's key.
#[derive(CopOptions)]
pub struct MultilineHashKeyLineBreaksOptions {
    #[option(
        name = "AllowMultilineFinalElement",
        default = false,
        description = "Allow the final element to span multiple lines without forcing each key onto its own line."
    )]
    pub allow_multiline_final_element: bool,
}

#[cop(
    name = "Layout/MultilineHashKeyLineBreaks",
    description = "Checks that each key in a multi-line hash literal starts on a separate line.",
    default_severity = "warning",
    // RuboCop ships this cop `Enabled: false` (opt-in); the bundled
    // `default.yml` disables it too. This fallback keeps every config path
    // faithful.
    default_enabled = false,
    options = MultilineHashKeyLineBreaksOptions,
)]
impl MultilineHashKeyLineBreaks {
    #[on_node(kind = "hash")]
    fn check_hash(&self, node: NodeId, cx: &Cx<'_>) {
        // RuboCop: `return unless starts_with_curly_brace?(node)` and
        // `return unless node.loc.begin`. A braced hash's source starts with
        // `{`; brace-less (kwarg) hashes do not. This excludes kwargs hashes,
        // which `Layout/MultilineMethodArgumentLineBreaks` handles.
        if !cx.raw_source(cx.range(node)).starts_with('{') {
            return;
        }
        let opts = cx.options_or_default::<MultilineHashKeyLineBreaksOptions>();
        // RuboCop passes `node.children` — every pair and kwsplat.
        check_element_line_breaks(
            cx,
            &cx.children(node),
            opts.allow_multiline_final_element,
            MSG,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{MultilineHashKeyLineBreaks, MultilineHashKeyLineBreaksOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    fn allow_final() -> MultilineHashKeyLineBreaksOptions {
        MultilineHashKeyLineBreaksOptions {
            allow_multiline_final_element: true,
        }
    }

    #[test]
    fn flags_keys_sharing_a_line() {
        test::<MultilineHashKeyLineBreaks>().expect_offense(indoc! {"
            {
              a: 1, b: 2,
                    ^^^^ Each key in a multi-line hash must start on a separate line.
              c: 3
            }
        "});
    }

    #[test]
    fn accepts_each_key_on_own_line() {
        test::<MultilineHashKeyLineBreaks>().expect_no_offenses(indoc! {"
            {
              a: 1,
              b: 2,
              c: 3
            }
        "});
    }

    #[test]
    fn accepts_single_line_hash() {
        test::<MultilineHashKeyLineBreaks>().expect_no_offenses("{ a: 1, b: 2 }\n");
    }

    #[test]
    fn accepts_empty_hash() {
        test::<MultilineHashKeyLineBreaks>().expect_no_offenses("{}\n");
    }

    // kwargs hash (no braces) is out of scope — handled by
    // Layout/MultilineMethodArgumentLineBreaks.
    #[test]
    fn ignores_brace_less_kwargs_hash() {
        test::<MultilineHashKeyLineBreaks>().expect_no_offenses(indoc! {"
            foo(a: 1, b: 2,
              c: 3)
        "});
    }

    // A `**kwsplat` is a `node.children` element too, so it participates in the
    // line-break check alongside the pairs.
    #[test]
    fn flags_kwsplat_sharing_a_line() {
        test::<MultilineHashKeyLineBreaks>().expect_offense(indoc! {"
            {
              **h, a: 1,
                   ^^^^ Each key in a multi-line hash must start on a separate line.
              b: 2
            }
        "});
    }

    #[test]
    fn corrects_keys_sharing_a_line() {
        test::<MultilineHashKeyLineBreaks>().expect_correction(
            indoc! {"
                {
                  a: 1, b: 2,
                        ^^^^ Each key in a multi-line hash must start on a separate line.
                  c: 3
                }
            "},
            "{\n  a: 1, \nb: 2,\n  c: 3\n}\n",
        );
    }

    // AllowMultilineFinalElement: false (default) flags a trailing multi-line
    // value that shares the opening line with earlier keys.
    #[test]
    fn default_flags_multiline_final_element() {
        test::<MultilineHashKeyLineBreaks>().expect_offense(indoc! {"
            { a: 1, b: foo(
                    ^^^^^^^ Each key in a multi-line hash must start on a separate line.
              bar
            )}
        "});
    }

    // AllowMultilineFinalElement: true accepts the same shape — only the
    // elements' start lines are compared, and both start on line 1.
    #[test]
    fn allow_final_accepts_multiline_final_element() {
        test::<MultilineHashKeyLineBreaks>()
            .with_options(&allow_final())
            .expect_no_offenses(indoc! {"
                { a: 1, b: foo(
                  bar
                )}
            "});
    }
}

murphy_plugin_api::submit_cop!(MultilineHashKeyLineBreaks);
