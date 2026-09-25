//! `RSpec/ContainExactly` — prefer `match_array` for all-splat args.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/ContainExactly
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_send` (`return if node.arguments.empty?`, then
//!   `check_populated_collection` requiring every child node to be
//!   `splat_type?`) with `RESTRICT_ON_SEND = [:contain_exactly]`.
//!   Non-empty all-splat argument lists flag (e.g.
//!   `contain_exactly(*array1, *array2)`); bare `contain_exactly` and any
//!   non-splat argument (e.g. `contain_exactly(content, *array)`) pass.
//!   The offense range is the matcher node per `add_offense(node)`.
//!   Detection is at parity; autocorrect (rewrite to
//!   `match_array(array1 + array2)`) is not ported in this batch — same
//!   convention as `RSpec/BeEmpty` (status: partial, autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with `methods = ["contain_exactly"]`:
//!
//! - `is_expected.to contain_exactly(*array1, *array2)` — flagged.
//! - `is_expected.to contain_exactly(*array)` — flagged (single splat).
//! - `is_expected.to contain_exactly` — bare, not flagged.
//! - `is_expected.to contain_exactly(content, *array)` — not flagged.
//! - `is_expected.to contain_exactly(1, 2)` — not flagged.
//!
//! ## No autocorrect
//!
//! Upstream rewrites to `match_array(...)`. This batch reports only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ContainExactly;

#[cop(
    name = "RSpec/ContainExactly",
    description = "Checks where `contain_exactly` is used.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl ContainExactly {
    #[on_node(kind = "send", methods = ["contain_exactly"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { args, .. } = *cx.kind(node) else {
            return;
        };
        let arg_ids = cx.list(args);
        if arg_ids.is_empty() {
            return;
        }
        let all_splat = arg_ids
            .iter()
            .all(|arg| matches!(*cx.kind(*arg), NodeKind::Splat(_)));
        if !all_splat {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            "Prefer `match_array` when matching array values.",
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::ContainExactly;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_all_splat_args() {
        test::<ContainExactly>().expect_offense(indoc! {r#"
                it { is_expected.to contain_exactly(*array1, *array2) }
                                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `match_array` when matching array values.
            "#});
    }

    #[test]
    fn flags_single_splat_arg() {
        test::<ContainExactly>().expect_offense(indoc! {r#"
                it { is_expected.to contain_exactly(*array) }
                                    ^^^^^^^^^^^^^^^^^^^^^^^ Prefer `match_array` when matching array values.
            "#});
    }

    #[test]
    fn does_not_flag_bare_matcher() {
        test::<ContainExactly>().expect_no_offenses(indoc! {r#"
                it { is_expected.to contain_exactly }
            "#});
    }

    #[test]
    fn does_not_flag_mixed_args() {
        test::<ContainExactly>().expect_no_offenses(indoc! {r#"
                it { is_expected.to contain_exactly(content, *array) }
            "#});
    }

    #[test]
    fn does_not_flag_plain_args() {
        test::<ContainExactly>().expect_no_offenses(indoc! {r#"
                it { is_expected.to contain_exactly(1, 2) }
            "#});
    }
}

murphy_plugin_api::submit_cop!(ContainExactly);
