//! `Rails/AssertNot` — flag `assert(!x)` (the receiver-less minitest
//! `assert` with a single negated argument) and recommend the
//! semantically-equivalent `assert_not(x)` form. Rails ships both in
//! its `ActiveSupport::TestCase`; the cop nudges projects toward the
//! positive-named helper since it reads more naturally than the
//! double-negative.
//!
//! ## AST shape
//!
//! Probed via `crates/murphy-translate/tests/coverage.rs` (temporary
//! `_probe_bang_ast_shape` test, removed before commit):
//!
//! - `!foo` parses as `Send(receiver=Some(foo), method="!", args=[])`
//!   — the bang is a method-style send on the receiver, **not** a
//!   dedicated `Not`/`Negation` `NodeKind` variant (the enum has no
//!   such variant; see `crates/murphy-ast/src/node.rs`).
//! - `not foo` produces the same `Send(.., "!", [])` shape, so this
//!   cop will also flag `assert(not foo)`. That is semantically
//!   correct (Ruby's `not` is `!`) — left as-is.
//!
//! ## Matched shape (Send node)
//!
//! Outer `Send(receiver=None, method="assert", args=[inner])`, where
//! `inner` is itself a `Send(receiver=Some(_), method="!", args=[])`.
//!
//! - **receiver None on the outer Send** — `obj.assert(!x)` is an
//!   intentional method call on a receiver and is intentionally
//!   ignored.
//! - **method == `assert`** — the minitest helper name.
//! - **exactly one arg** — `assert foo, "msg"` carries a message
//!   argument and is a different call shape; we only target the
//!   single-arg negated form spelled out in the task description.
//! - **inner is the bang send** — `Send(Some(_), "!", [])`. Any other
//!   inner shape (a positive call `assert foo`, a literal, a block
//!   send) doesn't match.
//!
//! ## No autocorrect
//!
//! Rewriting `assert !x` → `assert_not x` is mechanically safe in
//! Rails (both methods are defined on `ActiveSupport::TestCase`), but
//! v1 ships as detect-only; ADR 0006 requires a deliberate fix block
//! per cop. Tracked as a follow-up.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct AssertNot;

#[cop(
    name = "Rails/AssertNot",
    description = "Prefer `assert_not` over `assert !`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl AssertNot {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // Defensive pattern-match: the dispatcher feeds us only Send
        // nodes today (`KINDS = [send]`), but the `let-else` is free
        // insurance against a future kind-aliasing accident. Same
        // posture as `Rails/Output` / `Rails/RequestReferer`.
        let NodeKind::Send {
            receiver,
            method,
            args,
        } = *cx.kind(node)
        else {
            return;
        };
        // Gate 1: bare `assert` only — `obj.assert(!x)` is intentional.
        if receiver != OptNodeId::NONE {
            return;
        }
        // Gate 2: method must be exactly `assert`.
        if cx.symbol_str(method) != "assert" {
            return;
        }
        // Gate 3: exactly one argument. `assert foo, "msg"` is a
        // different call shape with an explicit message and is out of
        // scope per the task description; `assert()` with no args is
        // also out.
        let arg_ids = cx.list(args);
        let [inner_id] = arg_ids else {
            return;
        };
        // Gate 4: the lone argument must itself be the bang send
        // `Send(Some(_), "!", [])`. A positive call, literal, or any
        // other inner shape doesn't match.
        if !is_bang_send(cx, *inner_id) {
            return;
        }
        cx.emit_offense(cx.range(node), "Prefer `assert_not` over `assert !`.", None);
    }
}

/// `true` when `id` is a `Send(receiver=Some(_), method="!", args=[])`
/// — the AST shape Ruby's unary `!` (and the `not` keyword) emit.
fn is_bang_send(cx: &Cx<'_>, id: NodeId) -> bool {
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(id)
    else {
        return false;
    };
    // The bang has a receiver (the negated expression). A bare `Send`
    // whose method literally spells `"!"` with no receiver isn't the
    // Ruby `!x` operator and shouldn't count.
    if receiver == OptNodeId::NONE {
        return false;
    }
    cx.symbol_str(method) == "!"
}

#[cfg(test)]
mod tests {
    use super::AssertNot;
    use murphy_plugin_api::test_support::{expect_no_offenses, expect_offense, indoc};

    // === hit cases ===

    #[test]
    fn flags_assert_with_bang() {
        expect_offense!(
            AssertNot,
            indoc! {r#"
                assert !foo.empty?
                ^^^^^^^^^^^^^^^^^^ Prefer `assert_not` over `assert !`.
            "#}
        );
    }

    #[test]
    fn flags_assert_paren_bang() {
        expect_offense!(
            AssertNot,
            indoc! {r#"
                assert(!user.admin?)
                ^^^^^^^^^^^^^^^^^^^^ Prefer `assert_not` over `assert !`.
            "#}
        );
    }

    #[test]
    fn flags_assert_bang_literal() {
        // The cop matches the bang send shape regardless of what's
        // inside; even `!true` (always false, definitely a smell) is
        // a hit.
        expect_offense!(
            AssertNot,
            indoc! {r#"
                assert(!true)
                ^^^^^^^^^^^^^ Prefer `assert_not` over `assert !`.
            "#}
        );
    }

    #[test]
    fn flags_assert_bang_local() {
        // `!x` on a bare local works the same way — Send(Some(x), "!", []).
        expect_offense!(
            AssertNot,
            indoc! {r#"
                assert !x
                ^^^^^^^^^ Prefer `assert_not` over `assert !`.
            "#}
        );
    }

    // === no-hit cases ===

    #[test]
    fn does_not_flag_assert_without_bang() {
        expect_no_offenses!(AssertNot, "assert foo.empty?\n");
    }

    #[test]
    fn does_not_flag_assert_not() {
        // Already the recommended form — leave alone.
        expect_no_offenses!(AssertNot, "assert_not foo.empty?\n");
    }

    #[test]
    fn does_not_flag_receiver_assert() {
        // `obj.assert(!x)` is a method call on a receiver, not the
        // bare minitest `assert`.
        expect_no_offenses!(AssertNot, "obj.assert(!x)\n");
    }

    #[test]
    fn does_not_flag_assert_with_message_arg() {
        // `assert foo, "msg"` carries an explicit failure message;
        // the arity gate excludes it (and the single arg is not a
        // bang send anyway).
        expect_no_offenses!(AssertNot, "assert foo, \"msg\"\n");
    }

    #[test]
    fn does_not_flag_assert_with_no_args() {
        // `assert()` would be a runtime error, but defensively we
        // don't want to fire on the zero-arg arity either.
        expect_no_offenses!(AssertNot, "assert()\n");
    }
}
