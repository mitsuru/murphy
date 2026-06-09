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
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Def { receiver, .. } = *cx.kind(node) else {
            return;
        };
        let Some(receiver) = receiver.get() else {
            return;
        };
        // Only flag `def self::foo`, not `def Foo::bar` (constant resolution).
        if !matches!(*cx.kind(receiver), NodeKind::SelfExpr) {
            return;
        }
        let node_range = cx.range(node);
        let src = cx.raw_source(node_range);
        let Some(self_pos) = src.find("self::") else {
            return;
        };
        let colon_start = node_range.start + self_pos as u32 + "self".len() as u32;
        let colon_range = Range {
            start: colon_start,
            end: colon_start + 2,
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

    #[test]
    fn accepts_constant_receiver_with_colon() {
        // `def Foo::bar` uses `::` for constant resolution, not a style issue.
        test::<ColonMethodDefinition>().expect_no_offenses(
            "def Foo::bar\nend\n",
        );
    }

    #[test]
    fn flags_colon_method_definition_with_args() {
        test::<ColonMethodDefinition>().expect_offense(indoc! {"
            def self::bar(x)
                    ^^ Do not use `::` for defining class methods.
            end
        "});
    }

    #[test]
    fn flags_single_line_colon_method_definition() {
        test::<ColonMethodDefinition>().expect_offense(indoc! {"
            def self::bar; end
                    ^^ Do not use `::` for defining class methods.
        "});
    }
}
murphy_plugin_api::submit_cop!(ColonMethodDefinition);
