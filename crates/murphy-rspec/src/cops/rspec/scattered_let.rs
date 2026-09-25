//! `RSpec/ScatteredLet` — keep `let` definitions together.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/ScatteredLet
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_block` (`example_group_with_body?`): the
//!   direct children of the group body that are `let?` (bare `let` /
//!   `let!` blocks via the shared `is_let_node` helper, including the
//!   bare-send `block_pass` form) must sit on consecutive sibling
//!   indexes from the first `let`. Any later `let` past a non-`let`
//!   sibling flags the whole node per `add_offense(node)` with `Group
//!   all let/let! blocks in the example group together.`. Detection
//!   is at parity (verified vs 3.7.0, including `let!` and
//!   `subject`-between-lets cases); autocorrect (move the node after
//!   the first `let`) is not ported in this batch — same convention
//!   as `RSpec/HookArgument` (status: partial, autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` example groups:
//!
//! - `let` / `subject` / `let` — the second `let` flagged.
//! - `let` / `let` / `let` — contiguous, clean.
//! - `let` / `before` / `let!` — the `let!` flagged.
//! - Single `let` — clean.
//!
//! ## No autocorrect
//!
//! Upstream moves the scattered node after the first `let`. This
//! batch reports only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

use crate::cops::rspec_helpers::{is_example_group_call, is_let_node};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ScatteredLet;

#[cop(
    name = "RSpec/ScatteredLet",
    description = "Checks for let scattered across the example group.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl ScatteredLet {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, body, .. } = *cx.kind(node) else {
            return;
        };
        if !is_example_group_call(cx, call) {
            return;
        }
        let Some(body_id) = body.get() else {
            return;
        };
        // Direct children of the group body, mirroring
        // `body.each_child_node`. A lone (non-`Begin`) body is its own
        // single child.
        let children: Vec<NodeId> = match *cx.kind(body_id) {
            NodeKind::Begin(members) => cx.list(members).to_vec(),
            _ => vec![body_id],
        };
        let lets: Vec<(usize, NodeId)> = children
            .iter()
            .enumerate()
            .filter(|(_, id)| is_let_node(cx, **id))
            .map(|(idx, &id)| (idx, id))
            .collect();
        let Some(&(first_idx, _)) = lets.first() else {
            return;
        };
        for (pos, &(_, id)) in lets.iter().enumerate() {
            // `node.sibling_index == first_let.sibling_index + idx`.
            if children[first_idx + pos] != id {
                cx.emit_offense(
                    cx.range(id),
                    "Group all let/let! blocks in the example group together.",
                    None,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ScatteredLet;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_let_after_subject() {
        test::<ScatteredLet>().expect_offense(indoc! {r#"
                describe Foo do
                  let(:foo) { 1 }
                  subject { Foo }
                  let(:bar) { 2 }
                  ^^^^^^^^^^^^^^^ Group all let/let! blocks in the example group together.
                end
            "#});
    }

    #[test]
    fn flags_let_bang_after_hook() {
        test::<ScatteredLet>().expect_offense(indoc! {r#"
                describe Foo do
                  let(:foo) { 1 }
                  before { prepare }
                  let!(:baz) { 3 }
                  ^^^^^^^^^^^^^^^^ Group all let/let! blocks in the example group together.
                end
            "#});
    }

    #[test]
    fn ignores_grouped_lets() {
        test::<ScatteredLet>().expect_no_offenses(indoc! {r#"
                describe Foo do
                  subject { Foo }
                  before { prepare }
                  let(:foo) { 1 }
                  let(:bar) { 2 }
                  let!(:baz) { 3 }
                end
            "#});
    }

    #[test]
    fn ignores_single_let() {
        test::<ScatteredLet>().expect_no_offenses(indoc! {r#"
                describe Foo do
                  let(:foo) { 1 }
                end
            "#});
    }

    #[test]
    fn ignores_no_lets() {
        test::<ScatteredLet>().expect_no_offenses(indoc! {r#"
                describe Foo do
                  subject { Foo }
                  it { foo }
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(ScatteredLet);
