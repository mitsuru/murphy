//! `RSpec/DescribedClassModuleWrapping` — avoid opening modules and defining specs within them.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/DescribedClassModuleWrapping
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `include_rspec_blocks?`
//!   (`(any_block (send #explicit_rspec? #ExampleGroups.all ...) ...)`)
//!   inside `on_module`: any `Block` / `Numblock` in the module subtree
//!   whose call is an example-group selector (`describe`, `context`,
//!   `feature`, `example_group` plus the focused/skipped variants) with an
//!   explicit `RSpec` / `::RSpec` receiver flags the whole `Module` node
//!   per `add_offense(node)`. A bare `describe` inside a module matches
//!   neither arm upstream and is clean here too. No autocorrect upstream,
//!   none here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Module`. Flags when the subtree contains an explicitly
//! namespaced example group:
//!
//! - `module M; RSpec.describe Foo do; end; end` — flagged (whole module).
//! - `module M; RSpec.context 'x' do; end; end` — flagged.
//! - `module M; describe Foo do; end; end` — bare receiver, not flagged.
//! - `module M; class Foo; end; end` — no example group, not flagged.
//! - Top-level `RSpec.describe Foo` — not inside a module, not flagged.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; hoisting the group out of the module
//! needs human judgement about the constant namespace.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::is_example_group_name;

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct DescribedClassModuleWrapping;

#[cop(
    name = "RSpec/DescribedClassModuleWrapping",
    description = "Avoid opening modules and defining specs within them.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl DescribedClassModuleWrapping {
    #[on_node(kind = "module")]
    fn check_module(&self, node: NodeId, cx: &Cx<'_>) {
        for id in core::iter::once(node).chain(cx.descendants(node)) {
            let call = match *cx.kind(id) {
                NodeKind::Block { call, .. } => call,
                NodeKind::Numblock { send, .. } => send,
                _ => continue,
            };
            if is_explicit_rspec_group(cx, call) {
                cx.emit_offense(
                    cx.range(node),
                    "Avoid opening modules and defining specs within them.",
                    None,
                );
                return;
            }
        }
    }
}

/// `true` when `call` is an example-group selector with an explicit
/// `RSpec` receiver (`RSpec.describe`, `::RSpec.describe`).
///
/// Mirrors upstream `(send #explicit_rspec? #ExampleGroups.all ...)` where
/// `explicit_rspec?` is `(const {nil? cbase} :RSpec)`. The translator
/// collapses bare `RSpec` and cbase `::RSpec` to the same
/// `Const { scope: None }` (see `translate_constant_path`), so both
/// spellings match here.
fn is_explicit_rspec_group(cx: &Cx<'_>, call: NodeId) -> bool {
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(call)
    else {
        return false;
    };
    let Some(rid) = receiver.get() else {
        return false;
    };
    if !matches!(
        *cx.kind(rid),
        NodeKind::Const { scope, name }
            if scope == OptNodeId::NONE && cx.symbol_str(name) == "RSpec"
    ) {
        return false;
    }
    is_example_group_name(cx.symbol_str(method))
}

#[cfg(test)]
mod tests {
    use super::DescribedClassModuleWrapping;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_rspec_describe_inside_module() {
        test::<DescribedClassModuleWrapping>().expect_offense(indoc! {r#"
                module MyModule; RSpec.describe MyClass do; end; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid opening modules and defining specs within them.
            "#});
    }

    #[test]
    fn flags_rspec_context_inside_module() {
        test::<DescribedClassModuleWrapping>().expect_offense(indoc! {r#"
                module MyModule; RSpec.context 'x' do; end; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid opening modules and defining specs within them.
            "#});
    }

    #[test]
    fn does_not_flag_bare_describe_inside_module() {
        // Upstream requires `#explicit_rspec?`; a bare `describe` matches
        // neither arm.
        test::<DescribedClassModuleWrapping>().expect_no_offenses(indoc! {r#"
                module MyModule
                  describe MyClass do
                  end
                end
            "#});
    }

    #[test]
    fn does_not_flag_module_without_specs() {
        test::<DescribedClassModuleWrapping>().expect_no_offenses(indoc! {r#"
                module MyModule
                  class Foo
                  end
                end
            "#});
    }

    #[test]
    fn does_not_flag_top_level_rspec_describe() {
        test::<DescribedClassModuleWrapping>().expect_no_offenses(indoc! {r#"
                RSpec.describe MyClass do
                end
            "#});
    }

    #[test]
    fn does_not_flag_other_receiver_group() {
        test::<DescribedClassModuleWrapping>().expect_no_offenses(indoc! {r#"
                module MyModule
                  Other.describe MyClass do
                  end
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(DescribedClassModuleWrapping);
