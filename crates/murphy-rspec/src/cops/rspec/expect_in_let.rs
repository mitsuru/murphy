//! `RSpec/ExpectInLet` — do not use `expect` in `let`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/ExpectInLet
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_block` (`let?` with a non-nil body, then
//!   `expectation` search over the body subtree). `let?` is
//!   `(block (send nil? #Helpers.all ...) ...)` — bare `let` / `let!`
//!   only; the `send` + `block_pass` form never reaches `on_block`, so
//!   only `Block` is handled (upstream also skips `numblock` here via
//!   `rubocop:disable InternalAffairs/NumblockHandler`). Each match
//!   reports its selector per `add_offense(expect.loc.selector)` with
//!   `Do not use \`%<expect>s\` in let`. No autocorrect upstream, none
//!   here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block`. Flags every bare expectation call inside a
//! bare `let` / `let!` body:
//!
//! - `let(:foo) do expect(foo).to eq(1) end` — flagged.
//! - `let!(:foo) do expect(foo).to eq(1) end` — flagged.
//! - `let(:foo) do should eq(1) end` — flagged (`should` selector).
//! - `it do expect(foo).to eq(1) end` — not a helper, not flagged.
//! - `obj.let(:foo) do expect(foo).to eq(1) end` — explicit receiver,
//!   not flagged.
//! - `let(:foo) do foo end` — no expectation, not flagged.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; moving the expectation into an example
//! needs human judgement.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

/// Expectation selectors (`Expectations` in rubocop-rspec's default
/// config): `are_expected`, `expect`, `expect_any_instance_of`,
/// `is_expected`, `should`, `should_not`, `should_not_receive`,
/// `should_receive`.
fn is_expectation_name(name: &str) -> bool {
    matches!(
        name,
        "are_expected"
            | "expect"
            | "expect_any_instance_of"
            | "is_expected"
            | "should"
            | "should_not"
            | "should_not_receive"
            | "should_receive"
    )
}

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ExpectInLet;

#[cop(
    name = "RSpec/ExpectInLet",
    description = "Do not use `expect` in let.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl ExpectInLet {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, body, .. } = *cx.kind(node) else {
            return;
        };
        let NodeKind::Send { receiver, method, .. } = *cx.kind(call) else {
            return;
        };
        if receiver != OptNodeId::NONE {
            return;
        }
        if !matches!(cx.symbol_str(method), "let" | "let!") {
            return;
        }
        let Some(body_id) = body.get() else {
            return;
        };
        for id in core::iter::once(body_id).chain(cx.descendants(body_id)) {
            let NodeKind::Send {
                receiver,
                method: expect_method,
                ..
            } = *cx.kind(id)
            else {
                continue;
            };
            if receiver != OptNodeId::NONE {
                continue;
            }
            let expect_name = cx.symbol_str(expect_method);
            if !is_expectation_name(expect_name) {
                continue;
            }
            cx.emit_offense(
                cx.node(id).loc.name,
                &format!("Do not use `{expect_name}` in let"),
                None,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ExpectInLet;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_expect_in_let() {
        test::<ExpectInLet>().expect_offense(indoc! {r#"
                let(:foo) do
                  expect(foo).to eq(1)
                  ^^^^^^ Do not use `expect` in let
                end
            "#});
    }

    #[test]
    fn flags_expect_in_let_bang() {
        test::<ExpectInLet>().expect_offense(indoc! {r#"
                let!(:foo) do
                  expect(foo).to eq(1)
                  ^^^^^^ Do not use `expect` in let
                end
            "#});
    }

    #[test]
    fn flags_brace_let() {
        test::<ExpectInLet>().expect_offense(indoc! {r#"
                let(:foo) { expect(foo).to eq(1) }
                            ^^^^^^ Do not use `expect` in let
            "#});
    }

    #[test]
    fn flags_should_in_let() {
        test::<ExpectInLet>().expect_offense(indoc! {r#"
                let(:foo) do
                  should eq(1)
                  ^^^^^^ Do not use `should` in let
                end
            "#});
    }

    #[test]
    fn does_not_flag_expect_in_example() {
        test::<ExpectInLet>().expect_no_offenses(indoc! {r#"
                it do
                  expect(foo).to eq(1)
                end
            "#});
    }

    #[test]
    fn does_not_flag_explicit_receiver_let() {
        // Upstream `let?` requires a bare (`nil?`) receiver.
        test::<ExpectInLet>().expect_no_offenses(indoc! {r#"
                obj.let(:foo) do
                  expect(foo).to eq(1)
                end
            "#});
    }

    #[test]
    fn does_not_flag_let_without_expectation() {
        test::<ExpectInLet>().expect_no_offenses(indoc! {r#"
                let(:foo) do
                  foo
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(ExpectInLet);
