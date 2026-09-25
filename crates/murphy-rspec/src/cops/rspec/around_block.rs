//! `RSpec/AroundBlock` — checks that `around` hooks actually run the test.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/AroundBlock
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `hook_block`
//!   (`(block (send nil? :around sym ?) (args $...) ...)`) plus
//!   `hook_numblock` (`(numblock (send nil? :around sym ?) ...)`), both
//!   bare-receiver only. A block with no args flags the whole block per
//!   `add_no_arg_offense`; a block whose proxy arg is never used flags
//!   the arg per `check_for_unused_proxy`. Usage mirrors
//!   `find_arg_usage`
//!   (`{(send $... {:call :run}) (send _ _ $...) (yield $...)
//!   (block-pass $...)}`): an `Lvar` read of the proxy counts only when
//!   its parent is a `Send` (receiver or argument, covering `.call` /
//!   `.run` and passing the proxy along), a `Yield`, or a `BlockPass`.
//!   A bare `test` statement body does not count, matching upstream.
//!   The numblock arm flags the body per
//!   `add_offense(block.children.last)` when `_1` is unused. Messages
//!   mirror `MSG_NO_ARG` / `MSG_UNUSED_ARG`. No autocorrect upstream,
//!   none here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` and `Numblock` whose call is a bare `around`
//! (an optional scope sym such as `around(:each)` still matches):
//!
//! - `around do; foo; end` — no proxy arg, flagged (whole block).
//! - `around do |t|; foo; end` — `t` unused, flagged (the arg).
//! - `around do |t|; t.call; end` — used, not flagged.
//! - `around do |t|; t.run; end` — used, not flagged.
//! - `around do |t|; yield(t); end` — used, not flagged.
//! - `around do |t|; foo(t); end` — passed along, not flagged.
//! - `around do |t|; t; end` — bare read, still flagged.
//! - `around { _1.call }` — numbered usage, not flagged.
//! - `around { foo }` — `_1` unused, flagged (the body).
//! - `RSpec.around do; end` — explicit receiver, not flagged.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; threading the proxy through needs
//! human judgement.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct AroundBlock;

#[cop(
    name = "RSpec/AroundBlock",
    description = "Checks that around blocks actually run the test.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl AroundBlock {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, args, body } = *cx.kind(node) else {
            return;
        };
        if !is_bare_around(cx, call) {
            return;
        }
        let arg_ids = block_arg_ids(cx, args);
        let Some(&proxy) = arg_ids.first() else {
            cx.emit_offense(
                cx.range(node),
                "Test object should be passed to around block.",
                None,
            );
            return;
        };
        let NodeKind::Arg(sym) = *cx.kind(proxy) else {
            return;
        };
        let name = cx.symbol_str(sym);
        if !is_proxy_used(cx, body, name) {
            cx.emit_offense(
                cx.range(proxy),
                &format!("You should call `{name}.call` or `{name}.run`."),
                None,
            );
        }
    }

    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Numblock { send, body, .. } = *cx.kind(node) else {
            return;
        };
        if !is_bare_around(cx, send) {
            return;
        }
        if is_proxy_used(cx, body, "_1") {
            return;
        }
        // Upstream flags `block.children.last` (the body); fall back to
        // the whole node when the body is absent.
        let range = match body.get() {
            Some(body_id) => cx.range(body_id),
            None => cx.range(node),
        };
        cx.emit_offense(
            range,
            "You should call `_1.call` or `_1.run`.",
            None,
        );
    }
}

/// `true` when `call` is a bare `around` (`(send nil? :around sym ?)`:
/// bare receiver, any args — the optional scope sym is ignored).
fn is_bare_around(cx: &Cx<'_>, call: NodeId) -> bool {
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(call)
    else {
        return false;
    };
    if receiver != OptNodeId::NONE {
        return false;
    }
    cx.symbol_str(method) == "around"
}

/// Block parameter ids (`|t|` → the `Arg` nodes).
fn block_arg_ids(cx: &Cx<'_>, args: NodeId) -> Vec<NodeId> {
    let NodeKind::Args(list) = *cx.kind(args) else {
        return Vec::new();
    };
    cx.list(list).to_vec()
}

/// `true` when an `Lvar` read of `name` appears as a `Send` receiver or
/// argument, a `Yield` argument, or a `BlockPass` value inside `body`.
///
/// Mirrors upstream `find_arg_usage` + `usage.include?(s(:lvar, name))`:
/// a bare `test` statement is an `Lvar` whose parent is the `Block` body
/// (or a `Begin`), which matches no arm and does not count as usage.
fn is_proxy_used(cx: &Cx<'_>, body: OptNodeId, name: &str) -> bool {
    let Some(body_id) = body.get() else {
        return false;
    };
    for id in core::iter::once(body_id).chain(cx.descendants(body_id)) {
        let matches_name = match *cx.kind(id) {
            NodeKind::Lvar(sym) => cx.symbol_str(sym) == name,
            _ => continue,
        };
        if !matches_name {
            continue;
        }
        let Some(parent) = cx.parent(id).get() else {
            continue;
        };
        match *cx.kind(parent) {
            NodeKind::Send { .. } | NodeKind::Yield(_) | NodeKind::BlockPass(_) => return true,
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::AroundBlock;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_around_without_arg() {
        test::<AroundBlock>().expect_offense(indoc! {r#"
                around do; foo; end
                ^^^^^^^^^^^^^^^^^^^ Test object should be passed to around block.
            "#});
    }

    #[test]
    fn flags_around_with_unused_arg() {
        test::<AroundBlock>().expect_offense(indoc! {r#"
                around do |test|; foo; end
                           ^^^^ You should call `test.call` or `test.run`.
            "#});
    }

    #[test]
    fn does_not_flag_call_usage() {
        test::<AroundBlock>().expect_no_offenses(indoc! {r#"
                around do |test|
                  foo
                  test.call
                end
            "#});
    }

    #[test]
    fn does_not_flag_run_usage() {
        test::<AroundBlock>().expect_no_offenses(indoc! {r#"
                around do |test|
                  test.run
                end
            "#});
    }

    #[test]
    fn does_not_flag_yield_usage() {
        test::<AroundBlock>().expect_no_offenses(indoc! {r#"
                around do |test|
                  yield(test)
                end
            "#});
    }

    #[test]
    fn does_not_flag_passed_along_arg() {
        test::<AroundBlock>().expect_no_offenses(indoc! {r#"
                around do |test|
                  foo(test)
                end
            "#});
    }

    #[test]
    fn flags_bare_proxy_read() {
        // A bare `test` statement matches no `find_arg_usage` arm.
        test::<AroundBlock>().expect_offense(indoc! {r#"
                around do |test|; test; end
                           ^^^^ You should call `test.call` or `test.run`.
            "#});
    }

    #[test]
    fn flags_scoped_around_without_call() {
        test::<AroundBlock>().expect_offense(indoc! {r#"
                around(:each) do |test|; foo; end
                                  ^^^^ You should call `test.call` or `test.run`.
            "#});
    }

    #[test]
    fn does_not_flag_numblock_usage() {
        test::<AroundBlock>().expect_no_offenses(indoc! {r#"
                around { _1.call }
            "#});
    }

    #[test]
    fn flags_numblock_without_usage() {
        test::<AroundBlock>().expect_offense(indoc! {r#"
                around { _2 }
                         ^^ You should call `_1.call` or `_1.run`.
            "#});
    }

    #[test]
    fn does_not_flag_explicit_receiver() {
        test::<AroundBlock>().expect_no_offenses(indoc! {r#"
                RSpec.around do; foo; end
            "#});
    }

    #[test]
    fn does_not_flag_non_around_block() {
        test::<AroundBlock>().expect_no_offenses(indoc! {r#"
                before do; foo; end
            "#});
    }
}

murphy_plugin_api::submit_cop!(AroundBlock);
