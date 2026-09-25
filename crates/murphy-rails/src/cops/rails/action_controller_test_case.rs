//! `Rails/ActionControllerTestCase` — flag `ActionController::TestCase` superclasses.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/ActionControllerTestCase
//! upstream_version_checked: 2.35.0
//! version_added: "0.0"
//! safe: false
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 on_class shape
//!   `(class (const _ _) (const (const {nil? cbase} :ActionController) :TestCase) _)`,
//!   superclass-only offense range, `ActionDispatch::IntegrationTest`
//!   replacement, and TargetRailsVersion >= 5.0 gating (unset means newest).
//!   No Class.new handling upstream (custom on_class only, no EnforceSuperclass).
//!   File scope (`Include: ['**/test/**/*.rb']`) is enforced via the
//!   murphy-rails pack default.yml (engine `cop_applies_to_file` gate,
//!   verified vs rubocop-rails 2.38.0 default.yml, murphy-4gd.1.15).
//! ```
//!
//! ## Matched shapes (Class node)
//!
//! `Class(superclass=Const("ActionController::TestCase"))` — any class name
//! (including namespaced `Foo::Bar::MyControllerTest` and `::`-prefixed
//! names). `const_name` folds leading `::`, so `::ActionController::TestCase`
//! matches identically to the bare form.
//!
//! ## Autocorrect
//!
//! Replace the superclass range with `ActionDispatch::IntegrationTest`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ActionControllerTestCase;

#[cop(
    name = "Rails/ActionControllerTestCase",
    description = "Use `ActionDispatch::IntegrationTest` instead of `ActionController::TestCase`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl ActionControllerTestCase {
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        // Upstream `minimum_target_rails_version 5.0` — unset means newest.
        if !cx.rails_version_at_least(5, 0) {
            return;
        }
        let NodeKind::Class { superclass, .. } = *cx.kind(node) else {
            return;
        };
        let Some(super_id) = superclass.get() else {
            return;
        };
        if cx.const_name(super_id).as_deref() != Some("ActionController::TestCase") {
            return;
        }
        cx.emit_offense(
            cx.range(super_id),
            "Use `ActionDispatch::IntegrationTest` instead.",
            None,
        );
        cx.emit_edit(cx.range(super_id), "ActionDispatch::IntegrationTest");
    }
}

#[cfg(test)]
mod tests {
    use super::ActionControllerTestCase;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_action_controller_test_case() {
        test::<ActionControllerTestCase>().expect_offense(indoc! {r#"
            class MyControllerTest < ActionController::TestCase
                                     ^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `ActionDispatch::IntegrationTest` instead.
            end
        "#});
    }

    #[test]
    fn flags_cbase_action_controller_test_case() {
        test::<ActionControllerTestCase>().expect_offense(indoc! {r#"
            class MyControllerTest < ::ActionController::TestCase
                                     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `ActionDispatch::IntegrationTest` instead.
            end
        "#});
    }

    #[test]
    fn flags_namespaced_class_name() {
        test::<ActionControllerTestCase>().expect_offense(indoc! {r#"
            class Foo::Bar::MyControllerTest < ActionController::TestCase
                                               ^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `ActionDispatch::IntegrationTest` instead.
            end
        "#});
    }

    #[test]
    fn does_not_flag_integration_test() {
        test::<ActionControllerTestCase>().expect_no_offenses(
            "class MyControllerTest < ActionDispatch::IntegrationTest\nend\n",
        );
    }

    #[test]
    fn does_not_flag_custom_superclass() {
        test::<ActionControllerTestCase>()
            .expect_no_offenses("class MyControllerTest < SuperControllerTest\nend\n");
    }

    #[test]
    fn does_not_flag_below_rails_5() {
        test::<ActionControllerTestCase>()
            .with_target_rails_version(4, 2)
            .expect_no_offenses("class MyControllerTest < ActionController::TestCase\nend\n");
    }

    #[test]
    fn fires_at_rails_5() {
        test::<ActionControllerTestCase>()
            .with_target_rails_version(5, 0)
            .expect_offense(indoc! {r#"
                class MyControllerTest < ActionController::TestCase
                                         ^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `ActionDispatch::IntegrationTest` instead.
                end
            "#});
    }

    #[test]
    fn corrects_to_integration_test() {
        test::<ActionControllerTestCase>()
            .expect_correction(
                indoc! {r#"
                    class MyControllerTest < ActionController::TestCase
                                             ^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `ActionDispatch::IntegrationTest` instead.
                    end
                "#},
                "class MyControllerTest < ActionDispatch::IntegrationTest\nend\n",
            )
            .expect_no_offenses(
                "class MyControllerTest < ActionDispatch::IntegrationTest\nend\n",
            );
    }
}
murphy_plugin_api::submit_cop!(ActionControllerTestCase);
