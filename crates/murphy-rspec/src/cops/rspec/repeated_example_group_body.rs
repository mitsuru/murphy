//! `RSpec/RepeatedExampleGroupBody` — do not repeat group bodies.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/RepeatedExampleGroupBody
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_begin` (`several_example_groups?`: a `Begin`
//!   with two or more `example_group_with_body?` children — bare or
//!   `RSpec`-receiver `ExampleGroups.all` with a non-empty body):
//!   sibling groups are rejected when they contain `skip` / `pending`
//!   (`skip_or_pending_inside_block?`) and grouped by `signature_keys`
//!   (`[metadata, body, const_arg]` where metadata is args after the
//!   first, body is the block body, const_arg is the first arg when it
//!   is a `Const`). All three compare by source text. Groups of two or
//!   more flag the whole group block per `add_offense(group)` with
//!   `Repeated <method> block body on line(s) <others>`. Detection is at
//!   parity for the common shapes (verified vs 3.7.0, including metadata,
//!   const-arg, and skip cases); whitespace/comment normalisation gaps of
//!   source-text comparison remain — same `raw_source` convention as
//!   `RSpec/IdenticalEqualityAssertion` (status: partial, comparison as
//!   gap). No autocorrect upstream, none here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Begin` with sibling groups:
//!
//! - Two `describe` with same body, different string docs — both flagged.
//! - Different bodies — clean.
//! - Same body but `:tag` metadata differs — clean.
//! - `context Array` vs `context Hash` same body — const differs, clean.
//! - Group containing `skip` — excluded, clean.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; deduplicating groups needs human
//! judgement.

use std::collections::BTreeMap;

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::{is_example_group_name, is_rspec_or_bare_receiver, line_index_of_offset};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct RepeatedExampleGroupBody;

#[cop(
    name = "RSpec/RepeatedExampleGroupBody",
    description = "Check for repeated describe and context block body.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl RepeatedExampleGroupBody {
    #[on_node(kind = "begin")]
    fn check_begin(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Begin(members) = *cx.kind(node) else {
            return;
        };
        let children = cx.list(members).to_vec();
        let groups: Vec<NodeId> = children
            .into_iter()
            .filter(|&id| is_group_with_body(cx, id) && !has_skip_or_pending(cx, id))
            .collect();
        if groups.len() < 2 {
            return;
        }
        let mut by_sig: BTreeMap<String, Vec<NodeId>> = BTreeMap::new();
        for g in groups {
            let key = signature_key(cx, g);
            by_sig.entry(key).or_default().push(g);
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
                    "Repeated {method} block body on line(s) [{}]",
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

/// `true` when `id` is an example-group `Block` with a non-empty body.
///
/// Mirrors `example_group_with_body?`
/// (`(block (send #rspec? #ExampleGroups.all ...) args !nil?)`).
fn is_group_with_body(cx: &Cx<'_>, id: NodeId) -> bool {
    let NodeKind::Block { call, body, .. } = *cx.kind(id) else {
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
    if !is_example_group_name(cx.symbol_str(method)) {
        return false;
    }
    body.get().is_some()
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

/// Grouping key: metadata + body + const-arg, all by source text.
fn signature_key(cx: &Cx<'_>, group: NodeId) -> String {
    let NodeKind::Block { call, body, .. } = *cx.kind(group) else {
        return format!("blk:{}", cx.raw_source(cx.range(group)));
    };
    let NodeKind::Send { args, .. } = *cx.kind(call) else {
        return format!("blk:{}", cx.raw_source(cx.range(group)));
    };
    let arg_ids = cx.list(args);
    let meta_srcs: Vec<String> = if arg_ids.is_empty() {
        Vec::new()
    } else {
        arg_ids[1..]
            .iter()
            .map(|&m| cx.raw_source(cx.range(m)).to_owned())
            .collect()
    };
    let body_src = body
        .get()
        .map(|b| cx.raw_source(cx.range(b)).to_owned())
        .unwrap_or_default();
    let const_src = arg_ids
        .first()
        .filter(|&&first| matches!(*cx.kind(first), NodeKind::Const { .. }))
        .map(|&first| cx.raw_source(cx.range(first)).to_owned())
        .unwrap_or_default();
    format!(
        "meta:{}\nbody:{body_src}\nconst:{const_src}",
        meta_srcs.join("\n")
    )
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
    use super::RepeatedExampleGroupBody;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_repeated_bodies() {
        // Brace single-line groups keep the whole-block offense on one
        // line for caret annotation.
        test::<RepeatedExampleGroupBody>().expect_offense(indoc! {r#"
                describe('x') { it { foo } }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Repeated describe block body on line(s) [2]
                describe('y') { it { foo } }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Repeated describe block body on line(s) [1]
            "#});
    }

    #[test]
    fn ignores_different_bodies() {
        test::<RepeatedExampleGroupBody>().expect_no_offenses(indoc! {r#"
                describe 'x' do
                  it { foo }
                end
                describe 'y' do
                  it { bar }
                end
            "#});
    }

    #[test]
    fn ignores_metadata_difference() {
        // `:tag` metadata is part of the signature (verified vs 3.7.0).
        test::<RepeatedExampleGroupBody>().expect_no_offenses(indoc! {r#"
                context 'x', :tag do
                  it { foo }
                end
                context 'y' do
                  it { foo }
                end
            "#});
    }

    #[test]
    fn ignores_const_difference() {
        // `Array` vs `Hash` first-arg const differs (verified vs 3.7.0).
        test::<RepeatedExampleGroupBody>().expect_no_offenses(indoc! {r#"
                context Array do
                  it { is_expected.to respond_to :each }
                end
                context Hash do
                  it { is_expected.to respond_to :each }
                end
            "#});
    }

    #[test]
    fn ignores_single_group() {
        test::<RepeatedExampleGroupBody>().expect_no_offenses(indoc! {r#"
                describe 'x' do
                  it { foo }
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(RepeatedExampleGroupBody);
