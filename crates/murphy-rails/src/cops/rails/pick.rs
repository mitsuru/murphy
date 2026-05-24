//! `Rails/Pick` — flag the `pluck(:col).first` idiom in favour of the
//! Rails 6+ `pick(:col)` shorthand. `pick` materialises only the first
//! row at the SQL level (it tacks on a `LIMIT 1`), whereas
//! `pluck(:col).first` loads the entire column into Ruby first and then
//! discards everything but the first row — a needless round-trip and
//! allocation in any non-trivial table.
//!
//! ## Matched shape (Send node)
//!
//! Outer `Send(receiver=Some(inner), method="first", args=[])`, where
//! `inner` is itself `Send(receiver=_, method="pluck", args=[<single>])`.
//!
//! - **outer method == `first`** — the terminator we care about.
//!   `.last`, `.second`, etc. are intentionally out of scope (upstream
//!   RuboCop-rails also limits to `.first`).
//! - **outer args empty** — `pluck(:id).first(5)` carries a limit
//!   argument and is **not** rewritable to `pick(:id)` (pick has no
//!   multi-row form), so it's excluded.
//! - **outer receiver is a `pluck` Send with exactly one argument** —
//!   `pluck(:id, :name).first` (multi-column) is excluded because
//!   `pick` can only project one column; `pluck.first` (zero args) is
//!   excluded as a degenerate / non-equivalent form.
//! - **inner Send's receiver shape is unconstrained** — a const
//!   (`Post.pluck(:id).first`), a chain
//!   (`User.where(active: true).pluck(:name).first`), a local
//!   (`posts.pluck(:title).first`) all match. Mirrors upstream
//!   RuboCop-rails which doesn't gate on the receiver of `pluck`.
//! - **inner arg's content is unconstrained** — `:id`, a string, a
//!   send, anything; we only care about the arity. Mirrors upstream.
//!
//! Method chains like `Post.pluck(:id).first.something` still hit on
//! the inner `Post.pluck(:id).first` Send — the dispatcher visits every
//! Send node, and the gates above all apply to it independently of the
//! outer `.something` call. RuboCop-rails behaves the same way.
//!
//! ## No autocorrect
//!
//! Mechanically rewriting `pluck(:x).first` → `pick(:x)` is safe in
//! Rails 6+, but v1 ships as detect-only; ADR 0006 requires a deliberate
//! fix block per cop. Tracked as a follow-up.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct Pick;

#[cop(
    name = "Rails/Pick",
    description = "Prefer `pick(...)` over `pluck(...).first`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl Pick {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // Defensive pattern-match: the dispatcher feeds us only Send
        // nodes today (`KINDS = [send]`), but the `let-else` is free
        // insurance against a future kind-aliasing accident. Same
        // posture as `Rails/Output` / `Rails/RequestReferer` /
        // `Rails/AssertNot`.
        let NodeKind::Send {
            receiver,
            method,
            args,
        } = *cx.kind(node)
        else {
            return;
        };
        // Gate 1: outer method must be exactly `first`. `.last`,
        // `.second`, etc. aren't rewritable to `pick` and are out of
        // scope (matches upstream RuboCop-rails).
        if cx.symbol_str(method) != "first" {
            return;
        }
        // Gate 2: outer args must be empty. `.first(5)` carries a limit
        // argument and is not equivalent to `pick(:x)` (pick is
        // single-row only).
        if !cx.list(args).is_empty() {
            return;
        }
        // Gate 3: outer receiver must be `Send(_, "pluck", [_single])`.
        let Some(receiver_id) = receiver.get() else {
            return;
        };
        let NodeKind::Send {
            method: inner_method,
            args: inner_args,
            ..
        } = *cx.kind(receiver_id)
        else {
            return;
        };
        if cx.symbol_str(inner_method) != "pluck" {
            return;
        }
        // Rails 6+ `pick(*columns)` is variadic and equivalent to
        // `limit(1).pluck(*columns).first`, so multi-column
        // `pluck(:id, :name).first` is also rewritable to
        // `pick(:id, :name)`. Only the zero-args `pluck.first` form is
        // a degenerate non-equivalent shape that should be left alone
        // (roborev review feedback on murphy-cy1).
        if cx.list(inner_args).is_empty() {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            "Prefer `pick(...)` over `pluck(...).first`.",
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::Pick;
    use murphy_plugin_api::test_support::{expect_no_offenses, expect_offense, indoc};

    // === hit cases ===

    #[test]
    fn flags_pluck_id_first() {
        expect_offense!(
            Pick,
            indoc! {r#"
                Post.pluck(:id).first
                ^^^^^^^^^^^^^^^^^^^^^ Prefer `pick(...)` over `pluck(...).first`.
            "#}
        );
    }

    #[test]
    fn flags_chain_then_pluck_first() {
        // The receiver of `pluck` is itself a chain — the inner Send's
        // receiver shape is unconstrained, so this still hits.
        expect_offense!(
            Pick,
            indoc! {r#"
                User.where(active: true).pluck(:name).first
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `pick(...)` over `pluck(...).first`.
            "#}
        );
    }

    #[test]
    fn flags_local_receiver_pluck_first() {
        // Bare local receiver — same shape, still hits.
        expect_offense!(
            Pick,
            indoc! {r#"
                posts.pluck(:title).first
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `pick(...)` over `pluck(...).first`.
            "#}
        );
    }

    #[test]
    fn flags_inner_send_in_longer_chain() {
        // `Post.pluck(:id).first.something` — the dispatcher visits
        // every Send node, including the inner `Post.pluck(:id).first`
        // that is the outer `.something`'s receiver. That inner Send
        // matches all our gates on its own.
        expect_offense!(
            Pick,
            indoc! {r#"
                Post.pluck(:id).first.something
                ^^^^^^^^^^^^^^^^^^^^^ Prefer `pick(...)` over `pluck(...).first`.
            "#}
        );
    }

    // === no-hit cases ===

    #[test]
    fn flags_multi_column_pluck() {
        // Rails 6+ `pick(*columns)` accepts multiple column names and is
        // equivalent to `limit(1).pluck(*columns).first` — so
        // multi-column `pluck(...).first` is also rewritable. Promoted
        // from a no-offense expectation in response to roborev review
        // feedback on murphy-cy1.
        expect_offense!(
            Pick,
            indoc! {r#"
                Post.pluck(:id, :name).first
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `pick(...)` over `pluck(...).first`.
            "#}
        );
    }

    #[test]
    fn does_not_flag_first_with_limit_arg() {
        // `.first(5)` carries a limit; not equivalent to `pick(:x)`.
        expect_no_offenses!(Pick, "Post.pluck(:id).first(5)\n");
    }

    #[test]
    fn does_not_flag_bare_first() {
        // No `pluck` in the chain.
        expect_no_offenses!(Pick, "obj.first\n");
    }

    #[test]
    fn does_not_flag_pluck_then_last() {
        // Different terminator — out of scope (matches upstream).
        expect_no_offenses!(Pick, "Post.pluck(:id).last\n");
    }

    #[test]
    fn does_not_flag_pluck_zero_args_then_first() {
        // `pluck.first` (zero args) is a degenerate non-equivalent
        // form — `pick` requires a single explicit column.
        expect_no_offenses!(Pick, "Post.pluck.first\n");
    }

    #[test]
    fn does_not_flag_pluck_without_first() {
        // No terminator — just `pluck`, no chain to rewrite.
        expect_no_offenses!(Pick, "Post.pluck(:id)\n");
    }
}
