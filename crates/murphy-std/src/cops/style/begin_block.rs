//! `Style/BeginBlock` — avoid the use of `BEGIN` blocks.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/BeginBlock
//! upstream_version_checked: 1.86.2
//! version_added: "0.9"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Complete 1:1 port. No autocorrect (RuboCop has none — there is no
//!   drop-in replacement for BEGIN blocks). Offense is reported on the
//!   BEGIN keyword (first 5 bytes of the preexe node).
//! ```
//!
//! ## What is checked
//!
//! Any `BEGIN { ... }` block (Ruby preexe / `PreExecutionNode`) is flagged.
//! These are Perl-style constructs that execute code before the rest of the
//! file is parsed, making control flow harder to follow and reason about.
//!
//! ## No autocorrect
//!
//! There is no safe, mechanical replacement for a `BEGIN` block. RuboCop
//! does not autocorrect this cop, and neither does Murphy.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const MSG: &str = "Avoid the use of `BEGIN` blocks.";

#[derive(Default)]
pub struct BeginBlock;

#[cop(
    name = "Style/BeginBlock",
    description = "Avoid the use of `BEGIN` blocks.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl BeginBlock {
    #[on_node(kind = "preexe")]
    fn check(&self, node: NodeId, cx: &Cx<'_>) {
        if !matches!(*cx.kind(node), NodeKind::Preexe(_)) {
            return;
        }
        let node_range = cx.range(node);
        // The `BEGIN` keyword is the first 5 bytes of the preexe node.
        let keyword_range = Range {
            start: node_range.start,
            end: node_range.start + 5,
        };
        cx.emit_offense(keyword_range, MSG, None);
    }
}

#[cfg(test)]
mod tests {
    use super::BeginBlock;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_begin_block() {
        test::<BeginBlock>().expect_offense(indoc! {"
            BEGIN { test }
            ^^^^^ Avoid the use of `BEGIN` blocks.
        "});
    }

    #[test]
    fn flags_multiline_begin_block() {
        test::<BeginBlock>().expect_offense(indoc! {"
            BEGIN {
            ^^^^^ Avoid the use of `BEGIN` blocks.
              test
            }
        "});
    }

    #[test]
    fn accepts_regular_code() {
        test::<BeginBlock>().expect_no_offenses("test\n");
    }

    #[test]
    fn no_autocorrect_for_begin_block() {
        test::<BeginBlock>().expect_no_corrections(indoc! {"
            BEGIN { test }
        "});
    }
}

murphy_plugin_api::submit_cop!(BeginBlock);
