//! `RSpec/Pending` — no pending or skipped examples.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/Pending
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_send`: `pending_block?` (skipped groups with an
//!   RSpec-or-bare receiver, or bare skipped/pending examples) flags the
//!   send (trimmed to exclude a wrapping block via
//!   `send_without_block_range`), and `skipped?` flags regular groups
//!   (any args) and regular examples (at least one arg) carrying
//!   `skipped_in_metadata?` (`:skip` / `:pending` sym arg or a
//!   `skip:` / `pending:` pair with a `true` / `Str` / `Dstr` value),
//!   plus body-less regular examples (`it "x"` without `do...end`).
//!   Bare `pending` / `skip` calls inside examples flag via
//!   `pending_block?` exactly like upstream (verified vs 3.7.0,
//!   including `RSpec.xdescribe`, metadata-string, and `pending: false`
//!   clean cases). `default_enabled = false` mirrors upstream
//!   (`Enabled: false`). No autocorrect upstream, none here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send`:
//!
//! - `xit "x" do end` — skipped example, flagged.
//! - `xdescribe "x" do end` / `RSpec.xdescribe` — skipped group, flagged.
//! - `pending "x" do end` / `skip "x" do end` — pending, flagged.
//! - `it "x", skip: true do end` — metadata, flagged.
//! - `describe Foo, :skip do end` — group metadata, flagged.
//! - `it "x"` without a body — flagged.
//! - `it "x" do end` — clean.
//! - `it "x", pending: false do end` — falsy metadata, clean.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; un-pending a spec needs human
//! judgement about whether the work is done.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::{is_rspec_or_bare_receiver, send_without_block_range};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct Pending;

#[cop(
    name = "RSpec/Pending",
    description = "Checks for any pending or skipped examples.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions
)]
impl Pending {
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
        if is_pending_block(cx, receiver, &name) || is_skipped(cx, node, receiver, &name) {
            let _ = args;
            cx.emit_offense(
                send_without_block_range(cx, node),
                "Pending spec found.",
                None,
            );
        }
    }
}

/// Upstream `pending_block?`: skipped groups (RSpec-or-bare receiver) or
/// bare skipped/pending examples.
fn is_pending_block(cx: &Cx<'_>, receiver: OptNodeId, name: &str) -> bool {
    if is_skipped_group_name(name) {
        return is_rspec_or_bare_receiver(cx, receiver);
    }
    if is_skipped_or_pending_example_name(name) {
        return receiver == OptNodeId::NONE;
    }
    false
}

/// Upstream `skipped?`: a skippable group/example with skip metadata,
/// or a body-less regular example.
fn is_skipped(cx: &Cx<'_>, send: NodeId, receiver: OptNodeId, name: &str) -> bool {
    if is_skippable(cx, receiver, name, send) && has_skip_metadata(cx, send) {
        return true;
    }
    is_bodyless_regular_example(cx, send, receiver, name)
}

/// Upstream `skippable?`: regular groups (any call shape) or regular
/// examples with at least one argument.
fn is_skippable(cx: &Cx<'_>, receiver: OptNodeId, name: &str, send: NodeId) -> bool {
    if is_regular_group_name(name) {
        return is_rspec_or_bare_receiver(cx, receiver);
    }
    if is_regular_example_name(name) && receiver == OptNodeId::NONE {
        let NodeKind::Send { args, .. } = *cx.kind(send) else {
            return false;
        };
        return !cx.list(args).is_empty();
    }
    false
}

/// Upstream `skipped_regular_example_without_body?`: a regular example
/// with at least one argument and no wrapping block.
fn is_bodyless_regular_example(
    cx: &Cx<'_>,
    send: NodeId,
    receiver: OptNodeId,
    name: &str,
) -> bool {
    if !is_regular_example_name(name) || receiver != OptNodeId::NONE {
        return false;
    }
    let NodeKind::Send { args, .. } = *cx.kind(send) else {
        return false;
    };
    if cx.list(args).is_empty() {
        return false;
    }
    cx.block_node(send).get().is_none()
}

/// Upstream `skipped_in_metadata?`: a bare `:skip` / `:pending` sym arg,
/// or a `skip:` / `pending:` pair whose value is `true` / `Str` / `Dstr`.
fn has_skip_metadata(cx: &Cx<'_>, send: NodeId) -> bool {
    let NodeKind::Send { args, .. } = *cx.kind(send) else {
        return false;
    };
    for arg in cx.list(args).iter().copied() {
        match *cx.kind(arg) {
            NodeKind::Sym(sym)
                if matches!(cx.symbol_str(sym), "skip" | "pending") =>
            {
                return true;
            }
            NodeKind::Hash(pairs) => {
                for pair_id in cx.list(pairs).iter().copied() {
                    let NodeKind::Pair { key, value } = *cx.kind(pair_id) else {
                        continue;
                    };
                    let is_skip_key = matches!(*cx.kind(key), NodeKind::Sym(s) if matches!(cx.symbol_str(s), "skip" | "pending"));
                    if !is_skip_key {
                        continue;
                    }
                    if matches!(*cx.kind(value), NodeKind::True_)
                        || matches!(*cx.kind(value), NodeKind::Str(_))
                        || matches!(*cx.kind(value), NodeKind::Dstr(_))
                    {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

fn is_regular_group_name(name: &str) -> bool {
    matches!(
        name,
        "describe" | "context" | "feature" | "example_group"
    )
}

fn is_skipped_group_name(name: &str) -> bool {
    matches!(name, "xdescribe" | "xcontext" | "xfeature")
}

fn is_regular_example_name(name: &str) -> bool {
    matches!(
        name,
        "it" | "specify" | "example" | "scenario" | "its"
    )
}

fn is_skipped_or_pending_example_name(name: &str) -> bool {
    matches!(
        name,
        "xit" | "xspecify" | "xexample" | "xscenario" | "skip" | "pending"
    )
}

#[cfg(test)]
mod tests {
    use super::Pending;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_bodiless_it() {
        test::<Pending>().expect_offense(indoc! {r#"
                describe MyClass do
                  it "should be true"
                  ^^^^^^^^^^^^^^^^^^^ Pending spec found.
                end
            "#});
    }

    #[test]
    fn flags_xit_block() {
        test::<Pending>().expect_offense(indoc! {r#"
                describe Foo do
                  xit 'x' do; end
                  ^^^^^^^ Pending spec found.
                end
            "#});
    }

    #[test]
    fn flags_pending_example() {
        test::<Pending>().expect_offense(indoc! {r#"
                describe Foo do
                  pending 'x' do; end
                  ^^^^^^^^^^^ Pending spec found.
                end
            "#});
    }

    #[test]
    fn flags_skip_example() {
        test::<Pending>().expect_offense(indoc! {r#"
                describe Foo do
                  skip 'x' do; end
                  ^^^^^^^^ Pending spec found.
                end
            "#});
    }

    #[test]
    fn flags_xdescribe() {
        test::<Pending>().expect_offense(indoc! {r#"
                xdescribe 'x' do; end
                ^^^^^^^^^^^^^ Pending spec found.
            "#});
    }

    #[test]
    fn flags_explicit_rspec_xdescribe() {
        test::<Pending>().expect_offense(indoc! {r#"
                RSpec.xdescribe 'x' do; end
                ^^^^^^^^^^^^^^^^^^^ Pending spec found.
            "#});
    }

    #[test]
    fn flags_metadata_skip_true() {
        test::<Pending>().expect_offense(indoc! {r#"
                describe Foo do
                  it 'x', skip: true do; end
                  ^^^^^^^^^^^^^^^^^^ Pending spec found.
                end
            "#});
    }

    #[test]
    fn flags_group_metadata_skip_sym() {
        test::<Pending>().expect_offense(indoc! {r#"
                describe Foo, :skip do; end
                ^^^^^^^^^^^^^^^^^^^ Pending spec found.
            "#});
    }

    #[test]
    fn flags_pending_call_inside_example() {
        // Bare `pending` inside an example matches `pending_block?`
        // (verified vs 3.7.0).
        test::<Pending>().expect_offense(indoc! {r#"
                describe Foo do
                  it 'x' do
                    pending
                    ^^^^^^^ Pending spec found.
                  end
                end
            "#});
    }

    #[test]
    fn ignores_plain_it() {
        test::<Pending>().expect_no_offenses(indoc! {r#"
                describe Foo do
                  it 'x' do; end
                end
            "#});
    }

    #[test]
    fn ignores_pending_false_metadata() {
        // Only `true` / `Str` / `Dstr` values count (verified vs 3.7.0).
        test::<Pending>().expect_no_offenses(indoc! {r#"
                describe Foo do
                  it 'x', pending: false do; end
                end
            "#});
    }

    #[test]
    fn ignores_non_rspec_receiver() {
        test::<Pending>().expect_no_offenses(indoc! {r#"
                obj.xit 'x' do; end
            "#});
    }
}

murphy_plugin_api::submit_cop!(Pending);
