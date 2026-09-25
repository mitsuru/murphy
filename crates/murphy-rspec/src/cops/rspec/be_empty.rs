//! `RSpec/BeEmpty` — prefer `be_empty` for empty-array checks.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/BeEmpty
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `expect_array_matcher?` (`(send (send nil? :expect _)
//!   #Runners.all ${(send nil? :match_array (array)) (send nil?
//!   :contain_exactly)} _?)`) with `RESTRICT_ON_SEND = [:contain_exactly,
//!   :match_array]`. Bare `contain_exactly` (zero args) and bare
//!   `match_array([])` (single empty array) flag only when the parent is a
//!   `to` / `to_not` / `not_to` runner whose receiver is bare `expect`
//!   with a single arg; the optional trailing message arg (`_?`) is
//!   allowed. The offense range is the matcher node per
//!   `add_offense(expect)`. Detection is at parity; autocorrect (replace
//!   with `be_empty`) is not ported in this batch — same convention as
//!   `RSpec/BeEql` (status: partial, autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with `methods = ["contain_exactly", "match_array"]`.
//! Flags when bare and empty, inside an `expect(...).to/...` runner:
//!
//! - `expect(array).to contain_exactly` — flagged.
//! - `expect(array).to match_array([])` — flagged.
//! - `expect(array).to contain_exactly, "msg"` — flagged (message arg allowed).
//! - `expect(array).to be_empty` — different matcher, not flagged.
//! - `expect(array).to match_array([1])` — non-empty array, not flagged.
//! - `expect(array).to contain_exactly(1)` — has args, not flagged.
//!
//! ## No autocorrect
//!
//! Upstream replaces the matcher with `be_empty`. This batch reports only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct BeEmpty;

#[cop(
    name = "RSpec/BeEmpty",
    description = "Prefer using be_empty when checking for an empty array.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl BeEmpty {
    #[on_node(kind = "send", methods = ["contain_exactly", "match_array"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send {
            receiver,
            method,
            args,
        } = *cx.kind(node)
        else {
            return;
        };
        if receiver != OptNodeId::NONE {
            return;
        }
        let method_name = cx.symbol_str(method);
        let arg_ids = cx.list(args);
        match method_name {
            "contain_exactly" => {
                if !arg_ids.is_empty() {
                    return;
                }
            }
            "match_array" => {
                if arg_ids.len() != 1 {
                    return;
                }
                let NodeKind::Array(items) = *cx.kind(arg_ids[0]) else {
                    return;
                };
                if !cx.list(items).is_empty() {
                    return;
                }
            }
            _ => return,
        }
        let Some(parent) = cx.parent(node).get() else {
            return;
        };
        let NodeKind::Send {
            receiver: p_recv,
            method: p_method,
            args: p_args,
        } = *cx.kind(parent)
        else {
            return;
        };
        if !matches!(
            cx.symbol_str(p_method),
            "to" | "to_not" | "not_to"
        ) {
            return;
        }
        let Some(expect_id) = p_recv.get() else {
            return;
        };
        let NodeKind::Send {
            receiver: e_recv,
            method: e_method,
            args: e_args,
        } = *cx.kind(expect_id)
        else {
            return;
        };
        if e_recv != OptNodeId::NONE {
            return;
        }
        if cx.symbol_str(e_method) != "expect" {
            return;
        }
        if cx.list(e_args).len() != 1 {
            return;
        }
        let p_arg_ids = cx.list(p_args);
        if p_arg_ids.first() != Some(&node) {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            "Use `be_empty` matchers for checking an empty array.",
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::BeEmpty;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_contain_exactly() {
        test::<BeEmpty>().expect_offense(indoc! {r#"
                expect(array).to contain_exactly
                                 ^^^^^^^^^^^^^^^ Use `be_empty` matchers for checking an empty array.
            "#});
    }

    #[test]
    fn flags_contain_exactly_with_message() {
        test::<BeEmpty>().expect_offense(indoc! {r#"
                expect(array).to contain_exactly, "with a message"
                                 ^^^^^^^^^^^^^^^ Use `be_empty` matchers for checking an empty array.
            "#});
    }

    #[test]
    fn flags_match_array_empty() {
        test::<BeEmpty>().expect_offense(indoc! {r#"
                expect(array).to match_array([])
                                 ^^^^^^^^^^^^^^^ Use `be_empty` matchers for checking an empty array.
            "#});
    }

    #[test]
    fn flags_match_array_empty_with_message() {
        test::<BeEmpty>().expect_offense(indoc! {r#"
                expect(array).to match_array([]), "with a message"
                                 ^^^^^^^^^^^^^^^ Use `be_empty` matchers for checking an empty array.
            "#});
    }

    #[test]
    fn does_not_flag_be_empty() {
        test::<BeEmpty>().expect_no_offenses(indoc! {r#"
                expect(array).to be_empty
            "#});
    }

    #[test]
    fn does_not_flag_match_array_non_empty() {
        test::<BeEmpty>().expect_no_offenses(indoc! {r#"
                expect(array).to match_array([1])
            "#});
    }

    #[test]
    fn does_not_flag_contain_exactly_with_args() {
        test::<BeEmpty>().expect_no_offenses(indoc! {r#"
                expect(array).to contain_exactly(1)
            "#});
    }

    #[test]
    fn does_not_flag_bare_matcher_without_expect() {
        // Upstream requires an `expect(...)` runner receiver.
        test::<BeEmpty>().expect_no_offenses(indoc! {r#"
                foo.to contain_exactly
            "#});
    }
}

murphy_plugin_api::submit_cop!(BeEmpty);
