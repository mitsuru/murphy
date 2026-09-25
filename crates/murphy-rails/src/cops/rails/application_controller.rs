//! `Rails/ApplicationController` — flag controllers subclassing `ActionController::Base`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/ApplicationController
//! upstream_version_checked: 2.35.0
//! version_added: "0.0"
//! safe: false
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 EnforceSuperclass shapes: class form
//!   `(class (const _ !:ApplicationController) BASE ...)`, Class.new form
//!   `(send (const {nil? cbase} :Class) :new BASE)` with exactly one arg,
//!   superclass-only offense range, and `ApplicationController` replacement.
//!   Self-definitions (`class ApplicationController < ...`,
//!   `ApplicationController = Class.new(...)`, block form) are excluded.
//!   No TargetRailsVersion gating upstream.
//! ```
//!
//! ## Matched shapes
//!
//! - `Class(name != ApplicationController, superclass="ActionController::Base")`
//! - `Send(receiver=global Class, method="new", args=[Const("ActionController::Base")])`
//!   not directly assigned to `ApplicationController` (including the
//!   `ApplicationController = Class.new(...) { }` block form).
//!
//! `const_name` folds leading `::`, so `::ActionController::Base` matches
//! identically. Offense range is the superclass node only; autocorrect
//! replaces it with `ApplicationController`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

const SUPERCLASS: &str = "ApplicationController";
const BASE: &str = "ActionController::Base";

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ApplicationController;

#[cop(
    name = "Rails/ApplicationController",
    description = "Controllers should subclass `ApplicationController`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl ApplicationController {
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Class { name, superclass, .. } = *cx.kind(node) else {
            return;
        };
        // Exclude `class ApplicationController < ...` self-definition.
        // Upstream `(const _ !:ApplicationController)` checks the short name.
        if short_const_name(cx, name).as_deref() == Some(SUPERCLASS) {
            return;
        }
        let Some(super_id) = superclass.get() else {
            return;
        };
        if cx.const_name(super_id).as_deref() != Some(BASE) {
            return;
        }
        cx.emit_offense(
            cx.range(super_id),
            "Controllers should subclass `ApplicationController`.",
            None,
        );
        cx.emit_edit(cx.range(super_id), SUPERCLASS);
    }

    // Mirrors upstream `on_send` for `Class.new(...)` via EnforceSuperclass.
    #[on_node(kind = "send", methods = ["new"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // Must be `Class.new` with a global `Class` receiver.
        if cx.method_name(node) != Some("new") {
            return;
        }
        let Some(recv) = cx.call_receiver(node).get() else {
            return;
        };
        if !cx.is_global_const(recv, "Class") {
            return;
        }
        let args = cx.call_arguments(node);
        if args.len() != 1 {
            return;
        }
        let base_id = args[0];
        if cx.const_name(base_id).as_deref() != Some(BASE) {
            return;
        }
        // Exclude `ApplicationController = Class.new(...)` (direct casgn parent)
        // and `ApplicationController = Class.new(...) { }` (block parent + casgn grandparent).
        if is_superclass_assignment(cx, node, SUPERCLASS) {
            return;
        }
        cx.emit_offense(
            cx.range(base_id),
            "Controllers should subclass `ApplicationController`.",
            None,
        );
        cx.emit_edit(cx.range(base_id), SUPERCLASS);
    }
}

/// Short (unqualified) const name — the `name` symbol of a Const node.
/// Mirrors upstream `!:SUPERCLASS` which compares the bare constant name.
fn short_const_name(cx: &Cx<'_>, id: NodeId) -> Option<String> {
    match *cx.kind(id) {
        NodeKind::Const { name, .. } => Some(cx.symbol_str(name).to_string()),
        _ => cx.const_name(id),
    }
}

/// True when the `Class.new` send is assigned to `superclass_name`:
/// `Name = Class.new(...)` or `Name = Class.new(...) { }`.
fn is_superclass_assignment(cx: &Cx<'_>, send_id: NodeId, superclass_name: &str) -> bool {
    // Direct: `Name = Class.new(...)` — parent is Casgn.
    if let Some(parent) = cx.parent(send_id).get() {
        match *cx.kind(parent) {
            NodeKind::Casgn { name, .. } if cx.symbol_str(name) == superclass_name => {
                return true;
            }
            NodeKind::Block { call, .. } | NodeKind::Numblock { send: call, .. } | NodeKind::Itblock { send: call, .. }
                if call == send_id =>
            {
                // Block form: `Name = Class.new(...) { }` — grandparent is Casgn.
                if let Some(grand) = cx.parent(parent).get()
                    && let NodeKind::Casgn { name, .. } = *cx.kind(grand)
                    && cx.symbol_str(name) == superclass_name
                {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::ApplicationController;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_controller_subclassing_base() {
        test::<ApplicationController>().expect_offense(indoc! {r#"
            class MyController < ActionController::Base; end
                                 ^^^^^^^^^^^^^^^^^^^^^^ Controllers should subclass `ApplicationController`.
        "#});
    }

    #[test]
    fn flags_cbase_controller_base() {
        test::<ApplicationController>().expect_offense(indoc! {r#"
            class MyController < ::ActionController::Base; end
                                 ^^^^^^^^^^^^^^^^^^^^^^^^ Controllers should subclass `ApplicationController`.
        "#});
    }

    #[test]
    fn allows_application_controller_definition() {
        test::<ApplicationController>()
            .expect_no_offenses("class ApplicationController < ActionController::Base; end\n");
    }

    #[test]
    fn allows_application_controller_class_new() {
        test::<ApplicationController>()
            .expect_no_offenses("ApplicationController = Class.new(ActionController::Base)\n");
    }

    #[test]
    fn flags_class_new_base() {
        test::<ApplicationController>().expect_offense(indoc! {r#"
            MyController = Class.new(ActionController::Base)
                                     ^^^^^^^^^^^^^^^^^^^^^^ Controllers should subclass `ApplicationController`.
        "#});
    }

    #[test]
    fn flags_anonymous_class_new() {
        test::<ApplicationController>().expect_offense(indoc! {r#"
            Class.new(ActionController::Base) {}
                      ^^^^^^^^^^^^^^^^^^^^^^ Controllers should subclass `ApplicationController`.
        "#});
    }

    #[test]
    fn does_not_flag_application_controller_subclass() {
        test::<ApplicationController>()
            .expect_no_offenses("class MyController < ApplicationController; end\n");
    }

    #[test]
    fn corrects_class_superclass() {
        test::<ApplicationController>()
            .expect_correction(
                indoc! {r#"
                    class MyController < ActionController::Base; end
                                         ^^^^^^^^^^^^^^^^^^^^^^ Controllers should subclass `ApplicationController`.
                "#},
                "class MyController < ApplicationController; end\n",
            )
            .expect_no_offenses("class MyController < ApplicationController; end\n");
    }

    #[test]
    fn corrects_class_new() {
        test::<ApplicationController>()
            .expect_correction(
                indoc! {r#"
                    MyController = Class.new(ActionController::Base)
                                             ^^^^^^^^^^^^^^^^^^^^^^ Controllers should subclass `ApplicationController`.
                "#},
                "MyController = Class.new(ApplicationController)\n",
            )
            .expect_no_offenses("MyController = Class.new(ApplicationController)\n");
    }
}
murphy_plugin_api::submit_cop!(ApplicationController);
