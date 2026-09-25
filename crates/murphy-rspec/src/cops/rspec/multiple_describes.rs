//! `RSpec/MultipleDescribes` — flags multiple top-level example groups.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/MultipleDescribes
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `TopLevelGroup` + `MultipleDescribes#on_top_level_group`:
//!   top-level groups are `Block`s whose call is an `ExampleGroups.all` selector
//!   with an RSpec-or-bare receiver and whose ancestors up to the root are only
//!   `Begin`/`Class`/`Module` (sclass excluded). Shared groups
//!   (`shared_examples`, `shared_context`, …) are not example groups and do not
//!   count. When more than one top-level example group exists, the first group's
//!   `Send` is flagged per `add_offense(node.send_node)`. No autocorrect upstream,
//!   none here.
//! ```
//!
//! ## Matched shapes
//!
//! File-scoped (`#[on_new_investigation]`): collects top-level example-group
//! `Block`s in source order via `cx.descendants`.
//!
//! - Two top-level `describe`s — first flagged.
//! - Single top-level `describe` — clean.
//! - Nested second `describe` — clean (inner is not top-level).
//! - `shared_examples` + `describe` — shared group does not count.
//! - `describe` wrapped in `class Foo ... end` — still top-level per
//!   `TopLevelGroup#top_level_nodes` (walks through `class`/`module`).

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

use crate::cops::rspec_helpers::{is_example_group_call, is_top_level_block, send_without_block_range};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct MultipleDescribes;

#[cop(
    name = "RSpec/MultipleDescribes",
    description = "Checks for multiple top-level example groups.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl MultipleDescribes {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let root = cx.root();
        let mut groups: Vec<NodeId> = Vec::new();
        for node in core::iter::once(root).chain(cx.descendants(root)) {
            let NodeKind::Block { call, .. } = *cx.kind(node) else {
                continue;
            };
            if !is_example_group_call(cx, call) {
                continue;
            }
            if !is_top_level_block(cx, node) {
                continue;
            }
            groups.push(node);
        }
        if groups.len() < 2 {
            return;
        }
        let first = groups[0];
        let NodeKind::Block { call, .. } = *cx.kind(first) else {
            return;
        };
        cx.emit_offense(
            send_without_block_range(cx, call),
            "Do not use multiple top-level example groups - try to nest them.",
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::MultipleDescribes;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_two_top_level_describes() {
        test::<MultipleDescribes>().expect_offense(indoc! {r#"
                describe MyClass, '.do_something' do; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not use multiple top-level example groups - try to nest them.
                describe MyClass, '.do_something_else' do; end
            "#});
    }

    #[test]
    fn flags_two_top_level_describes_class_only() {
        test::<MultipleDescribes>().expect_offense(indoc! {r#"
                describe MyClass do; end
                ^^^^^^^^^^^^^^^^ Do not use multiple top-level example groups - try to nest them.
                describe MyOtherClass do; end
            "#});
    }

    #[test]
    fn ignores_single_top_level_group() {
        test::<MultipleDescribes>().expect_no_offenses(indoc! {r#"
                describe MyClass do
                end
            "#});
    }

    #[test]
    fn ignores_nested_second_group() {
        test::<MultipleDescribes>().expect_no_offenses(indoc! {r#"
                describe MyClass do
                  describe '.do_something' do
                  end
                end
            "#});
    }

    #[test]
    fn ignores_shared_groups() {
        test::<MultipleDescribes>().expect_no_offenses(indoc! {r#"
                shared_examples_for 'behaves' do
                end
                shared_examples_for 'misbehaves' do
                end
                describe MyClass do
                end
            "#});
    }

    #[test]
    fn flags_top_level_groups_through_class_wrapper() {
        // `TopLevelGroup` walks through `class`/`module`; a class wrapper
        // does not hide top-level groups from each other.
        test::<MultipleDescribes>().expect_offense(indoc! {r#"
                class Foo
                  describe MyClass do; end
                  ^^^^^^^^^^^^^^^^ Do not use multiple top-level example groups - try to nest them.
                  describe MyOtherClass do; end
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(MultipleDescribes);
