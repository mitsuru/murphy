//! `Rails/I18nLocaleAssignment` — flag direct assignment to the
//! `I18n.locale` attribute (`I18n.locale = "ja"`). Direct assignment
//! is process-global and leaks across requests / threads in
//! environments without per-request isolation; the
//! `I18n.with_locale(locale) { ... }` block-form sets the locale only
//! for the lexical scope and restores the previous value, which is
//! what Rails apps actually want.
//!
//! ## Matched shape (Send node)
//!
//! `Send(receiver=Const{scope=None, name="I18n"}, method=:locale=, args=[_])`.
//!
//! - `Const { scope: None }` restricts to the top-level `I18n`.
//!   `Foo::I18n.locale = "x"` (namespaced) is intentionally ignored.
//! - `method == :locale=` — exactly the attr-writer; reads
//!   (`I18n.locale`), block form (`I18n.with_locale(...)`), and other
//!   attrs (`I18n.config = ...`) are out of scope.
//! - One argument — `attr =` always emits exactly one arg.
//!
//! ## No autocorrect
//!
//! The block form requires wrapping a code region in
//! `I18n.with_locale(<locale>) { ... }`; the cop has no view of the
//! lexical extent the user wants the locale to apply to. Detect-only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, cop, node_pattern};

// RuboCop NodePattern equivalent: `(send (const nil? :I18n) :locale= _)`.
node_pattern!(
    is_i18n_locale_assignment,
    "(send (const nil? :I18n) :locale= _)"
);

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct I18nLocaleAssignment;

#[cop(
    name = "Rails/I18nLocaleAssignment",
    description = "Use `I18n.with_locale(...) { ... }` instead of `I18n.locale = ...` to keep the change request-local.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl I18nLocaleAssignment {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        if !is_i18n_locale_assignment(node, cx) {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            "Use `I18n.with_locale(...) { ... }` instead of `I18n.locale = ...` to keep the change request-local.",
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::I18nLocaleAssignment;
    use murphy_plugin_api::test_support::{expect_no_offenses, expect_offense, indoc};

    // === hit cases ===

    #[test]
    fn flags_i18n_locale_string_literal() {
        expect_offense!(
            I18nLocaleAssignment,
            indoc! {r#"
                I18n.locale = "ja"
                ^^^^^^^^^^^^^^^^^^ Use `I18n.with_locale(...) { ... }` instead of `I18n.locale = ...` to keep the change request-local.
            "#}
        );
    }

    #[test]
    fn flags_i18n_locale_symbol() {
        expect_offense!(
            I18nLocaleAssignment,
            indoc! {r#"
                I18n.locale = :en
                ^^^^^^^^^^^^^^^^^ Use `I18n.with_locale(...) { ... }` instead of `I18n.locale = ...` to keep the change request-local.
            "#}
        );
    }

    #[test]
    fn flags_i18n_locale_variable() {
        expect_offense!(
            I18nLocaleAssignment,
            indoc! {r#"
                I18n.locale = locale
                ^^^^^^^^^^^^^^^^^^^^ Use `I18n.with_locale(...) { ... }` instead of `I18n.locale = ...` to keep the change request-local.
            "#}
        );
    }

    // === no-hit cases ===

    #[test]
    fn does_not_flag_with_locale_block() {
        // The block form is exactly the recommended fix.
        expect_no_offenses!(
            I18nLocaleAssignment,
            "I18n.with_locale(\"ja\") { translate(:hello) }\n"
        );
    }

    #[test]
    fn does_not_flag_i18n_locale_read() {
        // `I18n.locale` (read) — not an assignment.
        expect_no_offenses!(I18nLocaleAssignment, "current = I18n.locale\n");
    }

    #[test]
    fn does_not_flag_scoped_i18n() {
        // `Foo::I18n.locale = "x"` is some other namespace's I18n.
        expect_no_offenses!(I18nLocaleAssignment, "Foo::I18n.locale = \"x\"\n");
    }

    #[test]
    fn does_not_flag_other_i18n_attr() {
        // `I18n.config = ...` and other writers are out of scope.
        expect_no_offenses!(I18nLocaleAssignment, "I18n.config = config_obj\n");
    }

    #[test]
    fn does_not_flag_lvar_i18n_locale() {
        // Lowercase `i18n` is a local — not the I18n const.
        expect_no_offenses!(I18nLocaleAssignment, "i18n.locale = \"ja\"\n");
    }
}
