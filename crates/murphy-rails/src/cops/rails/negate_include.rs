//! `Rails/NegateInclude` — flag `!x.include?(y)` and recommend
//! `x.exclude?(y)` (an ActiveSupport monkey-patch on Enumerable that
//! reads better than the negation).
//!
//! ## Matched shape (Send node)
//!
//! `Send(receiver=Send(receiver=_, method=:include?, args=[_, ...]), method=:!, args=[])`.
//!
//! - Outer Send is the unary bang: `method=:!`, receiver is the inner
//!   `include?` Send, no args. (Recall from `Rails/AssertNot` that
//!   `!x` and `not x` both parse as `Send(receiver=Some(x), :!, [])`
//!   — `Not` is not its own `NodeKind` variant.)
//! - Inner Send is the include-check: any receiver (`_`), method
//!   exactly `:include?`, and ≥1 argument (`_ ...`). Mirrors upstream
//!   RuboCop-rails which doesn't gate on the receiver kind — any
//!   `include?` call counts (Array, Hash, Set, custom Enumerable).
//!
//! ## False-positive note
//!
//! `!x.include?(y)` on a custom (non-Enumerable) class that defines
//! its own `include?` still hits. Upstream RuboCop-rails accepts the
//! same risk — the `exclude?` rewrite assumes the receiver implements
//! the Enumerable monkey-patch ActiveSupport ships. Real-world false
//! positives are rare in Rails codebases, where the dominant
//! `include?` callers are AR scopes and basic Enumerable collections.
//!
//! ## No autocorrect
//!
//! Mechanical `!x.include?(y)` → `x.exclude?(y)` is safe in Rails apps
//! (ActiveSupport defines `exclude?` on Enumerable), but ADR 0006
//! requires a deliberate fix block per cop and this v1 ships
//! detect-only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, cop, node_pattern};

// RuboCop NodePattern equivalent:
//   `(send (send _ :include? _ ...) :!)`
//
// - Outer: `:!` send, exactly 0 args (the unary `!` shape).
// - Inner: `:include?` send, ≥1 args (`_ ...`), receiver unconstrained.
node_pattern!(is_negate_include, "(send (send _ :include? _ ...) :!)");

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct NegateInclude;

#[cop(
    name = "Rails/NegateInclude",
    description = "Use `exclude?` instead of `!include?`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl NegateInclude {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        if !is_negate_include(node, cx) {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            "Use `exclude?` instead of `!include?`.",
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::NegateInclude;
    use murphy_plugin_api::test_support::{indoc, test};

    // === hit cases ===

    #[test]
    fn flags_negate_array_include() {
        test::<NegateInclude>().expect_offense(indoc! {r#"
                !arr.include?(x)
                ^^^^^^^^^^^^^^^^ Use `exclude?` instead of `!include?`.
            "#});
    }

    #[test]
    fn flags_negate_hash_include() {
        // Hash#include? on a non-parenthesised negation. Note: writing
        // it as `!(hash.include?(:key))` (with explicit grouping
        // parens) does **not** match — the parens parse as a `Begin`
        // wrapper around the inner Send, so the outer Send's receiver
        // becomes `Begin([...])` rather than the inner `include?`
        // Send. Out of scope for v1; if dogfood surfaces this shape we
        // can extend the DSL with a `begin`-stripping helper.
        test::<NegateInclude>().expect_offense(indoc! {r#"
                !hash.include?(:key)
                ^^^^^^^^^^^^^^^^^^^^ Use `exclude?` instead of `!include?`.
            "#});
    }

    #[test]
    fn flags_negate_chain_include() {
        // Receiver is itself a chain — still hits.
        test::<NegateInclude>().expect_offense(indoc! {r#"
                !user.tags.include?("admin")
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `exclude?` instead of `!include?`.
            "#});
    }

    #[test]
    fn flags_negate_include_with_literal_arg() {
        test::<NegateInclude>().expect_offense(indoc! {r#"
                !arr.include?("foo")
                ^^^^^^^^^^^^^^^^^^^^ Use `exclude?` instead of `!include?`.
            "#});
    }

    // === no-hit cases ===

    #[test]
    fn does_not_flag_plain_include() {
        // No negation — leave alone.
        test::<NegateInclude>().expect_no_offenses("arr.include?(x)\n");
    }

    #[test]
    fn does_not_flag_exclude() {
        // Already the recommended form.
        test::<NegateInclude>().expect_no_offenses("arr.exclude?(x)\n");
    }

    #[test]
    fn does_not_flag_negate_empty() {
        // Different method on the inner Send.
        test::<NegateInclude>().expect_no_offenses("!arr.empty?\n");
    }

    #[test]
    fn does_not_flag_bare_include() {
        // `!include?(x)` (bare include?) has receiver = None on the
        // inner Send. The DSL's `_` for receiver requires a receiver
        // (it accepts any node, but None has no node) — so this
        // does NOT match. Bare `include?` is also semantically
        // different (it's a class-level Module#include?, e.g.).
        test::<NegateInclude>().expect_no_offenses("!include?(x)\n");
    }

    #[test]
    fn does_not_flag_negate_include_no_args() {
        // `!arr.include?` (zero args) is ill-formed call; the DSL's
        // `_ ...` arity-≥1 gate excludes it.
        test::<NegateInclude>().expect_no_offenses("!arr.include?\n");
    }

    #[test]
    fn does_not_flag_other_negation_target() {
        // `!arr.size.zero?` — outer `!` on `zero?`, not `include?`.
        test::<NegateInclude>().expect_no_offenses("!arr.size.zero?\n");
    }
}
