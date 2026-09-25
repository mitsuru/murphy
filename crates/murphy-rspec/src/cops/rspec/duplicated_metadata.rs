//! `RSpec/DuplicatedMetadata` — avoid duplicated metadata symbols.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/DuplicatedMetadata
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `Metadata#on_metadata` (`Avoid duplicated metadata`):
//!   for `rspec_metadata` blocks (`(block (send #rspec?
//!   {#Examples.all #ExampleGroups.all #SharedGroups.all #Hooks.all} _
//!   $...) ...)`) the trailing metadata args (all args after the first)
//!   are scanned for duplicated `Sym` entries via left-sibling equality.
//!   Each second-and-later occurrence flags the `Sym` node per
//!   `add_offense(node)`. Hash metadata (`foo: 1`) is ignored here as
//!   upstream only checks symbols. Detection is at parity for direct
//!   blocks; `RSpec.configure { |c| c.before(...) }` indirection via
//!   `metadata_in_block` is not ported in this batch (status: partial,
//!   autocorrect as gap — upstream removes the duplicate with surrounding
//!   comma/space).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` whose call is an RSpec example / group / shared
//! group / hook with an RSpec-or-bare receiver:
//!
//! - `describe 'x', :a, :a do; end` — second `:a`, flagged.
//! - `it 'x', :a, :a do; end` — second `:a`, flagged.
//! - `before(:each, :a, :a) do; end` — second `:a`, flagged.
//! - `describe 'x', :a do; end` — no duplicate, not flagged.
//! - `describe 'x', :a, :b do; end` — distinct, not flagged.
//! - `describe 'x', :a, foo: 1 do; end` — hash ignored, not flagged.
//!
//! ## No autocorrect
//!
//! Upstream removes the duplicate with surrounding comma/space. This batch
//! reports only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

use crate::cops::rspec_helpers::{is_example_group_name, is_hook_name, is_rspec_or_bare_receiver};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct DuplicatedMetadata;

#[cop(
    name = "RSpec/DuplicatedMetadata",
    description = "Avoid duplicated metadata.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl DuplicatedMetadata {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, .. } = *cx.kind(node) else {
            return;
        };
        let NodeKind::Send {
            receiver,
            method,
            args,
        } = *cx.kind(call)
        else {
            return;
        };
        if !is_rspec_or_bare_receiver(cx, receiver) {
            return;
        }
        if !is_metadata_owner(cx.symbol_str(method)) {
            return;
        }
        let arg_ids = cx.list(args);
        if arg_ids.len() < 2 {
            return;
        }
        // Upstream `$...` captures all args after the first (description /
        // scope). Hash trailing arg is split off in `on_metadata_arguments`
        // and ignored for symbol duplication.
        let mut seen: Vec<String> = Vec::new();
        for &arg in &arg_ids[1..] {
            let NodeKind::Sym(sym) = *cx.kind(arg) else {
                continue;
            };
            let name = cx.symbol_str(sym).to_owned();
            if seen.iter().any(|s| s == &name) {
                cx.emit_offense(cx.range(arg), "Avoid duplicated metadata.", None);
            } else {
                seen.push(name);
            }
        }
    }
}

/// `true` when `name` can carry RSpec metadata (`rspec_metadata` owners:
/// examples, example groups, shared groups, hooks).
fn is_metadata_owner(name: &str) -> bool {
    is_example_name(name)
        || is_example_group_name(name)
        || is_shared_group_name(name)
        || is_hook_name(name)
        || is_include_name(name)
}

/// Example selectors (`Examples.all`): regular + focused + skipped +
/// pending, mirroring `config/default.yml` Language section.
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

/// Shared-group selectors (`SharedGroups.all`).
fn is_shared_group_name(name: &str) -> bool {
    matches!(
        name,
        "shared_examples" | "shared_examples_for" | "shared_context"
    )
}

/// Include selectors (`Includes.all`).
fn is_include_name(name: &str) -> bool {
    matches!(
        name,
        "it_behaves_like" | "it_should_behave_like" | "include_examples" | "include_context"
    )
}

#[cfg(test)]
mod tests {
    use super::DuplicatedMetadata;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_duplicate_in_describe() {
        test::<DuplicatedMetadata>().expect_offense(indoc! {r#"
                describe 'Something', :a, :a do; end
                                          ^^ Avoid duplicated metadata.
            "#});
    }

    #[test]
    fn flags_duplicate_in_it() {
        test::<DuplicatedMetadata>().expect_offense(indoc! {r#"
                it 'x', :a, :a do; end
                            ^^ Avoid duplicated metadata.
            "#});
    }

    #[test]
    fn flags_duplicate_in_hook() {
        test::<DuplicatedMetadata>().expect_offense(indoc! {r#"
                before(:each, :a, :a) do; end
                                  ^^ Avoid duplicated metadata.
            "#});
    }

    #[test]
    fn does_not_flag_single_metadata() {
        test::<DuplicatedMetadata>().expect_no_offenses(indoc! {r#"
                describe 'Something', :a do; end
            "#});
    }

    #[test]
    fn does_not_flag_distinct_metadata() {
        test::<DuplicatedMetadata>().expect_no_offenses(indoc! {r#"
                describe 'Something', :a, :b do; end
            "#});
    }

    #[test]
    fn does_not_flag_without_metadata() {
        test::<DuplicatedMetadata>().expect_no_offenses(indoc! {r#"
                describe 'Something' do; end
            "#});
    }

    #[test]
    fn does_not_flag_explicit_receiver() {
        test::<DuplicatedMetadata>().expect_no_offenses(indoc! {r#"
                Other.describe 'x', :a, :a do; end
            "#});
    }
}

murphy_plugin_api::submit_cop!(DuplicatedMetadata);
