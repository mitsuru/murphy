//! `RSpec/RepeatedSubjectCall` — do not call `subject` twice in one example.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/RepeatedSubjectCall
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_top_level_group`: subject definitions
//!   (`subject` / `subject!` blocks, named via a leading `Sym` arg)
//!   are attributed to their nearest enclosing example group and
//!   inherited by nested examples; each example body is then scanned
//!   in document order for bare calls to `subject` plus the in-scope
//!   names. The first call to a name marks it used; a repeat is an
//!   offense unless the call is chained (its parent is a `Send` /
//!   `Csend`, covering both upstream's `chained?` and
//!   `parent.send_type?` guards) or sits outside an `expect { ... }`
//!   block. The offense lands on the whole `expect` block per
//!   `add_offense(block_node)` with `Calls to subject are memoized,
//!   this block is misleading`. Dispatch here is per example block
//!   rather than per top-level group, so pathologically nested
//!   shapes (groups inside `def`) still flag — same visible
//!   behaviour for real-world specs (verified vs 3.7.0). No
//!   autocorrect upstream, none here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` example calls:
//!
//! - `it { subject; expect { subject }.to ... }` — the `expect`
//!   block flagged.
//! - Two `expect { subject }.to ...` in one example — the second
//!   `expect` block flagged.
//! - `expect { my_method }.to ...` twice — `my_method` is not a
//!   subject, clean.
//! - `expect { subject.a }.to ...` twice — chained, clean.
//! - Single `subject` call — clean.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; memoizing the value needs human
//! judgement.

use std::collections::HashSet;

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::{is_bare_example_block, is_example_group_call};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct RepeatedSubjectCall;

#[cop(
    name = "RSpec/RepeatedSubjectCall",
    description = "Checks for repeated calls to subject, which is memoized.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl RepeatedSubjectCall {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        if !is_bare_example_block(cx, node) {
            return;
        }
        let NodeKind::Block { body, .. } = *cx.kind(node) else {
            return;
        };
        let Some(body_id) = body.get() else {
            return;
        };
        let names = subject_names_in_scope(cx, node);
        let mut used: HashSet<String> = HashSet::new();
        for call in bare_calls_in_order(cx, body_id) {
            let NodeKind::Send { method, .. } = *cx.kind(call) else {
                continue;
            };
            let name = cx.symbol_str(method).to_owned();
            if !names.contains(&name) {
                continue;
            }
            if !used.insert(name) {
                detect_offense(cx, call);
            }
        }
    }
}

/// Subject names visible inside `example`: `subject` plus every
/// subject definition attributed to an enclosing example group.
///
/// Mirrors `detect_subjects_in_scope` (each `subject` / `subject!`
/// block attributed to its nearest enclosing example group) combined
/// with the `subject_names` accumulation down the group nesting.
fn subject_names_in_scope(cx: &Cx<'_>, example: NodeId) -> HashSet<String> {
    let mut enclosing: Vec<NodeId> = Vec::new();
    let mut cur = example;
    while let Some(p) = cx.parent(cur).get() {
        if let NodeKind::Block { call, .. } = *cx.kind(p)
            && is_example_group_call(cx, call)
        {
            enclosing.push(p);
        }
        cur = p;
    }
    let mut names: HashSet<String> = HashSet::new();
    names.insert("subject".to_owned());
    if enclosing.is_empty() {
        return names;
    }
    let root = cx.root();
    for id in core::iter::once(root).chain(cx.descendants(root)) {
        let Some(name) = subject_definition_name(cx, id) else {
            continue;
        };
        if nearest_enclosing_group(cx, id).is_some_and(|g| enclosing.contains(&g)) {
            names.insert(name);
        }
    }
    names
}

/// The defined subject name when `id` is a subject-definition block:
/// the leading `Sym` arg (`subject(:admin)`) or `subject`.
///
/// Mirrors upstream `subject?` (`(block (send nil? { #Subjects.all
/// (sym $_) | $#Subjects.all }) args ...)`).
fn subject_definition_name(cx: &Cx<'_>, id: NodeId) -> Option<String> {
    let NodeKind::Block { call, .. } = *cx.kind(id) else {
        return None;
    };
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(call)
    else {
        return None;
    };
    if receiver != OptNodeId::NONE {
        return None;
    }
    if !matches!(cx.symbol_str(method), "subject" | "subject!") {
        return None;
    }
    for &arg in cx.list(args) {
        if let NodeKind::Sym(s) = *cx.kind(arg) {
            return Some(cx.symbol_str(s).to_owned());
        }
    }
    Some("subject".to_owned())
}

/// Nearest ancestor example-group block, if any.
fn nearest_enclosing_group(cx: &Cx<'_>, id: NodeId) -> Option<NodeId> {
    let mut cur = id;
    while let Some(p) = cx.parent(cur).get() {
        if let NodeKind::Block { call, .. } = *cx.kind(p)
            && is_example_group_call(cx, call)
        {
            return Some(p);
        }
        cur = p;
    }
    None
}

/// Bare `Send` nodes under `root` in document order.
///
/// Mirrors upstream `subject_calls` (`def_node_search` for
/// `(send nil? %)`), which visits matches in source order.
fn bare_calls_in_order(cx: &Cx<'_>, root: NodeId) -> Vec<NodeId> {
    fn walk(cx: &Cx<'_>, id: NodeId, out: &mut Vec<NodeId>) {
        if let NodeKind::Send { receiver, .. } = *cx.kind(id)
            && receiver == OptNodeId::NONE
        {
            out.push(id);
        }
        for child in cx.children(id) {
            walk(cx, child, out);
        }
    }

    let mut out = Vec::new();
    walk(cx, root, &mut out);
    out
}

/// Flag the enclosing `expect { ... }` block for a repeated call.
///
/// Mirrors `detect_offense`: skips chained calls and calls whose
/// parent is a send, then flags `expect_block` (the nearest ancestor
/// `expect` block).
fn detect_offense(cx: &Cx<'_>, call: NodeId) {
    if let Some(p) = cx.parent(call).get()
        && matches!(
            *cx.kind(p),
            NodeKind::Send { .. } | NodeKind::Csend { .. }
        )
    {
        return;
    }
    let mut cur = call;
    while let Some(p) = cx.parent(cur).get() {
        if let NodeKind::Block { call: block_call, .. } = *cx.kind(p)
            && let NodeKind::Send { method, .. } = *cx.kind(block_call)
            && cx.symbol_str(method) == "expect"
        {
            cx.emit_offense(
                cx.range(p),
                "Calls to subject are memoized, this block is misleading",
                None,
            );
            return;
        }
        cur = p;
    }
}

#[cfg(test)]
mod tests {
    use super::RepeatedSubjectCall;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_second_subject_in_expect() {
        test::<RepeatedSubjectCall>().expect_offense(indoc! {r#"
                it do
                  subject
                  expect { subject }.to not_change { A.count }
                  ^^^^^^^^^^^^^^^^^^ Calls to subject are memoized, this block is misleading
                end
            "#});
    }

    #[test]
    fn flags_second_of_two_expects() {
        test::<RepeatedSubjectCall>().expect_offense(indoc! {r#"
                it do
                  expect { subject }.to change { A.count }
                  expect { subject }.to not_change { A.count }
                  ^^^^^^^^^^^^^^^^^^ Calls to subject are memoized, this block is misleading
                end
            "#});
    }

    #[test]
    fn ignores_non_subject_calls() {
        // `my_method` is not a subject name, so repeats are fine.
        test::<RepeatedSubjectCall>().expect_no_offenses(indoc! {r#"
                it do
                  expect { my_method }.to change { A.count }
                  expect { my_method }.to not_change { A.count }
                end
            "#});
    }

    #[test]
    fn ignores_chained_subject() {
        // `subject.a` / `subject.b` are chained sends, skipped
        // upstream via `chained?` / `parent.send_type?`.
        test::<RepeatedSubjectCall>().expect_no_offenses(indoc! {r#"
                it do
                  expect { subject.a }.to change { A.count }
                  expect { subject.b }.to not_change { A.count }
                end
            "#});
    }

    #[test]
    fn ignores_single_subject_call() {
        test::<RepeatedSubjectCall>().expect_no_offenses(indoc! {r#"
                it do
                  expect { subject }.to change { A.count }
                end
            "#});
    }

    #[test]
    fn flags_named_subject_repeat() {
        test::<RepeatedSubjectCall>().expect_offense(indoc! {r#"
                describe Foo do
                  subject(:admin) { create(:admin) }
                  it do
                    admin
                    expect { admin }.to change { A.count }
                    ^^^^^^^^^^^^^^^^ Calls to subject are memoized, this block is misleading
                  end
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(RepeatedSubjectCall);
