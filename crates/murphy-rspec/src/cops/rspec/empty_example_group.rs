//! `RSpec/EmptyExampleGroup` — flag example groups without any tests.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/EmptyExampleGroup
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_block` (`example_group_body`: `(block (send
//!   #rspec? #ExampleGroups.all ...) args $_)`) with the `any_def` /
//!   `example?`-ancestor guards. A group is offensive when its body is
//!   absent or contains no runnable example: no example / group / include
//!   block (`(block (send #rspec? {#Examples #ExampleGroups #Includes}
//!   ...) ...)`), no bare example / include send (`(send nil?
//!   {#Examples #Includes} ...)`), and no example nested inside a
//!   non-hook block (`examples_inside_block?`: `(block !(send nil?
//!   #Hooks.all ...) _ #examples?)`). Conditionals (`if` / `case`)
//!   count via their branches since the descendant scan reaches them;
//!   heredoc edge handling from `FinalEndLocation` does not apply here.
//!   The offense range is the group `Send` trimmed to exclude a wrapping
//!   block via `send_without_block_range`, matching
//!   `add_offense(node.send_node)`. Detection is at parity for the common
//!   shapes; autocorrect (remove the whole group) is not ported in this
//!   batch — same convention as `RSpec/EmptyHook` (status: partial,
//!   autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` whose call is an RSpec-or-bare example group
//! (`describe`, `context`, `feature`, `example_group` plus focused /
//! skipped variants):
//!
//! - `describe Bacon do; end` — empty, flagged.
//! - `describe Bacon do; let(:x) { y }; end` — only helpers, flagged.
//! - `describe Bacon do; it 'x' do; end; end` — has example, not flagged.
//! - `describe Bacon do; pending 'later'; end` — bare `pending` send
//!   counts as an example, not flagged.
//! - `describe Bacon do; it_behaves_like 'x'; end` — include, not flagged.
//! - `before do; it { x }; end` inside a group — hook body examples do not
//!   count, but the group itself still flags unless it has its own example.
//!
//! ## No autocorrect
//!
//! Upstream removes the whole group. This batch reports only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

use crate::cops::rspec_helpers::{
    is_example_group_name, is_rspec_or_bare_receiver, send_without_block_range,
};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct EmptyExampleGroup;

#[cop(
    name = "RSpec/EmptyExampleGroup",
    description = "Checks if an example group does not include any tests.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl EmptyExampleGroup {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, body, .. } = *cx.kind(node) else {
            return;
        };
        let NodeKind::Send {
            receiver, method, ..
        } = *cx.kind(call)
        else {
            return;
        };
        if !is_rspec_or_bare_receiver(cx, receiver) {
            return;
        }
        if !is_example_group_name(cx.symbol_str(method)) {
            return;
        }
        // Upstream guards: groups inside `def`/`defs` or inside an example
        // block never flag.
        for anc in cx.ancestors(node) {
            match *cx.kind(anc) {
                NodeKind::Def { .. } | NodeKind::Defs { .. } => return,
                NodeKind::Block { call: anc_call, .. } => {
                    if let NodeKind::Send {
                        receiver: r,
                        method: m,
                        ..
                    } = *cx.kind(anc_call)
                        && r == murphy_plugin_api::OptNodeId::NONE
                        && is_example_name(cx.symbol_str(m))
                    {
                        return;
                    }
                }
                _ => {}
            }
        }
        let Some(body_id) = body.get() else {
            cx.emit_offense(
                send_without_block_range(cx, call),
                "Empty example group detected.",
                None,
            );
            return;
        };
        if has_runnable_example(cx, body_id) {
            return;
        }
        cx.emit_offense(
            send_without_block_range(cx, call),
            "Empty example group detected.",
            None,
        );
    }
}

/// `true` when `body` contains a runnable example: an example / group /
/// include block, a bare example / include send, or an example nested
/// inside a non-hook block. Hook subtrees are not descended into,
/// mirroring `examples_inside_block?` (`!(send nil? #Hooks.all ...)`).
fn has_runnable_example(cx: &Cx<'_>, body: NodeId) -> bool {
    // Direct body node itself can be the example (single-statement group).
    if is_runnable_node(cx, body) {
        return true;
    }
    for id in cx.descendants(body) {
        // Do not count examples that only run inside hooks.
        if is_inside_hook(cx, id) {
            continue;
        }
        if is_runnable_node(cx, id) {
            return true;
        }
    }
    false
}

/// `true` when `id` is itself an example / group / include block or a
/// bare example / include send.
fn is_runnable_node(cx: &Cx<'_>, id: NodeId) -> bool {
    match *cx.kind(id) {
        NodeKind::Block { call, .. } => {
            let NodeKind::Send {
                receiver, method, ..
            } = *cx.kind(call)
            else {
                return false;
            };
            if !is_rspec_or_bare_receiver(cx, receiver) {
                return false;
            }
            let name = cx.symbol_str(method);
            is_example_name(name) || is_example_group_name(name) || is_include_name(name)
        }
        NodeKind::Send {
            receiver, method, ..
        } => {
            if receiver != murphy_plugin_api::OptNodeId::NONE {
                return false;
            }
            let name = cx.symbol_str(method);
            is_example_name(name) || is_include_name(name)
        }
        _ => false,
    }
}

/// `true` when `id` sits inside a hook block (`before` / `after` /
/// `around` family, bare receiver).
fn is_inside_hook(cx: &Cx<'_>, id: NodeId) -> bool {
    for anc in cx.ancestors(id) {
        if let NodeKind::Block { call, .. } = *cx.kind(anc)
            && let NodeKind::Send {
                receiver, method, ..
            } = *cx.kind(call)
            && receiver == murphy_plugin_api::OptNodeId::NONE
            && crate::cops::rspec_helpers::is_hook_name(cx.symbol_str(method))
        {
            return true;
        }
    }
    false
}

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

fn is_include_name(name: &str) -> bool {
    matches!(
        name,
        "it_behaves_like" | "it_should_behave_like" | "include_examples" | "include_context"
    )
}

#[cfg(test)]
mod tests {
    use super::EmptyExampleGroup;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_empty_group() {
        test::<EmptyExampleGroup>().expect_offense(indoc! {r#"
                describe Bacon do
                ^^^^^^^^^^^^^^ Empty example group detected.
                end
            "#});
    }

    #[test]
    fn flags_group_with_only_let() {
        test::<EmptyExampleGroup>().expect_offense(indoc! {r#"
                describe Bacon do
                ^^^^^^^^^^^^^^ Empty example group detected.
                  let(:bacon) { Bacon.new }
                end
            "#});
    }

    #[test]
    fn does_not_flag_group_with_example() {
        test::<EmptyExampleGroup>().expect_no_offenses(indoc! {r#"
                describe Bacon do
                  it 'is chunky' do
                    expect(bacon.chunky?).to be_truthy
                  end
                end
            "#});
    }

    #[test]
    fn does_not_flag_pending_send() {
        test::<EmptyExampleGroup>().expect_no_offenses(indoc! {r#"
                describe Bacon do
                  pending 'will add tests later'
                end
            "#});
    }

    #[test]
    fn does_not_flag_include() {
        test::<EmptyExampleGroup>().expect_no_offenses(indoc! {r#"
                describe Bacon do
                  it_behaves_like 'an animal'
                end
            "#});
    }

    #[test]
    fn does_not_flag_nested_group() {
        test::<EmptyExampleGroup>().expect_no_offenses(indoc! {r#"
                describe Bacon do
                  context 'extra chunky' do
                    it 'is chunky' do
                    end
                  end
                end
            "#});
    }

    #[test]
    fn does_not_flag_explicit_receiver_group() {
        // Explicit non-RSpec receivers are some other DSL.
        test::<EmptyExampleGroup>().expect_no_offenses(indoc! {r#"
                Other.describe Bacon do
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(EmptyExampleGroup);
