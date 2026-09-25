//! `RSpec/EmptyHook` — flags empty `before`/`after`/`around` hooks.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/EmptyHook
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `empty_hook?` (`(block $(send nil? #Hooks.all ...) _ nil?)`):
//!   bare receiver only, any args, empty body (`body.is_none()`). Offense range is
//!   the hook `Send` per `add_offense(hook)`. Detection is at parity; autocorrect
//!   (removing the empty hook block) is intentionally not ported in this batch —
//!   same convention as `Bundler/OrderedGems` (status: partial, autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block`. Flags when the call is a bare hook
//! (`before`, `after`, `around`, `prepend_before`, `append_before`,
//! `prepend_after`, `append_after`) and the body is empty:
//!
//! - `before {}` — flagged.
//! - `after do; end` — flagged.
//! - `before(:all) do end` — flagged (scope arg does not matter).
//! - `before { create_users }` — body present, not flagged.
//! - `before { nil }` — body is a `Nil` node (not `None`), not flagged,
//!   matching upstream's `nil?` body check.
//!
//! Receiver must be bare (`nil?` upstream): `RSpec.before {}` or
//! `obj.before {}` are some other DSL and are skipped.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::{is_hook_name, send_without_block_range};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct EmptyHook;

#[cop(
    name = "RSpec/EmptyHook",
    description = "Checks for empty before and after hooks.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl EmptyHook {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, body, .. } = *cx.kind(node) else {
            return;
        };
        if body.get().is_some() {
            return;
        }
        let NodeKind::Send { receiver, method, .. } = *cx.kind(call) else {
            return;
        };
        if receiver != OptNodeId::NONE {
            return;
        }
        if !is_hook_name(cx.symbol_str(method)) {
            return;
        }
        cx.emit_offense(
            send_without_block_range(cx, call),
            "Empty hook detected.",
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::EmptyHook;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_empty_before_braces() {
        test::<EmptyHook>().expect_offense(indoc! {r#"
                before {}
                ^^^^^^ Empty hook detected.
            "#});
    }

    #[test]
    fn flags_empty_after_do_end() {
        test::<EmptyHook>().expect_offense(indoc! {r#"
                after do; end
                ^^^^^ Empty hook detected.
            "#});
    }

    #[test]
    fn flags_empty_before_with_scope() {
        test::<EmptyHook>().expect_offense(indoc! {r#"
                before(:all) do
                ^^^^^^^^^^^^ Empty hook detected.
                end
            "#});
    }

    #[test]
    fn flags_empty_around() {
        test::<EmptyHook>().expect_offense(indoc! {r#"
                around { }
                ^^^^^^ Empty hook detected.
            "#});
    }

    #[test]
    fn does_not_flag_non_empty_hook() {
        test::<EmptyHook>().expect_no_offenses(indoc! {r#"
                before { create_users }
            "#});
    }

    #[test]
    fn does_not_flag_multiline_non_empty_hook() {
        test::<EmptyHook>().expect_no_offenses(indoc! {r#"
                before(:all) do
                  create_users
                end
            "#});
    }

    #[test]
    fn does_not_flag_nil_body_as_empty() {
        // `before { nil }` has a `Nil` body node, not an absent body;
        // upstream `nil?` requires absence.
        test::<EmptyHook>().expect_no_offenses(indoc! {r#"
                before { nil }
            "#});
    }

    #[test]
    fn does_not_flag_non_hook_block() {
        test::<EmptyHook>().expect_no_offenses(indoc! {r#"
                it { }
            "#});
    }

    #[test]
    fn does_not_flag_explicit_receiver_hook() {
        test::<EmptyHook>().expect_no_offenses(indoc! {r#"
                obj.before {}
            "#});
    }
}

murphy_plugin_api::submit_cop!(EmptyHook);
