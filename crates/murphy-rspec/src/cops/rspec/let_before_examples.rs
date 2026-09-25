//! `RSpec/LetBeforeExamples` — `let` definitions must come before the examples.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/LetBeforeExamples
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_block` gated by `example_group_with_body?`
//!   (an RSpec-or-bare example-group call with a body) and
//!   `multiline_block?` (the body is a `Begin`). The first body child
//!   matching `example_or_group?` — a `Block` whose call is a *bare*
//!   example-group or example selector (no `RSpec` gateway, so a nested
//!   `RSpec.describe` never counts), or a bare `Includes.examples` send
//!   (`it_behaves_like` / `it_should_behave_like` /
//!   `include_examples`, but not `include_context`) — splits the body;
//!   every later sibling matching `let?` (bare `let` / `let!` block, or
//!   a bare send whose last arg is a `BlockPass`) is flagged whole per
//!   `add_offense(sibling)` with `Move `let` before the examples in the
//!   group.` Detection is at parity; autocorrect (move the `let` above
//!   the first example) is not ported in this batch — same convention
//!   as `RSpec/HooksBeforeExamples` (status: partial, autocorrect as
//!   gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` group calls with a `Begin` body:
//!
//! - `describe { it {}; let(:a) {} }` — flagged (whole `let`).
//! - `describe { let(:a) {}; it {} }` — `let` first, clean.
//! - `describe { context {}; let(:a) {} }` — nested group counts as
//!   the first example, `let` flagged.
//! - `describe { it_behaves_like 'x'; let(:a) {} }` — shared examples
//!   count, `let` flagged.
//! - `describe { include_context 'x'; let(:a) {} }` — context includes
//!   never count, clean.
//! - `describe { include_examples 'x' do end; let(:a) {} }` —
//!   block-form includes never count, clean.
//! - `describe { RSpec.describe 'y' do end; let(:a) {} }` —
//!   namespaced groups never count, clean.
//! - `describe { shared_examples 'x' do end; let(:a) {} }` — shared
//!   groups never count, clean.
//!
//! ## No autocorrect
//!
//! Upstream moves the `let` above the first example. This batch reports
//! only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::{is_example_group_name, is_rspec_or_bare_receiver};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct LetBeforeExamples;

#[cop(
    name = "RSpec/LetBeforeExamples",
    description = "Checks for `let` definitions that come after an example.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl LetBeforeExamples {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, body, .. } = *cx.kind(node) else {
            return;
        };
        // Upstream `example_group_with_body?`: an RSpec-or-bare
        // example-group call with a body.
        if !is_group_call(cx, call) {
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
        let Some(first_idx) = members.iter().position(|id| is_example_or_group(cx, *id)) else {
            return;
        };
        for sibling in &members[first_idx + 1..] {
            if !is_let_node(cx, *sibling) {
                continue;
            }
            cx.emit_offense(
                cx.range(*sibling),
                "Move `let` before the examples in the group.",
                None,
            );
        }
    }
}

/// `true` when `call` is an RSpec-or-bare example-group entrypoint.
fn is_group_call(cx: &Cx<'_>, call: NodeId) -> bool {
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(call)
    else {
        return false;
    };
    if !is_rspec_or_bare_receiver(cx, receiver) {
        return false;
    }
    is_example_group_name(cx.symbol_str(method))
}

/// `true` when `id` is the first-example split point per upstream
/// `example_or_group?`: a `Block` whose call is a *bare* group or
/// example selector, or a bare `Includes.examples` send.
///
/// Both arms are bare-receiver-only: a nested `RSpec.describe` block, a
/// block-form `include_examples`, and `include_context` never count
/// (all verified vs 3.7.0).
fn is_example_or_group(cx: &Cx<'_>, id: NodeId) -> bool {
    match *cx.kind(id) {
        NodeKind::Block { call, .. } => {
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
            is_example_group_name(name) || is_example_name(name)
        }
        NodeKind::Send {
            receiver, method, ..
        } => {
            if receiver != OptNodeId::NONE {
                return false;
            }
            is_include_example(cx.symbol_str(method))
        }
        _ => false,
    }
}

/// `true` when `name` is a bare example selector (`Examples.all`).
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

/// `true` when `name` is an `Includes.examples` selector
/// (`it_behaves_like` / `it_should_behave_like` / `include_examples`).
/// `Includes.context` (`include_context`) never counts.
fn is_include_example(name: &str) -> bool {
    matches!(
        name,
        "it_behaves_like" | "it_should_behave_like" | "include_examples"
    )
}

/// `true` when `id` is a `let?` node: a bare `let` / `let!` block, or a
/// bare `let` / `let!` send whose last arg is a `BlockPass` (`&blk`).
fn is_let_node(cx: &Cx<'_>, id: NodeId) -> bool {
    match *cx.kind(id) {
        NodeKind::Block { call, .. } => {
            let NodeKind::Send {
                receiver, method, ..
            } = *cx.kind(call)
            else {
                return false;
            };
            if receiver != OptNodeId::NONE {
                return false;
            }
            matches!(cx.symbol_str(method), "let" | "let!")
        }
        NodeKind::Send {
            receiver, method, args,
        } => {
            if receiver != OptNodeId::NONE {
                return false;
            }
            if !matches!(cx.symbol_str(method), "let" | "let!") {
                return false;
            }
            let arg_ids = cx.list(args);
            let Some(&last) = arg_ids.last() else {
                return false;
            };
            matches!(*cx.kind(last), NodeKind::BlockPass(_))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::LetBeforeExamples;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_let_after_example() {
        test::<LetBeforeExamples>().expect_offense(indoc! {r#"
                describe 'x' do
                  it 'y' do
                  end
                  let(:bar) { 2 }
                  ^^^^^^^^^^^^^^^ Move `let` before the examples in the group.
                end
            "#});
    }

    #[test]
    fn does_not_flag_let_first() {
        test::<LetBeforeExamples>().expect_no_offenses(indoc! {r#"
                describe 'x' do
                  let(:foo) { 1 }
                  let(:bar) { 2 }
                  it 'y' do
                  end
                end
            "#});
    }

    #[test]
    fn flags_let_after_group() {
        // A nested group counts as the first example (verified vs
        // 3.7.0).
        test::<LetBeforeExamples>().expect_offense(indoc! {r#"
                describe 'x' do
                  context 'y' do
                  end
                  let(:bar) { 2 }
                  ^^^^^^^^^^^^^^^ Move `let` before the examples in the group.
                end
            "#});
    }

    #[test]
    fn flags_let_after_shared_examples_send() {
        // Bare `Includes.examples` sends count (verified vs 3.7.0).
        test::<LetBeforeExamples>().expect_offense(indoc! {r#"
                describe 'x' do
                  it_behaves_like 'y'
                  let(:bar) { 2 }
                  ^^^^^^^^^^^^^^^ Move `let` before the examples in the group.
                end
            "#});
    }

    #[test]
    fn does_not_flag_let_after_include_context() {
        // `Includes.context` never counts (verified vs 3.7.0).
        test::<LetBeforeExamples>().expect_no_offenses(indoc! {r#"
                describe 'x' do
                  include_context 'y'
                  let(:bar) { 2 }
                end
            "#});
    }

    #[test]
    fn does_not_flag_let_after_include_examples_block() {
        // Block-form includes never count (verified vs 3.7.0).
        test::<LetBeforeExamples>().expect_no_offenses(indoc! {r#"
                describe 'x' do
                  include_examples 'y' do
                  end
                  let(:bar) { 2 }
                end
            "#});
    }

    #[test]
    fn does_not_flag_let_after_namespaced_group() {
        // The block arm is bare-receiver-only (verified vs 3.7.0).
        test::<LetBeforeExamples>().expect_no_offenses(indoc! {r#"
                describe 'x' do
                  RSpec.describe 'y' do
                  end
                  let(:bar) { 2 }
                end
            "#});
    }

    #[test]
    fn flags_let_bang_after_example() {
        test::<LetBeforeExamples>().expect_offense(indoc! {r#"
                describe 'x' do
                  it 'y' do
                  end
                  let!(:bar) { 2 }
                  ^^^^^^^^^^^^^^^^ Move `let` before the examples in the group.
                end
            "#});
    }

    #[test]
    fn flags_every_late_let() {
        test::<LetBeforeExamples>().expect_offense(indoc! {r#"
                describe 'x' do
                  it 'a' do
                  end
                  let(:x) { 1 }
                  ^^^^^^^^^^^^^ Move `let` before the examples in the group.
                  let(:y) { 2 }
                  ^^^^^^^^^^^^^ Move `let` before the examples in the group.
                end
            "#});
    }

    #[test]
    fn flags_nested_group_late_let() {
        test::<LetBeforeExamples>().expect_offense(indoc! {r#"
                describe 'x' do
                  context 'y' do
                    it 'a' do
                    end
                    let(:x) { 1 }
                    ^^^^^^^^^^^^^ Move `let` before the examples in the group.
                  end
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(LetBeforeExamples);
