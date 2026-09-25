//! `RSpec/IdenticalEqualityAssertion` — flag equality checks with identical sides.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/IdenticalEqualityAssertion
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `equality_check?` (`(send (send nil? :expect $_) :to
//!   {(send nil? {:eql :eq :be} $_) (send (send nil? :be) :== $_)})`)
//!   with `RESTRICT_ON_SEND = [:to]`. Only `to` matches (`to_not` /
//!   `not_to` never flag); the receiver must be a bare single-arg
//!   `expect`; the single matcher arg must be a bare single-arg `eq` /
//!   `eql` / `be`, or a bare zero-arg `be` compared with `==` to a single
//!   value. Side equality is decided by `raw_source` text comparison
//!   (whitespace sensitive), the same convention as
//!   `Lint/BinaryOperatorWithIdenticalOperands`; RuboCop uses structural
//!   AST `==`. The offense range is the `to` node per `add_offense(node)`.
//!   No autocorrect upstream, none here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with `methods = ["to"]`:
//!
//! - `expect(foo.bar).to eq(foo.bar)` — flagged.
//! - `expect(foo.bar).to eql(foo.bar)` — flagged.
//! - `expect(foo.bar).to be(foo.bar)` — flagged.
//! - `expect(foo.bar).to be == foo.bar` — flagged.
//! - `expect(foo.bar).to eq(2)` — different sides, not flagged.
//! - `expect(foo).to eq(bar)` — different sides, not flagged.
//! - `expect(foo.bar).not_to eq(foo.bar)` — `not_to` never flags.
//! - `expect(foo).to eq` — arg-less matcher, not flagged.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; identical sides need human judgement.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct IdenticalEqualityAssertion;

#[cop(
    name = "RSpec/IdenticalEqualityAssertion",
    description = "Checks for equality assertions with identical expressions on both sides.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl IdenticalEqualityAssertion {
    #[on_node(kind = "send", methods = ["to"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send {
            receiver,
            args: to_args,
            ..
        } = *cx.kind(node)
        else {
            return;
        };
        let Some(expect_id) = receiver.get() else {
            return;
        };
        let NodeKind::Send {
            receiver: e_recv,
            method: e_method,
            args: e_args,
        } = *cx.kind(expect_id)
        else {
            return;
        };
        if e_recv != OptNodeId::NONE || cx.symbol_str(e_method) != "expect" {
            return;
        };
        let e_arg_ids = cx.list(e_args);
        if e_arg_ids.len() != 1 {
            return;
        }
        let to_arg_ids = cx.list(to_args);
        if to_arg_ids.len() != 1 {
            return;
        }
        let matcher_id = to_arg_ids[0];
        let NodeKind::Send {
            receiver: m_recv,
            method: m_method,
            args: m_args,
        } = *cx.kind(matcher_id)
        else {
            return;
        };
        let m_name = cx.symbol_str(m_method);
        // `eq(Y)` / `eql(Y)` / `be(Y)`: bare single-arg matcher.
        let right = if m_recv == OptNodeId::NONE && matches!(m_name, "eq" | "eql" | "be") {
            let m_arg_ids = cx.list(m_args);
            if m_arg_ids.len() != 1 {
                return;
            }
            m_arg_ids[0]
        // `be == Y`: `==` on a bare zero-arg `be` with a single value.
        } else if m_name == "==" {
            let Some(be_id) = m_recv.get() else {
                return;
            };
            let NodeKind::Send {
                receiver: be_recv,
                method: be_method,
                args: be_args,
            } = *cx.kind(be_id)
            else {
                return;
            };
            if be_recv != OptNodeId::NONE || cx.symbol_str(be_method) != "be" {
                return;
            }
            if !cx.list(be_args).is_empty() {
                return;
            }
            let m_arg_ids = cx.list(m_args);
            if m_arg_ids.len() != 1 {
                return;
            }
            m_arg_ids[0]
        } else {
            return;
        };
        let left_src = cx.raw_source(cx.range(e_arg_ids[0]));
        let right_src = cx.raw_source(cx.range(right));
        if left_src != right_src {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            "Identical expressions on both sides of the equality may indicate a flawed test.",
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::IdenticalEqualityAssertion;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_eq_with_identical_sides() {
        test::<IdenticalEqualityAssertion>().expect_offense(indoc! {r#"
                expect(foo.bar).to eq(foo.bar)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Identical expressions on both sides of the equality may indicate a flawed test.
            "#});
    }

    #[test]
    fn flags_eql_with_identical_sides() {
        test::<IdenticalEqualityAssertion>().expect_offense(indoc! {r#"
                expect(foo.bar).to eql(foo.bar)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Identical expressions on both sides of the equality may indicate a flawed test.
            "#});
    }

    #[test]
    fn flags_be_with_identical_sides() {
        test::<IdenticalEqualityAssertion>().expect_offense(indoc! {r#"
                expect(foo.bar).to be(foo.bar)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Identical expressions on both sides of the equality may indicate a flawed test.
            "#});
    }

    #[test]
    fn flags_be_equals_with_identical_sides() {
        test::<IdenticalEqualityAssertion>().expect_offense(indoc! {r#"
                expect(foo.bar).to be == foo.bar
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Identical expressions on both sides of the equality may indicate a flawed test.
            "#});
    }

    #[test]
    fn does_not_flag_different_sides() {
        test::<IdenticalEqualityAssertion>().expect_no_offenses(indoc! {r#"
                expect(foo.bar).to eq(2)
            "#});
        test::<IdenticalEqualityAssertion>().expect_no_offenses(indoc! {r#"
                expect(foo).to eq(bar)
            "#});
    }

    #[test]
    fn does_not_flag_not_to() {
        // Upstream restricts to `to` only.
        test::<IdenticalEqualityAssertion>().expect_no_offenses(indoc! {r#"
                expect(foo.bar).not_to eq(foo.bar)
            "#});
    }

    #[test]
    fn does_not_flag_argless_matcher() {
        test::<IdenticalEqualityAssertion>().expect_no_offenses(indoc! {r#"
                expect(foo).to eq
            "#});
    }

    #[test]
    fn does_not_flag_be_with_arg_equals() {
        // `be(1) == 1` is not the bare-`be` shape upstream matches.
        test::<IdenticalEqualityAssertion>().expect_no_offenses(indoc! {r#"
                expect(foo).to be(1) == 1
            "#});
    }
}

murphy_plugin_api::submit_cop!(IdenticalEqualityAssertion);
