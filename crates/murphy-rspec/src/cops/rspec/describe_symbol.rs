//! `RSpec/DescribeSymbol` — avoid describing a symbol.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/DescribeSymbol
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `describe_symbol?` (`(send #rspec? :describe $sym ...)`)
//!   with `RESTRICT_ON_SEND = [:describe]`. Receiver gating (bare plus
//!   `RSpec`/`::RSpec`) mirrors `#rspec?`; offense range is the `Sym` first
//!   arg per `add_offense(match)`. No autocorrect upstream, none here.
//!   Fires on nested groups too (no TopLevelGroup gating upstream).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with `methods = ["describe"]`. Flags when the first
//! positional arg is a `Sym`:
//!
//! - `describe :my_method do ... end` — flagged.
//! - `RSpec.describe :sym, "desc" do ... end` — flagged.
//! - `describe "#method"` — string, not flagged.
//! - `context :sym` — different selector, not flagged (dispatched only on
//!   `describe`).
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; the fix (string vs method description)
//! needs human judgement.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::is_rspec_or_bare_receiver;

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct DescribeSymbol;

#[cop(
    name = "RSpec/DescribeSymbol",
    description = "Avoid describing symbols.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl DescribeSymbol {
    #[on_node(kind = "send", methods = ["describe"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, args, .. } = *cx.kind(node) else {
            return;
        };
        if !is_rspec_or_bare_receiver(cx, receiver) {
            return;
        }
        let arg_ids = cx.list(args);
        let Some(&first) = arg_ids.first() else {
            return;
        };
        if !matches!(*cx.kind(first), NodeKind::Sym(_)) {
            return;
        }
        // Silence unused-import lint for OptNodeId re-export parity with peers.
        let _ = OptNodeId::NONE;
        cx.emit_offense(cx.range(first), "Avoid describing symbols.", None);
    }
}

#[cfg(test)]
mod tests {
    use super::DescribeSymbol;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_describe_symbol() {
        test::<DescribeSymbol>().expect_offense(indoc! {r#"
                describe(:some_method) { }
                         ^^^^^^^^^^^^ Avoid describing symbols.
            "#});
    }

    #[test]
    fn flags_describe_symbol_with_extra_args() {
        test::<DescribeSymbol>().expect_offense(indoc! {r#"
                describe(:some_method, "description") { }
                         ^^^^^^^^^^^^ Avoid describing symbols.
            "#});
    }

    #[test]
    fn flags_rspec_describe_symbol() {
        test::<DescribeSymbol>().expect_offense(indoc! {r#"
                RSpec.describe(:some_method, "description") { }
                               ^^^^^^^^^^^^ Avoid describing symbols.
            "#});
    }

    #[test]
    fn flags_nested_describe_symbol() {
        test::<DescribeSymbol>().expect_offense(indoc! {r#"
                RSpec.describe Foo do
                  describe :to_s do
                           ^^^^^ Avoid describing symbols.
                  end
                end
            "#});
    }

    #[test]
    fn does_not_flag_string_arg() {
        test::<DescribeSymbol>().expect_no_offenses(indoc! {r##"
                describe("#some_method") { }
            "##});
    }

    #[test]
    fn does_not_flag_context_symbol() {
        // `context :sym` is a different selector; this cop only watches `describe`.
        test::<DescribeSymbol>().expect_no_offenses(indoc! {r#"
                context(:some_method) { }
            "#});
    }

    #[test]
    fn does_not_flag_non_rspec_receiver() {
        test::<DescribeSymbol>().expect_no_offenses(indoc! {r#"
                Other.describe(:sym) { }
            "#});
    }
}

murphy_plugin_api::submit_cop!(DescribeSymbol);
