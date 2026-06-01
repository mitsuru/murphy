//! `Rails/I18nLocaleAssignment` — flag direct assignment to the
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/I18nLocaleAssignment
//! upstream_version_checked: 2.35.0
//! status: partial
//! gap_issues:
//!   - murphy-l5v2
//! notes: >
//!   Path gating (RuboCop only flags in test/spec paths) is not implemented;
//!   Murphy flags in all files. Known limitation, not a blocker.
//! ```
//!
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
//!   Murphy's AST translator maps both `I18n` and `::I18n` (cbase-qualified
//!   top-level constant) to `Const { scope: None }` — both forms match
//!   the same pattern and are flagged. `Foo::I18n.locale = "x"` (namespaced
//!   with a non-cbase parent) produces `Const { scope: Some(_) }` and is
//!   intentionally ignored.
//! - `method == :locale=` — exactly the attr-writer; reads
//!   (`I18n.locale`), block form (`I18n.with_locale(...)`), and other
//!   attrs (`I18n.config = ...`) are out of scope.
//! - One argument — `attr =` always emits exactly one arg.
//!
//! ## Pending by default
//!
//! Upstream RuboCop-rails ships this cop as `Enabled: pending` — disabled
//! until explicitly opted in, with a deprecation notice if run without
//! opting in. Murphy maps `pending` to `default_enabled = false`, consistent
//! with `Rails/EnvironmentVariableAccess`.
//!
//! ## No autocorrect
//!
//! The block form requires wrapping a code region in
//! `I18n.with_locale(<locale>) { ... }`; the cop has no view of the
//! lexical extent the user wants the locale to apply to. Detect-only.
//!
//! ## Known limitation
//!
//! RuboCop gates this cop to spec/test paths. Murphy does not implement
//! path gating in v1; the cop fires on all files. This is a known gap
//! tracked in `murphy-l5v2`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, cop, def_node_matcher};

// RuboCop NodePattern equivalent: `(send (const nil? :I18n) :locale= _)`.
//
// `nil?` on the Const scope slot matches both absent (`None`, bare `I18n`)
// and the cbase-qualified form (`::I18n`), because Murphy's AST translator
// collapses both to `Const { scope: None }`.
def_node_matcher!(
    is_i18n_locale_assignment,
    "(send (const nil? :I18n) :locale= _)"
);

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct I18nLocaleAssignment;

#[cop(
    name = "Rails/I18nLocaleAssignment",
    description = "Use `I18n.with_locale` with block instead of `I18n.locale=`.",
    default_severity = "warning",
    default_enabled = false,
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
            "Use `I18n.with_locale` with block instead of `I18n.locale=`.",
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::I18nLocaleAssignment;
    use murphy_plugin_api::test_support::{indoc, test};

    // === hit cases ===

    #[test]
    fn flags_i18n_locale_string_literal() {
        test::<I18nLocaleAssignment>().expect_offense(indoc! {r#"
                I18n.locale = "ja"
                ^^^^^^^^^^^^^^^^^^ Use `I18n.with_locale` with block instead of `I18n.locale=`.
            "#});
    }

    #[test]
    fn flags_i18n_locale_symbol() {
        test::<I18nLocaleAssignment>().expect_offense(indoc! {r#"
                I18n.locale = :en
                ^^^^^^^^^^^^^^^^^ Use `I18n.with_locale` with block instead of `I18n.locale=`.
            "#});
    }

    #[test]
    fn flags_i18n_locale_variable() {
        test::<I18nLocaleAssignment>().expect_offense(indoc! {r#"
                I18n.locale = locale
                ^^^^^^^^^^^^^^^^^^^^ Use `I18n.with_locale` with block instead of `I18n.locale=`.
            "#});
    }

    #[test]
    fn flags_cbase_qualified_i18n_locale() {
        // `::I18n.locale = :en` — the cbase qualifier (`::`) makes this
        // an explicit top-level constant reference. Murphy's AST translator
        // collapses cbase-parent Const nodes to `Const { scope: None }`,
        // identical to the bare `I18n` form, so the same pattern matches.
        test::<I18nLocaleAssignment>().expect_offense(indoc! {r#"
                ::I18n.locale = :en
                ^^^^^^^^^^^^^^^^^^^ Use `I18n.with_locale` with block instead of `I18n.locale=`.
            "#});
    }

    // === no-hit cases ===

    #[test]
    fn does_not_flag_with_locale_block() {
        // The block form is exactly the recommended fix.
        test::<I18nLocaleAssignment>()
            .expect_no_offenses("I18n.with_locale(\"ja\") { translate(:hello) }\n");
    }

    #[test]
    fn does_not_flag_i18n_locale_read() {
        // `I18n.locale` (read) — not an assignment.
        test::<I18nLocaleAssignment>().expect_no_offenses("current = I18n.locale\n");
    }

    #[test]
    fn does_not_flag_scoped_i18n() {
        // `Foo::I18n.locale = "x"` is some other namespace's I18n
        // (non-cbase parent scope → not matched).
        test::<I18nLocaleAssignment>().expect_no_offenses("Foo::I18n.locale = \"x\"\n");
    }

    #[test]
    fn does_not_flag_other_i18n_attr() {
        // `I18n.config = ...` and other writers are out of scope.
        test::<I18nLocaleAssignment>().expect_no_offenses("I18n.config = config_obj\n");
    }

    #[test]
    fn does_not_flag_lvar_i18n_locale() {
        // Lowercase `i18n` is a local — not the I18n const.
        test::<I18nLocaleAssignment>().expect_no_offenses("i18n.locale = \"ja\"\n");
    }
}
murphy_plugin_api::submit_cop!(I18nLocaleAssignment);
