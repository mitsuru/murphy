//! `Style/ColonMethodDefinition` — checks for `def self::foo` instead of `def self.foo`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ColonMethodDefinition
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Uses source-text scanning since Defs node does not preserve
//!   double-colon vs dot distinction for operator.
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, Range, cop};

const MSG: &str = "Do not use `::` for defining class methods.";

#[derive(Default)]
pub struct ColonMethodDefinition;

#[cop(
    name = "Style/ColonMethodDefinition",
    description = "Do not use `::` for defining class methods.",
    default_severity = "warning",
    default_enabled = true,
    options = murphy_plugin_api::NoOptions
)]
impl ColonMethodDefinition {
    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Defs { receiver, .. } = *cx.kind(node) else {
            return;
        };
        let recv_end = cx.range(receiver).end;
        let gap_src = cx.raw_source(Range {
            start: recv_end,
            end: recv_end + 2,
        });
        if gap_src != "::" {
            return;
        }
        let colon_range = Range {
            start: recv_end,
            end: recv_end + 2,
        };
        cx.emit_offense(colon_range, MSG, None);
        cx.emit_edit(colon_range, ".");
    }
}

#[cfg(test)]
mod tests {
    use super::ColonMethodDefinition;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_colon_method_definition() {
        test::<ColonMethodDefinition>().expect_offense(indoc! {"
            def self::bar
                   ^^ Do not use `::` for defining class methods.
            end
        "});
    }

    #[test]
    fn accepts_dot_method_definition() {
        test::<ColonMethodDefinition>().expect_no_offenses(
            "def self.bar\nend\n",
        );
    }
}
murphy_plugin_api::submit_cop!(ColonMethodDefinition);
