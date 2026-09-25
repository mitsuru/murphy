//! `RSpec/RepeatedExample` — do not repeat examples in a group.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/RepeatedExample
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_block` (`example_group?`): `ExampleGroup#examples`
//!   in scope grouped by `example_signature` (`[metadata, implementation]`,
//!   plus definition args for `its`). The doc string is NOT part of the
//!   key, so same-body/different-description pairs flag. Bodies and
//!   metadata compare by source text; `its` args join the key. Groups of
//!   two or more flag the whole example block per `add_offense(to_node)`.
//!   Detection is at parity for the common shapes (verified vs 3.7.0);
//!   whitespace/comment normalisation gaps of source-text comparison
//!   remain — same `raw_source` convention as
//!   `RSpec/IdenticalEqualityAssertion` (status: partial, comparison as
//!   gap). No autocorrect upstream, none here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` example groups:
//!
//! - Two `it` with same body, different descriptions — both flagged.
//! - Two `it` with same description, different bodies — clean.
//! - Single example — clean.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; merging examples needs human judgement.

use std::collections::BTreeMap;

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::{is_bare_example_block, is_example_group_call, is_scope_change_block};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct RepeatedExample;

#[cop(
    name = "RSpec/RepeatedExample",
    description = "Check for repeated examples within example groups.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl RepeatedExample {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, .. } = *cx.kind(node) else {
            return;
        };
        if !is_example_group_call(cx, call) {
            return;
        }
        let examples = examples_in_scope(cx, node);
        let mut groups: BTreeMap<String, Vec<NodeId>> = BTreeMap::new();
        for ex in examples {
            let key = example_key(cx, ex);
            groups.entry(key).or_default().push(ex);
        }
        for members in groups.values() {
            if members.len() < 2 {
                continue;
            }
            for &ex in members {
                cx.emit_offense(
                    cx.range(ex),
                    "Don't repeat examples within an example group.",
                    None,
                );
            }
        }
    }
}

/// Example blocks in scope, in document order.
///
/// Mirrors `ExampleGroup#examples` via `#find_all_in_scope`.
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

/// Grouping key: metadata + implementation (+ args for `its`).
fn example_key(cx: &Cx<'_>, example: NodeId) -> String {
    let NodeKind::Block { call, body, .. } = *cx.kind(example) else {
        return format!("blk:{}", cx.raw_source(cx.range(example)));
    };
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(call)
    else {
        return format!("blk:{}", cx.raw_source(cx.range(example)));
    };
    let is_its = receiver == OptNodeId::NONE && cx.symbol_str(method) == "its";
    let arg_ids = cx.list(args);
    // Metadata is everything after the doc string (first arg). With no
    // args there is no doc and no metadata.
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
    if is_its {
        let args_src: Vec<String> = arg_ids
            .iter()
            .map(|&a| cx.raw_source(cx.range(a)).to_owned())
            .collect();
        return format!(
            "meta:{}\nbody:{body_src}\nargs:{}",
            meta_srcs.join("\n"),
            args_src.join("\n")
        );
    }
    format!("meta:{}\nbody:{body_src}", meta_srcs.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::RepeatedExample;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_same_body_different_descriptions() {
        // Brace single-line shapes keep the whole-block offense on one
        // line (multiline `do...end` ranges cannot be caret-annotated).
        test::<RepeatedExample>().expect_offense(indoc! {r#"
                RSpec.describe User do
                  it('is valid') { expect(user).to be_valid }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Don't repeat examples within an example group.
                  it('validates the user') { expect(user).to be_valid }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Don't repeat examples within an example group.
                end
            "#});
    }

    #[test]
    fn flags_same_short_body() {
        test::<RepeatedExample>().expect_offense(indoc! {r#"
                RSpec.describe User do
                  it('a') { foo }
                  ^^^^^^^^^^^^^^^ Don't repeat examples within an example group.
                  it('b') { foo }
                  ^^^^^^^^^^^^^^^ Don't repeat examples within an example group.
                end
            "#});
    }

    #[test]
    fn ignores_different_bodies() {
        test::<RepeatedExample>().expect_no_offenses(indoc! {r#"
                RSpec.describe User do
                  it 'a' do
                    foo
                  end
                  it 'a' do
                    bar
                  end
                end
            "#});
    }

    #[test]
    fn ignores_single_example() {
        test::<RepeatedExample>().expect_no_offenses(indoc! {r#"
                RSpec.describe User do
                  it 'a' do
                    foo
                  end
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(RepeatedExample);
