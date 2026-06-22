//! `Style/ColonMethodDefinition` — checks for `def self::foo` instead of `def self.foo`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ColonMethodDefinition
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   RuboCop inspects every `defs` node and flags when `loc.operator`
//!   is `::`, regardless of whether the receiver is `self` or a
//!   constant. Murphy has no operator location, so the operator is
//!   located as the first `.`/`::` source token after the receiver's
//!   range end (Ruby only allows a `self` or single-const receiver in
//!   a `defs`, so a naive `::` scan would be wrong only for receiver-
//!   internal scope-resolution, which is not valid syntax here). The
//!   token-after-receiver approach is robust to whitespace such as
//!   `def self . bar`.
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, SourceTokenKind, cop};

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
        // Only singleton method definitions (`def self.foo` / `def Foo.foo`)
        // have a receiver. RuboCop flags whenever the receiver-to-name
        // operator is `::`, for both `self` and constant receivers.
        let NodeKind::Def { receiver, .. } = *cx.kind(node) else {
            return;
        };
        let Some(receiver) = receiver.get() else {
            return;
        };
        // The operator is the first `.`/`::` token after the receiver's
        // range. The receiver's range fully spans its own bytes (e.g.
        // `self` or `Foo`), so this never picks up a token inside the
        // receiver. Ruby disallows multi-segment receivers in a `defs`
        // (`def Foo::Bar.baz` is a syntax error), so the first operator
        // token is always the def operator.
        let recv_end = cx.range(receiver).end;
        let node_end = cx.range(node).end;
        let toks = cx.sorted_tokens();
        let idx = toks.partition_point(|t| t.range.start < recv_end);
        let Some(op) = toks[idx..]
            .iter()
            .take_while(|t| t.range.start < node_end)
            .find(|t| {
                t.kind == SourceTokenKind::Other
                    && matches!(cx.raw_source(t.range), "." | "::")
            })
        else {
            return;
        };
        if cx.raw_source(op.range) != "::" {
            return;
        }
        cx.emit_offense(op.range, MSG, None);
        cx.emit_edit(op.range, ".");
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
    fn flags_constant_receiver_with_colon() {
        // RuboCop flags `def Foo::bar` exactly like `def self::bar`:
        // the receiver-to-name operator is `::`.
        test::<ColonMethodDefinition>().expect_offense(indoc! {"
            def Foo::bar
                   ^^ Do not use `::` for defining class methods.
            end
        "});
    }

    #[test]
    fn accepts_constant_receiver_with_dot() {
        test::<ColonMethodDefinition>().expect_no_offenses(
            "def Foo.bar\nend\n",
        );
    }

    #[test]
    fn flags_spaced_colon_operator() {
        // The operator is located by token, so surrounding whitespace
        // does not throw off the offense range.
        test::<ColonMethodDefinition>().expect_offense(indoc! {"
            def self :: bar
                     ^^ Do not use `::` for defining class methods.
            end
        "});
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
