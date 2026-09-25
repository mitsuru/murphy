//! `RSpec/IteratedExpectation` — use the `all` matcher instead of iterating.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/IteratedExpectation
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_block` (`each?`: `(block (send ... :each)
//!   (args (arg $_)) (...))`) and `on_numblock` (`each_numblock?`,
//!   argument `_1`). A body that is a single `expect(lvar).to ...`
//!   (`single_expectation?`) or whose every child is one
//!   (`only_expectations?`) flags the `each` send per
//!   `add_offense(node.send_node)` with `Prefer using the `all` matcher
//!   instead of iterating over an array.` `expectation?` is `(send (send
//!   nil? :expect (lvar %)) :to ...)` — bare `expect` with exactly the
//!   block variable, `:to` only (`not_to` is clean), any matcher args.
//!   Two deliberate narrowings, both verified vs 3.7.0: the `each` send
//!   must carry no call args (`(send ... :each)` has no trailing `...`,
//!   so `[a].each(1) { }` is clean) and must have an explicit receiver —
//!   bare `each { |u| expect(u).to x }` crashes upstream
//!   (`single_expectation_replacement` calls `node.receiver.source` on
//!   nil) and reports no offense, so it is clean here too. Detection is
//!   at parity otherwise; autocorrect (rewrite to
//!   `expect(coll).to all(matcher)`) is not ported in this batch — same
//!   convention as `RSpec/IncludeExamples` (status: partial, autocorrect
//!   as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` (`each` send, exactly one plain block arg) and
//! `Numblock` (`each` send, `_1`):
//!
//! - `[a].each { |u| expect(u).to be_valid }` — flagged (the send).
//! - `[a].each { expect(_1).to be_valid }` — flagged (the send).
//! - `[a].each { |u| expect(u).to x; expect(u).to y }` — all children
//!   expectations, flagged.
//! - `[a].each { |u| expect(u).to x; puts u }` — mixed body, clean.
//! - `[a].each { |u| expect(u).not_to x }` — `not_to`, clean.
//! - `[a].each { |u| expect(@u).to x }` — not an lvar, clean.
//! - `arr&.each { |u| expect(u).to x }` — `csend`, clean.
//! - `[a].each(1) { |u| expect(u).to x }` — send has args, clean.
//! - `each { |u| expect(u).to x }` — bare receiver (upstream crash
//!   shape), clean.
//!
//! ## No autocorrect
//!
//! Upstream rewrites to `expect(collection).to all(matcher)`. This batch
//! reports only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

use crate::cops::rspec_helpers::example_call_range;

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct IteratedExpectation;

#[cop(
    name = "RSpec/IteratedExpectation",
    description = "Check that `all` matcher is used instead of iterating over an array.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl IteratedExpectation {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, args, body } = *cx.kind(node) else {
            return;
        };
        if !is_each_send(cx, call) {
            return;
        }
        let Some(arg_name) = single_block_arg(cx, args) else {
            return;
        };
        let Some(body_id) = body.get() else {
            return;
        };
        if !is_single_or_only_expectations(cx, body_id, &arg_name) {
            return;
        }
        cx.emit_offense(
            example_call_range(cx, call, body_id),
            "Prefer using the `all` matcher instead of iterating over an array.",
            None,
        );
    }

    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Numblock { send, body, .. } = *cx.kind(node) else {
            return;
        };
        if !is_each_send(cx, send) {
            return;
        }
        let Some(body_id) = body.get() else {
            return;
        };
        if !is_single_or_only_expectations(cx, body_id, "_1") {
            return;
        }
        // `send_without_block_range` only trims `Block`-wrapped sends;
        // a `Numblock` send range covers the whole node, so trim to the
        // pre-opener end via the body like it does.
        cx.emit_offense(
            example_call_range(cx, send, body_id),
            "Prefer using the `all` matcher instead of iterating over an array.",
            None,
        );
    }
}

/// `true` when `send` is an explicit-receiver, arg-less `each` call.
///
/// Mirrors `(send ... :each)`: no trailing call args (verified vs 3.7.0 —
/// `[a].each(1) { }` is clean). The explicit-receiver gate mirrors the
/// observable upstream behavior for bare `each`, which crashes in the
/// autocorrect block and reports no offense.
fn is_each_send(cx: &Cx<'_>, send: NodeId) -> bool {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(send)
    else {
        return false;
    };
    if receiver.get().is_none() {
        return false;
    }
    if cx.symbol_str(method) != "each" {
        return false;
    }
    cx.list(args).is_empty()
}

/// The block-variable name when `args` is `(args (arg _))` — exactly one
/// plain argument — else `None`.
fn single_block_arg(cx: &Cx<'_>, args: NodeId) -> Option<String> {
    let NodeKind::Args(list) = *cx.kind(args) else {
        return None;
    };
    let ids = cx.list(list);
    let [only] = ids else {
        return None;
    };
    let NodeKind::Arg(sym) = *cx.kind(*only) else {
        return None;
    };
    Some(cx.symbol_str(sym).to_owned())
}

/// Mirrors `single_expectation?` / `only_expectations?`: the body itself
/// is an expectation over `lvar_name`, or a `Begin` whose every child is.
fn is_single_or_only_expectations(cx: &Cx<'_>, body: NodeId, lvar_name: &str) -> bool {
    if is_expectation(cx, body, lvar_name) {
        return true;
    }
    let NodeKind::Begin(members) = *cx.kind(body) else {
        return false;
    };
    let members = cx.list(members);
    !members.is_empty() && members.iter().all(|id| is_expectation(cx, *id, lvar_name))
}

/// Mirrors `expectation?`: `(send (send nil? :expect (lvar %)) :to ...)`
/// — bare `expect` with exactly the block variable, `:to` only.
fn is_expectation(cx: &Cx<'_>, node: NodeId, lvar_name: &str) -> bool {
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(node)
    else {
        return false;
    };
    if cx.symbol_str(method) != "to" {
        return false;
    }
    let Some(recv) = receiver.get() else {
        return false;
    };
    let NodeKind::Send {
        receiver: inner_recv,
        method: inner_method,
        args: inner_args,
    } = *cx.kind(recv)
    else {
        return false;
    };
    if inner_recv.get().is_some() || cx.symbol_str(inner_method) != "expect" {
        return false;
    }
    let args = cx.list(inner_args);
    let [only] = args else {
        return false;
    };
    let NodeKind::Lvar(sym) = *cx.kind(*only) else {
        return false;
    };
    cx.symbol_str(sym) == lvar_name
}

#[cfg(test)]
mod tests {
    use super::IteratedExpectation;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_single_expectation() {
        test::<IteratedExpectation>().expect_offense(indoc! {r#"
                [a, b].each { |u| expect(u).to be_valid }
                ^^^^^^^^^^^ Prefer using the `all` matcher instead of iterating over an array.
            "#});
    }

    #[test]
    fn flags_numblock_expectation() {
        test::<IteratedExpectation>().expect_offense(indoc! {r#"
                [a, b].each { expect(_1).to be_valid }
                ^^^^^^^^^^^ Prefer using the `all` matcher instead of iterating over an array.
            "#});
    }

    #[test]
    fn flags_only_expectations() {
        test::<IteratedExpectation>().expect_offense(indoc! {r#"
                [a, b].each { |u| expect(u).to be_valid; expect(u).to be_present }
                ^^^^^^^^^^^ Prefer using the `all` matcher instead of iterating over an array.
            "#});
    }

    #[test]
    fn does_not_flag_mixed_body() {
        test::<IteratedExpectation>().expect_no_offenses(indoc! {r#"
                [a, b].each { |u| expect(u).to be_valid; puts u }
            "#});
    }

    #[test]
    fn does_not_flag_not_to() {
        // `expectation?` pins `:to`; `not_to` never matches (verified
        // vs 3.7.0).
        test::<IteratedExpectation>().expect_no_offenses(indoc! {r#"
                [a, b].each { |u| expect(u).not_to be_valid }
            "#});
    }

    #[test]
    fn does_not_flag_ivar_subject() {
        test::<IteratedExpectation>().expect_no_offenses(indoc! {r#"
                [a, b].each { |u| expect(@u).to be_valid }
            "#});
    }

    #[test]
    fn does_not_flag_safe_navigation_each() {
        // `(send ... :each)` never matches `csend` (verified vs 3.7.0).
        test::<IteratedExpectation>().expect_no_offenses(indoc! {r#"
                arr&.each { |u| expect(u).to be_valid }
            "#});
    }

    #[test]
    fn does_not_flag_each_with_call_args() {
        // No trailing `...` in the pattern: send args never match
        // (verified vs 3.7.0).
        test::<IteratedExpectation>().expect_no_offenses(indoc! {r#"
                [a, b].each(1) { |u| expect(u).to be_valid }
            "#});
    }

    #[test]
    fn does_not_flag_bare_each() {
        // Bare `each` crashes upstream in the autocorrect block and
        // reports no offense, so it stays clean here (verified vs
        // 3.7.0).
        test::<IteratedExpectation>().expect_no_offenses(indoc! {r#"
                each { |u| expect(u).to be_valid }
            "#});
    }

    #[test]
    fn does_not_flag_two_block_args() {
        // `(args (arg $_))` is exactly one plain arg (verified vs
        // 3.7.0).
        test::<IteratedExpectation>().expect_no_offenses(indoc! {r#"
                h.each { |k, v| expect(k).to be_valid }
            "#});
    }

    #[test]
    fn does_not_flag_block_without_args() {
        test::<IteratedExpectation>().expect_no_offenses(indoc! {r#"
                [a, b].each { expect(x).to be_valid }
            "#});
    }
}

murphy_plugin_api::submit_cop!(IteratedExpectation);
