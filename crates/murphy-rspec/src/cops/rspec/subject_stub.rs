//! `RSpec/SubjectStub` — do not stub methods of the object under test.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/SubjectStub
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_top_level_group` (`TopLevelGroup`: every
//!   top-level spec-group block starts a walk): `find_all_explicit`
//!   collects named `subject(:name)` definitions (`subject?`: bare
//!   `subject` / `subject!` blocks, first `Sym` arg or `:subject`) and
//!   `let(:name)` overrides (`let?`: bare `let` blocks only, never
//!   `let!`) per enclosing example group, and
//!   `find_subject_expectations` walks `Send` / `Def` / `Block` /
//!   `Begin` children carrying the accumulated names (minus overrides,
//!   always plus `:subject`) down the nesting. `message_expectation?`
//!   matches `allow(name)` / `expect(name)` (bare subject reference, no
//!   args) and bare `is_expected` running `to` / `to_not` / `not_to`
//!   with a `receive` / `receive_messages` / `receive_message_chain` /
//!   `have_received` matcher searched in the subtree. The first match
//!   flags the runner send per `add_offense(stub)` with `Do not stub
//!   methods of the object under test.` Detection is at parity
//!   (verified vs 3.7.0, including nested-subject accumulation, the
//!   `let` override, anonymous `subject`, `is_expected`, and
//!   `not_to`); upstream ships no autocorrect and none is added here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on top-level spec-group `Block`s:
//!
//! - `subject(:article) { ... }` + `allow(article).to receive(:x)` —
//!   flagged (the runner send).
//! - `allow(subject).to receive(:x)` with no explicit subject —
//!   flagged (`:subject` is always in scope).
//! - `is_expected.to receive(:x)` — flagged.
//! - `expect(article).to include(x)` — no receive-family matcher,
//!   clean.
//! - `allow(other).to receive(:x)` — not a subject name, clean.
//! - `let(:article)` in a nested group — overrides the outer subject,
//!   `allow(article)` there is clean.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; restructuring the subject needs human
//! judgement.

use std::collections::HashMap;

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::{is_example_group_call, is_spec_group_call, is_top_level_block};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SubjectStub;

#[cop(
    name = "RSpec/SubjectStub",
    description = "Checks for stubbed test subjects.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl SubjectStub {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, .. } = *cx.kind(node) else {
            return;
        };
        // Upstream `TopLevelGroup`: only top-level spec groups start a
        // walk (shared groups included — `on_top_level_group` fires for
        // every top-level `spec_group?`).
        if !is_spec_group_call(cx, call) {
            return;
        }
        if !is_top_level_block(cx, node) {
            return;
        }
        let explicit = find_all_explicit(cx, node, SubjectKind::Subject);
        let overrides = find_all_explicit(cx, node, SubjectKind::Let);
        find_subject_expectations(cx, node, &[], &explicit, &overrides);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SubjectKind {
    Subject,
    Let,
}

/// `find_all_explicit`: every `subject?` / `let?` block below `root`,
/// keyed by its nearest ancestor example-group block.
///
/// `subject?` is a bare `subject` / `subject!` block: a leading `Sym`
/// arg names it, otherwise it is the anonymous `:subject`. `let?` is a
/// bare `let` block with a leading `Sym` arg (`let!` never overrides).
fn find_all_explicit(
    cx: &Cx<'_>,
    root: NodeId,
    kind: SubjectKind,
) -> HashMap<NodeId, Vec<String>> {
    let mut out: HashMap<NodeId, Vec<String>> = HashMap::new();
    for id in cx.descendants(root) {
        let NodeKind::Block { call, .. } = *cx.kind(id) else {
            continue;
        };
        let name = match kind {
            SubjectKind::Subject => subject_block_name(cx, call),
            SubjectKind::Let => let_block_name(cx, call),
        };
        let Some(name) = name else { continue };
        // Nearest ancestor `Block` that is an example group
        // (`each_ancestor(:block).find { example_group? }`).
        let outer = cx
            .ancestors(id)
            .filter(|&anc| matches!(*cx.kind(anc), NodeKind::Block { .. }))
            .find(|&anc| {
                matches!(*cx.kind(anc), NodeKind::Block { call, .. } if is_example_group_call(cx, call))
            });
        let Some(outer) = outer else { continue };
        out.entry(outer).or_default().push(name);
    }
    out
}

/// Bare `subject` / `subject!` call name: leading `Sym` arg, else the
/// anonymous `:subject`.
fn subject_block_name(cx: &Cx<'_>, call: NodeId) -> Option<String> {
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
    let first_sym = cx.list(args).first().and_then(|&first| {
        let NodeKind::Sym(sym) = *cx.kind(first) else {
            return None;
        };
        Some(cx.symbol_str(sym).to_owned())
    });
    Some(first_sym.unwrap_or_else(|| "subject".to_owned()))
}

/// Bare `let(:name)` call name. Upstream `let?` is `let` only.
fn let_block_name(cx: &Cx<'_>, call: NodeId) -> Option<String> {
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
    if cx.symbol_str(method) != "let" {
        return None;
    }
    let &first = cx.list(args).first()?;
    let NodeKind::Sym(sym) = *cx.kind(first) else {
        return None;
    };
    Some(cx.symbol_str(sym).to_owned())
}

/// `find_subject_expectations`: carry the accumulated subject names down
/// `Send` / `Def` / `Block` / `Begin` children (mirroring upstream's
/// `each_child_node(:send, :def, :block, :begin)`); the first
/// `message_expectation?` match flags and prunes that subtree.
fn find_subject_expectations(
    cx: &Cx<'_>,
    node: NodeId,
    inherited: &[String],
    explicit: &HashMap<NodeId, Vec<String>>,
    overrides: &HashMap<NodeId, Vec<String>>,
) {
    let mut names: Vec<String> = inherited.to_vec();
    if let Some(extra) = explicit.get(&node) {
        names.extend(extra.iter().cloned());
    }
    if let Some(dropped) = overrides.get(&node) {
        names.retain(|name| !dropped.contains(name));
    }
    if is_subject_stub(cx, node, &names) {
        cx.emit_offense(
            cx.range(node),
            "Do not stub methods of the object under test.",
            None,
        );
        return;
    }
    for child in cx.children(node) {
        if !matches!(
            *cx.kind(child),
            NodeKind::Send { .. } | NodeKind::Def { .. } | NodeKind::Block { .. } | NodeKind::Begin(_)
        ) {
            continue;
        }
        find_subject_expectations(cx, child, &names, explicit, overrides);
    }
}

/// `message_expectation?`: `allow(name)` / `expect(name)` with a bare,
/// arg-less subject reference, or bare `is_expected`, running a
/// `Runners.all` method (`to` / `to_not` / `not_to`) with a
/// receive-family matcher in the subtree.
fn is_subject_stub(cx: &Cx<'_>, node: NodeId, names: &[String]) -> bool {
    let NodeKind::Send {
        receiver,
        method,
        ..
    } = *cx.kind(node)
    else {
        return false;
    };
    if !matches!(cx.symbol_str(method), "to" | "to_not" | "not_to") {
        return false;
    }
    let Some(recv) = receiver.get() else {
        return false;
    };
    let NodeKind::Send {
        receiver: inner_receiver,
        method: inner_method,
        args: inner_args,
    } = *cx.kind(recv)
    else {
        return false;
    };
    if inner_receiver != OptNodeId::NONE {
        return false;
    }
    let inner_name = cx.symbol_str(inner_method);
    let subject_matched = match inner_name {
        "expect" | "allow" => {
            // `(send nil? {:expect :allow} (send nil? %))` — one bare,
            // arg-less subject reference; `%` must name a subject.
            let inner_arg_ids = cx.list(inner_args);
            if inner_arg_ids.len() != 1 {
                return false;
            }
            let NodeKind::Send {
                receiver: ref_receiver,
                method: ref_method,
                args: ref_args,
            } = *cx.kind(inner_arg_ids[0])
            else {
                return false;
            };
            if ref_receiver != OptNodeId::NONE {
                return false;
            }
            if !cx.list(ref_args).is_empty() {
                return false;
            }
            let reference = cx.symbol_str(ref_method).to_owned();
            names.contains(&reference) || reference == "subject"
        }
        "is_expected" => {
            // `(send nil? :is_expected)` — no args.
            if !cx.list(inner_args).is_empty() {
                return false;
            }
            true
        }
        _ => return false,
    };
    if !subject_matched {
        return false;
    }
    // `#message_expectation_matcher?`: a bare `receive` /
    // `receive_messages` / `receive_message_chain` / `have_received`
    // send searched in the node's subtree.
    core::iter::once(node)
        .chain(cx.descendants(node))
        .any(|id| {
            let NodeKind::Send {
                receiver: sub_receiver,
                method: sub_method,
                ..
            } = *cx.kind(id)
            else {
                return false;
            };
            sub_receiver == OptNodeId::NONE
                && matches!(
                    cx.symbol_str(sub_method),
                    "receive" | "receive_messages" | "receive_message_chain" | "have_received"
                )
        })
}
#[cfg(test)]
mod tests {
    use super::SubjectStub;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_allow_on_named_subject() {
        test::<SubjectStub>().expect_offense(indoc! {r#"
                describe Article do
                  subject(:article) { Article.new }

                  it 'indicates that the author is unknown' do
                    allow(article).to receive(:author).and_return(nil)
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not stub methods of the object under test.
                    expect(article.description).to include('by an unknown author')
                  end
                end
            "#});
    }

    #[test]
    fn flags_allow_on_anonymous_subject() {
        test::<SubjectStub>().expect_offense(indoc! {r#"
                describe Article do
                  it 'x' do
                    allow(subject).to receive(:author)
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not stub methods of the object under test.
                  end
                end
            "#});
    }

    #[test]
    fn flags_is_expected_stub() {
        test::<SubjectStub>().expect_offense(indoc! {r#"
                describe Article do
                  it 'x' do
                    is_expected.to receive(:author)
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not stub methods of the object under test.
                  end
                end
            "#});
    }

    #[test]
    fn ignores_non_subject_stub() {
        test::<SubjectStub>().expect_no_offenses(indoc! {r#"
                describe Article do
                  subject(:article) { Article.new }

                  it 'x' do
                    allow(other).to receive(:author)
                  end
                end
            "#});
    }

    #[test]
    fn ignores_expectation_without_receive_matcher() {
        test::<SubjectStub>().expect_no_offenses(indoc! {r#"
                describe Article do
                  subject(:article) { Article.new }

                  it 'x' do
                    expect(article.description).to include('unknown')
                  end
                end
            "#});
    }

    #[test]
    fn ignores_let_override_in_nested_group() {
        test::<SubjectStub>().expect_no_offenses(indoc! {r#"
                describe Article do
                  subject(:article) { Article.new }

                  context 'nested' do
                    let(:article) { double }

                    it 'x' do
                      allow(article).to receive(:author)
                    end
                  end
                end
            "#});
    }

    #[test]
    fn flags_nested_subjects_accumulated() {
        let offenses = murphy_plugin_api::test_support::run_cop::<SubjectStub>(indoc! {r#"
                describe Article do
                  subject(:article) { Article.new }

                  context 'other' do
                    subject(:piece) { Article.new }

                    it 'y' do
                      allow(article).to receive(:a)
                      allow(piece).to receive(:b)
                    end
                  end
                end
            "#});
        assert_eq!(offenses.len(), 2);
        assert!(
            offenses
                .iter()
                .all(|o| o.message == "Do not stub methods of the object under test.")
        );
    }
}

murphy_plugin_api::submit_cop!(SubjectStub);
