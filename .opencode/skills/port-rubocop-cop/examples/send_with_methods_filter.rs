//! Starter template — `Send` dispatch with `methods = [...]` filter.
//!
//! Use this when porting a RuboCop cop whose `on_send` is gated by
//! `RESTRICT_ON_SEND = %i[name1 name2]`. The host pre-filters by method
//! symbol so the cop body never runs for unrelated calls.
//!
//! Strip this header, rename `MyCop` / `Pack/MyCop`, and replace the
//! TODO blocks. See `crates/murphy-rspec/src/cops/rspec/describe_class.rs`
//! for the worked example this template is distilled from.

//! `Pack/MyCop` — one-line summary.
//!
//! ## Matched shapes
//! - `target_method(...)` with empty receiver (TODO: spell out).
//!
//! ## Why this shape
//! TODO: motivation; what RuboCop calls this case.
//!
//! ## No autocorrect
//! TODO: reason, or delete this section and emit edits.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

#[derive(Default)]
pub struct MyCop;

#[cop(
    name = "Pack/MyCop",
    description = "TODO: human-readable one-liner.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl MyCop {
    #[on_node(kind = "send", methods = ["target_method"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // The `methods = [...]` filter on `#[on_node]` gates dispatch
        // before this body runs — only `Send { method == "target_method" }`
        // reaches here. The `let-else` is defensive against a future
        // kind-aliasing accident; statically unreachable today.
        let NodeKind::Send { receiver, args, .. } = *cx.kind(node) else {
            return;
        };

        // TODO: keep, narrow, or remove the empty-receiver gate.
        // RuboCop matchers that look at `foo(...)` mean bare `foo`,
        // not `obj.foo(...)`.
        if receiver != OptNodeId::NONE {
            return;
        }

        // TODO: walk `cx.list(args)` and decide whether to emit.
        let _ = args;

        cx.emit_offense(
            cx.range(node),
            "TODO: offense message.",
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::MyCop;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn target_call_dispatch_contract() {
        test::<MyCop>()
            .expect_offense(indoc! {r#"
                target_method(1)
                ^^^^^^^^^^^^^^^^ TODO: offense message.
            "#})
            // Method name matches but the call goes through a receiver —
            // `obj.target_method(1)` is not the bare DSL call we mean to
            // flag. Pinned alongside the positive case so a regression
            // in the empty-receiver gate fails this single test.
            .expect_no_offenses(indoc! {r#"
                obj.target_method(1)
            "#})
            // Unrelated method names never reach the body because of
            // the `methods = [...]` filter on `#[on_node]`.
            .expect_no_offenses(indoc! {r#"
                other_method(1)
            "#});
    }
}
