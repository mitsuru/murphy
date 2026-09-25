//! `RSpec/RedundantAround` — remove redundant `around` hooks.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/RedundantAround
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `match_redundant_around_hook_block?`
//!   (`(any_block (send _ :around ...) ... (send _ :run))` — a single
//!   `example.run` statement) and `match_redundant_around_hook_send?`
//!   (`(send _ :around ... (block-pass (sym :run)))` — `around(&:run)`)
//!   with `RESTRICT_ON_SEND = [:around]` for the send arm and `on_block` /
//!   `on_numblock` for the block arm. Block form flags only when the body
//!   is exactly one `example.run` send (any receiver, method `run`, no
//!   args); `foo { example.run }` nesting or extra statements stay clean.
//!   The offense range is the `around` send trimmed via
//!   `send_without_block_range` (single-line) rather than the whole block.
//!   Detection is at parity; autocorrect (remove the hook) is not ported
//!   in this batch — same convention as `RSpec/ReceiveCounts`
//!   (status: partial, autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! - `around do |example| example.run end` — flagged (whole block).
//! - `around(&:run)` — flagged.
//! - `around do _1.run end` — numblock, flagged.
//! - `config.around do |example| example.run end` — flagged.
//! - `around do |example| example.run; foo end` — extra statement, clean.
//! - `around do |example| foo { example.run } end` — nested, clean.
//!
//! ## No autocorrect
//!
//! Upstream removes the hook. This batch reports only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

use crate::cops::rspec_helpers::send_without_block_range;

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct RedundantAround;

#[cop(
    name = "RSpec/RedundantAround",
    description = "Remove redundant around hook.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl RedundantAround {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, body, .. } = *cx.kind(node) else {
            return;
        };
        if !is_around_send(cx, call) {
            return;
        }
        let Some(body_id) = body.get() else {
            return;
        };
        if !is_bare_run_send(cx, body_id) {
            return;
        }
        cx.emit_offense(
            send_without_block_range(cx, call),
            "Remove redundant `around` hook.",
            None,
        );
    }

    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Numblock { send, body, .. } = *cx.kind(node) else {
            return;
        };
        if !is_around_send(cx, send) {
            return;
        }
        let Some(body_id) = body.get() else {
            return;
        };
        if !is_bare_run_send(cx, body_id) {
            return;
        }
        cx.emit_offense(
            send_without_block_range(cx, send),
            "Remove redundant `around` hook.",
            None,
        );
    }

    #[on_node(kind = "send", methods = ["around"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { args, .. } = *cx.kind(node) else {
            return;
        };
        for arg in cx.list(args).iter().copied() {
            if matches!(*cx.kind(arg), NodeKind::BlockPass(_))
                && let NodeKind::BlockPass(inner) = *cx.kind(arg)
                && let Some(sym_id) = inner.get()
                && matches!(*cx.kind(sym_id), NodeKind::Sym(s) if cx.symbol_str(s) == "run")
            {
                cx.emit_offense(
                    cx.range(node),
                    "Remove redundant `around` hook.",
                    None,
                );
                return;
            }
        }
    }
}

fn is_around_send(cx: &Cx<'_>, call: NodeId) -> bool {
    let NodeKind::Send { method, .. } = *cx.kind(call) else {
        return false;
    };
    cx.symbol_str(method) == "around"
}

fn is_bare_run_send(cx: &Cx<'_>, id: NodeId) -> bool {
    let NodeKind::Send {
        method, args, ..
    } = *cx.kind(id)
    else {
        return false;
    };
    if cx.symbol_str(method) != "run" {
        return false;
    }
    cx.list(args).is_empty()
}

#[cfg(test)]
mod tests {
    use super::RedundantAround;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_redundant_around() {
        test::<RedundantAround>().expect_offense(indoc! {r#"
                around do |example|
                ^^^^^^ Remove redundant `around` hook.
                  example.run
                end
            "#});
    }

    #[test]
    fn flags_block_pass_around() {
        test::<RedundantAround>().expect_offense(indoc! {r#"
                around(&:run)
                ^^^^^^^^^^^^^ Remove redundant `around` hook.
            "#});
    }

    #[test]
    fn flags_config_around() {
        test::<RedundantAround>().expect_offense(indoc! {r#"
                config.around do |example|
                ^^^^^^^^^^^^^ Remove redundant `around` hook.
                  example.run
                end
            "#});
    }

    #[test]
    fn does_not_flag_with_extra_statement() {
        test::<RedundantAround>().expect_no_offenses(indoc! {r#"
                around do |example|
                  example.run
                  foo
                end
            "#});
    }

    #[test]
    fn does_not_flag_nested_run() {
        test::<RedundantAround>().expect_no_offenses(indoc! {r#"
                around do |example|
                  foo { example.run }
                end
            "#});
    }

    #[test]
    fn does_not_flag_empty_around() {
        test::<RedundantAround>().expect_no_offenses(indoc! {r#"
                around do |example|
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(RedundantAround);
