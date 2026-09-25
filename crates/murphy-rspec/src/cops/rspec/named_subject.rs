//! `RSpec/NamedSubject` — name the subject when referencing it explicitly.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/NamedSubject
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_block` (`example_or_hook_block?`: bare
//!   `Examples.all` / `Hooks.all`): every bare `subject` send inside
//!   flags its selector per `add_offense(node.loc.selector)` with `Name
//!   your test subject if you need to reference it explicitly.`.
//!   `EnforcedStyle: always` (default) always flags; `named_only` flags
//!   only when the nearest enclosing subject definition takes arguments
//!   (`nearest_subject` walking `Block` ancestors for a direct
//!   `subject?` child). Usages are zero-arg `(send nil? :subject)` only,
//!   so `subject(:user)` calls never flag. `IgnoreSharedExamples: true`
//!   (default) skips
//!   examples under `shared_examples` / `shared_examples_for` (verified
//!   vs 3.7.0, including hook blocks, bare-subject, and `subject!`
//!   definition cases). No autocorrect upstream, none here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on bare example/hook `Block`s:
//!
//! - `it { expect(subject.foo).to eq(1) }` with unnamed `subject` — the
//!   `subject` selector flagged.
//! - `it { is_expected.to be_valid }` — no explicit `subject`, clean.
//! - `before { subject.foo }` — hook blocks flag too.
//! - Example under `shared_examples` — clean by default.
//!
//! With `EnforcedStyle: named_only`, an unnamed `subject { }`
//! definition stays clean while `subject(:user) { }` still flags.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; naming the subject needs human
//! judgement about what the subject represents.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::{is_hook_name, is_rspec_or_bare_receiver};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct NamedSubject;

#[derive(CopOptions)]
pub struct NamedSubjectOptions {
    #[option(
        name = "EnforcedStyle",
        default = "always",
        description = "Whether explicit subject references always require a named subject or only when the definition is named."
    )]
    pub enforced_style: NamedSubjectStyle,
    #[option(
        name = "IgnoreSharedExamples",
        default = true,
        description = "Whether examples inside shared examples are ignored."
    )]
    pub ignore_shared_examples: bool,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum NamedSubjectStyle {
    #[option(value = "always")]
    Always,
    #[option(value = "named_only")]
    NamedOnly,
}

#[cop(
    name = "RSpec/NamedSubject",
    description = "Checks for explicitly referenced test subjects.",
    default_severity = "warning",
    default_enabled = true,
    options = NamedSubjectOptions
)]
impl NamedSubject {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, .. } = *cx.kind(node) else {
            return;
        };
        let NodeKind::Send {
            receiver,
            method,
            ..
        } = *cx.kind(call)
        else {
            return;
        };
        if receiver != OptNodeId::NONE {
            return;
        }
        let name = cx.symbol_str(method);
        if !is_example_name(name) && !is_hook_name(name) {
            return;
        }
        let opts = cx.options_or_default::<NamedSubjectOptions>();
        if opts.ignore_shared_examples && inside_shared_examples(cx, node) {
            return;
        }
        // Upstream `subject_usage(node)`: every bare `subject` send in
        // the block subtree.
        let usages: Vec<NodeId> = cx
            .descendants(node)
            .into_iter()
            .filter(|&id| is_bare_subject_send(cx, id))
            .collect();
        for usage in usages {
            let flag = opts.enforced_style == NamedSubjectStyle::Always
                || subject_definition_is_named(cx, usage);
            if flag {
                cx.emit_offense(
                    cx.node(usage).loc.name,
                    "Name your test subject if you need to reference it explicitly.",
                    None,
                );
            }
        }
    }
}

/// `true` when `id` is a bare zero-arg `subject` send.
///
/// Mirrors upstream `subject_usage` (`$(send nil? :subject)` — no
/// trailing `...`, so calls with arguments like `subject(:user)` do not
/// match).
fn is_bare_subject_send(cx: &Cx<'_>, id: NodeId) -> bool {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(id)
    else {
        return false;
    };
    receiver == OptNodeId::NONE
        && cx.symbol_str(method) == "subject"
        && cx.list(args).is_empty()
}

/// `true` when `node` sits under a `shared_examples` /
/// `shared_examples_for` group (bare or `RSpec` receiver).
///
/// Mirrors upstream `ignored_shared_example?` with
/// `IgnoreSharedExamples: true` (`shared_example?` covers
/// `SharedGroups.examples` only — `shared_context` does not count).
fn inside_shared_examples(cx: &Cx<'_>, node: NodeId) -> bool {
    cx.ancestors(node).any(|ancestor| {
        let NodeKind::Block { call, .. } = *cx.kind(ancestor) else {
            return false;
        };
        let NodeKind::Send {
            receiver,
            method,
            ..
        } = *cx.kind(call)
        else {
            return false;
        };
        if !is_rspec_or_bare_receiver(cx, receiver) {
            return false;
        }
        matches!(
            cx.symbol_str(method),
            "shared_examples" | "shared_examples_for"
        )
    })
}

/// `true` when the nearest enclosing subject definition takes arguments.
///
/// Mirrors upstream `subject_definition_is_named?` (`nearest_subject`
/// walking `Block` ancestors for a direct `subject?` child whose send
/// `arguments?`).
fn subject_definition_is_named(cx: &Cx<'_>, usage: NodeId) -> bool {
    for ancestor in cx.ancestors(usage) {
        if !matches!(*cx.kind(ancestor), NodeKind::Block { .. }) {
            continue;
        }
        if let Some(subject) = find_subject_child(cx, ancestor) {
            let NodeKind::Block { call, .. } = *cx.kind(subject) else {
                continue;
            };
            let NodeKind::Send { args, .. } = *cx.kind(call) else {
                continue;
            };
            return !cx.list(args).is_empty();
        }
    }
    false
}

/// The first direct subject-definition child of `block`, if any.
///
/// Mirrors upstream `find_subject` (`block_node.body&.child_nodes.find`
/// with `subject?` = bare `subject` / `subject!` blocks).
fn find_subject_child(cx: &Cx<'_>, block: NodeId) -> Option<NodeId> {
    let NodeKind::Block { body, .. } = *cx.kind(block) else {
        return None;
    };
    let body_id = body.get()?;
    let children: Vec<NodeId> = match *cx.kind(body_id) {
        NodeKind::Begin(members) => cx.list(members).to_vec(),
        _ => vec![body_id],
    };
    children.into_iter().find(|&id| {
        let NodeKind::Block { call, .. } = *cx.kind(id) else {
            return false;
        };
        let NodeKind::Send {
            receiver,
            method,
            ..
        } = *cx.kind(call)
        else {
            return false;
        };
        receiver == OptNodeId::NONE
            && matches!(cx.symbol_str(method), "subject" | "subject!")
    })
}

/// `Examples.all` in rubocop-rspec's default config: regular
/// (`it` / `specify` / `example` / `scenario` / `its`), focused
/// (`fit` / `fspecify` / `fexample` / `fscenario` / `focus`), skipped
/// (`xit` / `xspecify` / `xexample` / `xscenario` / `skip`) and pending
/// (`pending`).
fn is_example_name(name: &str) -> bool {
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

#[cfg(test)]
mod tests {
    use super::{NamedSubject, NamedSubjectOptions, NamedSubjectStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn named_only() -> NamedSubjectOptions {
        NamedSubjectOptions {
            enforced_style: NamedSubjectStyle::NamedOnly,
            ignore_shared_examples: true,
        }
    }

    fn no_ignore() -> NamedSubjectOptions {
        NamedSubjectOptions {
            enforced_style: NamedSubjectStyle::Always,
            ignore_shared_examples: false,
        }
    }

    #[test]
    fn flags_explicit_subject_with_unnamed_definition() {
        test::<NamedSubject>().expect_offense(indoc! {r#"
                RSpec.describe User do
                  subject { described_class.new }
                  it 'is valid' do
                    expect(subject.valid?).to be(true)
                           ^^^^^^^ Name your test subject if you need to reference it explicitly.
                  end
                end
            "#});
    }

    #[test]
    fn ignores_is_expected() {
        test::<NamedSubject>().expect_no_offenses(indoc! {r#"
                RSpec.describe User do
                  subject(:user) { described_class.new }
                  it { is_expected.to be_valid }
                end
            "#});
    }

    #[test]
    fn flags_named_definition_in_always_style() {
        test::<NamedSubject>().expect_offense(indoc! {r#"
                RSpec.describe User do
                  subject(:user) { described_class.new }
                  it 'is valid' do
                    expect(subject.valid?).to be(true)
                           ^^^^^^^ Name your test subject if you need to reference it explicitly.
                  end
                end
            "#});
    }

    #[test]
    fn flags_named_definition_in_named_only_style() {
        test::<NamedSubject>()
            .with_options(&named_only())
            .expect_offense(indoc! {r#"
                RSpec.describe User do
                  subject(:user) { described_class.new }
                  it 'is valid' do
                    expect(subject.valid?).to be(true)
                           ^^^^^^^ Name your test subject if you need to reference it explicitly.
                  end
                end
            "#});
    }

    #[test]
    fn ignores_unnamed_definition_in_named_only_style() {
        test::<NamedSubject>()
            .with_options(&named_only())
            .expect_no_offenses(indoc! {r#"
                RSpec.describe User do
                  subject { described_class.new }
                  it 'is valid' do
                    expect(subject.valid?).to be(true)
                  end
                end
            "#});
    }

    #[test]
    fn ignores_missing_definition_in_named_only_style() {
        // No enclosing subject definition: `nearest_subject` is nil
        // (verified vs 3.7.0).
        test::<NamedSubject>()
            .with_options(&named_only())
            .expect_no_offenses(indoc! {r#"
                it 'x' do
                  expect(subject).to eq(1)
                end
            "#});
    }

    #[test]
    fn flags_hook_block_subject() {
        test::<NamedSubject>().expect_offense(indoc! {r#"
                RSpec.describe User do
                  subject { described_class.new }
                  before do
                    subject.foo
                    ^^^^^^^ Name your test subject if you need to reference it explicitly.
                  end
                end
            "#});
    }

    #[test]
    fn ignores_shared_examples_by_default() {
        test::<NamedSubject>().expect_no_offenses(indoc! {r#"
                shared_examples 'x' do
                  subject { 1 }
                  it 'y' do
                    expect(subject).to eq(1)
                  end
                end
            "#});
    }

    #[test]
    fn flags_shared_examples_when_not_ignored() {
        test::<NamedSubject>()
            .with_options(&no_ignore())
            .expect_offense(indoc! {r#"
                shared_examples 'x' do
                  subject { 1 }
                  it 'y' do
                    expect(subject).to eq(1)
                           ^^^^^^^ Name your test subject if you need to reference it explicitly.
                  end
                end
            "#});
    }

    #[test]
    fn ignores_subject_call_with_arguments() {
        // `(send nil? :subject)` has no trailing `...`: `subject(:user)`
        // usages never flag (verified vs 3.7.0).
        test::<NamedSubject>().expect_no_offenses(indoc! {r#"
                RSpec.describe User do
                  subject(:user) { 1 }
                  it 'x' do
                    expect(subject(:user)).to eq(1)
                  end
                end
            "#});
    }

    #[test]
    fn flags_bare_subject_call() {
        test::<NamedSubject>().expect_offense(indoc! {r#"
                RSpec.describe User do
                  subject { 1 }
                  it 'x' do
                    subject
                    ^^^^^^^ Name your test subject if you need to reference it explicitly.
                  end
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(NamedSubject);
