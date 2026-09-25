//! `RSpec/RepeatedIncludeExample` — do not repeat shared-example includes.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/RepeatedIncludeExample
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_begin` (`several_include_examples?`: a
//!   `Begin` with two or more `include_examples?` children — bare
//!   `Includes.all` sends): children are kept only when every
//!   argument is static (`recursive_literal_or_const?`) via the
//!   shared `is_recursive_literal_or_const` helper, grouped by call
//!   args by source text (`signature_keys`: `item.arguments`), and
//!   groups of two or more flag the whole include send per
//!   `add_offense(item)` with `Repeated include of shared_examples
//!   <name> on line(s) <others>` (`name` is the first-arg source).
//!   Detection is at parity for the common shapes (verified vs
//!   3.7.0, including `it_behaves_like` and `include_context`);
//!   whitespace/comment normalisation gaps of source-text comparison
//!   remain — same `raw_source` convention as
//!   `RSpec/IdenticalEqualityAssertion` (status: partial, comparison
//!   as gap). No autocorrect upstream, none here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Begin` with sibling includes:
//!
//! - Two `include_examples 'cool'` — both flagged.
//! - Two `it_behaves_like 'a', 'thing'` — both flagged.
//! - `it_behaves_like 'a', 'x'` vs `it_behaves_like 'a', 'y'` — clean.
//! - Two `include_examples foo` (dynamic arg) — skipped, clean.
//! - Single include — clean.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; merging includes needs human
//! judgement.

use std::collections::BTreeMap;

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::{
    is_include_name, is_recursive_literal_or_const, line_index_of_offset,
};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct RepeatedIncludeExample;

#[cop(
    name = "RSpec/RepeatedIncludeExample",
    description = "Check for repeated include of shared examples.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl RepeatedIncludeExample {
    #[on_node(kind = "begin")]
    fn check_begin(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Begin(members) = *cx.kind(node) else {
            return;
        };
        let children = cx.list(members).to_vec();
        if children.iter().filter(|&&id| is_include_send(cx, id)).count() < 2 {
            return;
        }
        let mut by_sig: BTreeMap<String, Vec<NodeId>> = BTreeMap::new();
        for id in children {
            if !is_include_send(cx, id) || !has_literal_args(cx, id) {
                continue;
            }
            by_sig.entry(signature_key(cx, id)).or_default().push(id);
        }
        let src = cx.source();
        for members in by_sig.values() {
            if members.len() < 2 {
                continue;
            }
            let lines: Vec<usize> = members
                .iter()
                .map(|&m| line_index_of_offset(src, cx.range(m).start) + 1)
                .collect();
            for (idx, &item) in members.iter().enumerate() {
                let others: Vec<usize> = lines
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != idx)
                    .map(|(_, l)| *l)
                    .collect();
                let name = first_arg_source(cx, item);
                let msg = format!(
                    "Repeated include of shared_examples {name} on line(s) [{}]",
                    others
                        .iter()
                        .map(|n| n.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                cx.emit_offense(cx.range(item), &msg, None);
            }
        }
    }
}

/// `true` when `id` is a bare include send.
///
/// Mirrors `include_examples?` (`(send nil? #Includes.all ...)`).
fn is_include_send(cx: &Cx<'_>, id: NodeId) -> bool {
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(id)
    else {
        return false;
    };
    receiver == OptNodeId::NONE && is_include_name(cx.symbol_str(method))
}

/// `true` when every call argument is a recursive literal or const.
///
/// Mirrors `literal_include_examples?`
/// (`include_examples? && arguments.all?(&:recursive_literal_or_const?)`).
fn has_literal_args(cx: &Cx<'_>, id: NodeId) -> bool {
    let NodeKind::Send { args, .. } = *cx.kind(id) else {
        return false;
    };
    cx.list(args)
        .iter()
        .all(|&a| is_recursive_literal_or_const(cx, a))
}

/// Grouping key: every call arg by source text.
///
/// Mirrors `signature_keys` (`item.arguments` node equality).
fn signature_key(cx: &Cx<'_>, id: NodeId) -> String {
    let NodeKind::Send { args, .. } = *cx.kind(id) else {
        return format!("send:{}", cx.raw_source(cx.range(id)));
    };
    cx.list(args)
        .iter()
        .map(|&a| cx.raw_source(cx.range(a)).to_owned())
        .collect::<Vec<_>>()
        .join("\n")
}

fn first_arg_source(cx: &Cx<'_>, id: NodeId) -> String {
    let NodeKind::Send { args, .. } = *cx.kind(id) else {
        return String::new();
    };
    cx.list(args)
        .first()
        .map(|&a| cx.raw_source(cx.range(a)).to_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::RepeatedIncludeExample;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_repeated_includes() {
        test::<RepeatedIncludeExample>().expect_offense(indoc! {r#"
                describe 'foo' do
                  include_examples 'cool'
                  ^^^^^^^^^^^^^^^^^^^^^^^ Repeated include of shared_examples 'cool' on line(s) [3]
                  include_examples 'cool'
                  ^^^^^^^^^^^^^^^^^^^^^^^ Repeated include of shared_examples 'cool' on line(s) [2]
                end
            "#});
    }

    #[test]
    fn flags_repeated_behaves_like() {
        test::<RepeatedIncludeExample>().expect_offense(indoc! {r#"
                describe 'foo' do
                  it_behaves_like 'a', 'thing'
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Repeated include of shared_examples 'a' on line(s) [3]
                  it_behaves_like 'a', 'thing'
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Repeated include of shared_examples 'a' on line(s) [2]
                end
            "#});
    }

    #[test]
    fn ignores_different_args() {
        test::<RepeatedIncludeExample>().expect_no_offenses(indoc! {r#"
                describe 'foo' do
                  it_behaves_like 'a', 'thing'
                  it_behaves_like 'a', 'person'
                end
            "#});
    }

    #[test]
    fn ignores_dynamic_args() {
        // Non-literal args never group upstream.
        test::<RepeatedIncludeExample>().expect_no_offenses(indoc! {r#"
                describe 'foo' do
                  include_examples foo
                  include_examples foo
                end
            "#});
    }

    #[test]
    fn ignores_single_include() {
        test::<RepeatedIncludeExample>().expect_no_offenses(indoc! {r#"
                describe 'foo' do
                  include_examples 'cool'
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(RepeatedIncludeExample);
