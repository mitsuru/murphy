//! `Rails/NegateInclude` — flag `!x.include?(y)` and recommend
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/NegateInclude
//! upstream_version_checked: 2.35.0
//! status: partial
//! gap_issues:
//!   - murphy-h8ke
//! notes: >
//!   Backfilled metadata; full upstream parity audit still needs to confirm no remaining behavior gaps.
//! ```
//!
//! `x.exclude?(y)` (an ActiveSupport monkey-patch on Enumerable that
//! reads better than the negation), autocorrecting the rewrite.
//!
//! ## Matched shape (Send node)
//!
//! `Send(receiver=Send(receiver=Some(_), method=:include?, args=[_]), method=:!, args=[])`.
//!
//! - Outer Send is the unary bang: `method=:!`, receiver is the inner
//!   `include?` Send, no args. Recall from `Rails/AssertNot` that
//!   `!x` and `not x` both parse as `Send(Some(x), :!, [])` — `Not`
//!   is not its own `NodeKind` variant — so this also flags the
//!   `not x.include?(y)` form, correctly.
//! - Inner Send: non-nil receiver (`!nil?`), method exactly
//!   `:include?`, and **exactly one** positional argument. The arity
//!   gate mirrors RuboCop-rails's `(send $!nil? :include? $_)` — bare
//!   `include?(a, b)` is a custom method on something that isn't
//!   `Enumerable#include?`, and `exclude?` does not accept multiple
//!   args, so we don't flag it and we don't autocorrect it.
//!
//! ## False-positive note
//!
//! `!x.include?(y)` on a custom (non-Enumerable) class that defines
//! its own `include?` still hits. Upstream RuboCop-rails accepts the
//! same risk (`Safe: false` in `config/default.yml`) — the `exclude?`
//! rewrite assumes the receiver implements the Enumerable monkey-patch
//! ActiveSupport ships. Real-world false positives are rare in Rails
//! codebases, where the dominant `include?` callers are AR scopes and
//! basic Enumerable collections.
//!
//! ## Default-on
//!
//! Upstream ships this cop as `Enabled: pending`, which means "off
//! until explicitly enabled, with a warning otherwise". Murphy doesn't
//! model `pending` consistently. For this cop specifically, the
//! convenience choice is `default_enabled = true` — the `exclude?`
//! rewrite is unambiguously desirable in Rails codebases that use
//! ActiveSupport. Other upstream-`pending` cops may differ; see
//! `Rails/I18nLocaleAssignment`, which maps `pending` to
//! `default_enabled = false`.
//!
//! ## Autocorrect (unsafe upstream)
//!
//! `!x.include?(y)` rewrites to `x.exclude?(y)` via two surgical
//! edits:
//!
//! 1. Delete the leading negation token — the slice
//!    `[outer.start, inner.start)`, which covers `!` (and any
//!    whitespace) or the `not ` keyword form.
//! 2. Rename the inner Send's selector — `cx.node(inner).loc.name` —
//!    from `include?` to `exclude?`.
//!
//! The two edits don't overlap, so the receiver and argument source
//! pass through untouched (no whitespace or parenthesisation drift).
//!
//! This is `Safe: false` upstream because the receiver might not
//! implement `exclude?` (e.g. `IPAddr`). Murphy doesn't currently
//! distinguish safe/unsafe autocorrect — users opt in by running
//! `--fix`, so we ship the rewrite.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop, def_node_matcher};

// RuboCop NodePattern equivalent:
//   `(send (send $!nil? :include? $_) :!)`
//
// - Outer: `:!` send, exactly 0 args (the unary `!` shape).
// - Inner: receiver non-None (`!nil?`), method `:include?`, exactly
//   one positional arg.
def_node_matcher!(is_negate_include, "(send (send !nil? :include? _) :!)");

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct NegateInclude;

#[cop(
    name = "Rails/NegateInclude",
    description = "Prefer `collection.exclude?(obj)` over `!collection.include?(obj)`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl NegateInclude {
    // `methods = ["!"]` mirrors upstream `RESTRICT_ON_SEND = %i[!]` —
    // dispatch only on bang sends. The pattern already gates on `:!`;
    // the filter is the parity surface.
    #[on_node(kind = "send", methods = ["!"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        if !is_negate_include(node, cx) {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            "Use `.exclude?` and remove the negation part.",
            None,
        );
        emit_correction(node, cx);
    }
}

/// Emit the two non-overlapping edits that rewrite `!x.include?(y)` to
/// `x.exclude?(y)` in place. Bails on shape mismatches — the pattern
/// already validated them, so this is defensive against a future AST
/// refactor (no panic on a malformed Send).
fn emit_correction(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Send {
        receiver: outer_receiver,
        ..
    } = *cx.kind(node)
    else {
        return;
    };
    let Some(inner) = outer_receiver.get() else {
        return;
    };

    // Strip `!`/`not ` (and any whitespace between the negation token
    // and the inner Send). The outer range begins at the negation
    // token; the inner range begins at the first byte of the
    // `include?` call's receiver — everything in between is the
    // negation prefix.
    let negation_prefix = Range {
        start: cx.range(node).start,
        end: cx.range(inner).start,
    };
    cx.emit_edit(negation_prefix, "");

    // Rename the inner Send's selector: `include?` -> `exclude?`.
    // `loc.name` is the parser-gem-style selector range, so this is
    // exactly the eight bytes we need to overwrite.
    cx.emit_edit(cx.node(inner).loc.name, "exclude?");
}

#[cfg(test)]
mod tests {
    use super::NegateInclude;
    use murphy_plugin_api::test_support::{indoc, run_cop_with_edits, test};

    // === hit cases ===

    #[test]
    fn flags_negate_array_include() {
        test::<NegateInclude>().expect_offense(indoc! {r#"
                !arr.include?(x)
                ^^^^^^^^^^^^^^^^ Use `.exclude?` and remove the negation part.
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
                ^^^^^^^^^^^^^^^^^^^^ Use `.exclude?` and remove the negation part.
            "#});
    }

    #[test]
    fn flags_negate_chain_include() {
        // Receiver is itself a chain — still hits.
        test::<NegateInclude>().expect_offense(indoc! {r#"
                !user.tags.include?("admin")
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `.exclude?` and remove the negation part.
            "#});
    }

    #[test]
    fn flags_negate_include_with_literal_arg() {
        test::<NegateInclude>().expect_offense(indoc! {r#"
                !arr.include?("foo")
                ^^^^^^^^^^^^^^^^^^^^ Use `.exclude?` and remove the negation part.
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
        // inner Send. The pattern's `!nil?` requires a present, non-nil
        // receiver — so this does NOT match. Bare `include?` is also
        // semantically different (it's a class-level `Module#include?`,
        // e.g.).
        test::<NegateInclude>().expect_no_offenses("!include?(x)\n");
    }

    #[test]
    fn does_not_flag_negate_include_no_args() {
        // `!arr.include?` (zero args) is ill-formed `Enumerable#include?`;
        // the pattern's exactly-one-arg gate excludes it.
        test::<NegateInclude>().expect_no_offenses("!arr.include?\n");
    }

    #[test]
    fn does_not_flag_negate_include_multi_arg() {
        // `!arr.include?(x, y)` — multi-arg `include?` is a custom
        // method, not `Enumerable#include?`. RuboCop's pattern uses
        // `$_` (exactly one capture), and our pattern mirrors that
        // arity gate — no offense, no autocorrect. Pinning this as a
        // contract: a future loosening of the arity gate would silently
        // start rewriting unrelated `include?` callers to broken
        // `exclude?(x, y)` calls.
        test::<NegateInclude>().expect_no_offenses("!arr.include?(x, y)\n");
    }

    #[test]
    fn does_not_flag_other_negation_target() {
        // `!arr.size.zero?` — outer `!` on `zero?`, not `include?`.
        test::<NegateInclude>().expect_no_offenses("!arr.size.zero?\n");
    }

    // === autocorrect ===

    #[test]
    fn corrects_negate_array_include() {
        test::<NegateInclude>().expect_correction(
            indoc! {r#"
                !arr.include?(x)
                ^^^^^^^^^^^^^^^^ Use `.exclude?` and remove the negation part.
            "#},
            "arr.exclude?(x)\n",
        );
    }

    #[test]
    fn corrects_negate_hash_include_symbol_arg() {
        // Symbol arg source is preserved byte-for-byte (`:key`).
        test::<NegateInclude>().expect_correction(
            indoc! {r#"
                !hash.include?(:key)
                ^^^^^^^^^^^^^^^^^^^^ Use `.exclude?` and remove the negation part.
            "#},
            "hash.exclude?(:key)\n",
        );
    }

    #[test]
    fn corrects_negate_chain_include() {
        // Multi-segment receiver source is reproduced verbatim.
        test::<NegateInclude>().expect_correction(
            indoc! {r#"
                !user.tags.include?("admin")
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `.exclude?` and remove the negation part.
            "#},
            "user.tags.exclude?(\"admin\")\n",
        );
    }

    #[test]
    fn corrects_not_keyword_form() {
        // Ruby's `not foo` parses identically to `!foo` — same Send
        // shape, same rewrite. Pinning this so a future parser change
        // (or a DSL `not`-specific branch) can't silently regress.
        test::<NegateInclude>().expect_correction(
            indoc! {r#"
                not arr.include?(x)
                ^^^^^^^^^^^^^^^^^^^ Use `.exclude?` and remove the negation part.
            "#},
            "arr.exclude?(x)\n",
        );
    }

    #[test]
    fn correction_reaches_fixpoint() {
        // Apply both edits, then re-run the cop on the result: zero
        // offenses. This pins idempotence — the rewrite must not
        // produce something the cop would flag again. Two edits:
        // delete `!` and rename `include?` -> `exclude?`.
        let run = run_cop_with_edits::<NegateInclude>("!arr.include?(x)\n");
        assert_eq!(run.edits.len(), 2);
        let mut replacements: Vec<&str> =
            run.edits.iter().map(|e| e.replacement.as_str()).collect();
        replacements.sort();
        assert_eq!(replacements, ["", "exclude?"]);
        test::<NegateInclude>().expect_no_offenses("arr.exclude?(x)\n");
    }
}
murphy_plugin_api::submit_cop!(NegateInclude);
