//! `RSpec/PendingWithoutReason` — pending or skipped examples need a reason.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/PendingWithoutReason
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_send`: `metadata_without_reason?`
//!   (`(send #rspec? {#ExampleGroups.all #Examples.all} ... {<(sym
//!   {:pending :skip}) ...> (hash <(pair (sym {:pending :skip}) true)
//!   ...>)})`) flags `it "x", :pending` / `pending: true` with
//!   `Give the reason for pending/skip.`; `skipped_by_example_method?` /
//!   `skipped_by_example_method_with_block?` flag bare `pending` / `skip` /
//!   `xit` / ... as examples at group level; `skipped_by_example_group_method?`
//!   flags `xdescribe` / `xcontext` / `xfeature` with the fixed
//!   `Give the reason for skip.`; `skipped_in_example?` flags bare
//!   zero-arg `pending` / `skip` inside examples.
//!   `parent_node` (skip `Begin` to the enclosing `Block`) gates bare calls
//!   so `pending` as an argument, inside `if`, or inside `FactoryBot.define`
//!   stays clean, and calls with a reason arg stay clean.
//!   Verified vs 3.7.0, including metadata-string, `pending: false`,
//!   receiver, conditional, and shared-example cases.
//!   No autocorrect upstream, none here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send`:
//!
//! - `it "x", :pending do end` — flagged.
//! - `it "x", pending: true do end` — flagged.
//! - `it "x", pending: "reason" do end` — reason, clean.
//! - `pending` (bare, inside example/group) — flagged.
//! - `pending "reason"` (inside example, no block) — reason, clean.
//! - `pending "x" do end` (at group level, with block) — flagged.
//! - `xit "x" do end` — flagged.
//! - `xdescribe "x" do end` — flagged (`Give the reason for skip.`).
//! - `Foo.pending` — receiver, clean.
//! - `pending if cond` — conditional, clean.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; writing the reason needs human judgement.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::{is_rspec_or_bare_receiver, send_without_block_range};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct PendingWithoutReason;

#[cop(
    name = "RSpec/PendingWithoutReason",
    description = "Checks for pending or skipped examples without reason.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl PendingWithoutReason {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send {
            receiver,
            method,
            args,
        } = *cx.kind(node)
        else {
            return;
        };
        let name = cx.symbol_str(method).to_owned();
        let arg_ids = cx.list(args);

        // `xdescribe` / `xcontext` / `xfeature` — always needs a reason.
        if matches!(name.as_str(), "xdescribe" | "xcontext" | "xfeature") {
            if is_rspec_or_bare_receiver(cx, receiver) {
                cx.emit_offense(
                    send_without_block_range(cx, node),
                    "Give the reason for skip.",
                    None,
                );
            }
            return;
        }

        // `xit` / `xspecify` / ... — skipped examples always need a reason.
        if matches!(
            name.as_str(),
            "xit" | "xspecify" | "xexample" | "xscenario"
        ) && receiver == OptNodeId::NONE
        {
            cx.emit_offense(
                send_without_block_range(cx, node),
                &format!("Give the reason for {name}."),
                None,
            );
            return;
        }

        // Regular examples / groups with `:pending` / `:skip` metadata or
        // `pending: true` / `skip: true` (reason strings stay clean).
        if is_example_or_group_name(&name)
            && is_rspec_or_bare_receiver(cx, receiver)
            && let Some(reason) = metadata_without_reason(cx, arg_ids)
        {
            cx.emit_offense(
                send_without_block_range(cx, node),
                &format!("Give the reason for {reason}."),
                None,
            );
            return;
        }

        // Bare `pending` / `skip` handling.
        if !matches!(name.as_str(), "pending" | "skip") {
            return;
        }
        if receiver != OptNodeId::NONE {
            return;
        }
        let has_block = cx.block_node(node).get().is_some();
        if has_block {
            // `pending "x" do end` as an example at group level flags;
            // the same shape inside an example (`skip "x" do end` nested)
            // stays clean per upstream.
            if let Some(parent) = effective_parent(cx, node)
                && is_group_block(cx, parent)
            {
                cx.emit_offense(
                    send_without_block_range(cx, node),
                    &format!("Give the reason for {name}."),
                    None,
                );
            }
            return;
        }
        if !arg_ids.is_empty() {
            // `pending "reason"` / `skip "reason"` — reason provided.
            return;
        }
        // Bare `pending` / `skip` — flags only as a statement inside an
        // example or group (parent_node skips `Begin`).
        if let Some(parent) = effective_parent(cx, node)
            && (is_group_block(cx, parent) || is_example_block(cx, parent))
        {
            cx.emit_offense(cx.range(node), &format!("Give the reason for {name}."), None);
        }
    }
}

/// Upstream `parent_node`: the block wrapping `node` (if any), else `node`,
/// then its parent, skipping a `Begin` to the enclosing statement scope.
fn effective_parent(cx: &Cx<'_>, node: NodeId) -> Option<NodeId> {
    let base = cx.block_node(node).get().unwrap_or(node);
    let parent = cx.parent(base).get()?;
    match *cx.kind(parent) {
        NodeKind::Begin(_) => cx.parent(parent).get(),
        _ => Some(parent),
    }
}

fn is_group_block(cx: &Cx<'_>, id: NodeId) -> bool {
    let NodeKind::Block { call, .. } = *cx.kind(id) else {
        return false;
    };
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(call)
    else {
        return false;
    };
    if !is_rspec_or_bare_receiver(cx, receiver) {
        // Shared groups use bare receivers too.
        if receiver != OptNodeId::NONE {
            return false;
        }
    }
    let name = cx.symbol_str(method);
    matches!(
        name,
        "describe"
            | "context"
            | "feature"
            | "example_group"
            | "xdescribe"
            | "xcontext"
            | "xfeature"
            | "fdescribe"
            | "fcontext"
            | "ffeature"
            | "shared_examples"
            | "shared_examples_for"
            | "shared_context"
    )
}

fn is_example_block(cx: &Cx<'_>, id: NodeId) -> bool {
    let NodeKind::Block { call, .. } = *cx.kind(id) else {
        return false;
    };
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(call)
    else {
        return false;
    };
    if receiver != OptNodeId::NONE {
        return false;
    }
    let name = cx.symbol_str(method);
    matches!(
        name,
        "it" | "specify"
            | "example"
            | "scenario"
            | "its"
            | "fit"
            | "fspecify"
            | "fexample"
            | "fscenario"
            | "focus"
            | "xit"
            | "xspecify"
            | "xexample"
            | "xscenario"
            | "skip"
            | "pending"
    )
}

fn is_example_or_group_name(name: &str) -> bool {
    matches!(
        name,
        "describe"
            | "context"
            | "feature"
            | "example_group"
            | "xdescribe"
            | "xcontext"
            | "xfeature"
            | "fdescribe"
            | "fcontext"
            | "ffeature"
            | "it"
            | "specify"
            | "example"
            | "scenario"
            | "its"
            | "fit"
            | "fspecify"
            | "fexample"
            | "fscenario"
            | "focus"
            | "xit"
            | "xspecify"
            | "xexample"
            | "xscenario"
            | "skip"
            | "pending"
    )
}

/// The pending/skip key when metadata carries no reason, else `None`.
///
/// Mirrors `metadata_without_reason?`: a bare `:pending` / `:skip` sym, or
/// a `pending: true` / `skip: true` pair. String values (`pending:
/// "reason"`) and `false` stay clean.
fn metadata_without_reason(cx: &Cx<'_>, args: &[NodeId]) -> Option<String> {
    for &arg in args {
        match *cx.kind(arg) {
            NodeKind::Sym(sym) => {
                let s = cx.symbol_str(sym);
                if matches!(s, "pending" | "skip") {
                    return Some(s.to_owned());
                }
            }
            NodeKind::Hash(pairs) => {
                for pair_id in cx.list(pairs).iter().copied() {
                    let NodeKind::Pair { key, value } = *cx.kind(pair_id) else {
                        continue;
                    };
                    let is_key = matches!(*cx.kind(key), NodeKind::Sym(s) if matches!(cx.symbol_str(s), "pending" | "skip"));
                    if !is_key {
                        continue;
                    }
                    if matches!(*cx.kind(value), NodeKind::True_) {
                        let NodeKind::Sym(s) = *cx.kind(key) else {
                            continue;
                        };
                        return Some(cx.symbol_str(s).to_owned());
                    }
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::PendingWithoutReason;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_metadata_sym() {
        test::<PendingWithoutReason>().expect_offense(indoc! {r#"
                RSpec.describe Foo do
                  it "does something", :pending do
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Give the reason for pending.
                  end
                end
            "#});
    }

    #[test]
    fn flags_metadata_true() {
        test::<PendingWithoutReason>().expect_offense(indoc! {r#"
                describe "something", pending: true do
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Give the reason for pending.
                end
            "#});
    }

    #[test]
    fn allows_metadata_reason() {
        test::<PendingWithoutReason>().expect_no_offenses(indoc! {r#"
                RSpec.describe Foo do
                  it "does something", pending: "reason" do
                  end
                end
            "#});
    }

    #[test]
    fn flags_bare_pending_in_example() {
        test::<PendingWithoutReason>().expect_offense(indoc! {r#"
                RSpec.describe Foo do
                  it "does something" do
                    pending
                    ^^^^^^^ Give the reason for pending.
                  end
                end
            "#});
    }

    #[test]
    fn allows_pending_with_reason_in_example() {
        test::<PendingWithoutReason>().expect_no_offenses(indoc! {r#"
                RSpec.describe Foo do
                  it "does something" do
                    pending "reason"
                  end
                end
            "#});
    }

    #[test]
    fn flags_pending_example_with_block() {
        test::<PendingWithoutReason>().expect_offense(indoc! {r#"
                RSpec.describe Foo do
                  pending "does something" do
                  ^^^^^^^^^^^^^^^^^^^^^^^^ Give the reason for pending.
                  end
                end
            "#});
    }

    #[test]
    fn flags_xit() {
        test::<PendingWithoutReason>().expect_offense(indoc! {r#"
                RSpec.describe Foo do
                  xit "something" do
                  ^^^^^^^^^^^^^^^ Give the reason for xit.
                  end
                end
            "#});
    }

    #[test]
    fn flags_xdescribe() {
        test::<PendingWithoutReason>().expect_offense(indoc! {r#"
                xdescribe "something" do
                ^^^^^^^^^^^^^^^^^^^^^ Give the reason for skip.
                end
            "#});
    }

    #[test]
    fn ignores_receiver() {
        test::<PendingWithoutReason>().expect_no_offenses(indoc! {r#"
                RSpec.describe Foo do
                  it "does something" do
                    Foo.pending
                  end
                end
            "#});
    }

    #[test]
    fn ignores_conditional() {
        test::<PendingWithoutReason>().expect_no_offenses(indoc! {r#"
                RSpec.describe Foo do
                  it "does something" do
                    pending if cond
                  end
                end
            "#});
    }

    #[test]
    fn ignores_factory_bot() {
        test::<PendingWithoutReason>().expect_no_offenses(indoc! {r#"
                FactoryBot.define do
                  factory :task do
                    pending
                  end
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(PendingWithoutReason);
