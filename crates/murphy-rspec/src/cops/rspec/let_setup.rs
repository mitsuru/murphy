//! `RSpec/LetSetup` — do not use `let!` for unreferenced test setup.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/LetSetup
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_block`
//!   (`example_or_shared_group_or_including?`: a group block whose call
//!   is an RSpec-or-bare shared/example group send, or a bare
//!   `Includes.all` send). `ExampleGroup#lets` is a scoped search: `let?`
//!   nodes found without crossing nested scope-change blocks
//!   (RSpec-or-bare shared/example groups, bare `Includes.all` blocks)
//!   or bare example blocks — so `let!` inside `it` or a nested group
//!   belongs to that inner scope, while `let!` inside hooks still
//!   belongs to the group. Each scoped `let!` (`let_bang`: block form
//!   with exactly one `Sym` / `Str` arg, or bare-send form with exactly
//!   a name plus a `BlockPass`) whose name is never called as a bare
//!   zero-arg send (`method_called?`: `(send nil? %)` over the whole
//!   group subtree, so uses in nested groups, hooks, or sibling `let`s
//!   count, but `self.name` and `name(args)` do not) flags the `let!`
//!   send per `add_offense(let)` with `Do not use `let!` to setup
//!   objects not referenced in tests.` Detection is at parity;
//!   upstream ships no autocorrect and none is added here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` group calls:
//!
//! - `describe { let!(:a) {}; it { expect(1).to x } }` — unreferenced,
//!   flagged (the `let!` send).
//! - `describe { let!(:a) {}; it { expect(a).to x } }` — referenced,
//!   clean.
//! - `describe { let(:a) {}; it {} }` — plain `let`, clean.
//! - `describe { let!(:a) {}; before { a } }` — hook use counts,
//!   clean.
//! - `describe { it { let!(:a) {} } }` — scoped to the example, clean.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; hoisting setup into `before` needs
//! human judgement.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::{
    is_example_group_call, is_rspec_or_bare_receiver, send_without_block_range,
};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct LetSetup;

#[cop(
    name = "RSpec/LetSetup",
    description = "Checks unreferenced `let!` calls being used for test setup.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl LetSetup {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, .. } = *cx.kind(node) else {
            return;
        };
        if !is_investigated_group(cx, call) {
            return;
        }
        for (send, name) in scoped_let_bangs(cx, node) {
            if is_called(cx, node, &name) {
                continue;
            }
            cx.emit_offense(
                send_without_block_range(cx, send),
                "Do not use `let!` to setup objects not referenced in tests.",
                None,
            );
        }
    }
}

/// `true` when `call` opens an investigated scope per
/// `example_or_shared_group_or_including?`: an RSpec-or-bare
/// shared/example group send, or a bare `Includes.all` send
/// (`it_behaves_like` / `it_should_behave_like` / `include_examples` /
/// `include_context`).
fn is_investigated_group(cx: &Cx<'_>, call: NodeId) -> bool {
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(call)
    else {
        return false;
    };
    let name = cx.symbol_str(method);
    if is_example_group_call(cx, call) {
        return true;
    }
    if matches!(
        name,
        "shared_examples" | "shared_examples_for" | "shared_context"
    ) && is_rspec_or_bare_receiver(cx, receiver)
    {
        return true;
    }
    receiver == OptNodeId::NONE && is_include(name)
}

/// `true` when `name` is an `Includes.all` selector.
fn is_include(name: &str) -> bool {
    matches!(
        name,
        "it_behaves_like" | "it_should_behave_like" | "include_examples" | "include_context"
    )
}

/// `(send, name)` for every `let!` in the group's scope, mirroring
/// `ExampleGroup#lets` + `let_bang`: the scoped search never crosses a
/// nested scope-change block or example block, and each match is a
/// block-form `let!` with exactly one `Sym` / `Str` arg or a bare-send
/// `let!` with exactly a name plus a `BlockPass`.
fn scoped_let_bangs(cx: &Cx<'_>, group: NodeId) -> Vec<(NodeId, String)> {
    let mut out = Vec::new();
    // Tree walk (no duplicates): the group node itself is the
    // investigated block, never a scope boundary; nested boundaries
    // prune their whole subtree per `find_all_in_scope`.
    let mut stack: Vec<NodeId> = cx.children(group);
    while let Some(id) = stack.pop() {
        if is_scope_boundary(cx, id) {
            continue;
        }
        if let Some((send, name)) = let_bang(cx, id) {
            out.push((send, name));
        }
        stack.extend(cx.children(id));
    }
    out
}

/// `true` when `id` is a scope boundary per `scope_change?` /
/// `example?`: a `Block` whose call is an RSpec-or-bare shared/example
/// group, a bare `Includes.all` block, or a bare example block.
/// (`block`-only upstream, so `Numblock` never prunes.)
fn is_scope_boundary(cx: &Cx<'_>, id: NodeId) -> bool {
    let NodeKind::Block { call, .. } = *cx.kind(id) else {
        return false;
    };
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(call)
    else {
        return false;
    };
    let name = cx.symbol_str(method);
    if is_example_group_call(cx, call) {
        return true;
    }
    if matches!(
        name,
        "shared_examples" | "shared_examples_for" | "shared_context"
    ) && is_rspec_or_bare_receiver(cx, receiver)
    {
        return true;
    }
    if receiver != OptNodeId::NONE {
        return false;
    }
    is_include(name) || is_example_name(name)
}

/// `true` when `name` is a bare example selector (`Examples.all`).
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

/// The `(send, name)` pair when `id` is a `let_bang` node, else `None`.
///
/// Block form is `(block (send nil? :let! (sym/str _)) ...)` — exactly
/// one name arg; bare-send form is `(send nil? :let! (sym/str _)
/// block_pass)` — exactly a name plus a `BlockPass` (both verified vs
/// 3.7.0).
fn let_bang(cx: &Cx<'_>, id: NodeId) -> Option<(NodeId, String)> {
    match *cx.kind(id) {
        NodeKind::Block { call, .. } => {
            let NodeKind::Send {
                receiver,
                method,
                args,
            } = *cx.kind(call)
            else {
                return None;
            };
            if receiver != OptNodeId::NONE || cx.symbol_str(method) != "let!" {
                return None;
            }
            let arg_ids = cx.list(args);
            let [only] = arg_ids else {
                return None;
            };
            Some((call, first_arg_name(cx, *only)?))
        }
        NodeKind::Send {
            receiver,
            method,
            args,
        } => {
            if receiver != OptNodeId::NONE || cx.symbol_str(method) != "let!" {
                return None;
            }
            let arg_ids = cx.list(args);
            let [name_arg, blk] = arg_ids else {
                return None;
            };
            if !matches!(*cx.kind(*blk), NodeKind::BlockPass(_)) {
                return None;
            }
            Some((id, first_arg_name(cx, *name_arg)?))
        }
        _ => None,
    }
}

/// The `let` name of a first arg: `Str` content or `Sym` spelling.
fn first_arg_name(cx: &Cx<'_>, arg: NodeId) -> Option<String> {
    match *cx.kind(arg) {
        NodeKind::Str(id) => Some(cx.string_str(id).to_owned()),
        NodeKind::Sym(sym) => Some(cx.symbol_str(sym).to_owned()),
        _ => None,
    }
}

/// Mirrors `method_called?` (`(send nil? %)`): `true` when a bare,
/// zero-arg `name` send occurs anywhere in the group's subtree.
fn is_called(cx: &Cx<'_>, group: NodeId, name: &str) -> bool {
    core::iter::once(group)
        .chain(cx.descendants(group))
        .any(|id| match *cx.kind(id) {
            NodeKind::Send {
                receiver,
                method,
                args,
            } => {
                receiver == OptNodeId::NONE
                    && cx.symbol_str(method) == name
                    && cx.list(args).is_empty()
            }
            _ => false,
        })
}

#[cfg(test)]
mod tests {
    use super::LetSetup;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_unreferenced_let_bang() {
        test::<LetSetup>().expect_offense(indoc! {r#"
                describe 'x' do
                  let!(:widget) { create(:widget) }
                  ^^^^^^^^^^^^^ Do not use `let!` to setup objects not referenced in tests.
                  it 'counts' do
                    expect(Widget.count).to eq(1)
                  end
                end
            "#});
    }

    #[test]
    fn does_not_flag_referenced_let_bang() {
        test::<LetSetup>().expect_no_offenses(indoc! {r#"
                describe 'x' do
                  let!(:widget) { create(:widget) }
                  it 'counts' do
                    expect(widget).to be_present
                  end
                end
            "#});
    }

    #[test]
    fn does_not_flag_plain_let() {
        // Only `let!` is in scope (verified vs 3.7.0).
        test::<LetSetup>().expect_no_offenses(indoc! {r#"
                describe 'x' do
                  let(:widget) { create(:widget) }
                  it 'counts' do
                    expect(Widget.count).to eq(1)
                  end
                end
            "#});
    }

    #[test]
    fn does_not_flag_use_in_hook() {
        // `method_called?` spans the whole subtree (verified vs
        // 3.7.0).
        test::<LetSetup>().expect_no_offenses(indoc! {r#"
                describe 'x' do
                  let!(:widget) { create(:widget) }
                  before { widget.do_something }
                  it 'counts' do
                    expect(Widget.count).to eq(1)
                  end
                end
            "#});
    }

    #[test]
    fn does_not_flag_use_in_nested_group() {
        test::<LetSetup>().expect_no_offenses(indoc! {r#"
                describe 'x' do
                  let!(:widget) { create(:widget) }
                  context 'y' do
                    it 'counts' do
                      expect(widget).to be_present
                    end
                  end
                end
            "#});
    }

    #[test]
    fn does_not_flag_let_bang_inside_example() {
        // Scoped to the example, invisible to the group (verified vs
        // 3.7.0).
        test::<LetSetup>().expect_no_offenses(indoc! {r#"
                describe 'x' do
                  it 'counts' do
                    let!(:widget) { create(:widget) }
                  end
                end
            "#});
    }

    #[test]
    fn flags_bare_send_form() {
        // `(send nil? :let! (sym _) block_pass)` arm (verified vs
        // 3.7.0). The offense is the send itself.
        test::<LetSetup>().expect_offense(indoc! {r#"
                describe 'x' do
                  let!(:widget, &blk)
                  ^^^^^^^^^^^^^^^^^^^ Do not use `let!` to setup objects not referenced in tests.
                  it 'counts' do
                    expect(1).to eq(1)
                  end
                end
            "#});
    }

    #[test]
    fn flags_when_only_self_use() {
        // `method_called?` requires a bare send: `self.widget` never
        // counts as a use (verified vs 3.7.0).
        test::<LetSetup>().expect_offense(indoc! {r#"
                describe 'x' do
                  let!(:widget) { create(:widget) }
                  ^^^^^^^^^^^^^ Do not use `let!` to setup objects not referenced in tests.
                  it 'counts' do
                    self.widget.do_something
                  end
                end
            "#});
    }

    #[test]
    fn flags_string_name() {
        test::<LetSetup>().expect_offense(indoc! {r#"
                describe 'x' do
                  let!('widget') { create(:widget) }
                  ^^^^^^^^^^^^^^ Do not use `let!` to setup objects not referenced in tests.
                end
            "#});
    }

    #[test]
    fn flags_shared_group_let_bang() {
        test::<LetSetup>().expect_offense(indoc! {r#"
                shared_examples 'x' do
                  let!(:widget) { create(:widget) }
                  ^^^^^^^^^^^^^ Do not use `let!` to setup objects not referenced in tests.
                  it 'counts' do
                    expect(1).to eq(1)
                  end
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(LetSetup);
