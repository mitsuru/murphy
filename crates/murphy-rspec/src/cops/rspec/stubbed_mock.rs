//! `RSpec/StubbedMock` — message expectations must not configure a response.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/StubbedMock
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_send` (`RESTRICT_ON_SEND = [:to]`) with
//!   `expectation` (`(send $(send nil? #Expectations.all ...) :to $_)`:
//!   a bare expectation call — `expect`, `is_expected`,
//!   `expect_any_instance_of`, `should`-family — running `.to` with a
//!   matcher arg) and the four `on_expectation` shapes:
//!   `matcher_with_configured_response` (a single-arg `and_return` /
//!   `and_raise` / `and_throw` / `and_yield` / `and_call_original` /
//!   `and_wrap_original` on a message expectation), `matcher_with_return_block`
//!   (an empty-args block on a message expectation),
//!   `matcher_with_hash` (`receive_messages` with a hash, or
//!   `receive_message_chain` with a trailing hash), and
//!   `matcher_with_blockpass` (a trailing `&block` on `receive` /
//!   `receive_message_chain` / `receive(...).with`). The offense is the
//!   expectation call per `add_offense(expectation)` with `Prefer
//!   %<replacement>s over `%<method_name>s` when configuring a
//!   response.` Detection is at parity (verified vs 3.7.0, including the
//!   zero-arg `and_call_original`, block-with-params, unparenthesised
//!   `&block` on `to`, and `should`-with-receiver clean cases);
//!   upstream ships no autocorrect and none is added here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with `methods = ["to"]`:
//!
//! - `expect(foo).to receive(:bar).with(42).and_return("hi")` —
//!   flagged (the `expect(foo)` selector).
//! - `is_expected.to receive(:foo).and_return('bar')` — flagged (the
//!   `is_expected` selector, `allow(subject)` replacement).
//! - `expect(foo).to receive(:bar) { 'baz' }` — flagged (return block).
//! - `expect(foo).to receive_messages(foo: 'bar')` — flagged.
//! - `expect(foo).to(receive(:foo, &canned))` — flagged (block-pass).
//! - `allow(foo).to receive(:bar).with(42).and_return("hi")` —
//!   `allow` needs no rewrite, clean.
//! - `expect(foo).to receive(:bar).with(42)` — no response, clean.
//! - `expect(foo).to receive(:x).and_call_original` — zero-arg
//!   `and_call_original`, clean.
//! - `expect(foo).to receive(:bar) { |arg| arg }` — block with params,
//!   clean.
//! - `foo.should receive(:bar).and_return(1)` — explicit receiver,
//!   clean.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; swapping `expect` for `allow` changes
//! semantics and needs human judgement.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct StubbedMock;

#[cop(
    name = "RSpec/StubbedMock",
    description = "Checks that message expectations do not have a configured response.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl StubbedMock {
    // `methods = ["to"]` mirrors upstream `RESTRICT_ON_SEND = [:to]`.
    #[on_node(kind = "send", methods = ["to"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send {
            receiver, method, ..
        } = *cx.kind(node)
        else {
            return;
        };
        let _ = method;
        let Some(expectation) = receiver.get() else {
            return;
        };
        let NodeKind::Send {
            receiver: inner_receiver,
            method: inner_method,
            args: inner_args,
        } = *cx.kind(expectation)
        else {
            return;
        };
        // `(send nil? #Expectations.all ...)` — bare expectation call.
        if inner_receiver != OptNodeId::NONE {
            return;
        }
        let method_name = cx.symbol_str(inner_method).to_owned();
        if !is_expectation_name(&method_name) {
            return;
        }
        let _ = inner_args;
        // `$_` — the matcher is the first `.to` argument.
        let Some(&matcher) = cx.call_arguments(node).first() else {
            return;
        };
        if !has_configured_response(cx, matcher) {
            return;
        }
        cx.emit_offense(
            cx.range(expectation),
            &format!(
                "Prefer {} over `{}` when configuring a response.",
                replacement(&method_name),
                method_name
            ),
            None,
        );
    }
}

/// `true` when `name` is an `Expectations.all` selector
/// (`config/default.yml` Language section).
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

fn replacement(method_name: &str) -> &'static str {
    match method_name {
        "expect" => "`allow`",
        "is_expected" => "`allow(subject)`",
        "expect_any_instance_of" => "`allow_any_instance_of`",
        _ => "an allow statement",
    }
}

/// Any of the four `on_expectation` matcher shapes.
fn has_configured_response(cx: &Cx<'_>, matcher: NodeId) -> bool {
    has_configured_response_send(cx, matcher)
        || has_return_block(cx, matcher)
        || has_response_hash(cx, matcher)
        || has_blockpass(cx, matcher)
}

/// `matcher_with_configured_response`: `(send #message_expectation?
/// #configured_response? _)` — exactly one arg.
fn has_configured_response_send(cx: &Cx<'_>, matcher: NodeId) -> bool {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(matcher)
    else {
        return false;
    };
    if !is_configured_response_name(cx.symbol_str(method)) {
        return false;
    }
    if cx.list(args).len() != 1 {
        return false;
    }
    let Some(recv) = receiver.get() else {
        return false;
    };
    is_message_expectation(cx, recv)
}

/// `matcher_with_return_block`: `(block #message_expectation? (args) _)` —
/// empty block args, any body (upstream `_` also matches an empty body).
fn has_return_block(cx: &Cx<'_>, matcher: NodeId) -> bool {
    let NodeKind::Block { call, args, .. } = *cx.kind(matcher) else {
        return false;
    };
    if !is_message_expectation(cx, call) {
        return false;
    }
    let NodeKind::Args(list) = *cx.kind(args) else {
        return false;
    };
    cx.list(list).is_empty()
}

/// `matcher_with_hash`: `receive_messages(hash)` (single hash arg) or
/// `receive_message_chain(... hash)` (trailing hash arg).
fn has_response_hash(cx: &Cx<'_>, matcher: NodeId) -> bool {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(matcher)
    else {
        return false;
    };
    if receiver != OptNodeId::NONE {
        return false;
    }
    let arg_ids = cx.list(args);
    match cx.symbol_str(method) {
        "receive_messages" => {
            arg_ids.len() == 1 && matches!(*cx.kind(arg_ids[0]), NodeKind::Hash(_))
        }
        "receive_message_chain" => arg_ids
            .last()
            .is_some_and(|&last| matches!(*cx.kind(last), NodeKind::Hash(_))),
        _ => false,
    }
}

/// `matcher_with_blockpass`: `receive` / `receive_message_chain` with a
/// trailing `&block`, or `receive(...).with(..., &block)`.
fn has_blockpass(cx: &Cx<'_>, matcher: NodeId) -> bool {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(matcher)
    else {
        return false;
    };
    let arg_ids = cx.list(args);
    let trailing_blockpass = arg_ids
        .last()
        .is_some_and(|&last| matches!(*cx.kind(last), NodeKind::BlockPass(_)));
    let name = cx.symbol_str(method);
    if receiver == OptNodeId::NONE
        && matches!(name, "receive" | "receive_message_chain")
    {
        return trailing_blockpass;
    }
    // `receive(...).with(..., &block)`: the `.with` receiver must be a
    // bare `receive` (not `receive_message_chain`).
    if name == "with"
        && let Some(recv) = receiver.get()
        && let NodeKind::Send {
            receiver: inner_receiver,
            method: inner_method,
            ..
        } = *cx.kind(recv)
        && inner_receiver == OptNodeId::NONE
        && cx.symbol_str(inner_method) == "receive"
    {
        return trailing_blockpass;
    }
    false
}

fn is_configured_response_name(name: &str) -> bool {
    matches!(
        name,
        "and_return"
            | "and_raise"
            | "and_throw"
            | "and_yield"
            | "and_call_original"
            | "and_wrap_original"
    )
}

/// `message_expectation?`: a bare `receive` / `receive_message_chain`
/// send, or `.with` on a bare `receive` send.
fn is_message_expectation(cx: &Cx<'_>, id: NodeId) -> bool {
    let NodeKind::Send {
        receiver,
        method,
        ..
    } = *cx.kind(id)
    else {
        return false;
    };
    let name = cx.symbol_str(method);
    if receiver == OptNodeId::NONE {
        return matches!(name, "receive" | "receive_message_chain");
    }
    if name != "with" {
        return false;
    }
    let Some(recv) = receiver.get() else {
        return false;
    };
    let NodeKind::Send {
        receiver: inner_receiver,
        method: inner_method,
        ..
    } = *cx.kind(recv)
    else {
        return false;
    };
    inner_receiver == OptNodeId::NONE && cx.symbol_str(inner_method) == "receive"
}

#[cfg(test)]
mod tests {
    use super::StubbedMock;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_expect_with_configured_response() {
        test::<StubbedMock>().expect_offense(indoc! {r#"
                expect(foo).to receive(:bar).with(42).and_return("hello world")
                ^^^^^^^^^^^ Prefer `allow` over `expect` when configuring a response.
            "#});
    }

    #[test]
    fn flags_is_expected_with_configured_response() {
        test::<StubbedMock>().expect_offense(indoc! {r#"
                is_expected.to receive(:foo).and_return('bar')
                ^^^^^^^^^^^ Prefer `allow(subject)` over `is_expected` when configuring a response.
            "#});
    }

    #[test]
    fn flags_expect_any_instance_of() {
        test::<StubbedMock>().expect_offense(indoc! {r#"
                expect_any_instance_of(Officer).to receive(:alert).and_return(true)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `allow_any_instance_of` over `expect_any_instance_of` when configuring a response.
            "#});
    }

    #[test]
    fn flags_return_block() {
        test::<StubbedMock>().expect_offense(indoc! {r#"
                expect(foo).to receive(:bar) { 'baz' }
                ^^^^^^^^^^^ Prefer `allow` over `expect` when configuring a response.
            "#});
    }

    #[test]
    fn flags_receive_messages_hash() {
        test::<StubbedMock>().expect_offense(indoc! {r#"
                expect(foo).to receive_messages(foo: 'bar')
                ^^^^^^^^^^^ Prefer `allow` over `expect` when configuring a response.
            "#});
    }

    #[test]
    fn flags_chain_with_trailing_hash() {
        test::<StubbedMock>().expect_offense(indoc! {r#"
                expect(foo).to receive_message_chain(:a, :b, c: 'd')
                ^^^^^^^^^^^ Prefer `allow` over `expect` when configuring a response.
            "#});
    }

    #[test]
    fn flags_parenthesised_blockpass() {
        test::<StubbedMock>().expect_offense(indoc! {r#"
                expect(foo).to(receive(:foo, &canned))
                ^^^^^^^^^^^ Prefer `allow` over `expect` when configuring a response.
            "#});
    }

    #[test]
    fn flags_with_with_blockpass() {
        test::<StubbedMock>().expect_offense(indoc! {r#"
                expect(foo).to receive(:foo).with('bar', &canned)
                ^^^^^^^^^^^ Prefer `allow` over `expect` when configuring a response.
            "#});
    }

    #[test]
    fn flags_and_raise() {
        test::<StubbedMock>().expect_offense(indoc! {r#"
                expect(foo).to receive(:x).and_raise(SomeError)
                ^^^^^^^^^^^ Prefer `allow` over `expect` when configuring a response.
            "#});
    }

    #[test]
    fn ignores_allow_with_response() {
        test::<StubbedMock>().expect_no_offenses(indoc! {r#"
                allow(foo).to receive(:bar).with(42).and_return("hello world")
            "#});
    }

    #[test]
    fn ignores_expect_without_response() {
        test::<StubbedMock>().expect_no_offenses(indoc! {r#"
                expect(foo).to receive(:bar).with(42)
            "#});
    }

    #[test]
    fn ignores_zero_arg_and_call_original() {
        test::<StubbedMock>().expect_no_offenses(indoc! {r#"
                expect(foo).to receive(:x).and_call_original
            "#});
    }

    #[test]
    fn ignores_block_with_params() {
        test::<StubbedMock>().expect_no_offenses(indoc! {r#"
                expect(foo).to receive(:bar) { |arg| arg }
            "#});
    }

    #[test]
    fn ignores_should_with_receiver() {
        test::<StubbedMock>().expect_no_offenses(indoc! {r#"
                foo.should receive(:bar).and_return(1)
            "#});
    }

    #[test]
    fn ignores_not_to_with_response() {
        // `RESTRICT_ON_SEND` is `[:to]`; `not_to` never dispatches.
        test::<StubbedMock>().expect_no_offenses(indoc! {r#"
                expect(foo).not_to receive(:bar).and_return(1)
            "#});
    }

    #[test]
    fn flags_trailing_blockpass_on_receive() {
        test::<StubbedMock>().expect_offense(indoc! {r#"
                expect(foo).to receive(:bar, &canned)
                ^^^^^^^^^^^ Prefer `allow` over `expect` when configuring a response.
            "#});
    }
}

murphy_plugin_api::submit_cop!(StubbedMock);
