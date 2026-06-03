//! `Style/ClassCheck` — enforces consistent use of `Object#is_a?` or `Object#kind_of?`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ClassCheck
//! upstream_version_checked: 1.86.2
//! version_added: "0.24"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Full port. Both EnforcedStyle modes (is_a? default, kind_of?) are supported.
//!   Both send and csend are handled (mirrors RuboCop's `alias on_csend on_send`).
//!   Offense range and autocorrect target the selector only (loc.name).
//!   No argument-count guard (RuboCop has none either — any call to is_a?/kind_of?
//!   is flagged when it does not match the enforced style).
//! ```
//!
//! ## Matched shapes
//!
//! - **is_a?** (default): `var.kind_of?(Date)` → `var.is_a?(Date)`
//! - **kind_of?**: `var.is_a?(Date)` → `var.kind_of?(Date)`
//! - Safe-navigation variants (`&.`) are handled identically.
//!
//! ## Autocorrect
//!
//! Surgical rename of the selector (`loc.name`) only.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct ClassCheck;

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ClassCheckStyle {
    #[default]
    #[option(value = "is_a?")]
    IsA,
    #[option(value = "kind_of?")]
    KindOf,
}

impl ClassCheckStyle {
    fn method_name(self) -> &'static str {
        match self {
            ClassCheckStyle::IsA => "is_a?",
            ClassCheckStyle::KindOf => "kind_of?",
        }
    }
}

#[derive(CopOptions)]
pub struct ClassCheckOptions {
    #[option(
        name = "EnforcedStyle",
        default = "is_a?",
        description = "Preferred type-check method name."
    )]
    pub enforced_style: ClassCheckStyle,
}

const MSG: &str = "Prefer `Object#%prefer%` over `Object#%current%`.";

#[cop(
    name = "Style/ClassCheck",
    description = "Enforces consistent use of `Object#is_a?` or `Object#kind_of?`.",
    default_severity = "warning",
    default_enabled = true,
    options = ClassCheckOptions,
)]
impl ClassCheck {
    #[on_node(kind = "send", methods = ["is_a?", "kind_of?"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Csend { method, .. } = *cx.kind(node) else {
            return;
        };
        if matches!(cx.symbol_str(method), "is_a?" | "kind_of?") {
            check(node, cx);
        }
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let opts = cx.options_or_default::<ClassCheckOptions>();
    let method_name = cx.method_name(node).unwrap_or_default();

    // Only flag when the called method does not match the enforced style.
    if method_name == opts.enforced_style.method_name() {
        return;
    }

    let prefer = opts.enforced_style.method_name();
    let current = method_name;
    let message = MSG
        .replace("%prefer%", prefer)
        .replace("%current%", current);

    let selector_range = cx.node(node).loc.name;
    cx.emit_offense(selector_range, &message, None);
    cx.emit_edit(selector_range, opts.enforced_style.method_name());
}

#[cfg(test)]
mod tests {
    use super::*;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- Default style (is_a?): flags kind_of?, no offense for is_a? ---

    #[test]
    fn flags_kind_of_corrects_to_is_a() {
        test::<ClassCheck>().expect_correction(
            indoc! {r#"
                x.kind_of? y
                  ^^^^^^^^ Prefer `Object#is_a?` over `Object#kind_of?`.
            "#},
            "x.is_a? y\n",
        );
    }

    #[test]
    fn flags_csend_kind_of_corrects_to_is_a() {
        test::<ClassCheck>().expect_correction(
            indoc! {r#"
                x&.kind_of? y
                   ^^^^^^^^ Prefer `Object#is_a?` over `Object#kind_of?`.
            "#},
            "x&.is_a? y\n",
        );
    }

    #[test]
    fn no_offense_for_is_a_in_default_mode() {
        test::<ClassCheck>().expect_no_offenses("x.is_a? y\n");
    }

    #[test]
    fn no_offense_for_csend_is_a_in_default_mode() {
        test::<ClassCheck>().expect_no_offenses("x&.is_a? y\n");
    }

    // --- kind_of? style: flags is_a?, no offense for kind_of? ---

    #[test]
    fn kind_of_style_flags_is_a_corrects_to_kind_of() {
        test::<ClassCheck>()
            .with_options(&ClassCheckOptions {
                enforced_style: ClassCheckStyle::KindOf,
            })
            .expect_correction(
                indoc! {r#"
                    x.is_a? y
                      ^^^^^ Prefer `Object#kind_of?` over `Object#is_a?`.
                "#},
                "x.kind_of? y\n",
            );
    }

    #[test]
    fn kind_of_style_flags_csend_is_a_corrects_to_kind_of() {
        test::<ClassCheck>()
            .with_options(&ClassCheckOptions {
                enforced_style: ClassCheckStyle::KindOf,
            })
            .expect_correction(
                indoc! {r#"
                    x&.is_a? y
                       ^^^^^ Prefer `Object#kind_of?` over `Object#is_a?`.
                "#},
                "x&.kind_of? y\n",
            );
    }

    #[test]
    fn kind_of_style_no_offense_for_kind_of() {
        test::<ClassCheck>()
            .with_options(&ClassCheckOptions {
                enforced_style: ClassCheckStyle::KindOf,
            })
            .expect_no_offenses("x.kind_of? y\n");
    }

    #[test]
    fn kind_of_style_no_offense_for_csend_kind_of() {
        test::<ClassCheck>()
            .with_options(&ClassCheckOptions {
                enforced_style: ClassCheckStyle::KindOf,
            })
            .expect_no_offenses("x&.kind_of? y\n");
    }
}

murphy_plugin_api::submit_cop!(ClassCheck);
