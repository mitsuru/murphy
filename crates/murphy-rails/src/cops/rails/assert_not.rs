//! `Rails/AssertNot` — flag `assert(!x)` (the receiver-less minitest
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/AssertNot
//! upstream_version_checked: 2.35.0
//! status: partial
//! gap_issues:
//!   - murphy-juee
//! notes: >
//!   Known gaps remain around message-arg forms, autocorrect, and test-file gating.
//! ```
//!
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
//! Expressed declaratively with [`def_node_matcher!`] (RuboCop NodePattern
//! grammar): in DSL `nil?` means receiver-None on the outer Send,
//! `!nil?` on the inner Send forces a non-None receiver (the negated
//! expression), and the trailing argument list is omitted so each
//! Send must take exactly its specified arity (outer = 1 inner arg,
//! inner = 0).

use murphy_plugin_api::{Cx, NoOptions, NodeId, cop, def_node_matcher};

// RuboCop NodePattern equivalent:
//   `(send nil? :assert (send !nil? :!))`
//
// - Outer: receiver None (`nil?`), method `:assert`, exactly 1 arg.
// - Inner: receiver non-None (`!nil?`), method `:!`, exactly 0 args.
//
// Strict arity on both Sends is load-bearing (excludes
// `assert foo, "msg"`, `assert()`, and any inner-bang oddity).
def_node_matcher!(is_assert_bang, "(send nil? :assert (send !nil? :!))");

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
        if !is_assert_bang(node, cx) {
            return;
        }
        cx.emit_offense(cx.range(node), "Prefer `assert_not` over `assert !`.", None);
    }
}

#[cfg(test)]
mod tests {
    use super::AssertNot;
    use murphy_plugin_api::test_support::{indoc, test};

    // === hit cases ===

    #[test]
    fn flags_assert_with_bang() {
        test::<AssertNot>().expect_offense(indoc! {r#"
                assert !foo.empty?
                ^^^^^^^^^^^^^^^^^^ Prefer `assert_not` over `assert !`.
            "#});
    }

    #[test]
    fn flags_assert_paren_bang() {
        test::<AssertNot>().expect_offense(indoc! {r#"
                assert(!user.admin?)
                ^^^^^^^^^^^^^^^^^^^^ Prefer `assert_not` over `assert !`.
            "#});
    }

    #[test]
    fn flags_assert_bang_literal() {
        // The cop matches the bang send shape regardless of what's
        // inside; even `!true` (always false, definitely a smell) is
        // a hit.
        test::<AssertNot>().expect_offense(indoc! {r#"
                assert(!true)
                ^^^^^^^^^^^^^ Prefer `assert_not` over `assert !`.
            "#});
    }

    #[test]
    fn flags_assert_bang_local() {
        // `!x` on a bare local works the same way — Send(Some(x), "!", []).
        test::<AssertNot>().expect_offense(indoc! {r#"
                assert !x
                ^^^^^^^^^ Prefer `assert_not` over `assert !`.
            "#});
    }

    // === no-hit cases ===

    #[test]
    fn does_not_flag_assert_without_bang() {
        test::<AssertNot>().expect_no_offenses("assert foo.empty?\n");
    }

    #[test]
    fn does_not_flag_assert_not() {
        // Already the recommended form — leave alone.
        test::<AssertNot>().expect_no_offenses("assert_not foo.empty?\n");
    }

    #[test]
    fn does_not_flag_receiver_assert() {
        // `obj.assert(!x)` is a method call on a receiver, not the
        // bare minitest `assert`.
        test::<AssertNot>().expect_no_offenses("obj.assert(!x)\n");
    }

    #[test]
    fn does_not_flag_assert_with_message_arg() {
        // `assert foo, "msg"` carries an explicit failure message;
        // the arity gate excludes it (and the single arg is not a
        // bang send anyway).
        test::<AssertNot>().expect_no_offenses("assert foo, \"msg\"\n");
    }

    #[test]
    fn does_not_flag_assert_with_no_args() {
        // `assert()` would be a runtime error, but defensively we
        // don't want to fire on the zero-arg arity either.
        test::<AssertNot>().expect_no_offenses("assert()\n");
    }
}
