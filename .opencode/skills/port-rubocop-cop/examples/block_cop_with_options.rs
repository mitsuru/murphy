//! Starter template — `Block` dispatch with options.
//!
//! Use this when porting a RuboCop cop whose `on_block` polices the
//! body of some DSL block (RSpec `it`, Rails `before_action`, …) and
//! takes a `Max` / `Limit` / `Threshold` config key.
//!
//! Mirrors the shape of `RSpec/ExampleLength` and
//! `RSpec/MultipleExpectations`. See those in-tree for worked tests.
//!
//! Note: until murphy-9cr.9 lands, the cop reads `Default::default()`
//! at dispatch time — `murphy.toml` overrides are validated but not
//! yet applied at runtime. Spell this out in the doc-comment.

//! `Pack/MyBlockCop` — caps something inside a DSL block body.
//!
//! ## Matched shapes
//! `do_thing do … end` / `do_thing { … }` — bare receiver, target method
//! named `do_thing`. Explicit-receiver forms are skipped.
//!
//! ## Why this shape
//! TODO.
//!
//! ## Option
//! `max` (default `5`) — bodies whose count exceeds `max` are flagged.
//! Runtime option wiring (murphy-9cr.9) is not yet plumbed through `Cx`;
//! v1 honours the `Default`.
//!
//! ## No autocorrect
//! Splitting the body needs human judgement.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, OptNodeId, cop};

#[derive(Default)]
pub struct MyBlockCop;

#[derive(CopOptions)]
pub struct MyBlockCopOptions {
    #[option(default = 5, description = "Maximum allowed count inside the block body.")]
    pub max: i64,
}

#[cop(
    name = "Pack/MyBlockCop",
    description = "TODO: human-readable one-liner.",
    default_severity = "warning",
    default_enabled = true,
    options = MyBlockCopOptions
)]
impl MyBlockCop {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, body, .. } = *cx.kind(node) else {
            return;
        };
        if !is_target_call(cx, call) {
            return;
        }
        let Some(body_id) = body.get() else {
            return; // empty body — never long enough to exceed any max.
        };

        let opts = MyBlockCopOptions::default();
        let count = count_things(cx, body_id);
        if count <= opts.max as usize {
            return;
        }

        cx.emit_offense(
            cx.range(node),
            &format!("TODO: message ({count}/{max})", max = opts.max),
            None,
        );
    }
}

/// `true` when `call` is the bare DSL call this cop polices. Explicit
/// receivers (`Other.do_thing`) belong to some other DSL.
fn is_target_call(cx: &Cx<'_>, call: NodeId) -> bool {
    let NodeKind::Send { receiver, method, .. } = *cx.kind(call) else {
        return false;
    };
    receiver == OptNodeId::NONE && cx.symbol_str(method) == "do_thing"
}

/// TODO: replace with the real count predicate (descendants of a given
/// kind, raw-source line count, …).
fn count_things(cx: &Cx<'_>, body: NodeId) -> usize {
    cx.descendants(body).len()
}

#[cfg(test)]
mod tests {
    use super::MyBlockCop;
    use murphy_plugin_api::test_support::{indoc, run_cop, test};

    fn hits(source: &str) -> usize {
        // `run_cop` is the escape hatch for whole-block emit ranges
        // (multi-line carets are awkward today). The tester builder is
        // still used for shape-level negative cases below.
        run_cop::<MyBlockCop>(source).len()
    }

    #[test]
    fn flags_body_exceeding_default_max() {
        // Real statements inside the block — a comment-only body parses
        // to `body == None` and the cop short-circuits, so the test
        // would silently pass for the wrong reason. Six assignments
        // give the `count_things` descendant walk well over `max = 5`.
        let src = indoc! {r#"
            do_thing do
              a = 1
              b = 2
              c = 3
              d = 4
              e = 5
              f = 6
            end
        "#};
        assert_eq!(hits(src), 1);
    }

    #[test]
    fn ignores_unsupported_shapes() {
        // Explicit-receiver and empty-body shapes both short-circuit
        // before the body walk, so a single tester drives both.
        test::<MyBlockCop>()
            .expect_no_offenses(indoc! {r#"
                Other.do_thing do
                  # TODO: body
                end
            "#})
            .expect_no_offenses(indoc! {r#"
                do_thing do
                end
            "#});
    }
}
