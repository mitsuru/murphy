//! `RSpec/InstanceVariable` — avoid instance variables in specs; use `let`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/InstanceVariable
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_top_level_group` (`TopLevelGroup`: top-level
//!   spec groups — example groups and shared groups, bare or `RSpec`
//!   receiver) with `ivar_usage` (`(ivar _)` reads over the whole group
//!   subtree; assignments are `ivasgn` and never match). Reads inside a
//!   `Class.new` block or a custom-matcher block (`matcher :sym`,
//!   `RSpec::Matchers.define :sym`) are skipped per `valid_usage?`.
//!   With `AssignmentOnly: true` a read flags only when the same name is
//!   assigned (`ivasgn`, including valueless op-assign targets) somewhere
//!   in the group. The offense range is the read node per
//!   `add_offense(ivar)`. No autocorrect upstream, none here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` top-level spec groups:
//!
//! - `describe { before { @foo = [] }; it { expect(@foo).to be_empty } }`
//!   — the read flags; the assignment itself never does.
//! - `describe { it { expect(@bar).to be_empty } }` — unassigned read
//!   still flags by default; clean under `AssignmentOnly: true`.
//! - `describe { Class.new { it { expect(@foo).to x } } }` —
//!   dynamic-class scope, clean.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; hoisting state into `let` needs human
//! judgement.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::{is_example_group_call, is_rspec_or_bare_receiver, is_top_level_block};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct InstanceVariable;

#[derive(CopOptions)]
pub struct InstanceVariableOptions {
    #[option(
        name = "AssignmentOnly",
        default = false,
        description = "Only flag instance variables that are also assigned within the spec."
    )]
    pub assignment_only: bool,
}

#[cop(
    name = "RSpec/InstanceVariable",
    description = "Checks for instance variable usage in specs.",
    default_severity = "warning",
    default_enabled = true,
    options = InstanceVariableOptions,
)]
impl InstanceVariable {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, .. } = *cx.kind(node) else {
            return;
        };
        if !is_top_level_block(cx, node) {
            return;
        }
        if !is_spec_group_call(cx, call) {
            return;
        }
        let opts = cx.options_or_default::<InstanceVariableOptions>();
        for id in core::iter::once(node).chain(cx.descendants(node)) {
            let NodeKind::Ivar(sym) = *cx.kind(id) else {
                continue;
            };
            if valid_usage(cx, id) {
                continue;
            }
            if opts.assignment_only && !is_assigned(cx, node, cx.symbol_str(sym)) {
                continue;
            }
            cx.emit_offense(
                cx.range(id),
                "Avoid instance variables - use let, a method call, or a local variable (if possible).",
                None,
            );
        }
    }
}

/// `true` when `call` is a spec-group entrypoint (example groups or shared
/// groups) with a bare or `RSpec` receiver.
///
/// Mirrors upstream `spec_group?` gated through `TopLevelGroup`.
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

/// Mirrors upstream `valid_usage?`: the read sits inside a dynamic-class
/// (`Class.new do ... end`) or custom-matcher (`matcher :sym`,
/// `RSpec::Matchers.define :sym`) block.
fn valid_usage(cx: &Cx<'_>, ivar: NodeId) -> bool {
    for ancestor in cx.ancestors(ivar) {
        let NodeKind::Block { call, .. } = *cx.kind(ancestor) else {
            continue;
        };
        if is_dynamic_class_block(cx, call) || is_custom_matcher_block(cx, call) {
            return true;
        }
    }
    false
}

/// `(block (send (const nil? :Class) :new ...) ...)`.
fn is_dynamic_class_block(cx: &Cx<'_>, call: NodeId) -> bool {
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(call)
    else {
        return false;
    };
    if cx.symbol_str(method) != "new" {
        return false;
    }
    let Some(recv) = receiver.get() else {
        return false;
    };
    matches!(
        *cx.kind(recv),
        NodeKind::Const { scope, name }
            if scope == OptNodeId::NONE && cx.symbol_str(name) == "Class"
    )
}

/// `(block {(send nil? :matcher sym) (send (const (const nil? :RSpec)
/// :Matchers) :define sym)} ...)`: bare `matcher` or
/// `RSpec::Matchers.define` with a leading `Sym` arg.
fn is_custom_matcher_block(cx: &Cx<'_>, call: NodeId) -> bool {
    let NodeKind::Send {
        receiver, method, args,
    } = *cx.kind(call)
    else {
        return false;
    };
    let arg_ids = cx.list(args);
    let Some(&first) = arg_ids.first() else {
        return false;
    };
    if !matches!(*cx.kind(first), NodeKind::Sym(_)) {
        return false;
    }
    match cx.symbol_str(method) {
        "matcher" => receiver == OptNodeId::NONE,
        "define" => {
            let Some(recv) = receiver.get() else {
                return false;
            };
            let NodeKind::Const { scope, name } = *cx.kind(recv) else {
                return false;
            };
            if cx.symbol_str(name) != "Matchers" {
                return false;
            }
            let Some(inner) = scope.get() else {
                return false;
            };
            matches!(
                *cx.kind(inner),
                NodeKind::Const { scope, name }
                    if scope == OptNodeId::NONE && cx.symbol_str(name) == "RSpec"
            )
        }
        _ => false,
    }
}

/// Mirrors upstream `ivar_assigned?` (`(ivasgn % ...)` over the group):
/// any same-named `Ivasgn` in the subtree, including the valueless
/// targets of `||=` / `&&=` / op-assign.
fn is_assigned(cx: &Cx<'_>, group: NodeId, name: &str) -> bool {
    core::iter::once(group)
        .chain(cx.descendants(group))
        .any(|id| match *cx.kind(id) {
            NodeKind::Ivasgn { name: sym, .. } => cx.symbol_str(sym) == name,
            _ => false,
        })
}

#[cfg(test)]
mod tests {
    use super::{InstanceVariable, InstanceVariableOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    fn assignment_only() -> InstanceVariableOptions {
        InstanceVariableOptions {
            assignment_only: true,
        }
    }

    #[test]
    fn flags_ivar_read() {
        test::<InstanceVariable>().expect_offense(indoc! {r#"
                RSpec.describe MyClass do
                  before { @foo = [] }
                  it { expect(@foo).to be_empty }
                              ^^^^ Avoid instance variables - use let, a method call, or a local variable (if possible).
                end
            "#});
    }

    #[test]
    fn flags_unassigned_read_by_default() {
        test::<InstanceVariable>().expect_offense(indoc! {r#"
                RSpec.describe MyClass do
                  it { expect(@bar).to be_empty }
                              ^^^^ Avoid instance variables - use let, a method call, or a local variable (if possible).
                end
            "#});
    }

    #[test]
    fn assignment_only_allows_unassigned_read() {
        test::<InstanceVariable>()
            .with_options(&assignment_only())
            .expect_no_offenses(indoc! {r#"
                RSpec.describe MyClass do
                  it { expect(@bar).to be_empty }
                end
            "#});
    }

    #[test]
    fn assignment_only_flags_assigned_read() {
        test::<InstanceVariable>()
            .with_options(&assignment_only())
            .expect_offense(indoc! {r#"
                RSpec.describe MyClass do
                  before { @foo = [] }
                  it { expect(@foo).to be_empty }
                              ^^^^ Avoid instance variables - use let, a method call, or a local variable (if possible).
                end
            "#});
    }

    #[test]
    fn does_not_flag_dynamic_class_scope() {
        // `Class.new` blocks are exempt (verified vs 3.7.0).
        test::<InstanceVariable>().expect_no_offenses(indoc! {r#"
                RSpec.describe MyClass do
                  Class.new do
                    it { expect(@foo).to be_empty }
                  end
                end
            "#});
    }

    #[test]
    fn does_not_flag_custom_matcher_scope() {
        test::<InstanceVariable>().expect_no_offenses(indoc! {r#"
                RSpec.describe MyClass do
                  matcher :be_a_multiple_of do
                    match { |actual| expect(@foo).to be_empty }
                  end
                end
            "#});
    }

    #[test]
    fn does_not_flag_outside_group() {
        test::<InstanceVariable>().expect_no_offenses(indoc! {r#"
                @foo = []
                puts @foo
            "#});
    }
}

murphy_plugin_api::submit_cop!(InstanceVariable);
