//! `Rails/UniqBeforePluck` — flag the `pluck(:col).uniq` idiom and
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/UniqBeforePluck
//! upstream_version_checked: 2.35.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Audited against rubocop-rails 2.35.0 for murphy-h8ke; murphy-31sx
//!   closed the residual gaps. The `[!^any_block $(send $(send _ :pluck ...)
//!   :uniq ...)]` pattern is mirrored exactly: every block form
//!   (Block/Numblock/Itblock) suppresses the offense, `pluck`/`uniq`
//!   accept any arity, and the conservative/aggressive receiver gates,
//!   the uniq-selector offense range, and the distinct-insert
//!   autocorrect all match upstream.
//! ```
//!
//! recommend `distinct.pluck(:col)`. `uniq` materialises the entire
//! pluck result in Ruby memory and then de-duplicates client-side;
//! `distinct` pushes the dedup to the database, which is dramatically
//! cheaper on non-trivial tables.
//!
//! ## Matched shape (Send node)
//!
//! Outer `Send(receiver=Some(inner), method="uniq", args=[...])`, where
//! `inner` is itself `Send(receiver=_, method="pluck", args=[...])`,
//! mirroring upstream `[!^any_block $(send $(send _ :pluck ...) :uniq ...)]`.
//! Any block form (`pluck(:id).uniq { ... }`, `uniq { _1[0] }`,
//! `uniq { it[0] }`) suppresses the offense because the block changes
//! Array#uniq's equality key. Both `pluck` and `uniq` accept any arity —
//! zero-arg `pluck.uniq` and `uniq`-with-args are near-dead code at
//! runtime (`pluck` needs columns, `uniq` takes no arguments) but are
//! still flagged, matching upstream's `...` wildcards.
//!
//! `EnforcedStyle = "conservative"` (the default) only accepts `pluck`
//! whose receiver is a constant, matching RuboCop's model-class guard.
//! `EnforcedStyle = "aggressive"` accepts any `pluck` receiver.
//!
//! Same shape as `Rails/Pick` with `:first` → `:uniq`; see that cop's
//! module docs for the DSL semantics. Unlike `Pick` (where `first(5)`
//! is not rewritable to `pick`), `uniq` arguments do not affect the
//! `distinct.pluck` rewrite, so no arity gate applies on either side.
//!
//! ## Autocorrect
//!
//! Remove the trailing `.uniq` call and insert `.distinct` immediately
//! before the `pluck` selector's dot. Bare `pluck(:id).uniq` in
//! aggressive mode rewrites to `distinct.pluck(:id)`.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct UniqBeforePluck;

#[derive(CopOptions)]
pub struct UniqBeforePluckOptions {
    #[option(
        name = "EnforcedStyle",
        default = "conservative",
        description = "Whether to flag only model-class pluck calls or every pluck receiver."
    )]
    pub enforced_style: UniqBeforePluckStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum UniqBeforePluckStyle {
    #[option(value = "conservative")]
    Conservative,
    #[option(value = "aggressive")]
    Aggressive,
}

#[cop(
    name = "Rails/UniqBeforePluck",
    description = "Use `distinct` before `pluck`.",
    default_severity = "warning",
    default_enabled = true,
    options = UniqBeforePluckOptions,
)]
impl UniqBeforePluck {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let Some(pluck) = pluck_uniq_receiver(node, cx) else {
            return;
        };
        if is_block_call(node, cx) {
            return;
        }

        let opts = cx.options_or_default::<UniqBeforePluckOptions>();
        if opts.enforced_style == UniqBeforePluckStyle::Conservative
            && !pluck_receiver_is_const(pluck, cx)
        {
            return;
        }

        cx.emit_offense(cx.loc(node).name, "Use `distinct` before `pluck`.", None);
        emit_correction(node, pluck, cx);
    }
}

fn pluck_uniq_receiver(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    // Mirrors upstream `$(send $(send _ :pluck ...) :uniq ...)`: both
    // `pluck` and `uniq` accept any arity (`...` wildcards).
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(node)
    else {
        return None;
    };
    if cx.symbol_str(method) != "uniq" {
        return None;
    };

    let pluck = receiver.get()?;
    let NodeKind::Send {
        method: pluck_method,
        ..
    } = *cx.kind(pluck)
    else {
        return None;
    };
    if cx.symbol_str(pluck_method) != "pluck" {
        return None;
    }
    Some(pluck)
}

fn is_block_call(node: NodeId, cx: &Cx<'_>) -> bool {
    // Mirrors upstream `[!^any_block ...]`: the uniq send is ignored when
    // it is the call of ANY block form (cf. Rails/Output block-call check
    // for the `Block{call}` / `Numblock{send}` / `Itblock{send}` fields).
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    match *cx.kind(parent) {
        NodeKind::Block { call, .. } => call == node,
        NodeKind::Numblock { send, .. } => send == node,
        NodeKind::Itblock { send, .. } => send == node,
        _ => false,
    }
}

fn pluck_receiver_is_const(pluck: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Send { receiver, .. } = *cx.kind(pluck) else {
        return false;
    };
    let Some(receiver) = receiver.get() else {
        return false;
    };
    matches!(*cx.kind(receiver), NodeKind::Const { .. })
}

fn emit_correction(node: NodeId, pluck: NodeId, cx: &Cx<'_>) {
    cx.emit_edit(
        Range {
            start: cx.range(pluck).end,
            end: cx.range(node).end,
        },
        "",
    );

    if let Some(dot) = cx.call_operator_loc(pluck) {
        cx.emit_edit(
            Range {
                start: dot.start,
                end: dot.start,
            },
            ".distinct",
        );
    } else {
        cx.emit_edit(
            Range {
                start: cx.range(pluck).start,
                end: cx.range(pluck).start,
            },
            "distinct.",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{UniqBeforePluck, UniqBeforePluckOptions, UniqBeforePluckStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn aggressive() -> UniqBeforePluckOptions {
        UniqBeforePluckOptions {
            enforced_style: UniqBeforePluckStyle::Aggressive,
        }
    }

    // === hit cases ===

    #[test]
    fn flags_constant_receiver_pluck_id_uniq_by_default() {
        test::<UniqBeforePluck>().expect_correction(
            indoc! {r#"
                Post.pluck(:id).uniq
                                ^^^^ Use `distinct` before `pluck`.
            "#},
            "Post.distinct.pluck(:id)\n",
        );
    }

    #[test]
    fn flags_scoped_constant_receiver_by_default() {
        test::<UniqBeforePluck>().expect_correction(
            indoc! {r#"
                Admin::Post.pluck(:id).uniq
                                       ^^^^ Use `distinct` before `pluck`.
            "#},
            "Admin::Post.distinct.pluck(:id)\n",
        );
    }

    #[test]
    fn aggressive_flags_chain_then_pluck_uniq() {
        test::<UniqBeforePluck>()
            .with_options(&aggressive())
            .expect_correction(
                indoc! {r#"
                User.where(active: true).pluck(:name).uniq
                                                      ^^^^ Use `distinct` before `pluck`.
            "#},
                "User.where(active: true).distinct.pluck(:name)\n",
            );
    }

    #[test]
    fn aggressive_flags_local_receiver_pluck_uniq() {
        test::<UniqBeforePluck>()
            .with_options(&aggressive())
            .expect_correction(
                indoc! {r#"
                posts.pluck(:title).uniq
                                    ^^^^ Use `distinct` before `pluck`.
            "#},
                "posts.distinct.pluck(:title)\n",
            );
    }

    #[test]
    fn flags_multi_column_pluck() {
        // Multi-column `pluck(:id, :name).uniq` is also a candidate —
        // `distinct.pluck(:id, :name)` is the AR-relation equivalent.
        test::<UniqBeforePluck>().expect_correction(
            indoc! {r#"
                Post.pluck(:id, :name).uniq
                                       ^^^^ Use `distinct` before `pluck`.
            "#},
            "Post.distinct.pluck(:id, :name)\n",
        );
    }

    // === no-hit cases ===

    #[test]
    fn conservative_does_not_flag_chain_receiver() {
        test::<UniqBeforePluck>()
            .expect_no_offenses("User.where(active: true).pluck(:name).uniq\n");
    }

    #[test]
    fn conservative_does_not_flag_local_receiver() {
        test::<UniqBeforePluck>().expect_no_offenses("posts.pluck(:title).uniq\n");
    }

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
    fn does_not_flag_pluck_uniq_with_block() {
        test::<UniqBeforePluck>().expect_no_offenses("Post.pluck(:id).uniq { |x| x.id }\n");
    }

    #[test]
    fn does_not_flag_pluck_uniq_with_numblock() {
        // Mirrors upstream `ignores uniq with a numblock` spec:
        // `uniq { _1[0] }` is a Numblock whose call is the uniq send,
        // so the upstream `[!^any_block]` guard ignores it.
        test::<UniqBeforePluck>().expect_no_offenses("Post.pluck(:id).uniq { _1[0] }\n");
    }

    #[test]
    fn does_not_flag_pluck_uniq_with_itblock() {
        // Mirrors upstream `ignores uniq with an \`it\` block` spec
        // (Ruby 3.4): `uniq { it[0] }` is an Itblock whose call is the
        // uniq send, so the upstream `[!^any_block]` guard ignores it.
        test::<UniqBeforePluck>().expect_no_offenses("Post.pluck(:id).uniq { it[0] }\n");
    }

    #[test]
    fn flags_zero_arg_pluck_then_uniq() {
        // Upstream `:pluck ...` wildcard matches zero-arg `pluck` too
        // (near-dead code at runtime, still flagged for parity).
        test::<UniqBeforePluck>().expect_correction(
            indoc! {r#"
                Post.pluck.uniq
                           ^^^^ Use `distinct` before `pluck`.
            "#},
            "Post.distinct.pluck\n",
        );
    }

    #[test]
    fn flags_pluck_then_uniq_with_args() {
        // Upstream `:uniq ...` wildcard matches `uniq` with args too
        // (near-dead code at runtime, still flagged for parity).
        test::<UniqBeforePluck>().expect_correction(
            indoc! {r#"
                Post.pluck(:id).uniq(:foo)
                                ^^^^ Use `distinct` before `pluck`.
            "#},
            "Post.distinct.pluck(:id)\n",
        );
    }

    #[test]
    fn does_not_flag_pluck_without_uniq() {
        // No terminator.
        test::<UniqBeforePluck>().expect_no_offenses("Post.pluck(:id)\n");
    }

    #[test]
    fn corrects_parenthesized_uniq_call() {
        test::<UniqBeforePluck>().expect_correction(
            indoc! {r#"
                Post.pluck(:id).uniq()
                                ^^^^ Use `distinct` before `pluck`.
            "#},
            "Post.distinct.pluck(:id)\n",
        );
    }

    #[test]
    fn aggressive_corrects_bare_pluck_call() {
        test::<UniqBeforePluck>()
            .with_options(&aggressive())
            .expect_correction(
                indoc! {r#"
                pluck(:id).uniq
                           ^^^^ Use `distinct` before `pluck`.
            "#},
                "distinct.pluck(:id)\n",
            );
    }
}
murphy_plugin_api::submit_cop!(UniqBeforePluck);
