//! `Lint/LambdaWithoutLiteralBlock` — avoid `lambda` wrapping an existing proc.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/LambdaWithoutLiteralBlock
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's RESTRICT_ON_SEND=lambda check. Flags lambda calls whose
//!   only argument is a non-symbol block pass, skips literal lambda blocks and
//!   lambda(&:symbol_proc), and autocorrects by removing the lambda wrapper.
//! ```
//!
//! ## Matched shapes
//! - `lambda(&proc { ... })`
//! - `lambda(&Proc.new { ... })`
//!
//! ## Autocorrect
//! Replaces the `lambda(...)` wrapper with the proc argument.

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind};

#[derive(Default)]
pub struct LambdaWithoutLiteralBlock;

#[cop(
    name = "Lint/LambdaWithoutLiteralBlock",
    description = "Checks uses of lambda without a literal block.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl LambdaWithoutLiteralBlock {
    #[on_node(kind = "send", methods = ["lambda"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        if cx.parent(node).get().is_some_and(
            |parent| matches!(cx.kind(parent), NodeKind::Block { call, .. } if *call == node),
        ) {
            return;
        }

        let NodeKind::Send { receiver, args, .. } = *cx.kind(node) else {
            return;
        };
        if receiver.get().is_some() {
            return;
        }

        let args = cx.list(args);
        if args.len() != 1 {
            return;
        }
        let NodeKind::BlockPass(inner) = *cx.kind(args[0]) else {
            return;
        };
        let Some(inner) = inner.get() else {
            return;
        };
        if matches!(cx.kind(inner), NodeKind::Sym(_)) {
            return;
        }

        cx.emit_offense(
            cx.range(node),
            "lambda without a literal block is deprecated; use the proc without lambda instead.",
            None,
        );
        cx.emit_edit(cx.range(node), cx.raw_source(cx.range(inner)));
    }
}

murphy_plugin_api::submit_cop!(LambdaWithoutLiteralBlock);

#[cfg(test)]
mod tests {
    use super::LambdaWithoutLiteralBlock;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_lambda_with_proc_block_pass() {
        test::<LambdaWithoutLiteralBlock>().expect_correction(
            indoc! {r#"
                lambda(&proc { do_something })
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ lambda without a literal block is deprecated; use the proc without lambda instead.
            "#},
            "proc { do_something }\n",
        );
    }

    #[test]
    fn accepts_literal_lambda_block() {
        test::<LambdaWithoutLiteralBlock>().expect_no_offenses("lambda { do_something }\n");
    }

    #[test]
    fn accepts_symbol_proc() {
        test::<LambdaWithoutLiteralBlock>().expect_no_offenses("lambda(&:to_s)\n");
    }
}
