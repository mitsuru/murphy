//! `RSpec/OverwritingSetup` — no duplicate `let` / `subject` definitions.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/OverwritingSetup
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_block` (`example_group_with_body?`: bare or
//!   `RSpec`-receiver `ExampleGroups.all` with a non-empty body — shared
//!   groups excluded): direct body children that are `setup?` blocks
//!   (bare `let` / `let!` / `subject` / `subject!`) with all-basic-literal
//!   args group by first-arg name (`Str` / `Sym` value, else `:subject`
//!   for unnamed) and every repeat flags the whole block per
//!   `add_offense(duplicate)` with `` `<name>` is already defined. ``
//!   (verified vs 3.7.0, including `subject!`/`let!`/string-arg shapes
//!   and the shared-group/nested-scope clean cases). No autocorrect
//!   upstream, none here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` whose call is an example-group entrypoint with
//! a body:
//!
//! - Two `let(:foo)` in one group — the second flagged.
//! - `subject(:foo)` then `let(:foo)` — the second flagged.
//! - `let(:foo)` then `let!(:foo)` — the second flagged.
//! - Two unnamed `subject` — the second flagged as `` `subject` ``.
//! - Distinct names — clean.
//! - Duplicates in nested groups — separate scopes, clean.
//! - Shared groups — not example groups, clean.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; merging the setups needs human
//! judgement about which definition should win.

use std::collections::HashSet;

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::is_example_group_call;

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct OverwritingSetup;

#[cop(
    name = "RSpec/OverwritingSetup",
    description = "Checks if there is a `let`/`subject` that overwrites an existing one.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl OverwritingSetup {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, body, .. } = *cx.kind(node) else {
            return;
        };
        if !is_example_group_call(cx, call) {
            return;
        }
        let Some(body_id) = body.get() else {
            return;
        };
        // Upstream `node.body.each_child_node(:block)`: direct children
        // only — a multi-statement body is a `Begin`, otherwise the
        // single body node itself.
        let children: Vec<NodeId> = match *cx.kind(body_id) {
            NodeKind::Begin(members) => cx.list(members).to_vec(),
            _ => vec![body_id],
        };
        let mut seen: HashSet<String> = HashSet::new();
        for child in children {
            if !matches!(*cx.kind(child), NodeKind::Block { .. }) {
                continue;
            }
            if !is_common_setup(cx, child) {
                continue;
            }
            let name = setup_name(cx, child);
            if !seen.insert(name.clone()) {
                cx.emit_offense(
                    cx.range(child),
                    &format!("`{name}` is already defined."),
                    None,
                );
            }
        }
    }
}

/// `true` when `id` is a `setup?` block with all-basic-literal args.
///
/// Mirrors upstream `setup?` (`(block (send nil? {#Helpers.all
/// #Subjects.all} ...) ...)`) plus the `basic_literal?` guard:
/// `Helpers.all` is `let` / `let!`, `Subjects.all` is `subject` /
/// `subject!`, all with a bare receiver. Basic literals are the
/// non-composite literal node kinds (`Sym` / `Str` / `Int` / `Float` /
/// `True` / `False` / `Nil`); dynamic nodes (`Dstr` / `Dsym` / `Array` /
/// `Hash` / ...) fail the guard exactly like upstream's
/// `BASIC_LITERALS` exclusion of composite literals.
fn is_common_setup(cx: &Cx<'_>, id: NodeId) -> bool {
    let NodeKind::Block { call, .. } = *cx.kind(id) else {
        return false;
    };
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(call)
    else {
        return false;
    };
    if receiver != OptNodeId::NONE {
        return false;
    }
    if !matches!(
        cx.symbol_str(method),
        "let" | "let!" | "subject" | "subject!"
    ) {
        return false;
    }
    cx.list(args)
        .iter()
        .copied()
        .all(|arg| is_basic_literal(cx, arg))
}

/// `true` for non-composite literal nodes (upstream `basic_literal?`).
fn is_basic_literal(cx: &Cx<'_>, id: NodeId) -> bool {
    matches!(
        *cx.kind(id),
        NodeKind::Sym(_)
            | NodeKind::Str(_)
            | NodeKind::Int(_)
            | NodeKind::Float(_)
            | NodeKind::True_
            | NodeKind::False_
            | NodeKind::Nil
    )
}

/// The dedupe name for a setup block: the first `Str` / `Sym` argument
/// value, else `:subject` for unnamed definitions.
///
/// Mirrors upstream `first_argument_name` (`(send _ _ ({str sym} $_))`)
/// with `.to_sym`, falling back to `:subject` when the send has no
/// arguments.
fn setup_name(cx: &Cx<'_>, id: NodeId) -> String {
    let NodeKind::Block { call, .. } = *cx.kind(id) else {
        return "subject".to_owned();
    };
    let NodeKind::Send { args, .. } = *cx.kind(call) else {
        return "subject".to_owned();
    };
    let Some(&first) = cx.list(args).first() else {
        return "subject".to_owned();
    };
    match *cx.kind(first) {
        NodeKind::Sym(sym) => cx.symbol_str(sym).to_owned(),
        NodeKind::Str(str_id) => cx.string_str(str_id).to_owned(),
        _ => "subject".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::OverwritingSetup;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_duplicate_let() {
        test::<OverwritingSetup>().expect_offense(indoc! {r#"
                describe Foo do
                  let(:foo) { bar }
                  let(:foo) { baz }
                  ^^^^^^^^^^^^^^^^^ `foo` is already defined.
                end
            "#});
    }

    #[test]
    fn flags_subject_then_let_with_same_name() {
        test::<OverwritingSetup>().expect_offense(indoc! {r#"
                describe Foo do
                  subject(:foo) { bar }
                  let(:foo) { baz }
                  ^^^^^^^^^^^^^^^^^ `foo` is already defined.
                end
            "#});
    }

    #[test]
    fn flags_let_then_let_bang_with_same_name() {
        test::<OverwritingSetup>().expect_offense(indoc! {r#"
                describe Foo do
                  let(:foo) { bar }
                  let!(:foo) { baz }
                  ^^^^^^^^^^^^^^^^^^ `foo` is already defined.
                end
            "#});
    }

    #[test]
    fn flags_duplicate_unnamed_subjects() {
        test::<OverwritingSetup>().expect_offense(indoc! {r#"
                describe Foo do
                  subject { bar }
                  subject { baz }
                  ^^^^^^^^^^^^^^^ `subject` is already defined.
                end
            "#});
    }

    #[test]
    fn flags_duplicate_string_args() {
        test::<OverwritingSetup>().expect_offense(indoc! {r#"
                describe Foo do
                  let('foo') { bar }
                  let('foo') { baz }
                  ^^^^^^^^^^^^^^^^^^ `foo` is already defined.
                end
            "#});
    }

    #[test]
    fn ignores_distinct_names() {
        test::<OverwritingSetup>().expect_no_offenses(indoc! {r#"
                describe Foo do
                  subject(:test) { something }
                  let(:foo) { bar }
                  let(:baz) { baz }
                end
            "#});
    }

    #[test]
    fn ignores_shared_groups() {
        // `example_group_with_body?` covers example groups only
        // (verified vs 3.7.0).
        test::<OverwritingSetup>().expect_no_offenses(indoc! {r#"
                shared_examples 'x' do
                  let(:foo) { bar }
                  let(:foo) { baz }
                end
            "#});
    }

    #[test]
    fn ignores_nested_scopes() {
        // Each group holds one `let(:foo)` — separate scopes (verified
        // vs 3.7.0).
        test::<OverwritingSetup>().expect_no_offenses(indoc! {r#"
                describe Foo do
                  let(:foo) { bar }
                  context 'x' do
                    let(:foo) { baz }
                  end
                end
            "#});
    }

    #[test]
    fn ignores_non_literal_args() {
        // Non-basic-literal args fail `common_setup?` (verified vs
        // 3.7.0).
        test::<OverwritingSetup>().expect_no_offenses(indoc! {r#"
                describe Foo do
                  let(dynamic) { bar }
                  let(dynamic) { baz }
                end
            "#});
    }

    #[test]
    fn ignores_empty_group() {
        test::<OverwritingSetup>().expect_no_offenses(indoc! {r#"
                describe Foo do
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(OverwritingSetup);
