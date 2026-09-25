//! `Rails/HelperInstanceVariable` — do not use instance variables in helpers.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/HelperInstanceVariable
//! upstream_version_checked: 2.35.0
//! version_added: "2.0"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: `on_ivar` flags every ivar read outside a
//!   `class` ancestor, and `on_ivasgn` flags the name range unless the parent
//!   is an `or_asgn` (memoization `@x ||=`). Any `class` ancestor suppresses
//!   (modules alone do not). No autocorrect. Upstream Include
//!   (`**/app/helpers/**/*.rb`) has no file-path infrastructure in Murphy yet,
//!   so the cop fires in all files (audit tracked by murphy-4gd.1.15).
//! ```
//!
//! Checks for use of the helper methods which reference instance variables.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct HelperInstanceVariable;

#[cop(
    name = "Rails/HelperInstanceVariable",
    description = "Do not use instance variables in helpers.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl HelperInstanceVariable {
    #[on_node(kind = "ivar")]
    fn check_ivar(&self, node: NodeId, cx: &Cx<'_>) {
        if belongs_to_class(cx, node) {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            "Do not use instance variables in helpers.",
            None,
        );
    }

    #[on_node(kind = "ivasgn")]
    fn check_ivasgn(&self, node: NodeId, cx: &Cx<'_>) {
        // Upstream `node.parent.or_asgn_type?` — memoization is allowed.
        if let Some(parent) = cx.parent(node).get()
            && matches!(*cx.kind(parent), NodeKind::OrAsgn { .. })
        {
            return;
        }
        if belongs_to_class(cx, node) {
            return;
        }
        // Murphy leaves `loc.name == ZERO` on assignment nodes, so locate
        // the `@name` at the start of the expression range (`symbol_str`
        // carries the `@` sigil).
        let range = cx.range(node);
        let name_range = match *cx.kind(node) {
            NodeKind::Ivasgn { name, .. } => {
                let len = cx.symbol_str(name).len() as u32;
                murphy_plugin_api::Range {
                    start: range.start,
                    end: range.start + len,
                }
            }
            _ => range,
        };
        cx.emit_offense(
            name_range,
            "Do not use instance variables in helpers.",
            None,
        );
    }
}

/// Upstream `instance_variable_belongs_to_class?`: any `:class` ancestor.
fn belongs_to_class(cx: &Cx<'_>, node: NodeId) -> bool {
    cx.ancestors(node)
        .any(|anc| matches!(*cx.kind(anc), NodeKind::Class { .. }))
}

#[cfg(test)]
mod tests {
    use super::HelperInstanceVariable;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_ivar_read() {
        test::<HelperInstanceVariable>().expect_offense(indoc! {r#"
            def welcome_message
              "Hello #{@user.name}"
                       ^^^^^ Do not use instance variables in helpers.
            end
        "#});
    }

    #[test]
    fn flags_ivasgn() {
        test::<HelperInstanceVariable>().expect_offense(indoc! {r#"
            def welcome_message(user)
              @user_name = user.name
              ^^^^^^^^^^ Do not use instance variables in helpers.
            end
        "#});
    }

    #[test]
    fn does_not_flag_local() {
        test::<HelperInstanceVariable>().expect_no_offenses(indoc! {r#"
            def welcome_message(user)
              "Hello #{user.name}"
            end
        "#});
    }

    #[test]
    fn does_not_flag_memoization() {
        test::<HelperInstanceVariable>().expect_no_offenses(indoc! {r#"
            def foo
              @cache ||= heavy_load
            end
        "#});
    }

    #[test]
    fn does_not_flag_inside_class() {
        test::<HelperInstanceVariable>().expect_no_offenses(indoc! {r#"
            module ButtonHelper
              class Button
                def initialize(text:)
                  @text = text
                end
              end

              def button(**args)
                render Button.new(**args)
              end
            end
        "#});
    }

    #[test]
    fn does_not_flag_form_builder_class() {
        test::<HelperInstanceVariable>().expect_no_offenses(indoc! {r#"
            class MyFormBuilder < ActionView::Helpers::FormBuilder
              def do_something
                @template
                @template = do_something
              end
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(HelperInstanceVariable);
