//! `Rails/ApplicationRecord` — flag models subclassing `ActiveRecord::Base`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/ApplicationRecord
//! upstream_version_checked: 2.35.0
//! version_added: "0.0"
//! safe: false
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 EnforceSuperclass shapes with
//!   TargetRailsVersion >= 5.0 gating (unset means newest): class form,
//!   Class.new form (exactly one arg), superclass-only offense range, and
//!   `ApplicationRecord` replacement. Self-definitions are excluded.
//!   File scope (`Exclude: ['db/**/*.rb']`) is enforced via the murphy-rails
//!   pack default.yml (engine `cop_applies_to_file` gate, verified vs
//!   rubocop-rails 2.38.0 default.yml, murphy-4gd.1.15).
//! ```
//!
//! ## Matched shapes
//!
//! - `Class(name != ApplicationRecord, superclass="ActiveRecord::Base")`
//! - `Send(receiver=global Class, method="new", args=[Const("ActiveRecord::Base")])`
//!   not assigned to `ApplicationRecord`.
//!
//! Below Rails 5.0 (explicit `TargetRailsVersion: 4.2`) no offense fires.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

const SUPERCLASS: &str = "ApplicationRecord";
const BASE: &str = "ActiveRecord::Base";

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ApplicationRecord;

#[cop(
    name = "Rails/ApplicationRecord",
    description = "Models should subclass `ApplicationRecord`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl ApplicationRecord {
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        if !cx.rails_version_at_least(5, 0) {
            return;
        }
        let NodeKind::Class { name, superclass, .. } = *cx.kind(node) else {
            return;
        };
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
            "Models should subclass `ApplicationRecord`.",
            None,
        );
        cx.emit_edit(cx.range(super_id), SUPERCLASS);
    }

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
        if is_superclass_assignment(cx, node, SUPERCLASS) {
            return;
        }
        cx.emit_offense(
            cx.range(base_id),
            "Models should subclass `ApplicationRecord`.",
            None,
        );
        cx.emit_edit(cx.range(base_id), SUPERCLASS);
    }
}

fn short_const_name(cx: &Cx<'_>, id: NodeId) -> Option<String> {
    match *cx.kind(id) {
        NodeKind::Const { name, .. } => Some(cx.symbol_str(name).to_string()),
        _ => cx.const_name(id),
    }
}

fn is_superclass_assignment(cx: &Cx<'_>, send_id: NodeId, superclass_name: &str) -> bool {
    if let Some(parent) = cx.parent(send_id).get() {
        match *cx.kind(parent) {
            NodeKind::Casgn { name, .. } if cx.symbol_str(name) == superclass_name => {
                return true;
            }
            NodeKind::Block { call, .. } | NodeKind::Numblock { send: call, .. } | NodeKind::Itblock { send: call, .. }
                if call == send_id =>
            {
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
    use super::ApplicationRecord;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_model_subclassing_base() {
        test::<ApplicationRecord>().expect_offense(indoc! {r#"
            class MyModel < ActiveRecord::Base
                            ^^^^^^^^^^^^^^^^^^ Models should subclass `ApplicationRecord`.
            end
        "#});
    }

    #[test]
    fn flags_cbase_model_base() {
        test::<ApplicationRecord>().expect_offense(indoc! {r#"
            class MyModel < ::ActiveRecord::Base
                            ^^^^^^^^^^^^^^^^^^^^ Models should subclass `ApplicationRecord`.
            end
        "#});
    }

    #[test]
    fn allows_application_record_definition() {
        test::<ApplicationRecord>()
            .expect_no_offenses("class ApplicationRecord < ActiveRecord::Base\nend\n");
    }

    #[test]
    fn allows_application_record_class_new() {
        test::<ApplicationRecord>()
            .expect_no_offenses("ApplicationRecord = Class.new(ActiveRecord::Base)\n");
    }

    #[test]
    fn flags_class_new_base() {
        test::<ApplicationRecord>().expect_offense(indoc! {r#"
            MyModel = Class.new(ActiveRecord::Base)
                                ^^^^^^^^^^^^^^^^^^ Models should subclass `ApplicationRecord`.
        "#});
    }

    #[test]
    fn flags_anonymous_class_new() {
        test::<ApplicationRecord>().expect_offense(indoc! {r#"
            Class.new(ActiveRecord::Base) {}
                      ^^^^^^^^^^^^^^^^^^ Models should subclass `ApplicationRecord`.
        "#});
    }

    #[test]
    fn does_not_flag_below_rails_5() {
        test::<ApplicationRecord>()
            .with_target_rails_version(4, 2)
            .expect_no_offenses("class MyModel < ActiveRecord::Base; end\n");
    }

    #[test]
    fn fires_at_rails_5() {
        test::<ApplicationRecord>()
            .with_target_rails_version(5, 0)
            .expect_offense(indoc! {r#"
                class MyModel < ActiveRecord::Base; end
                                ^^^^^^^^^^^^^^^^^^ Models should subclass `ApplicationRecord`.
            "#});
    }

    #[test]
    fn corrects_class_superclass() {
        test::<ApplicationRecord>()
            .expect_correction(
                indoc! {r#"
                    class MyModel < ActiveRecord::Base; end
                                    ^^^^^^^^^^^^^^^^^^ Models should subclass `ApplicationRecord`.
                "#},
                "class MyModel < ApplicationRecord; end\n",
            )
            .expect_no_offenses("class MyModel < ApplicationRecord; end\n");
    }

    #[test]
    fn corrects_class_new() {
        test::<ApplicationRecord>()
            .expect_correction(
                indoc! {r#"
                    MyModel = Class.new(ActiveRecord::Base)
                                        ^^^^^^^^^^^^^^^^^^ Models should subclass `ApplicationRecord`.
                "#},
                "MyModel = Class.new(ApplicationRecord)\n",
            )
            .expect_no_offenses("MyModel = Class.new(ApplicationRecord)\n");
    }
}
murphy_plugin_api::submit_cop!(ApplicationRecord);
