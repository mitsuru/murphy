//! `Rails/UniqBeforePluck` — flag the `pluck(:col).uniq` idiom and
//! recommend `distinct.pluck(:col)`. `uniq` materialises the entire
//! pluck result in Ruby memory and then de-duplicates client-side;
//! `distinct` pushes the dedup to the database, which is dramatically
//! cheaper on non-trivial tables.
//!
//! ## Matched shape (Send node)
//!
//! Outer `Send(receiver=Some(inner), method="uniq", args=[])`, where
//! `inner` is itself `Send(receiver=_, method="pluck", args=[_, ...])`.
//! Block forms (`pluck(:id).uniq { ... }`) are skipped because the block
//! changes Array#uniq's equality key.
//!
//! `EnforcedStyle = "conservative"` (the default) only accepts `pluck`
//! whose receiver is a constant, matching RuboCop's model-class guard.
//! `EnforcedStyle = "aggressive"` accepts any `pluck` receiver.
//!
//! Same shape as `Rails/Pick` with `:first` → `:uniq`; see that cop's
//! module docs for the DSL semantics. `pluck` arity ≥1 (zero-arg
//! `pluck` is a degenerate form), outer `uniq` arity 0.
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
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(node)
    else {
        return None;
    };
    if cx.symbol_str(method) != "uniq" || !cx.list(args).is_empty() {
        return None;
    };

    let pluck = receiver.get()?;
    let NodeKind::Send {
        method: pluck_method,
        args: pluck_args,
        ..
    } = *cx.kind(pluck)
    else {
        return None;
    };
    if cx.symbol_str(pluck_method) != "pluck" || cx.list(pluck_args).is_empty() {
        return None;
    }
    Some(pluck)
}

fn is_block_call(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    matches!(*cx.kind(parent), NodeKind::Block { call, .. } if call == node)
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
