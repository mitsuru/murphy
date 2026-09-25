//! `RSpec/IsExpectedSpecify` — use `it` instead of one-line `specify` with `is_expected`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/IsExpectedSpecify
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_send` (`RESTRICT_ON_SEND = [:specify]`, any
//!   receiver per `(send _ :specify)`): the parent must be a single-line
//!   `Block` whose call is this send and whose body is
//!   `(send (send _ {:is_expected :are_expected}) ...)` — an arg-less
//!   `specify` wrapping a one-liner over `is_expected` / `are_expected`
//!   (any receiver on the inner expectation per `_`). Arity is exact: a
//!   `specify` with args never matches (verified vs 3.7.0). The offense
//!   range is the selector per `add_offense(selector)` with
//!   `Use `it` instead of `specify`.` Detection is at parity; autocorrect
//!   (replace the selector with `it`) is not ported in this batch — same
//!   convention as `RSpec/IncludeExamples` (status: partial, autocorrect
//!   as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with `methods = ["specify"]`:
//!
//! - `specify { is_expected.to be_truthy }` — flagged (the selector).
//! - `specify { are_expected.to be_truthy }` — flagged too.
//! - `it { is_expected.to be_truthy }` — different selector, clean.
//! - `specify do is_expected.to be_truthy end` — multiline, clean.
//! - `specify { expect(x).to eq(1) }` — no `is_expected`, clean.
//! - `specify("desc") { is_expected.to x }` — has args, clean.
//!
//! ## No autocorrect
//!
//! Upstream replaces the selector with `it`. This batch reports only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct IsExpectedSpecify;

#[cop(
    name = "RSpec/IsExpectedSpecify",
    description = "Check for `specify` with `is_expected` and one-liner expectations.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl IsExpectedSpecify {
    #[on_node(kind = "send", methods = ["specify"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // `(send _ :specify)`: exact arity — no args (verified vs 3.7.0).
        // Any receiver matches upstream `_`.
        let NodeKind::Send { args, .. } = *cx.kind(node) else {
            return;
        };
        if !cx.list(args).is_empty() {
            return;
        }
        let Some(parent) = cx.parent(node).get() else {
            return;
        };
        let NodeKind::Block { call, body, .. } = *cx.kind(parent) else {
            return;
        };
        if call != node || !cx.is_single_line(parent) {
            return;
        }
        let Some(body_id) = body.get() else {
            return;
        };
        // `(send (send _ {:is_expected :are_expected}) ...)`.
        let NodeKind::Send { receiver, .. } = *cx.kind(body_id) else {
            return;
        };
        let Some(recv) = receiver.get() else {
            return;
        };
        let NodeKind::Send { method, .. } = *cx.kind(recv) else {
            return;
        };
        if !matches!(cx.symbol_str(method), "is_expected" | "are_expected") {
            return;
        }
        cx.emit_offense(
            cx.node(node).loc.name,
            "Use `it` instead of `specify`.",
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::IsExpectedSpecify;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_specify_with_is_expected() {
        test::<IsExpectedSpecify>().expect_offense(indoc! {r#"
                specify { is_expected.to be_truthy }
                ^^^^^^^ Use `it` instead of `specify`.
            "#});
    }

    #[test]
    fn flags_specify_with_are_expected() {
        test::<IsExpectedSpecify>().expect_offense(indoc! {r#"
                specify { are_expected.to be_truthy }
                ^^^^^^^ Use `it` instead of `specify`.
            "#});
    }

    #[test]
    fn does_not_flag_it() {
        test::<IsExpectedSpecify>().expect_no_offenses(indoc! {r#"
                it { is_expected.to be_truthy }
            "#});
    }

    #[test]
    fn does_not_flag_multiline_specify() {
        test::<IsExpectedSpecify>().expect_no_offenses(indoc! {r#"
                specify do
                  is_expected.to be_truthy
                end
            "#});
    }

    #[test]
    fn does_not_flag_expect_body() {
        test::<IsExpectedSpecify>().expect_no_offenses(indoc! {r#"
                specify { expect(sqrt(4)).to eq(2) }
            "#});
    }

    #[test]
    fn does_not_flag_specify_with_args() {
        // Exact arity: `specify` with args never matches (verified vs
        // 3.7.0).
        test::<IsExpectedSpecify>().expect_no_offenses(indoc! {r#"
                specify("desc") { is_expected.to be_truthy }
            "#});
    }
}

murphy_plugin_api::submit_cop!(IsExpectedSpecify);
