//! `RSpec/ImplicitExpect` — consistent `is_expected` / `should` style.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/ImplicitExpect
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_send` (`RESTRICT_ON_SEND`: runners plus
//!   `should` / `should_not`) with `implicit_expect`: a bare `should` /
//!   `should_not` send flags its selector per `node.loc.selector`, and a
//!   `to` / `to_not` / `not_to` send on a bare `is_expected` receiver
//!   flags `is_expected.<runner>` (expression start through the runner
//!   selector) per `range_for_is_expected`. Under `EnforcedStyle:
//!   is_expected` (default) bare `should` / `should_not` report
//!   `Prefer `is_expected.to` / `is_expected.to_not` over ...`; under
//!   `EnforcedStyle: should` the `is_expected` forms report
//!   `Prefer `should` / `should_not` over ...` (a bare `should_not`
//!   maps to `is_expected.to_not` per `ENFORCED_REPLACEMENTS`, where the
//!   inverted duplicate keeps the last spelling). Detection is at
//!   parity; autocorrect (swap the spellings) is not ported in this
//!   batch — same convention as `RSpec/HookArgument` (status: partial,
//!   autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with `methods = ["to", "to_not", "not_to",
//! `"should", "should_not"]`. Default style `is_expected`:
//!
//! - `it { should be_truthy }` — flagged (range `should`).
//! - `it { should_not be_truthy }` — flagged (range `should_not`).
//! - `it { is_expected.to be_truthy }` — preferred spelling, clean.
//! - `it { is_expected.not_to be_truthy }` — clean.
//! - `it { expect(x).to be_truthy }` — explicit receiver, clean.
//! - `it { foo.should be_truthy }` — explicit receiver, clean.
//!
//! Style `should` (`EnforcedStyle: should`) mirrors: `is_expected.to`
//! flags (range `is_expected.to`), bare `should` passes.
//!
//! ## No autocorrect
//!
//! Upstream swaps `should` with `is_expected.to` (and the `not_to` /
//! `to_not` variants). This batch reports only.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, OptNodeId, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ImplicitExpect;

#[derive(CopOptions)]
pub struct ImplicitExpectOptions {
    #[option(
        name = "EnforcedStyle",
        default = "is_expected",
        description = "Whether implicit expectations use is_expected or should style."
    )]
    pub enforced_style: ImplicitExpectStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum ImplicitExpectStyle {
    #[option(value = "is_expected")]
    IsExpected,
    #[option(value = "should")]
    Should,
}

#[cop(
    name = "RSpec/ImplicitExpect",
    description = "Check that a consistent implicit expectation style is used.",
    default_severity = "warning",
    default_enabled = true,
    options = ImplicitExpectOptions,
)]
impl ImplicitExpect {
    #[on_node(kind = "send", methods = ["to", "to_not", "not_to", "should", "should_not"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, method, .. } = *cx.kind(node) else {
            return;
        };
        let method_name = cx.symbol_str(method);
        let opts = cx.options_or_default::<ImplicitExpectOptions>();
        if matches!(method_name, "should" | "should_not") {
            // Upstream `(send nil? ${:should :should_not} ...)`.
            if receiver != OptNodeId::NONE {
                return;
            }
            if opts.enforced_style == ImplicitExpectStyle::Should {
                return;
            }
            let good = if method_name == "should" {
                "is_expected.to"
            } else {
                "is_expected.to_not"
            };
            cx.emit_offense(
                cx.node(node).loc.name,
                &format!("Prefer `{good}` over `{method_name}`."),
                None,
            );
            return;
        }
        // Upstream `(send (send nil? $:is_expected) #Runners.all ...)`.
        let Some(recv_id) = receiver.get() else {
            return;
        };
        if !is_bare_is_expected(cx, recv_id) {
            return;
        }
        if opts.enforced_style == ImplicitExpectStyle::IsExpected {
            return;
        }
        let bad = format!("is_expected.{method_name}");
        let good = if method_name == "to" {
            "should"
        } else {
            "should_not"
        };
        cx.emit_offense(
            Range {
                start: cx.range(node).start,
                end: cx.node(node).loc.name.end,
            },
            &format!("Prefer `{good}` over `{bad}`."),
            None,
        );
    }
}

/// `true` when `id` is a bare `is_expected` send (the inner expectation
/// of upstream's `implicit_expect`).
fn is_bare_is_expected(cx: &Cx<'_>, id: NodeId) -> bool {
    let NodeKind::Send { receiver, method, .. } = *cx.kind(id) else {
        return false;
    };
    receiver == OptNodeId::NONE && cx.symbol_str(method) == "is_expected"
}

#[cfg(test)]
mod tests {
    use super::{ImplicitExpect, ImplicitExpectOptions, ImplicitExpectStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn should_style() -> ImplicitExpectOptions {
        ImplicitExpectOptions {
            enforced_style: ImplicitExpectStyle::Should,
        }
    }

    #[test]
    fn flags_should_in_default_style() {
        test::<ImplicitExpect>().expect_offense(indoc! {r#"
                it { should be_truthy }
                     ^^^^^^ Prefer `is_expected.to` over `should`.
            "#});
    }

    #[test]
    fn flags_should_not_in_default_style() {
        test::<ImplicitExpect>().expect_offense(indoc! {r#"
                it { should_not be_truthy }
                     ^^^^^^^^^^ Prefer `is_expected.to_not` over `should_not`.
            "#});
    }

    #[test]
    fn does_not_flag_is_expected_forms_in_default_style() {
        test::<ImplicitExpect>().expect_no_offenses(indoc! {r#"
                it { is_expected.to be_truthy }
                it { is_expected.not_to be_truthy }
                it { is_expected.to_not be_truthy }
            "#});
    }

    #[test]
    fn does_not_flag_explicit_expect_in_default_style() {
        test::<ImplicitExpect>().expect_no_offenses(indoc! {r#"
                it { expect(foo).to be_truthy }
            "#});
    }

    #[test]
    fn does_not_flag_receiver_should_in_default_style() {
        // `foo.should` is not an implicit expectation.
        test::<ImplicitExpect>().expect_no_offenses(indoc! {r#"
                it { foo.should be_truthy }
            "#});
    }

    #[test]
    fn flags_is_expected_to_in_should_style() {
        test::<ImplicitExpect>()
            .with_options(&should_style())
            .expect_offense(indoc! {r#"
                it { is_expected.to be_truthy }
                     ^^^^^^^^^^^^^^ Prefer `should` over `is_expected.to`.
            "#});
    }

    #[test]
    fn flags_is_expected_not_to_in_should_style() {
        test::<ImplicitExpect>()
            .with_options(&should_style())
            .expect_offense(indoc! {r#"
                it { is_expected.not_to be_truthy }
                     ^^^^^^^^^^^^^^^^^^ Prefer `should_not` over `is_expected.not_to`.
            "#});
    }

    #[test]
    fn flags_is_expected_to_not_in_should_style() {
        test::<ImplicitExpect>()
            .with_options(&should_style())
            .expect_offense(indoc! {r#"
                it { is_expected.to_not be_truthy }
                     ^^^^^^^^^^^^^^^^^^ Prefer `should_not` over `is_expected.to_not`.
            "#});
    }

    #[test]
    fn does_not_flag_should_forms_in_should_style() {
        test::<ImplicitExpect>()
            .with_options(&should_style())
            .expect_no_offenses(indoc! {r#"
                it { should be_truthy }
                it { should_not be_truthy }
            "#});
    }
}

murphy_plugin_api::submit_cop!(ImplicitExpect);
