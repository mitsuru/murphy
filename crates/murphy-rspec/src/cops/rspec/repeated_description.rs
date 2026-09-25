//! `RSpec/RepeatedDescription` — do not repeat descriptions in a group.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/RepeatedDescription
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_block` (`example_group?`: bare or `RSpec`
//!   receiver): `ExampleGroup#examples` in scope (bare `example?` blocks,
//!   stopping at nested groups, includes, and examples) split into
//!   non-`its` grouped by `[metadata, doc_string]` and `its` grouped by
//!   `[doc_string, example]`. Non-`its` examples with no doc string and
//!   no metadata (`[nil, nil]`, e.g. `it { foo }`) are skipped via the
//!   `signatures.any?` guard. Doc strings compare by value (`Str` value,
//!   `Sym` name, so `'a'` vs `"a"` match) and metadata by source text;
//!   `its` bodies compare by source text. Non-`its` offenses land on the
//!   definition send (trimmed via `send_without_block_range` since
//!   Murphy's `Send` covers the block); `its` offenses land on the whole
//!   block per `add_offense(its)` (`to_node`). Detection is at parity
//!   for the common shapes (verified vs 3.7.0, including metadata,
//!   `its`, and nested-scope cases); whitespace/quote-normalisation
//!   gaps of source-text comparison (e.g. `foo(1)` vs `foo( 1 )` bodies
//!   for `its`) remain — same `raw_source` convention as
//!   `RSpec/IdenticalEqualityAssertion` (status: partial, comparison as
//!   gap). No autocorrect upstream, none here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` example groups:
//!
//! - Two `it 'is valid'` in one group — both definitions flagged.
//! - `it 'is valid'` vs `it 'is valid', :flag` — metadata differs, clean.
//! - Two `it { foo }` (no doc) — skipped, clean.
//! - Two `its(:a) { b }` — both blocks flagged.
//! - `its(:a) { b }` vs `its(:a) { c }` — bodies differ, clean.
//! - Same description in a nested group — separate scopes, clean.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; rewording needs human judgement.

use std::collections::BTreeMap;

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::{
    is_bare_example_block, is_example_group_call, is_scope_change_block,
    send_without_block_range,
};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct RepeatedDescription;

#[cop(
    name = "RSpec/RepeatedDescription",
    description = "Check for repeated description strings in example groups.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl RepeatedDescription {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, .. } = *cx.kind(node) else {
            return;
        };
        if !is_example_group_call(cx, call) {
            return;
        }
        let examples = examples_in_scope(cx, node);
        // Split `its` from the rest, mirroring upstream.
        let mut plain: Vec<NodeId> = Vec::new();
        let mut its: Vec<NodeId> = Vec::new();
        for ex in examples {
            if is_its_block(cx, ex) {
                its.push(ex);
            } else {
                plain.push(ex);
            }
        }
        // Non-`its`: group by (doc, metadata), skip empty signatures.
        let mut groups: BTreeMap<String, Vec<NodeId>> = BTreeMap::new();
        for ex in plain {
            let Some(key) = plain_key(cx, ex) else {
                continue;
            };
            groups.entry(key).or_default().push(ex);
        }
        for members in groups.values() {
            if members.len() < 2 {
                continue;
            }
            for &ex in members {
                let def = definition_send(cx, ex);
                cx.emit_offense(
                    send_without_block_range(cx, def),
                    "Don't repeat descriptions within an example group.",
                    None,
                );
            }
        }
        // `its`: group by (doc, body).
        let mut its_groups: BTreeMap<String, Vec<NodeId>> = BTreeMap::new();
        for ex in its {
            let key = its_key(cx, ex);
            its_groups.entry(key).or_default().push(ex);
        }
        for members in its_groups.values() {
            if members.len() < 2 {
                continue;
            }
            for &ex in members {
                cx.emit_offense(
                    cx.range(ex),
                    "Don't repeat descriptions within an example group.",
                    None,
                );
            }
        }
    }
}

/// Example blocks in scope, in document order.
///
/// Mirrors `ExampleGroup#examples` via `#find_all_in_scope` (matched
/// blocks are returned without descending into them; nested groups,
/// includes, and examples stop the search).
fn examples_in_scope(cx: &Cx<'_>, group: NodeId) -> Vec<NodeId> {
    fn walk(cx: &Cx<'_>, id: NodeId, out: &mut Vec<NodeId>) {
        if is_bare_example_block(cx, id) {
            out.push(id);
            return;
        }
        if is_scope_change_block(cx, id) {
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

fn is_its_block(cx: &Cx<'_>, id: NodeId) -> bool {
    let NodeKind::Block { call, .. } = *cx.kind(id) else {
        return false;
    };
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(call)
    else {
        return false;
    };
    receiver == OptNodeId::NONE && cx.symbol_str(method) == "its"
}

fn definition_send(cx: &Cx<'_>, example: NodeId) -> NodeId {
    let NodeKind::Block { call, .. } = *cx.kind(example) else {
        return example;
    };
    call
}

/// Grouping key for non-`its` examples, or `None` for empty signatures.
///
/// Mirrors `example_signature` (`[metadata, doc_string]`) with the
/// `signatures.any?` guard: `None` when there is no doc string and no
/// metadata (e.g. `it { foo }`).
fn plain_key(cx: &Cx<'_>, example: NodeId) -> Option<String> {
    let def = definition_send(cx, example);
    let NodeKind::Send { args, .. } = *cx.kind(def) else {
        return None;
    };
    let arg_ids = cx.list(args);
    let &doc = arg_ids.first()?;
    let doc_key = doc_value_key(cx, doc);
    let mut meta_keys: Vec<String> = Vec::new();
    for &m in &arg_ids[1..] {
        meta_keys.push(cx.raw_source(cx.range(m)).to_owned());
    }
    // `[nil, nil]` guard: no doc value and no metadata. A doc node is
    // always present here (first arg exists), but a non-literal doc
    // (e.g. a bare `send`) still counts as a description — only the
    // zero-arg shape returns `None` above. Keep the guard for parity
    // with `signatures.any?` on empty metadata + dynamic doc.
    if meta_keys.is_empty() && doc_key == "dyn:<empty>" {
        return None;
    }
    Some(format!("doc:{doc_key}\nmeta:{}", meta_keys.join("\n")))
}

/// Value-based doc key: `Str` by string value (so `'a'` vs `"a"`
/// match), `Sym` by name, otherwise by source text.
fn doc_value_key(cx: &Cx<'_>, id: NodeId) -> String {
    match *cx.kind(id) {
        NodeKind::Str(s) => format!("str:{}", cx.string_str(s)),
        NodeKind::Sym(s) => format!("sym:{}", cx.symbol_str(s)),
        _ => format!("dyn:{}", cx.raw_source(cx.range(id))),
    }
}

/// Grouping key for `its` examples: doc plus body source.
fn its_key(cx: &Cx<'_>, example: NodeId) -> String {
    let NodeKind::Block { call, body, .. } = *cx.kind(example) else {
        return format!("blk:{}", cx.raw_source(cx.range(example)));
    };
    let NodeKind::Send { args, .. } = *cx.kind(call) else {
        return format!("blk:{}", cx.raw_source(cx.range(example)));
    };
    let arg_ids = cx.list(args);
    let doc = arg_ids
        .first()
        .map(|&d| doc_value_key(cx, d))
        .unwrap_or_default();
    let body_src = body
        .get()
        .map(|b| cx.raw_source(cx.range(b)).to_owned())
        .unwrap_or_default();
    format!("its:{doc}\nbody:{body_src}")
}

#[cfg(test)]
mod tests {
    use super::RepeatedDescription;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_repeated_descriptions() {
        test::<RepeatedDescription>().expect_offense(indoc! {r#"
                RSpec.describe User do
                  it 'is valid' do
                  ^^^^^^^^^^^^^ Don't repeat descriptions within an example group.
                  end
                  it 'is valid' do
                  ^^^^^^^^^^^^^ Don't repeat descriptions within an example group.
                  end
                end
            "#});
    }

    #[test]
    fn ignores_different_metadata() {
        // Same doc but `:flag` metadata differs (verified vs 3.7.0).
        test::<RepeatedDescription>().expect_no_offenses(indoc! {r#"
                RSpec.describe User do
                  it 'is valid' do
                  end
                  it 'is valid', :flag do
                  end
                end
            "#});
    }

    #[test]
    fn ignores_distinct_descriptions() {
        test::<RepeatedDescription>().expect_no_offenses(indoc! {r#"
                RSpec.describe User do
                  it 'a' do
                  end
                  it 'b' do
                  end
                end
            "#});
    }

    #[test]
    fn ignores_bare_its_without_doc() {
        // `it { foo }` has no doc/metadata — `signatures.any?` guard.
        test::<RepeatedDescription>().expect_no_offenses(indoc! {r#"
                RSpec.describe User do
                  it { foo }
                  it { foo }
                end
            "#});
    }

    #[test]
    fn flags_repeated_its() {
        test::<RepeatedDescription>().expect_offense(indoc! {r#"
                RSpec.describe User do
                  its(:a) { b }
                  ^^^^^^^^^^^^^ Don't repeat descriptions within an example group.
                  its(:a) { b }
                  ^^^^^^^^^^^^^ Don't repeat descriptions within an example group.
                end
            "#});
    }

    #[test]
    fn ignores_its_with_different_bodies() {
        test::<RepeatedDescription>().expect_no_offenses(indoc! {r#"
                RSpec.describe User do
                  its(:a) { b }
                  its(:a) { c }
                end
            "#});
    }

    #[test]
    fn ignores_nested_scopes() {
        // Same description in a nested group is a separate scope.
        test::<RepeatedDescription>().expect_no_offenses(indoc! {r#"
                RSpec.describe User do
                  it 'is valid' do
                  end
                  context 'x' do
                    it 'is valid' do
                    end
                  end
                end
            "#});
    }

    #[test]
    fn flags_single_quote_vs_double_quote() {
        // Doc strings compare by value, not source (verified vs 3.7.0).
        test::<RepeatedDescription>().expect_offense(indoc! {r#"
                RSpec.describe User do
                  it 'a' do
                  ^^^^^^ Don't repeat descriptions within an example group.
                  end
                  it "a" do
                  ^^^^^^ Don't repeat descriptions within an example group.
                  end
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(RepeatedDescription);
