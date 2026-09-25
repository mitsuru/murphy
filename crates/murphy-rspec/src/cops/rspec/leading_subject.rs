//! `RSpec/LeadingSubject` — `subject` must be the first definition in the group.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/LeadingSubject
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_block` (`subject?`: `(block (send nil?
//!   #Subjects.all ...) ...)` — bare `subject` / `subject!` only) gated
//!   by `InsideExampleGroup#inside_example_group?`, simplified to "any
//!   ancestor block is an RSpec-or-bare example group" per the
//!   `RSpec/EmptyLineAfterSubject` precedent. The enclosing block's body
//!   must be a `Begin`; the first preceding sibling matching `offending?`
//!   (`let?` bare `let` / `let!` block or `block_pass` send form,
//!   `hook?` bare `Hooks.all` block, `example?` bare `Examples.all`
//!   block, `spec_group?` RSpec-or-bare group block, `include?` bare
//!   `Includes.all` block or send) flags the whole subject block per
//!   `add_offense(node)` with `Declare `subject` above any other
//!   `%<offending>s` declarations.` (the offender's method name).
//!   Detection is at parity; autocorrect (move the subject above the
//!   offender) is not ported in this batch — same convention as
//!   `RSpec/HooksBeforeExamples` (status: partial, autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` whose call is a bare `subject` / `subject!`
//! inside an example group:
//!
//! - `describe { let(:a) {}; subject {} }` — flagged (whole subject).
//! - `describe { subject {}; let(:a) {} }` — subject first, clean.
//! - `describe { before {}; subject {} }` — hook first, flagged.
//! - `describe { it {}; subject {} }` — example first, flagged.
//! - `describe { context {}; subject {} }` — group first, flagged.
//! - `describe { it_behaves_like 'x'; subject {} }` — include first,
//!   flagged.
//! - `describe { subject {}; subject {} }` — subjects never offend,
//!   clean.
//! - Top-level `subject {}` — no group ancestor, clean.
//!
//! ## No autocorrect
//!
//! Upstream moves the subject above the offender. This batch reports
//! only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::{
    is_example_group_call, is_example_group_name, is_hook_name, is_rspec_or_bare_receiver,
};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct LeadingSubject;

#[cop(
    name = "RSpec/LeadingSubject",
    description = "Enforce that subject is the first definition in the test.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl LeadingSubject {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, .. } = *cx.kind(node) else {
            return;
        };
        // `subject?`: bare `subject` / `subject!` only.
        let NodeKind::Send {
            receiver, method, ..
        } = *cx.kind(call)
        else {
            return;
        };
        if receiver != OptNodeId::NONE {
            return;
        }
        if !matches!(cx.symbol_str(method), "subject" | "subject!") {
            return;
        }
        if !is_inside_example_group(cx, node) {
            return;
        }
        let Some(offender) = first_offending_sibling(cx, node) else {
            return;
        };
        cx.emit_offense(
            cx.range(node),
            &format!(
                "Declare `subject` above any other `{offender}` declarations."
            ),
            None,
        );
    }
}

/// Simplified `inside_example_group?`: `true` when any ancestor `Block`
/// is an RSpec-or-bare example group. Same simplification as
/// `RSpec/EmptyLineAfterSubject`.
fn is_inside_example_group(cx: &Cx<'_>, node: NodeId) -> bool {
    for anc in cx.ancestors(node) {
        if let NodeKind::Block { call, .. } = *cx.kind(anc) {
            let NodeKind::Send {
                receiver, method, ..
            } = *cx.kind(call)
            else {
                continue;
            };
            if !is_rspec_or_bare_receiver(cx, receiver) {
                continue;
            }
            if is_example_group_name(cx.symbol_str(method)) {
                return true;
            }
        }
    }
    false
}

/// The method name of the first preceding sibling of `node` (within the
/// enclosing block's `Begin` body) that matches upstream `offending?`,
/// else `None`.
fn first_offending_sibling(cx: &Cx<'_>, node: NodeId) -> Option<String> {
    // Upstream `parent(node)`: the nearest enclosing block's body. A
    // non-`Begin` body holds only `node` itself, so there is no
    // preceding sibling.
    let group = cx
        .ancestors(node)
        .find(|anc| matches!(*cx.kind(*anc), NodeKind::Block { .. }))?;
    let NodeKind::Block { body, .. } = *cx.kind(group) else {
        return None;
    };
let body_id = body.get()?;
    let NodeKind::Begin(members) = *cx.kind(body_id) else {
        return None;
    };
    for sibling in cx.list(members) {
        if *sibling == node {
            break;
        }
        if let Some(name) = offending_name(cx, *sibling) {
            return Some(name);
        }
    }
    None
}

/// The offender method name when `id` matches upstream `offending?`
/// (`let?`, `hook?`, `example?`, `spec_group?`, `include?`), else `None`.
fn offending_name(cx: &Cx<'_>, id: NodeId) -> Option<String> {
    match *cx.kind(id) {
        NodeKind::Block { call, .. } => {
            let name = bare_call_name(cx, call)?;
            if is_let_block(cx, id)
                || is_hook_name(name)
                || is_bare_example_name(name)
                || is_group_or_include_block_name(cx, call)
            {
                Some(name.to_owned())
            } else {
                None
            }
        }
        NodeKind::Numblock { send, .. } => {
            // `hook?`, `spec_group?` and `include?` are `any_block`
            // patterns; `example?` is `block`-only so numblock examples
            // never offend.
            let name = bare_call_name(cx, send)?;
            if is_hook_name(name) || is_group_or_include_block_name(cx, send) {
                Some(name.to_owned())
            } else {
                None
            }
        }
        NodeKind::Send { .. } => {
            // `include?` bare-send arm (`Includes.all`).
            let name = bare_call_name(cx, id)?;
            if is_include_example_or_context(name) {
                Some(name.to_owned())
            } else {
                None
            }
        }
        _ => None,
    }
}

/// The call method name when `call` is a bare send, else `None`.
fn bare_call_name<'a>(cx: &'a Cx<'a>, call: NodeId) -> Option<&'a str> {
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(call)
    else {
        return None;
    };
    if receiver != OptNodeId::NONE {
        return None;
    }
    Some(cx.symbol_str(method))
}

/// `true` when `id` is a `let?` block (`Helpers.all`: `let` / `let!`).
/// Any block shape counts — the name check is on the call.
fn is_let_block(cx: &Cx<'_>, id: NodeId) -> bool {
    let NodeKind::Block { call, .. } = *cx.kind(id) else {
        return false;
    };
    matches!(bare_call_name(cx, call), Some("let" | "let!"))
}

/// `true` when `name` is a bare example selector (`Examples.all`).
fn is_bare_example_name(name: &str) -> bool {
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

/// `true` when `call` is an RSpec-or-bare example group, shared group,
/// or bare `Includes.all` (examples + context) block call.
fn is_group_or_include_block_name(cx: &Cx<'_>, call: NodeId) -> bool {
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(call)
    else {
        return false;
    };
    let name = cx.symbol_str(method);
    if is_example_group_call(cx, call) {
        return true;
    }
    if matches!(
        name,
        "shared_examples" | "shared_examples_for" | "shared_context"
    ) && is_rspec_or_bare_receiver(cx, receiver)
    {
        return true;
    }
    receiver == OptNodeId::NONE && is_include_example_or_context(name)
}

/// `true` when `name` is an `Includes.all` selector (examples +
/// context).
fn is_include_example_or_context(name: &str) -> bool {
    matches!(
        name,
        "it_behaves_like" | "it_should_behave_like" | "include_examples" | "include_context"
    )
}

#[cfg(test)]
mod tests {
    use super::LeadingSubject;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_subject_after_let() {
        test::<LeadingSubject>().expect_offense(indoc! {r#"
                describe 'x' do
                  let(:params) { 1 }
                  subject { described_class.new(params) }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Declare `subject` above any other `let` declarations.
                end
            "#});
    }

    #[test]
    fn does_not_flag_subject_first() {
        test::<LeadingSubject>().expect_no_offenses(indoc! {r#"
                describe 'x' do
                  subject { described_class.new }
                  let(:params) { 1 }
                end
            "#});
    }

    #[test]
    fn flags_subject_after_hook() {
        test::<LeadingSubject>().expect_offense(indoc! {r#"
                describe 'x' do
                  before { prepare }
                  subject { y }
                  ^^^^^^^^^^^^^ Declare `subject` above any other `before` declarations.
                end
            "#});
    }

    #[test]
    fn flags_subject_after_example() {
        test::<LeadingSubject>().expect_offense(indoc! {r#"
                describe 'x' do
                  it { expect_something }
                  subject { y }
                  ^^^^^^^^^^^^^ Declare `subject` above any other `it` declarations.
                end
            "#});
    }

    #[test]
    fn flags_subject_after_group() {
        // Nested groups are `spec_group?` offenders (verified vs
        // 3.7.0).
        test::<LeadingSubject>().expect_offense(indoc! {r#"
                describe 'x' do
                  context 'y' do
                  end
                  subject { y }
                  ^^^^^^^^^^^^^ Declare `subject` above any other `context` declarations.
                end
            "#});
    }

    #[test]
    fn flags_subject_after_include() {
        test::<LeadingSubject>().expect_offense(indoc! {r#"
                describe 'x' do
                  it_behaves_like 'y'
                  subject { y }
                  ^^^^^^^^^^^^^ Declare `subject` above any other `it_behaves_like` declarations.
                end
            "#});
    }

    #[test]
    fn names_first_offender() {
        // `offending_node` stops at the first offending sibling
        // (verified vs 3.7.0).
        test::<LeadingSubject>().expect_offense(indoc! {r#"
                describe 'x' do
                  before { prepare }
                  let(:b) { 1 }
                  subject { y }
                  ^^^^^^^^^^^^^ Declare `subject` above any other `before` declarations.
                end
            "#});
    }

    #[test]
    fn flags_subject_bang_after_let_bang() {
        // The message carries the offender's real spelling, `let!`
        // (verified vs 3.7.0).
        test::<LeadingSubject>().expect_offense(indoc! {r#"
                describe 'x' do
                  let!(:b) { 1 }
                  subject! { y }
                  ^^^^^^^^^^^^^^ Declare `subject` above any other `let!` declarations.
                end
            "#});
    }

    #[test]
    fn does_not_flag_second_subject() {
        // Subjects never match `offending?` (verified vs 3.7.0).
        test::<LeadingSubject>().expect_no_offenses(indoc! {r#"
                describe 'x' do
                  subject { y }
                  subject { z }
                end
            "#});
    }

    #[test]
    fn does_not_flag_top_level_subject() {
        test::<LeadingSubject>().expect_no_offenses(indoc! {r#"
                subject { y }
            "#});
    }

    #[test]
    fn does_not_flag_receiver_subject() {
        // `subject?` requires a bare send (verified vs 3.7.0).
        test::<LeadingSubject>().expect_no_offenses(indoc! {r#"
                describe 'x' do
                  let(:a) { 1 }
                  foo.subject { y }
                end
            "#});
    }

    #[test]
    fn flags_nested_subject_in_scope() {
        test::<LeadingSubject>().expect_offense(indoc! {r#"
                describe 'x' do
                  context 'y' do
                    let(:a) { 1 }
                    subject { y }
                    ^^^^^^^^^^^^^ Declare `subject` above any other `let` declarations.
                  end
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(LeadingSubject);
