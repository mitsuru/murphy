//! `RSpec/VoidExpect` — flag `expect()` without `.to` / `.not_to`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/VoidExpect
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_send` (`expect?`: bare `expect`, gated by
//!   `inside_example?`) plus `on_block` (`expect_block?`: a `Block`
//!   whose call is a bare `expect`), each gated by `void?` (parent is
//!   a `Begin`, or parent is a `Block` whose body is the node itself).
//!   `inside_example?` walks `Block` ancestors for an example selector
//!   (`Examples.all`: regular + focused + skipped + pending, bare
//!   receiver only). The offense range is the whole node per
//!   `add_offense(node)`. No autocorrect upstream, none here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` (`methods = ["expect"]`) and on `Block` whose
//! call is a bare `expect`. Flags only inside examples and only when
//! void:
//!
//! - `it 'x' do expect(foo) end` — flagged (whole `expect(foo)`).
//! - `it 'x' do expect { foo } end` — flagged (whole block).
//! - `it 'x' do expect(foo).to eq(1) end` — chained, not flagged.
//! - `it 'x' do foo(expect(bar)) end` — argument position, not void,
//!   not flagged.
//! - `describe Foo do expect(foo) end` — not inside an example,
//!   not flagged.
//! - Bare `expect(foo)` at file top level — not inside an example,
//!   not flagged.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; chaining `.to` / `.not_to` (or
//! removing the call) needs human judgement.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

/// Example selectors (`Examples` in rubocop-rspec's default config):
/// regular (`it`, `specify`, `example`, `scenario`, `its`), focused
/// (`fit`, `fspecify`, `fexample`, `fscenario`, `focus`), skipped
/// (`xit`, `xspecify`, `xexample`, `xscenario`, `skip`) and pending
/// (`pending`).
fn is_example_name(name: &str) -> bool {
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

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct VoidExpect;

#[cop(
    name = "RSpec/VoidExpect",
    description = "Checks void `expect()`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl VoidExpect {
    #[on_node(kind = "send", methods = ["expect"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, .. } = *cx.kind(node) else {
            return;
        };
        if receiver != OptNodeId::NONE {
            return;
        }
        if !inside_example(cx, node) {
            return;
        }
        if !is_void(cx, node) {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            "Do not use `expect()` without `.to` or `.not_to`. Chain the methods or remove it.",
            None,
        );
    }

    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, .. } = *cx.kind(node) else {
            return;
        };
        let NodeKind::Send { receiver, method, .. } = *cx.kind(call) else {
            return;
        };
        if receiver != OptNodeId::NONE {
            return;
        }
        if cx.symbol_str(method) != "expect" {
            return;
        }
        if !inside_example(cx, node) {
            return;
        }
        if !is_void(cx, node) {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            "Do not use `expect()` without `.to` or `.not_to`. Chain the methods or remove it.",
            None,
        );
    }
}

/// `true` when some ancestor `Block` is an example.
///
/// Mirrors upstream `inside_example?`
/// (`node.each_ancestor(:block).any? { |a| example?(a) }`) where
/// `example?` is `(block (send nil? #Examples.all ...) ...)`: bare
/// receiver only, `Block` only (a numbered-parameter `Numblock` never
/// counts as the example itself).
fn inside_example(cx: &Cx<'_>, node: NodeId) -> bool {
    cx.ancestors(node).any(|ancestor| {
        let NodeKind::Block { call, .. } = *cx.kind(ancestor) else {
            return false;
        };
        let NodeKind::Send { receiver, method, .. } = *cx.kind(call) else {
            return false;
        };
        if receiver != OptNodeId::NONE {
            return false;
        }
        is_example_name(cx.symbol_str(method))
    })
}

/// `true` when the node is void: its parent is a `Begin` (one statement
/// among several), or its parent is a `Block` whose body is the node
/// itself (the sole statement).
///
/// Mirrors upstream `void?` (`parent.begin_type?`, or
/// `parent.block_type? && parent.body == expect`).
fn is_void(cx: &Cx<'_>, node: NodeId) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    match *cx.kind(parent) {
        NodeKind::Begin(_) => true,
        NodeKind::Block { body, .. } => body.get() == Some(node),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::VoidExpect;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_void_expect_in_example() {
        test::<VoidExpect>().expect_offense(indoc! {r#"
                it 'x' do
                  expect(foo)
                  ^^^^^^^^^^^ Do not use `expect()` without `.to` or `.not_to`. Chain the methods or remove it.
                end
            "#});
    }

    #[test]
    fn flags_void_expect_block_form() {
        test::<VoidExpect>().expect_offense(indoc! {r#"
                it 'x' do
                  expect { foo }
                  ^^^^^^^^^^^^^^ Do not use `expect()` without `.to` or `.not_to`. Chain the methods or remove it.
                end
            "#});
    }

    #[test]
    fn flags_first_of_two_statements() {
        // Multi-statement bodies wrap in `Begin`; the void member flags
        // while the chained member stays clean.
        test::<VoidExpect>().expect_offense(indoc! {r#"
                it 'x' do
                  expect(foo)
                  ^^^^^^^^^^^ Do not use `expect()` without `.to` or `.not_to`. Chain the methods or remove it.
                  expect(bar).to eq(1)
                end
            "#});
    }

    #[test]
    fn does_not_flag_chained_expect() {
        test::<VoidExpect>().expect_no_offenses(indoc! {r#"
                it 'x' do
                  expect(foo).to eq(1)
                end
            "#});
    }

    #[test]
    fn does_not_flag_expect_as_argument() {
        // Parent is the enclosing `Send`, not `Begin` / body-`Block`.
        test::<VoidExpect>().expect_no_offenses(indoc! {r#"
                it 'x' do
                  foo(expect(bar))
                end
            "#});
    }

    #[test]
    fn does_not_flag_outside_example() {
        test::<VoidExpect>().expect_no_offenses(indoc! {r#"
                describe Foo do
                  expect(foo)
                end
            "#});
    }

    #[test]
    fn does_not_flag_top_level_expect() {
        test::<VoidExpect>().expect_no_offenses(indoc! {r#"
                expect(foo)
            "#});
    }

    #[test]
    fn flags_void_expect_in_skipped_example() {
        test::<VoidExpect>().expect_offense(indoc! {r#"
                xit 'x' do
                  expect(foo)
                  ^^^^^^^^^^^ Do not use `expect()` without `.to` or `.not_to`. Chain the methods or remove it.
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(VoidExpect);
