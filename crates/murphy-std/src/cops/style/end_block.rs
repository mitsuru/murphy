//! `Style/EndBlock` — avoid the use of `END` blocks; use `Kernel#at_exit` instead.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/EndBlock
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Flags `END { ... }` blocks (postexe nodes) and autocorrects by replacing
//!   the `END` keyword with `at_exit`. This requires postexe nodes to be
//!   translated by murphy-translate, which was added alongside this cop.
//!   The offense is reported on the `END` keyword (first 3 bytes of the node).
//! ```
//!
//! ## What is checked
//!
//! Any `END { ... }` block (Ruby postexe / `PostExeNode`) is flagged.
//!
//! ## Autocorrect
//!
//! Replaces the `END` keyword with `at_exit`, producing `at_exit { ... }`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const MSG: &str = "Avoid the use of `END` blocks. Use `Kernel#at_exit` instead.";

#[derive(Default)]
pub struct EndBlock;

#[cop(
    name = "Style/EndBlock",
    description = "Avoid the use of `END` blocks. Use `Kernel#at_exit` instead.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl EndBlock {
    #[on_node(kind = "postexe")]
    fn check(&self, node: NodeId, cx: &Cx<'_>) {
        if !matches!(*cx.kind(node), NodeKind::Postexe(_)) {
            return;
        }
        let node_range = cx.range(node);
        // The `END` keyword is the first 3 bytes of the postexe node.
        let keyword_range = Range {
            start: node_range.start,
            end: node_range.start + 3,
        };
        cx.emit_offense(keyword_range, MSG, None);
        // Autocorrect: replace `END` with `at_exit`.
        cx.emit_edit(keyword_range, "at_exit");
    }
}

#[cfg(test)]
mod tests {
    use super::EndBlock;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_end_block() {
        test::<EndBlock>().expect_offense(indoc! {"
            END { puts 'Goodbye!' }
            ^^^ Avoid the use of `END` blocks. Use `Kernel#at_exit` instead.
        "});
    }

    #[test]
    fn flags_multiline_end_block() {
        test::<EndBlock>().expect_offense(indoc! {"
            END {
            ^^^ Avoid the use of `END` blocks. Use `Kernel#at_exit` instead.
              puts 'Goodbye!'
            }
        "});
    }

    #[test]
    fn autocorrects_to_at_exit() {
        test::<EndBlock>().expect_correction(
            indoc! {"
                END { puts 'Goodbye!' }
                ^^^ Avoid the use of `END` blocks. Use `Kernel#at_exit` instead.
            "},
            "at_exit { puts 'Goodbye!' }\n",
        );
    }

    #[test]
    fn accepts_at_exit() {
        test::<EndBlock>().expect_no_offenses("at_exit { puts 'Goodbye!' }\n");
    }
}

murphy_plugin_api::submit_cop!(EndBlock);
