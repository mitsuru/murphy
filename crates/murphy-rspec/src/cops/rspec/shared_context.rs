//! `RSpec/SharedContext` — use `shared_context` for setup, `shared_examples` for examples.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/SharedContext
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_block` (`shared_context` / `shared_example`
//!   matchers: a `Block` whose call is a `SharedGroups.context` /
//!   `.examples` selector with a bare or `RSpec` receiver) with
//!   `examples?` (a bare `Includes.examples` / `Examples.all` send
//!   anywhere in the subtree) and `context?` (a bare `Subjects.all` /
//!   `Helpers.all` / `Includes.context` / `Hooks.all` send anywhere in
//!   the subtree) via descendant search. A `shared_context` with only
//!   examples flags the send per `add_offense(send_node)` with `Use
//!   `shared_examples` when you don't define context.`; a
//!   `shared_examples` / `shared_examples_for` with only setup flags
//!   the send with `Use `shared_context` when you don't define
//!   examples.`. Detection is at parity (verified vs 3.7.0, including
//!   empty-group and mixed clean cases); autocorrect (replace the
//!   selector) is not ported in this batch — same convention as
//!   `RSpec/HookArgument` (status: partial, autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` shared groups:
//!
//! - `shared_context 'x' { it {} }` — only examples, flagged.
//! - `shared_examples 'x' { let(:a) {} }` — only setup, flagged.
//! - `shared_examples 'x' { before {} }` — only hooks, flagged.
//! - `shared_context 'x' {}` — empty, clean.
//! - `shared_context 'x' { let(:a) {}; it {} }` — mixed, clean.
//! - `shared_examples 'x' { it {} }` — has examples, clean.
//!
//! ## No autocorrect
//!
//! Upstream replaces the selector (`shared_context` <-> 
//! `shared_examples`). This batch reports only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::{is_rspec_or_bare_receiver, send_without_block_range};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SharedContext;

#[cop(
    name = "RSpec/SharedContext",
    description = "Checks for proper shared_context and shared_examples usage.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl SharedContext {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, .. } = *cx.kind(node) else {
            return;
        };
        let NodeKind::Send { receiver, method, .. } = *cx.kind(call) else {
            return;
        };
        if !is_rspec_or_bare_receiver(cx, receiver) {
            return;
        }
        let name = cx.symbol_str(method);
        let is_context = name == "shared_context";
        let is_examples = matches!(name, "shared_examples" | "shared_examples_for");
        if !is_context && !is_examples {
            return;
        }
        let has_examples = contains_example_send(cx, node);
        let has_context = contains_context_send(cx, node);
        if is_context && has_examples && !has_context {
            cx.emit_offense(
                send_without_block_range(cx, call),
                "Use `shared_examples` when you don't define context.",
                None,
            );
        } else if is_examples && has_context && !has_examples {
            cx.emit_offense(
                send_without_block_range(cx, call),
                "Use `shared_context` when you don't define examples.",
                None,
            );
        }
    }
}

/// `true` when the subtree holds a bare example/include send.
///
/// Mirrors upstream `examples?`
/// (`(send nil? {#Includes.examples #Examples.all} ...)`):
/// `Includes.examples` is `it_behaves_like` / `it_should_behave_like`
/// / `include_examples`; `Examples.all` is the regular + focused +
/// skipped + pending example selectors.
fn contains_example_send(cx: &Cx<'_>, root: NodeId) -> bool {
    for id in cx.descendants(root) {
        let NodeKind::Send { receiver, method, .. } = *cx.kind(id) else {
            continue;
        };
        if receiver != OptNodeId::NONE {
            continue;
        }
        if is_example_or_include_example(cx.symbol_str(method)) {
            return true;
        }
    }
    false
}

/// `true` when the subtree holds a bare setup send.
///
/// Mirrors upstream `context?`
/// (`(send nil? {#Subjects.all #Helpers.all #Includes.context
/// #Hooks.all} ...)`): `subject` / `subject!`, `let` / `let!`,
/// `include_context`, and the `Hooks.all` selectors.
fn contains_context_send(cx: &Cx<'_>, root: NodeId) -> bool {
    for id in cx.descendants(root) {
        let NodeKind::Send { receiver, method, .. } = *cx.kind(id) else {
            continue;
        };
        if receiver != OptNodeId::NONE {
            continue;
        }
        if is_setup_selector(cx.symbol_str(method)) {
            return true;
        }
    }
    false
}

fn is_example_or_include_example(name: &str) -> bool {
    matches!(
        name,
        "it_behaves_like" | "it_should_behave_like" | "include_examples"
            | "it" | "specify"
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

fn is_setup_selector(name: &str) -> bool {
    matches!(
        name,
        "subject" | "subject!"
            | "let"
            | "let!"
            | "include_context"
            | "before"
            | "after"
            | "around"
            | "prepend_before"
            | "append_before"
            | "prepend_after"
            | "append_after"
    )
}

#[cfg(test)]
mod tests {
    use super::SharedContext;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_shared_context_with_only_examples() {
        test::<SharedContext>().expect_offense(indoc! {r#"
                shared_context 'foo' do
                ^^^^^^^^^^^^^^^^^^^^ Use `shared_examples` when you don't define context.
                  it 'performs actions' do
                  end
                end
            "#});
    }

    #[test]
    fn flags_shared_examples_with_only_let() {
        test::<SharedContext>().expect_offense(indoc! {r#"
                shared_examples 'foo' do
                ^^^^^^^^^^^^^^^^^^^^^ Use `shared_context` when you don't define examples.
                  let(:foo) { :bar }
                end
            "#});
    }

    #[test]
    fn flags_shared_examples_with_only_hooks() {
        test::<SharedContext>().expect_offense(indoc! {r#"
                shared_examples 'foo' do
                ^^^^^^^^^^^^^^^^^^^^^ Use `shared_context` when you don't define examples.
                  before do
                    foo
                  end
                end
            "#});
    }

    #[test]
    fn ignores_empty_shared_context() {
        test::<SharedContext>().expect_no_offenses(indoc! {r#"
                shared_context 'empty' do
                end
            "#});
    }

    #[test]
    fn ignores_shared_context_with_let_and_example() {
        test::<SharedContext>().expect_no_offenses(indoc! {r#"
                shared_context 'foo' do
                  let(:foo) { :bar }

                  it 'performs actions' do
                  end
                end
            "#});
    }

    #[test]
    fn ignores_shared_examples_with_example() {
        test::<SharedContext>().expect_no_offenses(indoc! {r#"
                shared_examples 'foo' do
                  subject(:foo) { 'foo' }
                  let(:bar) { :baz }
                  before { initialize }

                  it 'works' do
                  end
                end
            "#});
    }

    #[test]
    fn ignores_empty_shared_examples() {
        test::<SharedContext>().expect_no_offenses(indoc! {r#"
                shared_examples 'empty' do
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(SharedContext);
