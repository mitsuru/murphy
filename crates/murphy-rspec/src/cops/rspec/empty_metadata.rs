//! `RSpec/EmptyMetadata` — avoid empty metadata hashes.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/EmptyMetadata
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `Metadata#on_metadata` (`Avoid empty metadata hash`):
//!   for `rspec_metadata` blocks (`(block (send #rspec?
//!   {#Examples.all #ExampleGroups.all #SharedGroups.all #Hooks.all} _
//!   $...) ...)`) the trailing argument is split off when it is a `Hash`
//!   (`on_metadata_arguments`); an empty hash (no pairs) flags per
//!   `add_offense(hash)`. A hash containing a `Kwsplat` (`**opts`) is
//!   skipped per `hash.children.any?(&:kwsplat_type?)`. Detection is at
//!   parity for direct blocks; `RSpec.configure { |c| c.before(...) }`
//!   indirection via `metadata_in_block` is not ported in this batch
//!   (status: partial, autocorrect as gap — upstream removes the hash with
//!   surrounding comma/space).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` whose call is an RSpec example / group / shared
//! group / hook with an RSpec-or-bare receiver:
//!
//! - `describe 'Something', {} do; end` — empty hash, flagged.
//! - `it 'x', {} do; end` — empty hash, flagged.
//! - `describe 'Something', { a: 1 } do; end` — non-empty, not flagged.
//! - `describe 'Something' do; end` — no hash, not flagged.
//! - `describe 'Something', **opts do; end` — kwsplat hash, not flagged.
//!
//! ## No autocorrect
//!
//! Upstream removes the hash with surrounding comma/space. This batch
//! reports only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

use crate::cops::rspec_helpers::{is_example_group_name, is_hook_name, is_rspec_or_bare_receiver};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct EmptyMetadata;

#[cop(
    name = "RSpec/EmptyMetadata",
    description = "Avoid empty metadata hash.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl EmptyMetadata {
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
        // `hook?` uses a bare receiver; example / group / shared owners
        // accept RSpec-or-bare. Gate hooks on bare explicitly upstream via
        // `(send nil? #Hooks.all ...)`; the shared `is_rspec_or_bare`
        // check above already admits `RSpec.before`, which never occurs in
        // practice — keep the owner check simple and parity-focused.
        if !is_metadata_owner(receiver, cx.symbol_str(method)) {
            return;
        }
        let arg_ids = cx.list(args);
        if arg_ids.is_empty() {
            return;
        }
        let Some(&last) = arg_ids.last() else {
            return;
        };
        let NodeKind::Hash(list) = *cx.kind(last) else {
            return;
        };
        let entries = cx.list(list);
        if !entries.is_empty() {
            // Non-empty hashes (pairs or `Kwsplat` like `**opts`) never
            // flag. Upstream checks `pairs.empty?` then skips kwsplat
            // children; both shapes are non-empty here.
            return;
        }
        cx.emit_offense(cx.range(last), "Avoid empty metadata hash.", None);
    }
}

/// `true` when `name` can carry RSpec metadata (`rspec_metadata` owners:
/// examples, example groups, shared groups, hooks). Hooks require a bare
/// receiver upstream (`(send nil? ...)`); examples / groups / shared
/// accept RSpec-or-bare.
fn is_metadata_owner(receiver: murphy_plugin_api::OptNodeId, name: &str) -> bool {
    if is_hook_name(name) {
        return receiver == murphy_plugin_api::OptNodeId::NONE;
    }
    is_example_name(name)
        || is_example_group_name(name)
        || is_shared_group_name(name)
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
    use super::EmptyMetadata;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_empty_hash_in_describe() {
        test::<EmptyMetadata>().expect_offense(indoc! {r#"
                describe 'Something', {} do; end
                                      ^^ Avoid empty metadata hash.
            "#});
    }

    #[test]
    fn flags_empty_hash_in_it() {
        test::<EmptyMetadata>().expect_offense(indoc! {r#"
                it 'x', {} do; end
                        ^^ Avoid empty metadata hash.
            "#});
    }

    #[test]
    fn does_not_flag_non_empty_hash() {
        test::<EmptyMetadata>().expect_no_offenses(indoc! {r#"
                describe 'Something', { a: 1 } do; end
            "#});
    }

    #[test]
    fn does_not_flag_without_hash() {
        test::<EmptyMetadata>().expect_no_offenses(indoc! {r#"
                describe 'Something' do; end
            "#});
    }

    #[test]
    fn does_not_flag_kwsplat_hash() {
        test::<EmptyMetadata>().expect_no_offenses(indoc! {r#"
                describe 'Something', **opts do; end
            "#});
    }

    #[test]
    fn does_not_flag_non_metadata_owner() {
        test::<EmptyMetadata>().expect_no_offenses(indoc! {r#"
                foo 'Something', {} do; end
            "#});
    }
}

murphy_plugin_api::submit_cop!(EmptyMetadata);
