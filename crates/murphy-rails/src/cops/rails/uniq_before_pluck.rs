//! `Rails/UniqBeforePluck` — flag the `pluck(:col).uniq` idiom and
//! recommend `distinct.pluck(:col)` (or `pluck(:col).distinct` on
//! ActiveRecord scopes). `uniq` materialises the entire pluck result
//! in Ruby memory and then de-duplicates client-side; `distinct`
//! pushes the dedup to the database, which is dramatically cheaper
//! on non-trivial tables.
//!
//! ## Matched shape (Send node)
//!
//! Outer `Send(receiver=Some(inner), method="uniq", args=[])`, where
//! `inner` is itself `Send(receiver=_, method="pluck", args=[_+])` —
//! i.e. ≥1 wildcard args via the murphy-ycx postfix quantifier.
//!
//! Same shape as `Rails/Pick` with `:first` → `:uniq`; see that cop's
//! module docs for the DSL semantics. `pluck` arity ≥1 (zero-arg
//! `pluck` is a degenerate form), outer `uniq` arity 0 (`uniq(&block)`
//! is a different idiom).
//!
//! ## No autocorrect
//!
//! The replacement (`distinct.pluck`) reorders Send nodes and only
//! makes sense for `ActiveRecord::Relation` receivers (a plain
//! `Array#pluck` doesn't have a `distinct` cousin). Detect-only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, cop, node_pattern};

// RuboCop NodePattern equivalent: `(send (send _ :pluck _+) :uniq)`.
// - Outer: receiver = inner Send, method `:uniq`, exactly 0 args.
// - Inner: receiver `_` (unconstrained), method `:pluck`, ≥1 arg
//   (postfix `_+` quantifier, murphy-ycx).
node_pattern!(is_pluck_uniq, "(send (send _ :pluck _+) :uniq)");

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct UniqBeforePluck;

#[cop(
    name = "Rails/UniqBeforePluck",
    description = "Use `distinct` before `pluck` instead of `uniq` after.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl UniqBeforePluck {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        if !is_pluck_uniq(node, cx) {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            "Use `distinct` before `pluck` instead of `uniq` after.",
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::UniqBeforePluck;
    use murphy_plugin_api::test_support::{indoc, test};

    // === hit cases ===

    #[test]
    fn flags_pluck_id_uniq() {
        test::<UniqBeforePluck>().expect_offense(indoc! {r#"
                Post.pluck(:id).uniq
                ^^^^^^^^^^^^^^^^^^^^ Use `distinct` before `pluck` instead of `uniq` after.
            "#});
    }

    #[test]
    fn flags_chain_then_pluck_uniq() {
        test::<UniqBeforePluck>().expect_offense(indoc! {r#"
                User.where(active: true).pluck(:name).uniq
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `distinct` before `pluck` instead of `uniq` after.
            "#});
    }

    #[test]
    fn flags_local_receiver_pluck_uniq() {
        test::<UniqBeforePluck>().expect_offense(indoc! {r#"
                posts.pluck(:title).uniq
                ^^^^^^^^^^^^^^^^^^^^^^^^ Use `distinct` before `pluck` instead of `uniq` after.
            "#});
    }

    #[test]
    fn flags_multi_column_pluck() {
        // Multi-column `pluck(:id, :name).uniq` is also a candidate —
        // `distinct.pluck(:id, :name)` is the AR-relation equivalent.
        test::<UniqBeforePluck>().expect_offense(indoc! {r#"
                Post.pluck(:id, :name).uniq
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `distinct` before `pluck` instead of `uniq` after.
            "#});
    }

    // === no-hit cases ===

    #[test]
    fn does_not_flag_distinct_then_pluck() {
        // Already the recommended form — leave alone. The chain
        // (send (send _ :distinct) :pluck _) does not match the
        // (send (send _ :pluck _ ...) :uniq) shape.
        test::<UniqBeforePluck>().expect_no_offenses("Post.distinct.pluck(:id)\n");
    }

    #[test]
    fn does_not_flag_pluck_distinct() {
        // `pluck.distinct` is also a recommended-equivalent form
        // (ActiveRecord chain ordering). Out of scope for the cop.
        test::<UniqBeforePluck>().expect_no_offenses("Post.pluck(:id).distinct\n");
    }

    #[test]
    fn does_not_flag_bare_uniq() {
        // No `pluck` in the chain.
        test::<UniqBeforePluck>().expect_no_offenses("arr.uniq\n");
    }

    #[test]
    fn flags_pluck_uniq_with_block() {
        // `Post.pluck(:id).uniq { |x| x.id }` — in the arena AST the
        // block does not enter the Send's arg list (which keeps the
        // `(send ... :uniq)` 0-arity gate happy), but the Send node's
        // `range` covers the full call-with-block expression. Upstream
        // RuboCop-rails behaves the same way (a custom dedup-key block
        // is still a client-side dedup that `distinct` on the AR
        // relation would have avoided).
        test::<UniqBeforePluck>().expect_offense(indoc! {r#"
                Post.pluck(:id).uniq { |x| x.id }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `distinct` before `pluck` instead of `uniq` after.
            "#});
    }

    #[test]
    fn does_not_flag_pluck_zero_args_then_uniq() {
        // Degenerate `pluck.uniq` — `pluck` with no args is
        // ill-formed for `distinct.pluck` rewriting too.
        test::<UniqBeforePluck>().expect_no_offenses("Post.pluck.uniq\n");
    }

    #[test]
    fn does_not_flag_pluck_without_uniq() {
        // No terminator.
        test::<UniqBeforePluck>().expect_no_offenses("Post.pluck(:id)\n");
    }
}
