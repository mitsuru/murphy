//! `RSpec/ExpectChange` — consistent `change` matcher style.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/ExpectChange
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `expect_change_with_arguments`
//!   (`(send nil? :change $_ ({sym str} $_))`) for `EnforcedStyle: block`
//!   and `expect_change_with_block`
//!   (`(block (send nil? :change) (args) (send ${(send nil? _) const} $_))`)
//!   for `EnforcedStyle: method_call` (default). Block-form bodies must be
//!   a simple `receiver.message` with no args: `const` (`Foo.bar`) or bare
//!   zero-arg `send` (`user.name`); `Foo.bar(:count)` (args) and
//!   `user.reload.name` (chained receiver) do not flag per upstream's
//!   "also good" examples. Messages mirror `MSG_BLOCK`
//!   (`Prefer `change(obj, :attr)``) and `MSG_CALL`
//!   (`Prefer `change { obj.attr }``) with `obj` from `raw_source`.
//!   Detection is at parity; autocorrect (rewriting the style) is not
//!   ported in this batch — same convention as `RSpec/HookArgument`
//!   (status: partial, autocorrect as gap; `Numblock` not ported).
//! ```
//!
//! ## Matched shapes
//!
//! Default style `method_call` (dispatched on `Block`):
//!
//! - `expect { run }.to change { Foo.bar }` — flagged.
//! - `expect { run }.to change { foo.baz }` — flagged.
//! - `expect { run }.to change { Foo.bar(:count) }` — args, not flagged.
//! - `expect { run }.to change { user.reload.name }` — chained, not flagged.
//! - `expect { run }.to change(Foo, :bar)` — method-call form, not flagged
//!   under this style.
//!
//! Style `block` (dispatched on `Send` `change`):
//!
//! - `expect { run }.to change(Foo, :bar)` — flagged.
//! - `expect { run }.to change { Foo.bar }` — block form, not flagged
//!   under this style.
//!
//! ## No autocorrect
//!
//! Upstream rewrites between styles (unsafe). This batch reports only.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ExpectChange;

#[derive(CopOptions)]
pub struct ExpectChangeOptions {
    #[option(
        name = "EnforcedStyle",
        default = "method_call",
        description = "Whether change matchers use method-call or block style."
    )]
    pub enforced_style: ExpectChangeStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum ExpectChangeStyle {
    #[option(value = "method_call")]
    MethodCall,
    #[option(value = "block")]
    Block,
}

#[cop(
    name = "RSpec/ExpectChange",
    description = "Checks for consistent style of change matcher.",
    default_severity = "warning",
    default_enabled = true,
    options = ExpectChangeOptions,
)]
impl ExpectChange {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<ExpectChangeOptions>();
        if opts.enforced_style != ExpectChangeStyle::MethodCall {
            return;
        }
        let NodeKind::Block { call, args, body } = *cx.kind(node) else {
            return;
        };
        let NodeKind::Send {
            receiver,
            method,
            args: change_args,
        } = *cx.kind(call)
        else {
            return;
        };
        if receiver != OptNodeId::NONE {
            return;
        }
        if cx.symbol_str(method) != "change" {
            return;
        }
        if !cx.list(change_args).is_empty() {
            return;
        }
        let NodeKind::Args(arg_list) = *cx.kind(args) else {
            return;
        };
        if !cx.list(arg_list).is_empty() {
            return;
        }
        let Some(body_id) = body.get() else {
            return;
        };
        let NodeKind::Send {
            receiver: body_recv,
            method: body_method,
            args: body_args,
        } = *cx.kind(body_id)
        else {
            return;
        };
        if !cx.list(body_args).is_empty() {
            return;
        }
        let Some(recv_id) = body_recv.get() else {
            return;
        };
        if !is_simple_receiver(cx, recv_id) {
            return;
        }
        let obj = cx.raw_source(cx.range(recv_id)).to_owned();
        let attr = cx.symbol_str(body_method).to_owned();
        cx.emit_offense(
            cx.range(node),
            &format!("Prefer `change({obj}, :{attr})`."),
            None,
        );
    }

    #[on_node(kind = "send", methods = ["change"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<ExpectChangeOptions>();
        if opts.enforced_style != ExpectChangeStyle::Block {
            return;
        }
        let NodeKind::Send {
            receiver, method, args,
        } = *cx.kind(node)
        else {
            return;
        };
        if receiver != OptNodeId::NONE {
            return;
        }
        if cx.symbol_str(method) != "change" {
            return;
        }
        let arg_ids = cx.list(args);
        if arg_ids.len() != 2 {
            return;
        }
        let recv_id = arg_ids[0];
        let msg_id = arg_ids[1];
        let attr = match *cx.kind(msg_id) {
            NodeKind::Sym(sym) => cx.symbol_str(sym).to_owned(),
            NodeKind::Str(sid) => cx.string_str(sid).to_owned(),
            _ => return,
        };
        let obj = cx.raw_source(cx.range(recv_id)).to_owned();
        cx.emit_offense(
            cx.range(node),
            &format!("Prefer `change {{ {obj}.{attr} }}`."),
            None,
        );
    }
}

/// `true` when `id` is a simple change receiver: `Const` (`Foo`) or bare
/// zero-arg `Send` (`user`). Chained calls (`user.reload`) do not match
/// upstream's `{(send nil? _) const}` for the simple case.
fn is_simple_receiver(cx: &Cx<'_>, id: NodeId) -> bool {
    match *cx.kind(id) {
        NodeKind::Const { .. } => true,
        NodeKind::Send {
            receiver,
            args,
            ..
        } => receiver == OptNodeId::NONE && cx.list(args).is_empty(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{ExpectChange, ExpectChangeOptions, ExpectChangeStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_block_form_const() {
        test::<ExpectChange>().expect_offense(indoc! {r#"
                expect { run }.to change { Foo.bar }
                                  ^^^^^^^^^^^^^^^^^^ Prefer `change(Foo, :bar)`.
            "#});
    }

    #[test]
    fn flags_block_form_send() {
        test::<ExpectChange>().expect_offense(indoc! {r#"
                expect { run }.to change { foo.baz }
                                  ^^^^^^^^^^^^^^^^^^ Prefer `change(foo, :baz)`.
            "#});
    }

    #[test]
    fn does_not_flag_block_with_args() {
        test::<ExpectChange>().expect_no_offenses(indoc! {r#"
                expect { run }.to change { Foo.bar(:count) }
            "#});
    }

    #[test]
    fn does_not_flag_chained() {
        test::<ExpectChange>().expect_no_offenses(indoc! {r#"
                expect { run }.to change { user.reload.name }
            "#});
    }

    #[test]
    fn does_not_flag_method_call_under_default() {
        test::<ExpectChange>().expect_no_offenses(indoc! {r#"
                expect { run }.to change(Foo, :bar)
            "#});
    }

    fn block_style() -> ExpectChangeOptions {
        ExpectChangeOptions {
            enforced_style: ExpectChangeStyle::Block,
        }
    }

    #[test]
    fn flags_method_call_under_block_style() {
        test::<ExpectChange>()
            .with_options(&block_style())
            .expect_offense(indoc! {r#"
                expect { run }.to change(Foo, :bar)
                                  ^^^^^^^^^^^^^^^^^ Prefer `change { Foo.bar }`.
            "#});
    }

    #[test]
    fn does_not_flag_block_under_block_style() {
        test::<ExpectChange>()
            .with_options(&block_style())
            .expect_no_offenses(indoc! {r#"
                expect { run }.to change { Foo.bar }
            "#});
    }
}

murphy_plugin_api::submit_cop!(ExpectChange);
