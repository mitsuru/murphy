//! `RSpec/BeEq` — prefer `be` over `eq` for booleans and nil.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/BeEq
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `eq_type_with_identity?` (`(send nil? :eq {boolean
//!   nil})`) with `RESTRICT_ON_SEND = [:eq]`. Bare `eq` with a single
//!   boolean / `nil` arg flags regardless of the surrounding runner
//!   (`to` / `not_to` / `to_not` all flag upstream since the pattern does
//!   not check the parent). The offense range is the `eq` selector per
//!   `add_offense(node.loc.selector)`. Detection is at parity; autocorrect
//!   (replace `eq` with `be`) is not ported in this batch — same
//!   convention as `RSpec/BeEql` (status: partial, autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with `methods = ["eq"]`. Flags when bare with a
//! single identity arg:
//!
//! - `expect(foo).to eq(true)` — flagged (range is `eq`).
//! - `expect(foo).not_to eq(false)` — flagged.
//! - `expect(foo).to eq(nil)` — flagged.
//! - `expect(foo).to eq(0)` — integer, not flagged.
//! - `expect(foo).to eq(:sym)` — symbol, not flagged.
//! - `expect(foo).to eq("str")` — string, not flagged.
//!
//! ## No autocorrect
//!
//! Upstream replaces `eq` with `be`. This batch reports only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct BeEq;

#[cop(
    name = "RSpec/BeEq",
    description = "Check for expectations where be can replace eq.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl BeEq {
    #[on_node(kind = "send", methods = ["eq"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send {
            receiver,
            method: _,
            args,
        } = *cx.kind(node)
        else {
            return;
        };
        if receiver != OptNodeId::NONE {
            return;
        }
        let arg_ids = cx.list(args);
        if arg_ids.len() != 1 {
            return;
        }
        if !matches!(
            *cx.kind(arg_ids[0]),
            NodeKind::True_ | NodeKind::False_ | NodeKind::Nil
        ) {
            return;
        }
        cx.emit_offense(
            cx.node(node).loc.name,
            "Prefer `be` over `eq`.",
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::BeEq;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_eq_true() {
        test::<BeEq>().expect_offense(indoc! {r#"
                expect(foo).to eq(true)
                               ^^ Prefer `be` over `eq`.
            "#});
    }

    #[test]
    fn flags_eq_false_not_to() {
        test::<BeEq>().expect_offense(indoc! {r#"
                expect(foo).not_to eq(false)
                                   ^^ Prefer `be` over `eq`.
            "#});
    }

    #[test]
    fn flags_eq_nil_to_not() {
        test::<BeEq>().expect_offense(indoc! {r#"
                expect(foo).to_not eq(nil)
                                   ^^ Prefer `be` over `eq`.
            "#});
    }

    #[test]
    fn does_not_flag_eq_int() {
        test::<BeEq>().expect_no_offenses(indoc! {r#"
                expect(foo).to eq(0)
            "#});
    }

    #[test]
    fn does_not_flag_eq_float() {
        test::<BeEq>().expect_no_offenses(indoc! {r#"
                expect(foo).to eq(1.0)
            "#});
    }

    #[test]
    fn does_not_flag_eq_sym() {
        test::<BeEq>().expect_no_offenses(indoc! {r#"
                expect(foo).to eq(:foo)
            "#});
    }

    #[test]
    fn does_not_flag_eq_string() {
        test::<BeEq>().expect_no_offenses(indoc! {r#"
                expect(foo).to eq("foo")
            "#});
    }

    #[test]
    fn does_not_flag_receiver_eq() {
        // Upstream requires a bare (`nil?`) receiver.
        test::<BeEq>().expect_no_offenses(indoc! {r#"
                foo.eq(true)
            "#});
    }
}

murphy_plugin_api::submit_cop!(BeEq);
