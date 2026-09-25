//! `RSpec/LeakyConstantDeclaration` — stub constants instead of declaring them.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/LeakyConstantDeclaration
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_casgn` / `on_class` / `on_module` gated by
//!   `inside_describe_block?` (any ancestor block that is a spec group:
//!   example groups or shared groups, bare or `RSpec` receiver) and
//!   `explicit_namespace?` (the assigned scope is nil). Each node type
//!   carries its own message (`Stub constant ...`, `Stub class constant
//!   ...`, `Stub module constant ...`) over the whole node per
//!   `add_offense(node)`. No autocorrect upstream, none here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Casgn`, `Class` and `Module` inside an example group:
//!
//! - `describe { FOO = 1 }` — flagged (whole assignment).
//! - `describe { Foo::BAR = 1 }` — explicit namespace, clean.
//! - `describe { class Foo; end }` — flagged (whole class).
//! - `describe { class Foo::Bar; end }` — explicit namespace, clean.
//! - `describe { module M; end }` — flagged (whole module).
//! - `describe { module Foo::Bar; end }` — explicit namespace, clean.
//! - Top-level `FOO = 1` — no group ancestor, clean.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; stubbing the constant needs human
//! judgement.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::{is_example_group_call, is_rspec_or_bare_receiver};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct LeakyConstantDeclaration;

#[cop(
    name = "RSpec/LeakyConstantDeclaration",
    description = "Checks that no class, module, or constant is declared.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl LeakyConstantDeclaration {
    #[on_node(kind = "casgn")]
    fn check_casgn(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Casgn { scope, .. } = *cx.kind(node) else {
            return;
        };
        // `explicit_namespace?`: a non-nil scope (`Foo::BAR`, `::FOO`)
        // is clean (verified vs 3.7.0).
        if scope != OptNodeId::NONE {
            return;
        }
        if !is_inside_spec_group(cx, node) {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            "Stub constant instead of declaring explicitly.",
            None,
        );
    }

    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Class { name, .. } = *cx.kind(node) else {
            return;
        };
        if has_explicit_namespace(cx, name) {
            return;
        }
        if !is_inside_spec_group(cx, node) {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            "Stub class constant instead of declaring explicitly.",
            None,
        );
    }

    #[on_node(kind = "module")]
    fn check_module(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Module { name, .. } = *cx.kind(node) else {
            return;
        };
        if has_explicit_namespace(cx, name) {
            return;
        }
        if !is_inside_spec_group(cx, node) {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            "Stub module constant instead of declaring explicitly.",
            None,
        );
    }
}

/// `true` when any ancestor `Block` / `Numblock` is a spec group
/// (example groups or shared groups, bare or `RSpec` receiver).
///
/// Mirrors upstream `inside_describe_block?`
/// (`each_ancestor(:block).any? { spec_group? }`).
fn is_inside_spec_group(cx: &Cx<'_>, node: NodeId) -> bool {
    for anc in cx.ancestors(node) {
        let call = match *cx.kind(anc) {
            NodeKind::Block { call, .. } => call,
            NodeKind::Numblock { send, .. } => send,
            _ => continue,
        };
        if is_spec_group_call(cx, call) {
            return true;
        }
    }
    false
}

/// `true` when `call` is a spec-group entrypoint (example groups or
/// shared groups) with a bare or `RSpec` receiver.
///
/// Mirrors upstream `spec_group?`. Same shape as
/// `RSpec/IndexedLet::is_spec_group_call`.
fn is_spec_group_call(cx: &Cx<'_>, call: NodeId) -> bool {
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
    is_example_group_call(cx, call)
        || matches!(
            name,
            "shared_examples" | "shared_examples_for" | "shared_context"
        )
}

/// `true` when a class/module `name` const carries an explicit
/// namespace (`Foo::Bar`, `::Foo`).
fn has_explicit_namespace(cx: &Cx<'_>, name: NodeId) -> bool {
    let NodeKind::Const { scope, .. } = *cx.kind(name) else {
        return false;
    };
    scope != OptNodeId::NONE
}

#[cfg(test)]
mod tests {
    use super::LeakyConstantDeclaration;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_constant_assignment() {
        test::<LeakyConstantDeclaration>().expect_offense(indoc! {r#"
                describe SomeClass do
                  CONSTANT_HERE = 'x'
                  ^^^^^^^^^^^^^^^^^^^ Stub constant instead of declaring explicitly.
                end
            "#});
    }

    #[test]
    fn does_not_flag_scoped_assignment() {
        // `explicit_namespace?` (verified vs 3.7.0).
        test::<LeakyConstantDeclaration>().expect_no_offenses(indoc! {r#"
                describe SomeClass do
                  Foo::BAR = 'x'
                end
            "#});
    }

    #[test]
    fn flags_class_declaration() {
        // Single-line shape: the whole `Class` node range fits one line
        // (same convention as `RSpec/DescribedClassModuleWrapping`).
        test::<LeakyConstantDeclaration>().expect_offense(indoc! {r#"
                describe SomeClass do class FooClass; end; end
                                      ^^^^^^^^^^^^^^^^^^^ Stub class constant instead of declaring explicitly.
            "#});
    }

    #[test]
    fn does_not_flag_scoped_class() {
        test::<LeakyConstantDeclaration>().expect_no_offenses(indoc! {r#"
                describe SomeClass do
                  class Foo::Bar
                  end
                end
            "#});
    }

    #[test]
    fn flags_module_declaration() {
        // Single-line shape, same convention as the class test above.
        test::<LeakyConstantDeclaration>().expect_offense(indoc! {r#"
                describe SomeClass do module SomeModule; end; end
                                      ^^^^^^^^^^^^^^^^^^^^^^ Stub module constant instead of declaring explicitly.
            "#});
    }

    #[test]
    fn does_not_flag_scoped_module() {
        test::<LeakyConstantDeclaration>().expect_no_offenses(indoc! {r#"
                describe SomeClass do
                  module Foo::Bar
                  end
                end
            "#});
    }

    #[test]
    fn does_not_flag_top_level_assignment() {
        test::<LeakyConstantDeclaration>().expect_no_offenses(indoc! {r#"
                CONSTANT_HERE = 'x'
            "#});
    }

    #[test]
    fn flags_assignment_inside_example() {
        // Any ancestor spec group counts, even through `it`
        // (verified vs 3.7.0).
        test::<LeakyConstantDeclaration>().expect_offense(indoc! {r#"
                describe SomeClass do
                  it 'x' do
                    FOO = 1
                    ^^^^^^^ Stub constant instead of declaring explicitly.
                  end
                end
            "#});
    }

    #[test]
    fn flags_assignment_in_shared_group() {
        test::<LeakyConstantDeclaration>().expect_offense(indoc! {r#"
                shared_examples 'x' do
                  FOO = 1
                  ^^^^^^^ Stub constant instead of declaring explicitly.
                end
            "#});
    }

    #[test]
    fn does_not_flag_plain_class_body() {
        test::<LeakyConstantDeclaration>().expect_no_offenses(indoc! {r#"
                class Foo
                  BAR = 1
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(LeakyConstantDeclaration);
