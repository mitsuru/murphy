//! `Rails/Pick` — flag the `pluck(:col).first` idiom in favour of the
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/Pick
//! upstream_version_checked: 2.35.0
//! status: partial
//! gap_issues:
//!   - murphy-as0h
//! notes: >
//!   csend shapes, offense message, and autocorrect (pluck→pick) implemented
//!   (murphy-gu5d). Remaining gap: TargetRailsVersion 6.0 minimum gating —
//!   no rails version infrastructure exists yet (murphy-as0h).
//! ```
//!
//! Rails 6+ `pick(:col)` shorthand. `pick` materialises only the first
//! row at the SQL level (it tacks on a `LIMIT 1`), whereas
//! `pluck(:col).first` loads the entire column into Ruby first and then
//! discards everything but the first row — a needless round-trip and
//! allocation in any non-trivial table.
//!
//! ## Matched shapes (Send and Csend nodes)
//!
//! Outer `Send`/`Csend`(receiver=Some(inner), method="first", args=[])`,
//! where `inner` is itself `Send`/`Csend`(receiver=_, method="pluck",
//! args=[_, ...])`.
//!
//! - **outer method == `first`** — the terminator we care about.
//!   `.last`, `.second`, etc. are intentionally out of scope (upstream
//!   RuboCop-rails also limits to `.first`).
//! - **outer args empty** — `pluck(:id).first(5)` carries a limit
//!   argument and is **not** rewritable to `pick(:id)` (pick has no
//!   multi-row form), so it's excluded.
//! - **outer receiver is a `pluck` Send/Csend with ≥1 argument** —
//!   `pluck.first` (zero args) is excluded as a degenerate /
//!   non-equivalent form. Rails 6+ `pick(*columns)` is variadic, so
//!   `pluck(:id, :name).first` (multi-column) **does** match
//!   (roborev review feedback on murphy-cy1).
//! - **inner Send/Csend's receiver shape is unconstrained** — a const
//!   (`Post.pluck(:id).first`), a chain
//!   (`User.where(active: true).pluck(:name).first`), a local
//!   (`posts.pluck(:title).first`) all match. Safe-navigation forms
//!   (`x.pluck(:a)&.first`, `x&.pluck(:a)&.first`) also match.
//!   Mirrors upstream RuboCop-rails which doesn't gate on the receiver
//!   of `pluck`.
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
//! A single [`def_node_matcher!`] pattern `(call (call _ :pluck _ ...)
//! :first)` covers all four outer/inner Send/Csend combinations, because
//! the `call` head expands to `{send csend}` at each level (murphy-b6nq) —
//! mirroring RuboCop-rails' `(call (call _ :pluck ...) :first)`:
//!
//! - outer Send + inner Send: `Post.pluck(:id).first`.
//! - outer Send + inner Csend: `x&.pluck(:a).first`.
//! - outer Csend + inner Send: `x.pluck(:a)&.first`.
//! - outer Csend + inner Csend: `x&.pluck(:a)&.first`.
//!
//! `_ ...` in the inner node's argument list means "one wildcard
//! followed by zero-or-more rest" — i.e. ≥1 arg — which rules out the
//! zero-arg `pluck.first` shape (a deliberate divergence from upstream's
//! bare `...`). Trailing argument placeholders are omitted on the outer
//! node (it must take exactly zero args, ruling out `.first(5)`).
//!
//! ## Offense message
//!
//! The message mirrors RuboCop's format:
//! `Prefer \`pick(%<args>s)\` over \`%<current>s\`.`
//! where `args` is the joined raw source of the pluck arguments and
//! `current` is the source from the pluck selector start to the first
//! selector end (e.g. `pluck(:a)&.first`, not `x.pluck(:a)&.first`).
//!
//! ## Autocorrect
//!
//! Mechanically rewriting `pluck(:x).first` → `pick(:x)` is safe in
//! Rails 6+. Two surgical edits:
//!
//! 1. Rename the inner `pluck` selector to `pick`.
//! 2. Delete the trailing `.first` / `&.first` (from inner range end
//!    to outer range end).
//!
//! ## Rails version note
//!
//! `pick` was introduced in Rails 6.0. Murphy currently has no
//! `TargetRailsVersion` gating infrastructure, so this cop will fire
//! regardless of the configured rails version. If you are on Rails < 6.0,
//! disable the cop via `Rails/Pick: {Enabled: false}` in `.murphy.yml`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop, def_node_matcher};

// --- node-pattern matchers --------------------------------------------------
//
// A single `call` head matches both `send` and `csend` at each level
// (murphy-b6nq), collapsing the four outer/inner combinations into one matcher
// that mirrors RuboCop-rails' `(call (call _ :pluck ...) :first)`.
//
// Pattern grammar: `_ ...` = one wildcard + rest ≥0 → arity ≥1 (excludes
// zero-arg `pluck.first`). This `_ ...` is a deliberate divergence from
// upstream's bare `...` (which would also match zero-arg `pluck.first`); see
// the `does_not_flag_pluck_zero_args_then_first` test. No trailing placeholders
// on the outer node (outer must have exactly zero args, ruling out `.first(5)`).
def_node_matcher!(is_pluck_first, "(call (call _ :pluck _ ...) :first)");

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
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// Core check shared by send and csend dispatch.
///
/// Emits an offense with a dynamic message mirroring RuboCop's format:
/// `Prefer \`pick(<args>)\` over \`<current>\`.`
///
/// - `<args>`: raw source of the inner pluck node's arguments, joined
///   with `", "`.
/// - `<current>`: raw source from the inner pluck node's selector start
///   to the outer first node's selector end (e.g. `pluck(:a)&.first`).
fn check(node: NodeId, cx: &Cx<'_>) {
    if !cx.rails_version_at_least(6, 0) {
        return;
    }

    if !is_pluck_first(node, cx) {
        return;
    }

    // Extract the inner pluck node from the outer's receiver.
    // Both Send and Csend use the same field layout (receiver is first),
    // but their NodeKind variants differ.
    let inner = match *cx.kind(node) {
        NodeKind::Send { receiver, .. } => {
            // Receiver is always present here (pattern gated on it).
            receiver.get().unwrap()
        }
        NodeKind::Csend { receiver, .. } => receiver,
        _ => return,
    };

    // Extract the inner pluck node's args (works for both Send and Csend).
    let inner_args = match *cx.kind(inner) {
        NodeKind::Send { args, .. } => args,
        NodeKind::Csend { args, .. } => args,
        _ => return,
    };

    // Build `<args>`: join raw source of each pluck arg with ", ".
    let args_str = cx
        .list(inner_args)
        .iter()
        .map(|&arg_id| cx.raw_source(cx.range(arg_id)))
        .collect::<Vec<_>>()
        .join(", ");

    // Build `<current>`: from the inner pluck selector start to the
    // outer first selector end. This mirrors RuboCop's offense_range
    // which runs from the inner method name to the outer method name end.
    let inner_name_start = cx.loc(inner).name.start;
    let outer_name_end = cx.loc(node).name.end;
    let current_str = if inner_name_start > 0 && outer_name_end > inner_name_start {
        cx.raw_source(Range {
            start: inner_name_start,
            end: outer_name_end,
        })
    } else {
        cx.raw_source(cx.range(node))
    };

    let msg = format!("Prefer `pick({args_str})` over `{current_str}`.");
    cx.emit_offense(cx.range(node), &msg, None);

    // Autocorrect: two surgical edits.
    // Edit 1: rename `pluck` → `pick` (inner selector).
    cx.emit_edit(cx.loc(inner).name, "pick");
    // Edit 2: delete from inner range end to outer range end, removing
    // `.first` or `&.first`.
    cx.emit_edit(
        Range {
            start: cx.range(inner).end,
            end: cx.range(node).end,
        },
        "",
    );
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
                ^^^^^^^^^^^^^^^^^^^^^ Prefer `pick(:id)` over `pluck(:id).first`.
            "#});
    }

    #[test]
    fn flags_receiverless_pluck_first() {
        // murphy-if9y: receiverless `pluck(:id)` (e.g. inside a model method
        // where the relation receiver is implicit `self`) is `(send nil :pluck
        // …)` in RuboCop, so the inner `(send _ :pluck _ ...)` wildcard receiver
        // matches the absent slot — `pluck(:id).first` IS flagged. Murphy missed
        // this before the `RecvOptNode` fix; standalone RuboCop NodePattern
        // confirms `(send (send _ :pluck _ ...) :first)` matches `pluck(:id).first`.
        test::<Pick>().expect_offense(indoc! {r#"
                pluck(:id).first
                ^^^^^^^^^^^^^^^^ Prefer `pick(:id)` over `pluck(:id).first`.
            "#});
    }

    #[test]
    fn flags_chain_then_pluck_first() {
        // The receiver of `pluck` is itself a chain — the inner Send's
        // receiver shape is unconstrained, so this still hits.
        test::<Pick>().expect_offense(indoc! {r#"
                User.where(active: true).pluck(:name).first
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `pick(:name)` over `pluck(:name).first`.
            "#});
    }

    #[test]
    fn flags_local_receiver_pluck_first() {
        // Bare local receiver — same shape, still hits.
        test::<Pick>().expect_offense(indoc! {r#"
                posts.pluck(:title).first
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `pick(:title)` over `pluck(:title).first`.
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
                ^^^^^^^^^^^^^^^^^^^^^ Prefer `pick(:id)` over `pluck(:id).first`.
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
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `pick(:id, :name)` over `pluck(:id, :name).first`.
            "#});
    }

    #[test]
    fn flags_csend_outer_send_inner() {
        // Safe-navigation on the outer `.first` call; inner `pluck` is
        // a regular send. RuboCop flags this as a csend (outer) + send
        // (inner) combination.
        test::<Pick>().expect_offense(indoc! {r#"
                x.pluck(:a)&.first
                ^^^^^^^^^^^^^^^^^^ Prefer `pick(:a)` over `pluck(:a)&.first`.
            "#});
    }

    #[test]
    fn flags_csend_both_outer_and_inner() {
        // Safe-navigation on both outer `.first` and inner `.pluck`.
        // RuboCop flags this combination as well.
        test::<Pick>().expect_offense(indoc! {r#"
                x&.pluck(:a)&.first
                ^^^^^^^^^^^^^^^^^^^ Prefer `pick(:a)` over `pluck(:a)&.first`.
            "#});
    }

    #[test]
    fn flags_send_outer_csend_inner() {
        // Safe-navigation on the inner `.pluck` call; outer `.first` is
        // a regular send. This is the fourth outer/inner combination
        // (`x&.pluck(:a).first`).
        test::<Pick>().expect_offense(indoc! {r#"
                x&.pluck(:a).first
                ^^^^^^^^^^^^^^^^^^ Prefer `pick(:a)` over `pluck(:a).first`.
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

    // === version-gating regression (murphy-as0h) ===

    #[test]
    fn does_not_fire_below_rails_6() {
        test::<Pick>()
            .with_target_rails_version(5, 2)
            .expect_no_offenses("Post.pluck(:id).first\n");
    }

    #[test]
    fn fires_at_rails_6() {
        test::<Pick>().with_target_rails_version(6, 0).expect_offense(indoc! {r#"
                Post.pluck(:id).first
                ^^^^^^^^^^^^^^^^^^^^^ Prefer `pick(:id)` over `pluck(:id).first`.
            "#});
    }

    #[test]
    fn fires_when_target_rails_version_is_unset() {
        test::<Pick>().expect_offense(indoc! {r#"
                Post.pluck(:id).first
                ^^^^^^^^^^^^^^^^^^^^^ Prefer `pick(:id)` over `pluck(:id).first`.
            "#});
    }

    // === autocorrect cases ===

    #[test]
    fn corrects_pluck_id_first_to_pick_id() {
        test::<Pick>()
            .expect_correction(
                indoc! {r#"
                    Post.pluck(:id).first
                    ^^^^^^^^^^^^^^^^^^^^^ Prefer `pick(:id)` over `pluck(:id).first`.
                "#},
                "Post.pick(:id)\n",
            )
            .expect_no_offenses("Post.pick(:id)\n");
    }

    #[test]
    fn corrects_chain_pluck_first_to_pick() {
        test::<Pick>()
            .expect_correction(
                indoc! {r#"
                    User.where(active: true).pluck(:name).first
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `pick(:name)` over `pluck(:name).first`.
                "#},
                "User.where(active: true).pick(:name)\n",
            )
            .expect_no_offenses("User.where(active: true).pick(:name)\n");
    }

    #[test]
    fn corrects_csend_outer_to_pick() {
        // `x.pluck(:a)&.first` → `x.pick(:a)`
        test::<Pick>()
            .expect_correction(
                indoc! {r#"
                    x.pluck(:a)&.first
                    ^^^^^^^^^^^^^^^^^^ Prefer `pick(:a)` over `pluck(:a)&.first`.
                "#},
                "x.pick(:a)\n",
            )
            .expect_no_offenses("x.pick(:a)\n");
    }

    #[test]
    fn corrects_multi_column_pluck_first() {
        test::<Pick>()
            .expect_correction(
                indoc! {r#"
                    Post.pluck(:id, :name).first
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `pick(:id, :name)` over `pluck(:id, :name).first`.
                "#},
                "Post.pick(:id, :name)\n",
            )
            .expect_no_offenses("Post.pick(:id, :name)\n");
    }
}
murphy_plugin_api::submit_cop!(Pick);
