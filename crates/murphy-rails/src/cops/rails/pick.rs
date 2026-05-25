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
//! `inner` is itself `Send(receiver=_, method="pluck", args=[_, ...])`.
//!
//! - **outer method == `first`** — the terminator we care about.
//!   `.last`, `.second`, etc. are intentionally out of scope (upstream
//!   RuboCop-rails also limits to `.first`).
//! - **outer args empty** — `pluck(:id).first(5)` carries a limit
//!   argument and is **not** rewritable to `pick(:id)` (pick has no
//!   multi-row form), so it's excluded.
//! - **outer receiver is a `pluck` Send with ≥1 argument** —
//!   `pluck.first` (zero args) is excluded as a degenerate /
//!   non-equivalent form. Rails 6+ `pick(*columns)` is variadic, so
//!   `pluck(:id, :name).first` (multi-column) **does** match
//!   (roborev review feedback on murphy-cy1).
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
//! ## Implementation
//!
//! Expressed declaratively with [`node_pattern!`] (RuboCop NodePattern
//! grammar). `_ ...` in the inner Send's argument list means "one
//! wildcard followed by zero-or-more rest" — i.e. ≥1 arg — which
//! rules out the zero-arg `pluck.first` shape. Trailing argument
//! placeholders are omitted on the outer Send (it must take exactly
//! zero args, ruling out `.first(5)`).
//!
//! ## No autocorrect
//!
//! Mechanically rewriting `pluck(:x).first` → `pick(:x)` is safe in
//! Rails 6+, but v1 ships as detect-only; ADR 0006 requires a deliberate
//! fix block per cop. Tracked as a follow-up.

use murphy_plugin_api::{Cx, NoOptions, NodeId, cop, node_pattern};

// RuboCop NodePattern equivalent: `(send (send _ :pluck _ ...) :first)`.
// - Outer: receiver = inner Send, method `:first`, exactly 0 args.
// - Inner: receiver `_` (unconstrained), method `:pluck`, ≥1 arg
//   (`_ ...` = one wildcard + rest, excludes zero-arg `pluck.first`).
node_pattern!(is_pluck_first, "(send (send _ :pluck _ ...) :first)");

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
        if !is_pluck_first(node, cx) {
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
    use murphy_plugin_api::test_support::{indoc, test};

    // === hit cases ===

    #[test]
    fn flags_pluck_id_first() {
        test::<Pick>().expect_offense(indoc! {r#"
                Post.pluck(:id).first
                ^^^^^^^^^^^^^^^^^^^^^ Prefer `pick(...)` over `pluck(...).first`.
            "#});
    }

    #[test]
    fn flags_chain_then_pluck_first() {
        // The receiver of `pluck` is itself a chain — the inner Send's
        // receiver shape is unconstrained, so this still hits.
        test::<Pick>().expect_offense(indoc! {r#"
                User.where(active: true).pluck(:name).first
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `pick(...)` over `pluck(...).first`.
            "#});
    }

    #[test]
    fn flags_local_receiver_pluck_first() {
        // Bare local receiver — same shape, still hits.
        test::<Pick>().expect_offense(indoc! {r#"
                posts.pluck(:title).first
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `pick(...)` over `pluck(...).first`.
            "#});
    }

    #[test]
    fn flags_inner_send_in_longer_chain() {
        // `Post.pluck(:id).first.something` — the dispatcher visits
        // every Send node, including the inner `Post.pluck(:id).first`
        // that is the outer `.something`'s receiver. That inner Send
        // matches all our gates on its own.
        test::<Pick>().expect_offense(indoc! {r#"
                Post.pluck(:id).first.something
                ^^^^^^^^^^^^^^^^^^^^^ Prefer `pick(...)` over `pluck(...).first`.
            "#});
    }

    #[test]
    fn flags_multi_column_pluck() {
        // Rails 6+ `pick(*columns)` accepts multiple column names and is
        // equivalent to `limit(1).pluck(*columns).first` — so
        // multi-column `pluck(...).first` is also rewritable. The
        // DSL's `_ ...` (one+rest) matches arity ≥1, which includes
        // multi-column.
        test::<Pick>().expect_offense(indoc! {r#"
                Post.pluck(:id, :name).first
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `pick(...)` over `pluck(...).first`.
            "#});
    }

    // === no-hit cases ===

    #[test]
    fn does_not_flag_first_with_limit_arg() {
        // `.first(5)` carries a limit; not equivalent to `pick(:x)`.
        // The outer Send's empty trailing arg list excludes this.
        test::<Pick>().expect_no_offenses("Post.pluck(:id).first(5)\n");
    }

    #[test]
    fn does_not_flag_bare_first() {
        // No `pluck` in the chain.
        test::<Pick>().expect_no_offenses("obj.first\n");
    }

    #[test]
    fn does_not_flag_pluck_then_last() {
        // Different terminator — out of scope (matches upstream).
        test::<Pick>().expect_no_offenses("Post.pluck(:id).last\n");
    }

    #[test]
    fn does_not_flag_pluck_zero_args_then_first() {
        // `pluck.first` (zero args) is a degenerate non-equivalent
        // form — `pick` requires at least one explicit column. The
        // DSL's `_ ...` arity-≥1 inner-args gate excludes this.
        test::<Pick>().expect_no_offenses("Post.pluck.first\n");
    }

    #[test]
    fn does_not_flag_pluck_without_first() {
        // No terminator — just `pluck`, no chain to rewrite.
        test::<Pick>().expect_no_offenses("Post.pluck(:id)\n");
    }
}
