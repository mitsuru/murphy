//! `RSpec/MultipleSubjects` — one `subject` per example group.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/MultipleSubjects
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_block` (`example_group?`: example groups only
//!   — shared groups excluded — bare or `RSpec` receiver):
//!   `ExampleGroup#subjects` (in-scope bare `subject` / `subject!`
//!   blocks, stopping at nested groups, includes, and examples) flags
//!   every definition but the last per `subjects[0...-1]` with `Do not
//!   set more than one subject per example group`. Detection is at
//!   parity (verified vs 3.7.0, including `subject!` which upstream
//!   flags without autocorrect); the named/unnamed autocorrect split
//!   (rewrite to `let` vs remove) is not ported in this batch — same
//!   report-only convention as the sibling cops (status: partial,
//!   autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` whose call is an example-group entrypoint:
//!
//! - Two named subjects — the first flagged.
//! - Two `subject!` — the first flagged.
//! - Two unnamed subjects — the first flagged.
//! - One subject — clean.
//! - Subjects in nested groups — separate scopes, clean.
//!
//! ## No autocorrect
//!
//! Upstream rewrites overwritten named subjects to `let` and removes
//! overwritten unnamed ones (`subject!` untouched). This batch reports
//! only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

use crate::cops::rspec_helpers::{
    is_bare_example_block, is_example_group_call, is_scope_change_block, is_subject_block,
};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct MultipleSubjects;

#[cop(
    name = "RSpec/MultipleSubjects",
    description = "Checks if an example group defines `subject` multiple times.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl MultipleSubjects {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, .. } = *cx.kind(node) else {
            return;
        };
        // Upstream `example_group?` (not `spec_group?`): shared groups
        // define helpers, not subjects.
        if !is_example_group_call(cx, call) {
            return;
        }
        let subjects = subjects_in_scope(cx, node);
        // `subjects[0...-1]`: every definition but the last is
        // overwritten.
        let Some((&last, rest)) = subjects.split_last() else {
            return;
        };
        let _ = last;
        for &subject in rest {
            cx.emit_offense(
                cx.range(subject),
                "Do not set more than one subject per example group",
                None,
            );
        }
    }
}

/// In-scope subject blocks for `group`, in document order.
///
/// Mirrors `ExampleGroup#subjects` via `#find_all_in_scope` (matched
/// blocks are returned without descending into them; nested groups,
/// includes, and examples stop the search).
fn subjects_in_scope(cx: &Cx<'_>, group: NodeId) -> Vec<NodeId> {
    fn walk(cx: &Cx<'_>, id: NodeId, out: &mut Vec<NodeId>) {
        if is_subject_block(cx, id) {
            out.push(id);
            return;
        }
        if is_scope_change_block(cx, id) || is_bare_example_block(cx, id) {
            return;
        }
        for child in cx.children(id) {
            walk(cx, child, out);
        }
    }

    let mut out = Vec::new();
    for child in cx.children(group) {
        walk(cx, child, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::MultipleSubjects;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_first_of_two_named_subjects() {
        test::<MultipleSubjects>().expect_offense(indoc! {r#"
                describe Foo do
                  subject(:user) { User.new }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not set more than one subject per example group
                  subject(:post) { Post.new }
                end
            "#});
    }

    #[test]
    fn flags_first_of_three_subjects() {
        // `subjects[0...-1]`: all but the last flag.
        test::<MultipleSubjects>().expect_offense(indoc! {r#"
                describe Foo do
                  subject(:a) { 1 }
                  ^^^^^^^^^^^^^^^^^ Do not set more than one subject per example group
                  subject(:b) { 2 }
                  ^^^^^^^^^^^^^^^^^ Do not set more than one subject per example group
                  subject(:c) { 3 }
                end
            "#});
    }

    #[test]
    fn flags_first_of_two_bang_subjects() {
        // `subject!` flags too (upstream skips only autocorrect).
        test::<MultipleSubjects>().expect_offense(indoc! {r#"
                describe Foo do
                  subject!(:a) { 1 }
                  ^^^^^^^^^^^^^^^^^^ Do not set more than one subject per example group
                  subject!(:b) { 2 }
                end
            "#});
    }

    #[test]
    fn flags_first_of_two_unnamed_subjects() {
        test::<MultipleSubjects>().expect_offense(indoc! {r#"
                describe Foo do
                  subject { 1 }
                  ^^^^^^^^^^^^^ Do not set more than one subject per example group
                  subject { 2 }
                end
            "#});
    }

    #[test]
    fn ignores_single_subject() {
        test::<MultipleSubjects>().expect_no_offenses(indoc! {r#"
                describe Foo do
                  subject(:post) { Post.new }
                end
            "#});
    }

    #[test]
    fn ignores_subjects_in_nested_groups() {
        // Each group holds one subject — separate scopes (verified vs
        // 3.7.0).
        test::<MultipleSubjects>().expect_no_offenses(indoc! {r#"
                describe Foo do
                  subject(:a) { 1 }
                  context 'x' do
                    subject(:b) { 2 }
                  end
                end
            "#});
    }

    #[test]
    fn ignores_subjects_in_examples() {
        test::<MultipleSubjects>().expect_no_offenses(indoc! {r#"
                describe Foo do
                  subject(:a) { 1 }
                  it 'x' do
                    subject(:b) { 2 }
                  end
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(MultipleSubjects);
