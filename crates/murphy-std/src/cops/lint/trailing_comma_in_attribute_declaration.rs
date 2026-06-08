//! `Lint/TrailingCommaInAttributeDeclaration` — checks for trailing commas in
//! attribute declarations.
//!
//! Leaving a trailing comma in an attribute declaration (e.g. `attr_reader :foo,`)
//! will nullify the next method definition by overriding it with a getter method.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/TrailingCommaInAttributeDeclaration
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   No autocorrect. Parity with RuboCop: flags trailing commas in
//!   `attr_reader`, `attr_writer`, `attr_accessor`, and `attr`
//!   declarations.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! attr_reader :foo,
//!
//! # good
//! attr_reader :foo
//! ```
//!
//! ## No autocorrect
//!
//! Autocorrect is not provided.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

const MSG: &str = "Avoid leaving a trailing comma in attribute declarations.";

#[derive(Default)]
pub struct TrailingCommaInAttributeDeclaration;

#[cop(
    name = "Lint/TrailingCommaInAttributeDeclaration",
    description = "Checks for trailing commas in attribute declarations.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl TrailingCommaInAttributeDeclaration {
    #[on_node(kind = "send", methods = ["attr_reader", "attr_writer", "attr_accessor", "attr"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let args = cx.call_arguments(node);
        if args.is_empty() {
            return;
        }

        let last_arg = *args.last().unwrap();
        let last_arg_end = cx.range(last_arg).end;

        // Search for a comma immediately after the last argument
        if let Some(tok) = cx.token_after(last_arg_end)
            && tok.kind == SourceTokenKind::Comma
            && tok.range.start < last_arg_end + 3
        {
            cx.emit_offense(tok.range, MSG, None);
            return;
        }

        if args.len() > 1 && matches!(cx.kind(last_arg), NodeKind::Def { .. } | NodeKind::Defs { .. }) {
            let second_last_arg = args[args.len() - 2];
            let second_last_end = cx.range(second_last_arg).end;
            let last_start = cx.range(last_arg).start;
            if let Some(range) = cx
                .tokens_in(Range {
                    start: second_last_end,
                    end: last_start,
                })
                .iter()
                .find(|tok| tok.kind == SourceTokenKind::Comma)
                .map(|tok| tok.range)
            {
                cx.emit_offense(range, MSG, None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TrailingCommaInAttributeDeclaration;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_trailing_comma() {
        test::<TrailingCommaInAttributeDeclaration>().expect_offense(indoc! {r#"
            attr_reader :bar,
                            ^ Avoid leaving a trailing comma in attribute declarations.
        "#});
    }

    #[test]
    fn accepts_no_trailing_comma() {
        test::<TrailingCommaInAttributeDeclaration>().expect_no_offenses(indoc! {"
            class Foo
              attr_reader :bar

              def baz
                puts 'Qux'
              end
            end
        "});
    }
}

murphy_plugin_api::submit_cop!(TrailingCommaInAttributeDeclaration);
