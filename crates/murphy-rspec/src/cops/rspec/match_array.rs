//! `RSpec/MatchArray` — prefer `contain_exactly` for array literals.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/MatchArray
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_send` (`return unless
//!   node.first_argument&.array_type?`, skip `match_array_with_empty_array?`
//!   — a single empty `(array)` arg — then `check_populated_array` which
//!   skips `percent_literal?` arrays) with `RESTRICT_ON_SEND =
//!   [:match_array]`. Flags `match_array([content1, content2])`; passes
//!   `match_array([])`, `match_array([content] + array)` (first arg is a
//!   `send`, not an array), `match_array(%w(...))` (percent literal) and
//!   non-array first args. Only the first argument is inspected, so extra
//!   trailing args do not suppress the offense. The offense range is the
//!   matcher node per `add_offense(node)`. Detection is at parity;
//!   autocorrect (rewrite to `contain_exactly(...)`) is not ported in this
//!   batch — same convention as `RSpec/ContainExactly` (status: partial,
//!   autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with `methods = ["match_array"]`:
//!
//! - `is_expected.to match_array([content1, content2])` — flagged.
//! - `is_expected.to match_array([])` — empty array, not flagged.
//! - `is_expected.to match_array([content] + array)` — not flagged.
//! - `is_expected.to match_array(%w(foo bar))` — percent literal, not flagged.
//! - `is_expected.to match_array(items)` — non-array arg, not flagged.
//! - `is_expected.to match_array` — bare, not flagged.
//!
//! ## No autocorrect
//!
//! Upstream rewrites to `contain_exactly(...)`. This batch reports only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct MatchArray;

#[cop(
    name = "RSpec/MatchArray",
    description = "Checks where `match_array` is used.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl MatchArray {
    #[on_node(kind = "send", methods = ["match_array"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { args, .. } = *cx.kind(node) else {
            return;
        };
        let arg_ids = cx.list(args);
        let Some(&first) = arg_ids.first() else {
            return;
        };
        if !matches!(*cx.kind(first), NodeKind::Array(_)) {
            return;
        }
        // `match_array_with_empty_array?`: a single empty `(array)` arg.
        if arg_ids.len() == 1 && cx.array_elements(first).is_empty() {
            return;
        }
        if cx.is_percent_literal(first) {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            "Prefer `contain_exactly` when matching an array literal.",
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::MatchArray;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_array_literal() {
        test::<MatchArray>().expect_offense(indoc! {r#"
                it { is_expected.to match_array([content1, content2]) }
                                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `contain_exactly` when matching an array literal.
            "#});
    }

    #[test]
    fn does_not_flag_empty_array() {
        test::<MatchArray>().expect_no_offenses(indoc! {r#"
                it { is_expected.to match_array([]) }
            "#});
    }

    #[test]
    fn does_not_flag_array_plus_variable() {
        // First arg is a `send` (`+`), not an array literal.
        test::<MatchArray>().expect_no_offenses(indoc! {r#"
                it { is_expected.to match_array([content] + array) }
            "#});
    }

    #[test]
    fn does_not_flag_percent_literal() {
        test::<MatchArray>().expect_no_offenses(indoc! {r#"
                it { is_expected.to match_array(%w(foo bar)) }
            "#});
    }

    #[test]
    fn does_not_flag_non_array_arg() {
        test::<MatchArray>().expect_no_offenses(indoc! {r#"
                it { is_expected.to match_array(items) }
            "#});
    }

    #[test]
    fn does_not_flag_bare_matcher() {
        test::<MatchArray>().expect_no_offenses(indoc! {r#"
                it { is_expected.to match_array }
            "#});
    }
}

murphy_plugin_api::submit_cop!(MatchArray);
