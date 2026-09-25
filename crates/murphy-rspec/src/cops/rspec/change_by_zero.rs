//! `RSpec/ChangeByZero` — prefer negated matchers over `to change.by(0)`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/ChangeByZero
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `expect_change_with_arguments`
//!   (`(send $(send nil? CHANGE_METHODS ...) :by (int 0))`) and
//!   `expect_change_with_block`
//!   (`(send (block $(send nil? CHANGE_METHODS) (args) (send (...) _))
//!   :by (int 0))`) with `CHANGE_METHODS = [:change, :a_block_changing,
//!   :changing]`, each gated by `register_offense`'s
//!   `node.parent.send_type?` (the `.by(0)` must sit inside another
//!   `Send`, e.g. `to` / `and`). Simple expectations flag the `.by(0)`
//!   node per `add_offense(node)` with `MSG`; compound chains (parent is
//!   `and` / `or` / `&` / `|`) use `MSG_COMPOUND` with the `NegatedMatcher`
//!   option (default unset → `negated matchers`). Detection is at parity;
//!   autocorrect (rewriting to `not_to change` / the negated matcher) is
//!   not ported in this batch — same convention as `RSpec/HookArgument`
//!   (status: partial, autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with the `change` family
//! (`change`, `a_block_changing`, `changing`):
//!
//! - `expect { r }.to change(Foo, :bar).by(0)` — flagged (the
//!   `change(Foo, :bar).by(0)` span).
//! - `expect { r }.to change { Foo.bar }.by(0)` — block form, flagged.
//! - `expect { r }.to change(Foo, :bar).by(1)` — non-zero, not flagged.
//! - `expect { r }.to eq(1)` — no change, not flagged.
//! - Lone `change(Foo, :bar).by(0)` outside an expectation — the `.by(0)`
//!   has no `Send` parent, not flagged.
//! - `expect { r }.to change(Foo, :a).by(0).and change(Foo, :b).by(0)` —
//!   compound message.
//!
//! ## No autocorrect
//!
//! Upstream rewrites to `not_to change` (or the configured negated
//! matcher for compound chains). This batch reports only.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ChangeByZero;

#[derive(CopOptions)]
pub struct ChangeByZeroOptions {
    #[option(
        name = "NegatedMatcher",
        description = "Negated matcher used for compound `change.by(0)` expectations (unset reports generic `negated matchers`)."
    )]
    pub negated_matcher: Option<String>,
}

#[cop(
    name = "RSpec/ChangeByZero",
    description = "Prefer negated matchers over `to change.by(0)`.",
    default_severity = "warning",
    default_enabled = true,
    options = ChangeByZeroOptions,
)]
impl ChangeByZero {
    #[on_node(kind = "send", methods = ["change", "a_block_changing", "changing"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send {
            receiver, method, ..
        } = *cx.kind(node)
        else {
            return;
        };
        if receiver != OptNodeId::NONE {
            return;
        }
        let change_method = cx.symbol_str(method).to_owned();
        // Arm 1: `change(...).by(0)` — the parent is the `.by(0)` send.
        if let Some(parent) = cx.parent(node).get()
            && is_by_zero(cx, parent, node)
            && let Some(by_parent) = cx.parent(parent).get()
            && matches!(*cx.kind(by_parent), NodeKind::Send { .. })
        {
            self.emit(cx, parent, by_parent, &change_method);
            return;
        }
        // Arm 2: `change { ... }.by(0)` — the change send (no args) is the
        // call of a no-arg block whose body is a `Send`, and `.by(0)` is
        // called on that block.
        let Some(block) = cx.block_node(node).get() else {
            return;
        };
        let NodeKind::Block { call, args, body } = *cx.kind(block) else {
            return;
        };
        if call != node {
            return;
        }
        let NodeKind::Args(arg_list) = *cx.kind(args) else {
            return;
        };
        if !cx.list(arg_list).is_empty() {
            return;
        }
        let NodeKind::Send { args: change_args, .. } = *cx.kind(node) else {
            return;
        };
        if !cx.list(change_args).is_empty() {
            return;
        }
        let Some(body_id) = body.get() else {
            return;
        };
        if !matches!(*cx.kind(body_id), NodeKind::Send { .. }) {
            return;
        }
        // Empty-args blocks (`change {}`) have no body node; the `body`
        // gate above already excludes them.
        let Some(by) = cx.parent(block).get() else {
            return;
        };
        if !is_by_zero(cx, by, block) {
            return;
        }
        let Some(by_parent) = cx.parent(by).get() else {
            return;
        };
        if !matches!(*cx.kind(by_parent), NodeKind::Send { .. }) {
            return;
        }
        self.emit(cx, by, by_parent, &change_method);
    }
}

impl ChangeByZero {
    fn emit(&self, cx: &Cx<'_>, by: NodeId, by_parent: NodeId, change_method: &str) {
        let NodeKind::Send { method, .. } = *cx.kind(by_parent) else {
            return;
        };
        let parent_method = cx.symbol_str(method);
        let message = if matches!(parent_method, "and" | "or" | "&" | "|") {
            let opts = cx.options_or_default::<ChangeByZeroOptions>();
            let preferred = match &opts.negated_matcher {
                Some(name) => format!("`{name}`"),
                None => "negated matchers".to_owned(),
            };
            format!(
                "Prefer {preferred} with compound expectations over `{change_method}.by(0)`."
            )
        } else {
            format!("Prefer `not_to change` over `to {change_method}.by(0)`.")
        };
        cx.emit_offense(cx.range(by), &message, None);
    }
}

/// `true` when `by` is `.by(0)` called on `target`.
///
/// Mirrors the outer `(send ... :by (int 0))` of both upstream patterns.
fn is_by_zero(cx: &Cx<'_>, by: NodeId, target: NodeId) -> bool {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(by)
    else {
        return false;
    };
    if cx.symbol_str(method) != "by" {
        return false;
    }
    if receiver.get() != Some(target) {
        return false;
    }
    let arg_ids = cx.list(args);
    if arg_ids.len() != 1 {
        return false;
    }
    matches!(*cx.kind(arg_ids[0]), NodeKind::Int(0))
}

#[cfg(test)]
mod tests {
    use super::{ChangeByZero, ChangeByZeroOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    fn negated_matcher() -> ChangeByZeroOptions {
        ChangeByZeroOptions {
            negated_matcher: Some("not_change".to_owned()),
        }
    }

    #[test]
    fn flags_change_by_zero() {
        test::<ChangeByZero>().expect_offense(indoc! {r#"
                expect { run }.to change(Foo, :bar).by(0)
                                  ^^^^^^^^^^^^^^^^^^^^^^^ Prefer `not_to change` over `to change.by(0)`.
            "#});
    }

    #[test]
    fn flags_block_change_by_zero() {
        test::<ChangeByZero>().expect_offense(indoc! {r#"
                expect { run }.to change { Foo.bar }.by(0)
                                  ^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `not_to change` over `to change.by(0)`.
            "#});
    }

    #[test]
    fn flags_changing_by_zero() {
        test::<ChangeByZero>().expect_offense(indoc! {r#"
                expect { run }.to changing { Foo.bar }.by(0)
                                  ^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `not_to change` over `to changing.by(0)`.
            "#});
    }

    #[test]
    fn does_not_flag_by_nonzero() {
        test::<ChangeByZero>().expect_no_offenses(indoc! {r#"
                expect { run }.to change(Foo, :bar).by(1)
            "#});
    }

    #[test]
    fn does_not_flag_lone_by_zero() {
        // No enclosing `Send` parent → `register_offense` bails.
        test::<ChangeByZero>().expect_no_offenses(indoc! {r#"
                change(Foo, :bar).by(0)
            "#});
    }

    #[test]
    fn flags_compound_with_generic_message() {
        test::<ChangeByZero>().expect_offense(indoc! {r#"
                expect { run }.to change(Foo, :a).by(0).and change(Foo, :b).by(0)
                                  ^^^^^^^^^^^^^^^^^^^^^ Prefer negated matchers with compound expectations over `change.by(0)`.
                                                            ^^^^^^^^^^^^^^^^^^^^^ Prefer negated matchers with compound expectations over `change.by(0)`.
            "#});
    }

    #[test]
    fn flags_compound_with_negated_matcher() {
        test::<ChangeByZero>()
            .with_options(&negated_matcher())
            .expect_offense(indoc! {r#"
                expect { run }.to change(Foo, :a).by(0).and change(Foo, :b).by(0)
                                  ^^^^^^^^^^^^^^^^^^^^^ Prefer `not_change` with compound expectations over `change.by(0)`.
                                                            ^^^^^^^^^^^^^^^^^^^^^ Prefer `not_change` with compound expectations over `change.by(0)`.
            "#});
    }

    #[test]
    fn does_not_flag_receiver_change() {
        // Upstream requires a bare (`nil?`) `change` receiver.
        test::<ChangeByZero>().expect_no_offenses(indoc! {r#"
                expect { run }.to obj.change(Foo, :bar).by(0)
            "#});
    }
}

murphy_plugin_api::submit_cop!(ChangeByZero);
