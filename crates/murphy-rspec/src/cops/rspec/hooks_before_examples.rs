//! `RSpec/HooksBeforeExamples` — hooks must come before the examples in a group.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/HooksBeforeExamples
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_block` (plus the `on_numblock` alias) gated by
//!   `example_group_with_body?` (a group call with a bare or `RSpec`
//!   receiver and a body) and `multiline_block?` (the body is a `Begin`).
//!   The first body child matching `example_or_group?` (a group or
//!   example block, or a bare `Includes.examples` send:
//!   `it_behaves_like` / `it_should_behave_like` / `include_examples`)
//!   splits the body; every later sibling matching `hook?` (a bare
//!   `Hooks.all` block) is flagged whole per `add_offense(sibling)` with
//!   `Move `%<hook>s` above the examples in the group.` Detection is at
//!   parity; autocorrect (move the hook above the first example) is not
//!   ported in this batch — same convention as `RSpec/ExpectChange`
//!   (status: partial, autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` and `Numblock` group calls with a `Begin` body:
//!
//! - `describe { it {}; before {} }` — flagged (whole `before {}`).
//! - `describe { it {}; after {} }` / `around` — flagged the same way.
//! - `describe { before {}; it {} }` — hook first, not flagged.
//! - `describe { before {} }` — no example, not flagged.
//! - `describe { it {} }` — single statement (no `Begin`), not flagged.
//! - `describe { include_examples 'x'; before {} }` — shared examples
//!   count as the first example, `before` flagged.
//! - `describe { it {}; it {}; before {} }` — hook after the second
//!   example still flags (it follows the *first* example).
//!
//! ## No autocorrect
//!
//! Upstream moves the hook above the first example. This batch reports
//! only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::{is_example_group_call, is_hook_name};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct HooksBeforeExamples;

#[cop(
    name = "RSpec/HooksBeforeExamples",
    description = "Checks for before/around/after hooks that come after an example.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl HooksBeforeExamples {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, body, .. } = *cx.kind(node) else {
            return;
        };
        self.check_group(call, body, cx);
    }

    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Numblock { send, body, .. } = *cx.kind(node) else {
            return;
        };
        self.check_group(send, body, cx);
    }

    fn check_group(&self, call: NodeId, body: OptNodeId, cx: &Cx<'_>) {
        // Upstream `example_group_with_body?`: a group call with a body.
        if !is_example_group_call(cx, call) {
            return;
        }
        let Some(body_id) = body.get() else {
            return;
        };
        // Upstream `multiline_block?`: the body is a `Begin` (more than
        // one statement).
        let NodeKind::Begin(members) = *cx.kind(body_id) else {
            return;
        };
        let members = cx.list(members);
        let Some(first_idx) = members
            .iter()
            .position(|id| is_example_or_group(cx, *id))
        else {
            return;
        };
        for sibling in &members[first_idx + 1..] {
            let Some(hook_name) = hook_block_name(cx, *sibling) else {
                continue;
            };
            cx.emit_offense(
                cx.range(*sibling),
                &format!("Move `{hook_name}` above the examples in the group."),
                None,
            );
        }
    }
}

/// `true` when `call` is a bare example call (`Examples.all`: regular +
/// focused + skipped + pending).
fn is_bare_example_call(cx: &Cx<'_>, call: NodeId) -> bool {
    let NodeKind::Send { receiver, method, .. } = *cx.kind(call) else {
        return false;
    };
    if receiver != OptNodeId::NONE {
        return false;
    }
    matches!(
        cx.symbol_str(method),
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

/// `true` when `method` includes shared examples in the group
/// (`Includes.examples`).
fn is_include_example(method: &str) -> bool {
    matches!(
        method,
        "it_behaves_like" | "it_should_behave_like" | "include_examples"
    )
}

/// `true` when `id` is the group's first-example split point per
/// upstream `example_or_group?`: a group or example block, or a bare
/// shared-example inclusion send.
fn is_example_or_group(cx: &Cx<'_>, id: NodeId) -> bool {
    match *cx.kind(id) {
        NodeKind::Block { call, .. } => {
            is_example_group_call(cx, call) || is_bare_example_call(cx, call)
        }
        NodeKind::Numblock { send, .. } => {
            is_example_group_call(cx, send) || is_bare_example_call(cx, send)
        }
        NodeKind::Send { receiver, method, .. } => {
            receiver == OptNodeId::NONE && is_include_example(cx.symbol_str(method))
        }
        _ => false,
    }
}

/// The hook selector when `id` is a hook block (`hook?`: bare
/// `Hooks.all` call with any block wrapper), else `None`.
fn hook_block_name(cx: &Cx<'_>, id: NodeId) -> Option<String> {
    let call = match *cx.kind(id) {
        NodeKind::Block { call, .. } => call,
        NodeKind::Numblock { send, .. } => send,
        _ => return None,
    };
    let NodeKind::Send { receiver, method, .. } = *cx.kind(call) else {
        return None;
    };
    if receiver != OptNodeId::NONE {
        return None;
    }
    let name = cx.symbol_str(method);
    if !is_hook_name(name) {
        return None;
    }
    Some(name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::HooksBeforeExamples;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_before_after_example() {
        test::<HooksBeforeExamples>().expect_offense(indoc! {r#"
                describe 'x' do
                  it('y') { }
                  before { prepare }
                  ^^^^^^^^^^^^^^^^^^ Move `before` above the examples in the group.
                end
            "#});
    }

    #[test]
    fn flags_after_after_example() {
        test::<HooksBeforeExamples>().expect_offense(indoc! {r#"
                describe 'x' do
                  it('y') { }
                  after { clean_up }
                  ^^^^^^^^^^^^^^^^^^ Move `after` above the examples in the group.
                end
            "#});
    }

    #[test]
    fn flags_hook_after_second_example() {
        // The split point is the *first* example; hooks after any
        // later example still flag.
        test::<HooksBeforeExamples>().expect_offense(indoc! {r#"
                describe 'x' do
                  it('a') { }
                  it('b') { }
                  around { |ex| ex.run }
                  ^^^^^^^^^^^^^^^^^^^^^^ Move `around` above the examples in the group.
                end
            "#});
    }

    #[test]
    fn flags_hook_after_include_examples() {
        // Shared-example inclusions count as examples for ordering.
        test::<HooksBeforeExamples>().expect_offense(indoc! {r#"
                describe 'x' do
                  include_examples 'shared'
                  before { prepare }
                  ^^^^^^^^^^^^^^^^^^ Move `before` above the examples in the group.
                end
            "#});
    }

    #[test]
    fn does_not_flag_hook_before_example() {
        test::<HooksBeforeExamples>().expect_no_offenses(indoc! {r#"
                describe 'x' do
                  before { prepare }
                  after { clean_up }
                  it('y') { }
                end
            "#});
    }

    #[test]
    fn does_not_flag_hooks_without_examples() {
        // No first example, so there is nothing to be after.
        test::<HooksBeforeExamples>().expect_no_offenses(indoc! {r#"
                describe 'x' do
                  before { prepare }
                  after { clean_up }
                end
            "#});
    }

    #[test]
    fn does_not_flag_single_statement_body() {
        // A lone example is no `Begin`, so ordering cannot apply.
        test::<HooksBeforeExamples>().expect_no_offenses(indoc! {r#"
                describe 'x' do
                  it('y') { }
                end
            "#});
    }

    #[test]
    fn does_not_flag_non_hook_after_example() {
        // Only hooks flag; a `let` after the example stays clean.
        test::<HooksBeforeExamples>().expect_no_offenses(indoc! {r#"
                describe 'x' do
                  it('y') { }
                  let(:foo) { 1 }
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(HooksBeforeExamples);
