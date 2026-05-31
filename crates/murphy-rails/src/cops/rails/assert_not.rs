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
//!   Test-file gating (only flag in test/spec paths) is not implemented;
//!   Murphy flags in all files. Known limitation, not a blocker.
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
//! Outer `Send(receiver=None, method="assert", args=[inner, ...])`, where
//! `inner` is itself a `Send(receiver=Some(_), method="!", args=[])`.
//! The trailing `...` on the outer Send allows an optional failure-message
//! second argument (e.g. `assert !foo, 'msg'`).
//!
//! Expressed declaratively with [`def_node_matcher!`] (RuboCop NodePattern
//! grammar): in DSL `nil?` means receiver-None on the outer Send,
//! `!nil?` on the inner Send forces a non-None receiver (the negated
//! expression), and the trailing `...` on the outer send permits zero
//! or more additional arguments after the negation.
//!
//! ## Autocorrect
//!
//! `assert !foo` rewrites to `assert_not foo` via two surgical edits:
//!
//! 1. Rename the outer Send's selector — `loc.name` covers exactly the
//!    bytes of `assert` — to `assert_not`.
//! 2. Delete the `!` negation prefix: the slice
//!    `[inner_bang.start, inner_receiver.start)` covers `!` (and any
//!    whitespace between the `!` token and the receiver), e.g. `not `.
//!
//! The two edits don't overlap, so the msg arg passes through untouched.
//!
//! ## Known limitation
//!
//! RuboCop gates this cop to test/spec paths. Murphy does not implement
//! path gating in v1; the cop fires on all files. This is a known gap
//! tracked in `murphy-juee`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop, def_node_matcher};

// RuboCop NodePattern equivalent:
//   `(send nil? :assert (send !nil? :!) ...)`
//
// - Outer: receiver None (`nil?`), method `:assert`, 1 or more args
//   (the `...` allows the optional failure-message second argument).
// - Inner: receiver non-None (`!nil?`), method `:!`, exactly 0 args.
def_node_matcher!(is_assert_bang, "(send nil? :assert (send !nil? :!) ...)");

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
        emit_correction(node, cx);
    }
}

/// Emit the two non-overlapping edits that rewrite `assert !foo` to
/// `assert_not foo` (and `assert !foo, msg` to `assert_not foo, msg`).
/// Bails on shape mismatches — the pattern already validated them, so
/// this is defensive against a future AST refactor.
fn emit_correction(node: NodeId, cx: &Cx<'_>) {
    // Edit 1: rename `assert` → `assert_not`. `loc.name` covers exactly
    // the bytes of the selector on the outer Send node.
    cx.emit_edit(cx.node(node).loc.name, "assert_not");

    // Edit 2: delete the `!` negation. `cx.first_argument` returns the
    // inner `!` Send (guaranteed by the pattern). Then get the inner
    // Send's receiver to compute the prefix range.
    let Some(inner_bang) = cx.first_argument(node).get() else {
        return;
    };
    let NodeKind::Send {
        receiver: bang_receiver,
        ..
    } = *cx.kind(inner_bang)
    else {
        return;
    };
    let Some(inner_receiver) = bang_receiver.get() else {
        return;
    };

    // Strip `!`/`not ` (and any whitespace between the negation token
    // and the inner receiver). The inner bang range starts at `!`/`not`;
    // the inner receiver range starts at the first byte of the receiver.
    let negation_prefix = Range {
        start: cx.range(inner_bang).start,
        end: cx.range(inner_receiver).start,
    };
    cx.emit_edit(negation_prefix, "");
}

#[cfg(test)]
mod tests {
    use super::AssertNot;
    use murphy_plugin_api::test_support::{indoc, run_cop_with_edits, test};

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

    #[test]
    fn flags_assert_bang_with_message() {
        test::<AssertNot>().expect_offense(indoc! {r#"
                assert !foo, 'a failure message'
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `assert_not` over `assert !`.
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
        // the first arg is not a bang send, so no offense.
        test::<AssertNot>().expect_no_offenses("assert foo, \"msg\"\n");
    }

    #[test]
    fn does_not_flag_assert_with_no_args() {
        // `assert()` would be a runtime error, but defensively we
        // don't want to fire on the zero-arg arity either.
        test::<AssertNot>().expect_no_offenses("assert()\n");
    }

    // === autocorrect ===

    #[test]
    fn corrects_assert_bang_local() {
        test::<AssertNot>().expect_correction(
            indoc! {r#"
                assert !x
                ^^^^^^^^^ Prefer `assert_not` over `assert !`.
            "#},
            "assert_not x\n",
        );
    }

    #[test]
    fn corrects_assert_bang_send() {
        // Multi-segment receiver source is reproduced verbatim.
        test::<AssertNot>().expect_correction(
            indoc! {r#"
                assert !foo.empty?
                ^^^^^^^^^^^^^^^^^^ Prefer `assert_not` over `assert !`.
            "#},
            "assert_not foo.empty?\n",
        );
    }

    #[test]
    fn corrects_assert_bang_with_message() {
        // The msg arg passes through untouched.
        test::<AssertNot>().expect_correction(
            indoc! {r#"
                assert !foo, 'a failure message'
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `assert_not` over `assert !`.
            "#},
            "assert_not foo, 'a failure message'\n",
        );
    }

    #[test]
    fn corrects_assert_paren_bang() {
        test::<AssertNot>().expect_correction(
            indoc! {r#"
                assert(!user.admin?)
                ^^^^^^^^^^^^^^^^^^^^ Prefer `assert_not` over `assert !`.
            "#},
            "assert_not(user.admin?)\n",
        );
    }

    #[test]
    fn correction_reaches_fixpoint() {
        // Apply both edits, then re-run the cop on the result: zero
        // offenses. This pins idempotence — the rewrite must not
        // produce something the cop would flag again.
        let run = run_cop_with_edits::<AssertNot>("assert !x\n");
        assert_eq!(run.edits.len(), 2);
        let mut replacements: Vec<&str> =
            run.edits.iter().map(|e| e.replacement.as_str()).collect();
        replacements.sort();
        assert_eq!(replacements, ["", "assert_not"]);
        test::<AssertNot>().expect_no_offenses("assert_not x\n");
    }
}
