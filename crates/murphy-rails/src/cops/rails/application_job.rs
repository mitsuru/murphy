//! `Rails/ApplicationJob` — flag jobs subclassing `ActiveJob::Base`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/ApplicationJob
//! upstream_version_checked: 2.35.0
//! version_added: "0.49"
//! version_changed: "2.5"
//! safe: false
//! safe_autocorrect: false
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 EnforceSuperclass shapes with
//!   TargetRailsVersion >= 5.0 gating (unset means newest): class form
//!   `(class (const _ !:ApplicationJob) (const (const {nil? cbase}
//!   :ActiveJob) :Base) ...)`, Class.new form
//!   `(send (const {nil? cbase} :Class) :new BASE)` with exactly one arg,
//!   superclass-only offense range, and `ApplicationJob` replacement.
//!   Self-definitions (`class ApplicationJob < ...`,
//!   `ApplicationJob = Class.new(...)`, block form) are excluded.
//! ```
//!
//! ## Matched shapes
//!
//! - `Class(name != ApplicationJob, superclass="ActiveJob::Base")`
//! - `Send(receiver=global Class, method="new", args=[Const("ActiveJob::Base")])`
//!   not directly assigned to `ApplicationJob` (including the
//!   `ApplicationJob = Class.new(...) { }` block form).
//!
//! `const_name` folds leading `::`, so `::ActiveJob::Base` matches
//! identically. Offense range is the superclass node only; autocorrect
//! replaces it with `ApplicationJob`.
//!
//! Below Rails 5.0 (explicit `TargetRailsVersion: 4.2`) no offense fires.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

const SUPERCLASS: &str = "ApplicationJob";
const BASE: &str = "ActiveJob::Base";

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ApplicationJob;

#[cop(
    name = "Rails/ApplicationJob",
    description = "Jobs should subclass `ApplicationJob`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl ApplicationJob {
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        // Upstream `minimum_target_rails_version 5.0` — unset means newest.
        if !cx.rails_version_at_least(5, 0) {
            return;
        }
        let NodeKind::Class { name, superclass, .. } = *cx.kind(node) else {
            return;
        };
        // Exclude `class ApplicationJob < ...` self-definition.
        // Upstream `(const _ !:ApplicationJob)` checks the short name.
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
            "Jobs should subclass `ApplicationJob`.",
            None,
        );
        cx.emit_edit(cx.range(super_id), SUPERCLASS);
    }

    // Mirrors upstream `on_send` for `Class.new(...)` via EnforceSuperclass.
    #[on_node(kind = "send", methods = ["new"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        if !cx.rails_version_at_least(5, 0) {
            return;
        }
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
        // Exclude `ApplicationJob = Class.new(...)` (direct casgn parent)
        // and `ApplicationJob = Class.new(...) { }` (block parent + casgn grandparent).
        if is_superclass_assignment(cx, node, SUPERCLASS) {
            return;
        }
        cx.emit_offense(
            cx.range(base_id),
            "Jobs should subclass `ApplicationJob`.",
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
    use super::ApplicationJob;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_job_subclassing_base() {
        test::<ApplicationJob>().expect_offense(indoc! {r#"
            class MyJob < ActiveJob::Base; end
                          ^^^^^^^^^^^^^^^ Jobs should subclass `ApplicationJob`.
        "#});
    }

    #[test]
    fn flags_cbase_job_base() {
        test::<ApplicationJob>().expect_offense(indoc! {r#"
            class MyJob < ::ActiveJob::Base; end
                          ^^^^^^^^^^^^^^^^^ Jobs should subclass `ApplicationJob`.
        "#});
    }

    #[test]
    fn flags_nested_module_job() {
        test::<ApplicationJob>().expect_offense(indoc! {r#"
            module Nested
              class MyJob < ActiveJob::Base; end
                            ^^^^^^^^^^^^^^^ Jobs should subclass `ApplicationJob`.
            end
        "#});
    }

    #[test]
    fn allows_application_job_definition() {
        test::<ApplicationJob>()
            .expect_no_offenses("class ApplicationJob < ActiveJob::Base; end\n");
    }

    #[test]
    fn allows_application_job_class_new() {
        test::<ApplicationJob>()
            .expect_no_offenses("ApplicationJob = Class.new(ActiveJob::Base)\n");
    }

    #[test]
    fn flags_class_new_base() {
        test::<ApplicationJob>().expect_offense(indoc! {r#"
            MyJob = Class.new(ActiveJob::Base)
                              ^^^^^^^^^^^^^^^ Jobs should subclass `ApplicationJob`.
        "#});
    }

    #[test]
    fn flags_anonymous_class_new() {
        test::<ApplicationJob>().expect_offense(indoc! {r#"
            Class.new(ActiveJob::Base) {}
                      ^^^^^^^^^^^^^^^ Jobs should subclass `ApplicationJob`.
        "#});
    }

    #[test]
    fn does_not_flag_below_rails_5() {
        test::<ApplicationJob>()
            .with_target_rails_version(4, 2)
            .expect_no_offenses("class MyJob < ActiveJob::Base; end\n");
    }

    #[test]
    fn fires_at_rails_5() {
        test::<ApplicationJob>()
            .with_target_rails_version(5, 0)
            .expect_offense(indoc! {r#"
                class MyJob < ActiveJob::Base; end
                              ^^^^^^^^^^^^^^^ Jobs should subclass `ApplicationJob`.
            "#});
    }

    #[test]
    fn corrects_class_superclass() {
        test::<ApplicationJob>()
            .expect_correction(
                indoc! {r#"
                    class MyJob < ActiveJob::Base; end
                                  ^^^^^^^^^^^^^^^ Jobs should subclass `ApplicationJob`.
                "#},
                "class MyJob < ApplicationJob; end\n",
            )
            .expect_no_offenses("class MyJob < ApplicationJob; end\n");
    }

    #[test]
    fn corrects_class_new() {
        test::<ApplicationJob>()
            .expect_correction(
                indoc! {r#"
                    MyJob = Class.new(ActiveJob::Base)
                                      ^^^^^^^^^^^^^^^ Jobs should subclass `ApplicationJob`.
                "#},
                "MyJob = Class.new(ApplicationJob)\n",
            )
            .expect_no_offenses("MyJob = Class.new(ApplicationJob)\n");
    }
}
murphy_plugin_api::submit_cop!(ApplicationJob);
