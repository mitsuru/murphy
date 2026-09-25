//! `RSpec/SkipBlockInsideExample` — do not pass a block to `skip` inside examples.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/SkipBlockInsideExample
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_block` (plus the `on_numblock` alias) gated by
//!   `node.method?(:skip)` and `inside_example?` (any ancestor `Block`
//!   that is an `example?`: a bare `Examples.all` call). The offense is
//!   the whole block per `add_offense(node)` with `Don't pass a block
//!   to `skip` inside examples.`. Detection is at parity (verified vs
//!   3.7.0, including the numblock and `pending`-clean cases);
//!   autocorrect (none upstream) needs nothing — same convention as
//!   `RSpec/ReturnFromStub` (status: partial, no autocorrect upstream).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` and `Numblock` whose call names `skip`:
//!
//! - `it { skip 'x' do end }` — flagged (whole block).
//! - `it { skip 'x' do _1 end }` (numblock) — flagged.
//! - `it { skip 'x' }` — no block, clean (`Send`, not dispatched).
//! - `skip 'x' do end` at top level — outside examples, clean.
//! - `it { pending 'x' do end }` — different method, clean.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; unwrapping the block needs human
//! judgement.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

use crate::cops::rspec_helpers::is_example_name;

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SkipBlockInsideExample;

#[cop(
    name = "RSpec/SkipBlockInsideExample",
    description = "Checks for passing a block to `skip` within examples.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl SkipBlockInsideExample {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, .. } = *cx.kind(node) else {
            return;
        };
        if cx.method_name(call) != Some("skip") {
            return;
        }
        if !inside_example(cx, node) {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            "Don't pass a block to `skip` inside examples.",
            None,
        );
    }

    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Numblock { send, .. } = *cx.kind(node) else {
            return;
        };
        if cx.method_name(send) != Some("skip") {
            return;
        }
        if !inside_example(cx, node) {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            "Don't pass a block to `skip` inside examples.",
            None,
        );
    }
}

/// `true` when any ancestor block is a bare example.
///
/// Mirrors upstream `inside_example?`
/// (`node.each_ancestor(:block).any? { example? }`) where `example?`
/// is `(block (send nil? #Examples.all ...) ...)`. Ancestor blocks of
/// any kind (`Block` / `Numblock`) count here so numbered-parameter
/// examples (`it { _1 }`) also gate the cop.
fn inside_example(cx: &Cx<'_>, node: NodeId) -> bool {
    for ancestor in cx.ancestors(node) {
        let call = match *cx.kind(ancestor) {
            NodeKind::Block { call, .. } => call,
            NodeKind::Numblock { send, .. } => send,
            _ => continue,
        };
        let NodeKind::Send { receiver, method, .. } = *cx.kind(call) else {
            continue;
        };
        if receiver != murphy_plugin_api::OptNodeId::NONE {
            continue;
        }
        if is_example_name(cx.symbol_str(method)) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::SkipBlockInsideExample;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_skip_block_inside_example() {
        test::<SkipBlockInsideExample>().expect_offense(indoc! {r#"
                it 'does something' do
                  skip 'not yet implemented' do end
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Don't pass a block to `skip` inside examples.
                end
            "#});
    }

    #[test]
    fn flags_skip_numblock_inside_example() {
        test::<SkipBlockInsideExample>().expect_offense(indoc! {r#"
                it 'does something' do
                  skip 'not yet implemented' do _1 end
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Don't pass a block to `skip` inside examples.
                end
            "#});
    }

    #[test]
    fn flags_multiline_skip_block() {
        // Multiline whole-block ranges cannot use the caret grammar;
        // assert via `run_cop`.
        let offenses = murphy_plugin_api::test_support::run_cop::<SkipBlockInsideExample>(indoc! {r#"
                it 'does something' do
                  skip 'not yet implemented' do
                    do_something
                  end
                end
            "#});
        assert_eq!(offenses.len(), 1);
        assert_eq!(
            offenses[0].message,
            "Don't pass a block to `skip` inside examples."
        );
    }

    #[test]
    fn ignores_skip_without_block() {
        test::<SkipBlockInsideExample>().expect_no_offenses(indoc! {r#"
                it 'does something' do
                  skip 'not yet implemented'
                end
            "#});
    }

    #[test]
    fn ignores_skip_block_outside_examples() {
        test::<SkipBlockInsideExample>().expect_no_offenses(indoc! {r#"
                skip 'not yet implemented' do
                end
            "#});
    }

    #[test]
    fn ignores_pending_block_inside_example() {
        test::<SkipBlockInsideExample>().expect_no_offenses(indoc! {r#"
                it 'does something' do
                  pending 'not yet implemented' do
                  end
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(SkipBlockInsideExample);
