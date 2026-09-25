//! `Rails/SafeNavigationWithBlank` — avoid `foo&.blank?` in conditionals.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/SafeNavigationWithBlank
//! upstream_version_checked: 2.35.0
//! version_added: "2.4"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_if`: flags when the `if` condition is
//!   directly a `csend` `blank?` call (modifier, keyword, and ternary forms;
//!   `unless` included). `&&`-wrapped, `begin`-wrapped, and `!`-negated
//!   conditions never match, matching the upstream node pattern. Whole-`if`
//!   offense with a minimal `&.` → `.` dot-range replacement. Autocorrect
//!   is unsafe upstream (`SafeAutoCorrect: false`).
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SafeNavigationWithBlank;

#[cop(
    name = "Rails/SafeNavigationWithBlank",
    description = "Avoid `foo&.blank?` in conditionals.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl SafeNavigationWithBlank {
    // Mirrors upstream `on_if`.
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if !matches!(*cx.kind(node), NodeKind::If { .. }) {
        return;
    }
    // Upstream `(if $(csend ... :blank?) ...)`: the condition itself must be
    // the safe-navigation `blank?` call — no `and`/`begin`/`!` wrappers.
    let Some(cond) = cx.if_condition(node).get() else {
        return;
    };
    if !matches!(*cx.kind(cond), NodeKind::Csend { .. }) {
        return;
    }
    if cx.method_name(cond) != Some("blank?") {
        return;
    }
    cx.emit_offense(
        cx.range(node),
        "Avoid calling `blank?` with the safe navigation operator in conditionals.",
        None,
    );
    cx.emit_edit(cx.loc(cond).dot(), ".");
}

#[cfg(test)]
mod tests {
    use super::SafeNavigationWithBlank;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_modifier_if() {
        test::<SafeNavigationWithBlank>().expect_correction(
            indoc! {r#"
                do_something if foo&.blank?
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid calling `blank?` with the safe navigation operator in conditionals.
            "#},
            "do_something if foo.blank?\n",
        );
    }

    #[test]
    fn flags_modifier_unless() {
        test::<SafeNavigationWithBlank>().expect_correction(
            indoc! {r#"
                do_something unless foo&.blank?
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid calling `blank?` with the safe navigation operator in conditionals.
            "#},
            "do_something unless foo.blank?\n",
        );
    }

    #[test]
    fn flags_keyword_if() {
        test::<SafeNavigationWithBlank>().expect_correction(
            indoc! {r#"
                if foo&.blank? then do_something end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid calling `blank?` with the safe navigation operator in conditionals.
            "#},
            "if foo.blank? then do_something end\n",
        );
    }

    #[test]
    fn flags_ternary() {
        test::<SafeNavigationWithBlank>().expect_correction(
            indoc! {r#"
                a = foo&.blank? ? 1 : 2
                    ^^^^^^^^^^^^^^^^^^^ Avoid calling `blank?` with the safe navigation operator in conditionals.
            "#},
            "a = foo.blank? ? 1 : 2\n",
        );
    }

    #[test]
    fn allows_plain_blank() {
        test::<SafeNavigationWithBlank>().expect_no_offenses("do_something if foo.blank?\n");
    }

    #[test]
    fn allows_bare_csend() {
        test::<SafeNavigationWithBlank>().expect_no_offenses("foo&.blank?\n");
    }

    #[test]
    fn allows_and_condition() {
        test::<SafeNavigationWithBlank>()
            .expect_no_offenses("do_something if foo&.blank? && x\n");
    }

    #[test]
    fn allows_begin_condition() {
        test::<SafeNavigationWithBlank>()
            .expect_no_offenses("do_something unless (foo&.blank?)\n");
    }

    #[test]
    fn allows_negated_condition() {
        test::<SafeNavigationWithBlank>()
            .expect_no_offenses("do_something if !foo&.blank?\n");
    }
}
murphy_plugin_api::submit_cop!(SafeNavigationWithBlank);
