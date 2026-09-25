//! `RSpec/ClassCheck` — consistent `be_a` / `be_kind_of` style.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/ClassCheck
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `offending?` (`!node.receiver &&
//!   !preferred_method_name?(node.method_name)`) with `RESTRICT_ON_SEND =
//!   [:be_a, :be_a_kind_of, :be_an, :be_kind_of]` and `EnforcedStyle`
//!   (`be_a` default, `be_kind_of` alternate). The `be_a` style accepts
//!   `be_a` / `be_an` and flags `be_kind_of` / `be_a_kind_of`; the
//!   `be_kind_of` style is the mirror. Upstream checks neither the
//!   argument list nor the surrounding `expect(...).to` runner, so a bare
//!   `be_kind_of` with zero args still flags. The offense range is the
//!   selector per `add_offense(node.loc.selector)`. Detection is at
//!   parity; autocorrect (replace with the preferred spelling) is not
//!   ported in this batch — same convention as `RSpec/BeNil` (status:
//!   partial, autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with bare receiver and methods `be_a`,
//! `be_a_kind_of`, `be_an`, `be_kind_of`. Default style `be_a`:
//!
//! - `expect(object).to be_kind_of(String)` — flagged.
//! - `expect(object).to be_a_kind_of(String)` — flagged.
//! - `expect(object).to be_a(String)` — preferred spelling, not flagged.
//! - `expect(object).to be_an(String)` — accepted spelling, not flagged.
//!
//! Alternate style `be_kind_of` (`EnforcedStyle: be_kind_of`) mirrors:
//! `be_a` / `be_an` flag, `be_kind_of` / `be_a_kind_of` pass.
//!
//! ## No autocorrect
//!
//! Upstream replaces with the preferred spelling. This batch reports only.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ClassCheck;

#[derive(CopOptions)]
pub struct ClassCheckOptions {
    #[option(
        name = "EnforcedStyle",
        default = "be_a",
        description = "Whether to enforce be_a or be_kind_of for class checks."
    )]
    pub enforced_style: ClassCheckStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum ClassCheckStyle {
    #[option(value = "be_a")]
    BeA,
    #[option(value = "be_kind_of")]
    BeKindOf,
}

#[cop(
    name = "RSpec/ClassCheck",
    description = "Enforces consistent use of `be_a` or `be_kind_of`.",
    default_severity = "warning",
    default_enabled = true,
    options = ClassCheckOptions,
)]
impl ClassCheck {
    #[on_node(
        kind = "send",
        methods = ["be_a", "be_a_kind_of", "be_an", "be_kind_of"]
    )]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, method, .. } = *cx.kind(node) else {
            return;
        };
        if receiver != OptNodeId::NONE {
            return;
        }
        let opts = cx.options_or_default::<ClassCheckOptions>();
        let method_name = cx.symbol_str(method);
        let preferred = match opts.enforced_style {
            ClassCheckStyle::BeA => "be_a",
            ClassCheckStyle::BeKindOf => "be_kind_of",
        };
        let accepted = match opts.enforced_style {
            ClassCheckStyle::BeA => matches!(method_name, "be_a" | "be_an"),
            ClassCheckStyle::BeKindOf => {
                matches!(method_name, "be_kind_of" | "be_a_kind_of")
            }
        };
        if accepted {
            return;
        }
        cx.emit_offense(
            cx.node(node).loc.name,
            &format!("Prefer `{preferred}` over `{method_name}`."),
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{ClassCheck, ClassCheckOptions, ClassCheckStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn kind_of_style() -> ClassCheckOptions {
        ClassCheckOptions {
            enforced_style: ClassCheckStyle::BeKindOf,
        }
    }

    #[test]
    fn flags_be_kind_of_in_default_style() {
        test::<ClassCheck>().expect_offense(indoc! {r#"
                expect(object).to be_kind_of(String)
                                  ^^^^^^^^^^ Prefer `be_a` over `be_kind_of`.
            "#});
    }

    #[test]
    fn flags_be_a_kind_of_in_default_style() {
        test::<ClassCheck>().expect_offense(indoc! {r#"
                expect(object).to be_a_kind_of(String)
                                  ^^^^^^^^^^^^ Prefer `be_a` over `be_a_kind_of`.
            "#});
    }

    #[test]
    fn does_not_flag_be_a_in_default_style() {
        test::<ClassCheck>().expect_no_offenses(indoc! {r#"
                expect(object).to be_a(String)
            "#});
    }

    #[test]
    fn does_not_flag_be_an_in_default_style() {
        test::<ClassCheck>().expect_no_offenses(indoc! {r#"
                expect(object).to be_an(String)
            "#});
    }

    #[test]
    fn flags_be_a_in_kind_of_style() {
        test::<ClassCheck>()
            .with_options(&kind_of_style())
            .expect_offense(indoc! {r#"
                expect(object).to be_a(String)
                                  ^^^^ Prefer `be_kind_of` over `be_a`.
            "#});
    }

    #[test]
    fn flags_be_an_in_kind_of_style() {
        test::<ClassCheck>()
            .with_options(&kind_of_style())
            .expect_offense(indoc! {r#"
                expect(object).to be_an(String)
                                  ^^^^^ Prefer `be_kind_of` over `be_an`.
            "#});
    }

    #[test]
    fn does_not_flag_be_kind_of_in_kind_of_style() {
        test::<ClassCheck>()
            .with_options(&kind_of_style())
            .expect_no_offenses(indoc! {r#"
                expect(object).to be_kind_of(String)
            "#});
    }

    #[test]
    fn does_not_flag_be_a_kind_of_in_kind_of_style() {
        test::<ClassCheck>()
            .with_options(&kind_of_style())
            .expect_no_offenses(indoc! {r#"
                expect(object).to be_a_kind_of(String)
            "#});
    }

    #[test]
    fn does_not_flag_receiver_be_kind_of() {
        // Upstream requires a bare (`nil?`) receiver.
        test::<ClassCheck>().expect_no_offenses(indoc! {r#"
                expect(foo).to obj.be_kind_of(String)
            "#});
    }
}

murphy_plugin_api::submit_cop!(ClassCheck);
