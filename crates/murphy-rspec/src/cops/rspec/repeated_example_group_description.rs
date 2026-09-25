//! `RSpec/RepeatedExampleGroupDescription` — do not repeat group descriptions.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/RepeatedExampleGroupDescription
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_begin` (`several_example_groups?`: a `Begin`
//!   with two or more example groups — bare or `RSpec`-receiver
//!   `ExampleGroups.all`): sibling groups are rejected when they
//!   contain `skip` / `pending` (`skip_or_pending_inside_block?`) and
//!   when they carry no description (`empty_description?`: a call with
//!   no args). Groups are keyed by `doc_string_and_metadata` (every
//!   call arg by source text); the group method name is NOT part of
//!   the key, so `context 'x'` vs `describe 'x'` flag. Groups of two
//!   or more flag the whole group block per `add_offense(group)` with
//!   `Repeated <method> block description on line(s) <others>`.
//!   Detection is at parity for the common shapes (verified vs 3.7.0,
//!   including metadata and cross-method cases); whitespace/comment
//!   normalisation gaps of source-text comparison remain — same
//!   `raw_source` convention as `RSpec/IdenticalEqualityAssertion`
//!   (status: partial, comparison as gap). No autocorrect upstream,
//!   none here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Begin` with sibling groups:
//!
//! - Two `describe 'cool'` — both flagged.
//! - `context 'x'` vs `describe 'x'` — both flagged (method ignored).
//! - Two `describe 'cool'` with different `:tag` metadata — clean.
//! - Two bare `describe do ... end` (no args) — skipped, clean.
//! - Group containing `skip` — excluded, clean.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; rewording needs human judgement.

use std::collections::BTreeMap;

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::{
    is_example_group_name, is_rspec_or_bare_receiver, line_index_of_offset,
};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct RepeatedExampleGroupDescription;

#[cop(
    name = "RSpec/RepeatedExampleGroupDescription",
    description = "Check for repeated example group descriptions.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl RepeatedExampleGroupDescription {
    #[on_node(kind = "begin")]
    fn check_begin(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Begin(members) = *cx.kind(node) else {
            return;
        };
        let children = cx.list(members).to_vec();
        let groups: Vec<NodeId> = children
            .into_iter()
            .filter(|&id| {
                is_example_group(cx, id) && has_description(cx, id) && !has_skip_or_pending(cx, id)
            })
            .collect();
        if groups.len() < 2 {
            return;
        }
        let mut by_sig: BTreeMap<String, Vec<NodeId>> = BTreeMap::new();
        for g in groups {
            by_sig.entry(description_key(cx, g)).or_default().push(g);
        }
        let src = cx.source();
        for members in by_sig.values() {
            if members.len() < 2 {
                continue;
            }
            let lines: Vec<usize> = members
                .iter()
                .map(|&g| line_index_of_offset(src, cx.range(g).start) + 1)
                .collect();
            for (idx, &group) in members.iter().enumerate() {
                let others: Vec<usize> = lines
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != idx)
                    .map(|(_, l)| *l)
                    .collect();
                let method = group_method(cx, group);
                let msg = format!(
                    "Repeated {method} block description on line(s) [{}]",
                    others
                        .iter()
                        .map(|n| n.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                cx.emit_offense(cx.range(group), &msg, None);
            }
        }
    }
}

/// `true` when `id` is an example-group `Block`.
///
/// Mirrors `example_group?` (`(any_block (send #rspec?
/// #ExampleGroups.all ...) ...)`); Murphy has no `Numblock` for these
/// shapes, so `Block` suffices.
fn is_example_group(cx: &Cx<'_>, id: NodeId) -> bool {
    let NodeKind::Block { call, .. } = *cx.kind(id) else {
        return false;
    };
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

/// `true` when the group call carries at least one argument.
///
/// Mirrors `empty_description?` (`(block (send _ _) ...)` matches no
/// args); groups without a description are rejected before grouping.
fn has_description(cx: &Cx<'_>, group: NodeId) -> bool {
    let NodeKind::Block { call, .. } = *cx.kind(group) else {
        return false;
    };
    let NodeKind::Send { args, .. } = *cx.kind(call) else {
        return false;
    };
    !cx.list(args).is_empty()
}

/// `true` when the group body contains a bare `skip` / `pending` send.
///
/// Mirrors `skip_or_pending_inside_block?`
/// (`(block <(send nil? {:skip :pending} ...) ...>)`).
fn has_skip_or_pending(cx: &Cx<'_>, group: NodeId) -> bool {
    for id in cx.descendants(group) {
        if let NodeKind::Send {
            receiver, method, ..
        } = *cx.kind(id)
            && receiver == OptNodeId::NONE
            && matches!(cx.symbol_str(method), "skip" | "pending")
        {
            return true;
        }
    }
    false
}

/// Grouping key: every call arg by source text.
///
/// Mirrors `doc_string_and_metadata` (`(block (send _ _ $_ $...)
/// ...)`); the method name is deliberately excluded so `context 'x'`
/// and `describe 'x'` share a key.
fn description_key(cx: &Cx<'_>, group: NodeId) -> String {
    let NodeKind::Block { call, .. } = *cx.kind(group) else {
        return format!("blk:{}", cx.raw_source(cx.range(group)));
    };
    let NodeKind::Send { args, .. } = *cx.kind(call) else {
        return format!("blk:{}", cx.raw_source(cx.range(group)));
    };
    cx.list(args)
        .iter()
        .map(|&a| cx.raw_source(cx.range(a)).to_owned())
        .collect::<Vec<_>>()
        .join("\n")
}

fn group_method(cx: &Cx<'_>, group: NodeId) -> String {
    let NodeKind::Block { call, .. } = *cx.kind(group) else {
        return "describe".to_owned();
    };
    let NodeKind::Send { method, .. } = *cx.kind(call) else {
        return "describe".to_owned();
    };
    cx.symbol_str(method).to_owned()
}

#[cfg(test)]
mod tests {
    use super::RepeatedExampleGroupDescription;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_repeated_descriptions() {
        // Brace single-line groups keep the whole-block offense on one
        // line for caret annotation (multiline `do...end` ranges
        // cannot be caret-annotated).
        test::<RepeatedExampleGroupDescription>().expect_offense(indoc! {r#"
                describe('cool') { foo }
                ^^^^^^^^^^^^^^^^^^^^^^^^ Repeated describe block description on line(s) [2]
                describe('cool') { foo }
                ^^^^^^^^^^^^^^^^^^^^^^^^ Repeated describe block description on line(s) [1]
            "#});
    }

    #[test]
    fn flags_cross_method_same_description() {
        // The method name is not part of the grouping key upstream.
        test::<RepeatedExampleGroupDescription>().expect_offense(indoc! {r#"
                context('x') { foo }
                ^^^^^^^^^^^^^^^^^^^^ Repeated context block description on line(s) [2]
                describe('x') { foo }
                ^^^^^^^^^^^^^^^^^^^^^ Repeated describe block description on line(s) [1]
            "#});
    }

    #[test]
    fn ignores_different_descriptions() {
        test::<RepeatedExampleGroupDescription>().expect_no_offenses(indoc! {r#"
                describe('cool') { foo }
                describe('another cool') { foo }
            "#});
    }

    #[test]
    fn ignores_metadata_difference() {
        test::<RepeatedExampleGroupDescription>().expect_no_offenses(indoc! {r#"
                describe('cool') { foo }
                describe('cool', :fast) { foo }
            "#});
    }

    #[test]
    fn ignores_empty_descriptions() {
        // `empty_description?`: arg-less groups never group.
        test::<RepeatedExampleGroupDescription>().expect_no_offenses(indoc! {r#"
                describe { foo }
                describe { foo }
            "#});
    }

    #[test]
    fn ignores_groups_with_skip() {
        test::<RepeatedExampleGroupDescription>().expect_no_offenses(indoc! {r#"
                describe('cool') { skip }
                describe('cool') { foo }
            "#});
    }
}

murphy_plugin_api::submit_cop!(RepeatedExampleGroupDescription);
