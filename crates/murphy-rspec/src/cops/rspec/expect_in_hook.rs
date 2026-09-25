//! `RSpec/ExpectInHook` — do not use `expect` in hooks.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/ExpectInHook
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_block` (aliased to `on_numblock`): `hook?`
//!   (`(any_block (send nil? #Hooks.all ...) ...)`, bare receiver only)
//!   with a non-nil body, then `expectation` search
//!   (`(send nil? #Expectations.all ...)`, bare receiver only) over the
//!   body subtree. Each match reports its selector per
//!   `add_offense(expect.loc.selector)` with
//!   `Do not use \`%<expect>s\` in \`%<hook>s\` hook`. No autocorrect
//!   upstream, none here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` and `Numblock`. Flags every bare expectation
//! call inside a bare hook body:
//!
//! - `before do expect(foo).to eq(1) end` — flagged (`expect` selector).
//! - `after do expect_any_instance_of(Foo).to receive(:bar) end` —
//!   flagged (`expect_any_instance_of` selector).
//! - `before do should eq(1) end` — flagged (`should` selector).
//! - `it do expect(foo).to eq(1) end` — not a hook, not flagged.
//! - `obj.before do expect(foo).to eq(1) end` — explicit receiver,
//!   not a hook per upstream `nil?`, not flagged.
//! - `before do ex.run end` — no expectation, not flagged.
//! - `before(:all) do expect(foo).to eq(1) end` — hook args do not
//!   matter, still flagged.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; moving the expectation into an example
//! needs human judgement.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::is_hook_name;

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
pub struct ExpectInHook;

#[cop(
    name = "RSpec/ExpectInHook",
    description = "Do not use `expect` in hooks such as `before`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl ExpectInHook {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, body, .. } = *cx.kind(node) else {
            return;
        };
        let Some(body_id) = body.get() else {
            return;
        };
        let Some(hook_name) = hook_call_name(cx, call) else {
            return;
        };
        check_body(cx, body_id, &hook_name);
    }

    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Numblock { send, body, .. } = *cx.kind(node) else {
            return;
        };
        let Some(body_id) = body.get() else {
            return;
        };
        let Some(hook_name) = hook_call_name(cx, send) else {
            return;
        };
        check_body(cx, body_id, &hook_name);
    }
}

/// The hook selector when `call` is a bare hook send, else `None`.
///
/// Mirrors upstream `hook?`: `nil?` receiver plus `Hooks.all` method.
fn hook_call_name(cx: &Cx<'_>, call: NodeId) -> Option<String> {
    let NodeKind::Send { receiver, method, .. } = *cx.kind(call) else {
        return None;
    };
    if receiver != OptNodeId::NONE {
        return None;
    }
    let name = cx.symbol_str(method);
    if !is_hook_name(name) {
        return None;
    }
    Some(name.to_owned())
}

/// Flag every bare expectation send in the `body` subtree.
fn check_body(cx: &Cx<'_>, body: NodeId, hook_name: &str) {
    for id in core::iter::once(body).chain(cx.descendants(body)) {
        let NodeKind::Send { receiver, method, .. } = *cx.kind(id) else {
            continue;
        };
        if receiver != OptNodeId::NONE {
            continue;
        }
        let expect_name = cx.symbol_str(method);
        if !is_expectation_name(expect_name) {
            continue;
        }
        cx.emit_offense(
            cx.node(id).loc.name,
            &format!("Do not use `{expect_name}` in `{hook_name}` hook"),
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::ExpectInHook;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_expect_in_before() {
        test::<ExpectInHook>().expect_offense(indoc! {r#"
                before do
                  expect(foo).to eq(1)
                  ^^^^^^ Do not use `expect` in `before` hook
                end
            "#});
    }

    #[test]
    fn flags_expect_any_instance_of_in_after() {
        test::<ExpectInHook>().expect_offense(indoc! {r#"
                after do
                  expect_any_instance_of(Foo).to receive(:bar)
                  ^^^^^^^^^^^^^^^^^^^^^^ Do not use `expect_any_instance_of` in `after` hook
                end
            "#});
    }

    #[test]
    fn flags_should_in_before() {
        test::<ExpectInHook>().expect_offense(indoc! {r#"
                before do
                  should eq(1)
                  ^^^^^^ Do not use `should` in `before` hook
                end
            "#});
    }

    #[test]
    fn flags_expect_in_scoped_hook() {
        // Hook scope args do not matter to this cop.
        test::<ExpectInHook>().expect_offense(indoc! {r#"
                before(:all) do
                  expect(foo).to eq(1)
                  ^^^^^^ Do not use `expect` in `before` hook
                end
            "#});
    }

    #[test]
    fn flags_nested_expect_in_hook() {
        // The search covers the whole body subtree.
        test::<ExpectInHook>().expect_offense(indoc! {r#"
                before do
                  foo(expect(bar))
                      ^^^^^^ Do not use `expect` in `before` hook
                end
            "#});
    }

    #[test]
    fn does_not_flag_expect_in_example() {
        test::<ExpectInHook>().expect_no_offenses(indoc! {r#"
                it do
                  expect(foo).to eq(1)
                end
            "#});
    }

    #[test]
    fn does_not_flag_explicit_receiver_hook() {
        // Upstream `hook?` requires a bare (`nil?`) receiver.
        test::<ExpectInHook>().expect_no_offenses(indoc! {r#"
                obj.before do
                  expect(foo).to eq(1)
                end
            "#});
    }

    #[test]
    fn does_not_flag_hook_without_expectation() {
        test::<ExpectInHook>().expect_no_offenses(indoc! {r#"
                around do |ex|
                  ex.run
                end
            "#});
    }

    #[test]
    fn does_not_flag_empty_hook() {
        test::<ExpectInHook>().expect_no_offenses(indoc! {r#"
                before do
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(ExpectInHook);
