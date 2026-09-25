//! `RSpec/ImplicitBlockExpectation` — no implicit block expectations on lambda subjects.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/ImplicitBlockExpectation
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_send` (`RESTRICT_ON_SEND`: `is_expected` /
//!   `should` / `should_not`, bare receiver) with `nearest_subject`
//!   (nearest enclosing multi-statement example group —
//!   `example_group_with_body?` with a `Begin` body — holding a `subject`
//!   / `subject!` block) and `lambda_subject?` (the subject body is a
//!   block whose call is stabby-lambda `->`, `proc` / `lambda`, or
//!   `Proc.new`). The offense range is the implicit-expectation send
//!   per `add_offense(implicit_expect)`. Murphy's stabby lambda is a
//!   `Block` over a `Lambda` marker (not parser-gem's
//!   `(send nil? :lambda)`), so that shape is accepted as an explicit
//!   documented superset of the `lambda?` arms. No autocorrect upstream,
//!   none here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on bare `Send` (`methods = ["is_expected", "should",
//! `"should_not"]`):
//!
//! - `describe { subject { -> { x } }; it { is_expected.to ... } }` —
//!   flagged (range `is_expected`).
//! - `describe { subject { proc { x } }; it { should ... } }` — flagged.
//! - `describe { subject { Proc.new { x } }; it { should_not ... } }` —
//!   flagged.
//! - `describe { subject { x }; it { is_expected.to ... } }` — plain
//!   subject, not flagged.
//! - `subject { -> { x } }; it { is_expected.to ... }` at top level —
//!   no enclosing group, not flagged.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; spelling the block expectation out
//! needs human judgement.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::is_example_group_call;

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ImplicitBlockExpectation;

#[cop(
    name = "RSpec/ImplicitBlockExpectation",
    description = "Check that implicit block expectation syntax is not used.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl ImplicitBlockExpectation {
    #[on_node(kind = "send", methods = ["is_expected", "should", "should_not"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, .. } = *cx.kind(node) else {
            return;
        };
        if receiver != OptNodeId::NONE {
            return;
        }
        let Some(subject) = nearest_subject(cx, node) else {
            return;
        };
        if !is_lambda_subject_body(cx, subject) {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            "Avoid implicit block expectations.",
            None,
        );
    }
}

/// The nearest enclosing multi-statement example group's `subject` /
/// `subject!` block, if any.
///
/// Mirrors upstream `nearest_subject` (block ancestors filtered by
/// `multi_statement_example_group?`, mapped through `find_subject`,
/// first hit wins).
fn nearest_subject(cx: &Cx<'_>, node: NodeId) -> Option<NodeId> {
    for ancestor in cx.ancestors(node) {
        let NodeKind::Block { call, body, .. } = *cx.kind(ancestor) else {
            continue;
        };
        if !is_example_group_call(cx, call) {
            continue;
        }
        let Some(body_id) = body.get() else {
            continue;
        };
        let NodeKind::Begin(members) = *cx.kind(body_id) else {
            continue;
        };
        if let Some(subject) = cx
            .list(members)
            .iter()
            .copied()
            .find(|id| is_subject_block(cx, *id))
        {
            return Some(subject);
        }
    }
    None
}

/// `true` when `id` is a `subject` / `subject!` block (`subject?`:
/// bare `Subjects.all` call with any block wrapper).
fn is_subject_block(cx: &Cx<'_>, id: NodeId) -> bool {
    let NodeKind::Block { call, .. } = *cx.kind(id) else {
        return false;
    };
    let NodeKind::Send { receiver, method, .. } = *cx.kind(call) else {
        return false;
    };
    if receiver != OptNodeId::NONE {
        return false;
    }
    matches!(cx.symbol_str(method), "subject" | "subject!")
}

/// `true` when the subject block's body is a lambda
/// (`lambda_subject?`: a block whose call is stabby `->`, `proc` /
/// `lambda`, or `Proc.new`).
fn is_lambda_subject_body(cx: &Cx<'_>, subject: NodeId) -> bool {
    let NodeKind::Block { body, .. } = *cx.kind(subject) else {
        return false;
    };
    let Some(body_id) = body.get() else {
        return false;
    };
    match *cx.kind(body_id) {
        NodeKind::Block { call, .. } => is_lambda_call(cx, call),
        NodeKind::Numblock { send, .. } => is_lambda_call(cx, send),
        _ => false,
    }
}

/// `true` when `call` builds a lambda: Murphy's `Lambda` marker for
/// stabby `->`, a bare `proc` / `lambda` send, or `Proc.new` on the
/// top-level `Proc` constant.
fn is_lambda_call(cx: &Cx<'_>, call: NodeId) -> bool {
    if matches!(*cx.kind(call), NodeKind::Lambda) {
        return true;
    }
    let NodeKind::Send { receiver, method, .. } = *cx.kind(call) else {
        return false;
    };
    let name = cx.symbol_str(method);
    if matches!(name, "proc" | "lambda") {
        return receiver == OptNodeId::NONE;
    }
    if name == "new" {
        let Some(rid) = receiver.get() else {
            return false;
        };
        let NodeKind::Const { scope, name } = *cx.kind(rid) else {
            return false;
        };
        return scope == OptNodeId::NONE && cx.symbol_str(name) == "Proc";
    }
    false
}

#[cfg(test)]
mod tests {
    use super::ImplicitBlockExpectation;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_is_expected_with_stabby_lambda_subject() {
        test::<ImplicitBlockExpectation>().expect_offense(indoc! {r#"
                describe 'x' do
                  subject { -> { do_something } }
                  it { is_expected.to change(something).to(new_value) }
                       ^^^^^^^^^^^ Avoid implicit block expectations.
                end
            "#});
    }

    #[test]
    fn flags_should_with_proc_subject() {
        test::<ImplicitBlockExpectation>().expect_offense(indoc! {r#"
                describe 'x' do
                  subject { proc { do_something } }
                  it { should change(something) }
                       ^^^^^^^^^^^^^^^^^^^^^^^^ Avoid implicit block expectations.
                end
            "#});
    }

    #[test]
    fn flags_should_not_with_proc_new_subject() {
        test::<ImplicitBlockExpectation>().expect_offense(indoc! {r#"
                describe 'x' do
                  subject { Proc.new { do_something } }
                  it { should_not change(something) }
                       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid implicit block expectations.
                end
            "#});
    }

    #[test]
    fn does_not_flag_plain_subject() {
        test::<ImplicitBlockExpectation>().expect_no_offenses(indoc! {r#"
                describe 'x' do
                  subject { do_something }
                  it { is_expected.to change(something).to(new_value) }
                end
            "#});
    }

    #[test]
    fn does_not_flag_without_subject() {
        test::<ImplicitBlockExpectation>().expect_no_offenses(indoc! {r#"
                describe 'x' do
                  let(:helper) { 1 }
                  it { is_expected.to be_truthy }
                end
            "#});
    }

    #[test]
    fn does_not_flag_lambda_subject_outside_group() {
        // No enclosing example group, so there is no nearest subject.
        test::<ImplicitBlockExpectation>().expect_no_offenses(indoc! {r#"
                subject { -> { do_something } }
                it { is_expected.to be_truthy }
            "#});
    }

    #[test]
    fn does_not_flag_explicit_receiver() {
        // `self.should` is not an implicit expectation.
        test::<ImplicitBlockExpectation>().expect_no_offenses(indoc! {r#"
                describe 'x' do
                  subject { -> { do_something } }
                  it { self.should be_truthy }
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(ImplicitBlockExpectation);
