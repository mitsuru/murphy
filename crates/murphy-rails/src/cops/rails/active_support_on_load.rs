//! `Rails/ActiveSupportOnLoad` — patch framework classes via `ActiveSupport.on_load`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/ActiveSupportOnLoad
//! upstream_version_checked: 2.35.0
//! version_added: "2.16"
//! version_changed: "2.24"
//! safe: true
//! safe_autocorrect: false
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_send` (no `on_csend` alias):
//!   `include` / `prepend` / `extend` with at least one argument on a const
//!   receiver listed in `LOAD_HOOKS` (plus `RAILS_5_2_LOAD_HOOKS` when
//!   `TargetRailsVersion >= 5.2` and `RAILS_7_1_LOAD_HOOKS` when >= 7.1;
//!   unset means newest, via `rails_version_at_least`). Upstream destructures
//!   `receiver, method, arguments = *node` where `arguments` is the FIRST
//!   argument only, so multi-arg calls correct to the first argument —
//!   mirrored here. Offense on the whole send; autocorrect replaces it with
//!   `ActiveSupport.on_load(:hook) { method first_arg }`.
//! ```
//!
//! ## Matched shapes
//!
//! - `ActiveRecord::Base.include(MyClass)` →
//!   `ActiveSupport.on_load(:active_record) { include MyClass }`
//! - `::ActiveRecord::Base.extend(MyClass)` → same hook (`const_name` folds
//!   the leading `::`, matching the other Rails cops)
//! - `foo.extend(MyClass)` / bare `include` / `include?` → no offense

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ActiveSupportOnLoad;

#[cop(
    name = "Rails/ActiveSupportOnLoad",
    description = "Use `ActiveSupport.on_load(...)` to patch Rails framework classes.",
    default_enabled = false,
    options = NoOptions,
)]
impl ActiveSupportOnLoad {
    // Mirrors upstream `RESTRICT_ON_SEND = %i[prepend include extend]`.
    // Upstream has no `on_csend` alias.
    #[on_node(kind = "send", methods = ["prepend", "include", "extend"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// `(framework const name, load hook)` pairs from upstream `LOAD_HOOKS`.
const LOAD_HOOKS: &[(&str, &str)] = &[
    ("ActionCable", "action_cable"),
    ("ActionCable::Channel::Base", "action_cable_channel"),
    ("ActionCable::Connection::Base", "action_cable_connection"),
    (
        "ActionCable::Connection::TestCase",
        "action_cable_connection_test_case",
    ),
    ("ActionController::API", "action_controller"),
    ("ActionController::Base", "action_controller"),
    (
        "ActionController::TestCase",
        "action_controller_test_case",
    ),
    (
        "ActionDispatch::IntegrationTest",
        "action_dispatch_integration_test",
    ),
    ("ActionDispatch::Request", "action_dispatch_request"),
    ("ActionDispatch::Response", "action_dispatch_response"),
    (
        "ActionDispatch::SystemTestCase",
        "action_dispatch_system_test_case",
    ),
    ("ActionMailbox::Base", "action_mailbox"),
    (
        "ActionMailbox::InboundEmail",
        "action_mailbox_inbound_email",
    ),
    ("ActionMailbox::Record", "action_mailbox_record"),
    ("ActionMailbox::TestCase", "action_mailbox_test_case"),
    ("ActionMailer::Base", "action_mailer"),
    ("ActionMailer::TestCase", "action_mailer_test_case"),
    ("ActionText::Content", "action_text_content"),
    ("ActionText::Record", "action_text_record"),
    ("ActionText::RichText", "action_text_rich_text"),
    ("ActionView::Base", "action_view"),
    ("ActionView::TestCase", "action_view_test_case"),
    ("ActiveJob::Base", "active_job"),
    ("ActiveJob::TestCase", "active_job_test_case"),
    ("ActiveRecord::Base", "active_record"),
    ("ActiveStorage::Attachment", "active_storage_attachment"),
    ("ActiveStorage::Blob", "active_storage_blob"),
    ("ActiveStorage::Record", "active_storage_record"),
    (
        "ActiveStorage::VariantRecord",
        "active_storage_variant_record",
    ),
    ("ActiveSupport::TestCase", "active_support_test_case"),
];

fn hook_for_const(cx: &Cx<'_>, const_name: &str) -> Option<&'static str> {
    if let Some((_, hook)) = LOAD_HOOKS.iter().find(|(name, _)| *name == const_name) {
        return Some(hook);
    }
    // Upstream `RAILS_5_2_LOAD_HOOKS`, gated on `target_rails_version >= 5.2`.
    if const_name == "ActiveRecord::ConnectionAdapters::SQLite3Adapter"
        && cx.rails_version_at_least(5, 2)
    {
        return Some("active_record_sqlite3adapter");
    }
    // Upstream `RAILS_7_1_LOAD_HOOKS`, gated on `target_rails_version >= 7.1`.
    if cx.rails_version_at_least(7, 1) {
        return match const_name {
            "ActiveRecord::TestFixtures" => Some("active_record_fixtures"),
            "ActiveModel::Model" => Some("active_model"),
            "ActionText::EncryptedRichText" => Some("action_text_encrypted_rich_text"),
            "ActiveRecord::ConnectionAdapters::PostgreSQLAdapter" => {
                Some("active_record_postgresqladapter")
            }
            "ActiveRecord::ConnectionAdapters::Mysql2Adapter" => {
                Some("active_record_mysql2adapter")
            }
            "ActiveRecord::ConnectionAdapters::TrilogyAdapter" => {
                Some("active_record_trilogyadapter")
            }
            _ => None,
        };
    }
    None
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
        return;
    }
    let method = match cx.method_name(node) {
        Some(m) => m.to_owned(),
        None => return,
    };
    let args = cx.call_arguments(node);
    // Upstream `return unless arguments` — the destructured first argument.
    let Some(&first) = args.first() else {
        return;
    };
    let Some(recv) = cx.call_receiver(node).get() else {
        return;
    };
    let Some(name) = cx.const_name(recv) else {
        return;
    };
    let Some(hook) = hook_for_const(cx, &name) else {
        return;
    };
    let first_src = cx.raw_source(cx.range(first)).to_owned();
    let node_src = cx.raw_source(cx.range(node)).to_owned();
    let preferred = format!("ActiveSupport.on_load(:{hook}) {{ {method} {first_src} }}");
    let msg = format!("Use `{preferred}` instead of `{node_src}`.");
    cx.emit_offense(cx.range(node), &msg, None);
    cx.emit_edit(cx.range(node), &preferred);
}

#[cfg(test)]
mod tests {
    use super::ActiveSupportOnLoad;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_include() {
        test::<ActiveSupportOnLoad>().expect_correction(
            indoc! {r#"
                ActiveRecord::Base.include(MyClass)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `ActiveSupport.on_load(:active_record) { include MyClass }` instead of `ActiveRecord::Base.include(MyClass)`.
            "#},
            "ActiveSupport.on_load(:active_record) { include MyClass }\n",
        );
    }

    #[test]
    fn flags_prepend() {
        test::<ActiveSupportOnLoad>().expect_correction(
            indoc! {r#"
                ActiveRecord::Base.prepend(MyClass)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `ActiveSupport.on_load(:active_record) { prepend MyClass }` instead of `ActiveRecord::Base.prepend(MyClass)`.
            "#},
            "ActiveSupport.on_load(:active_record) { prepend MyClass }\n",
        );
    }

    #[test]
    fn flags_extend() {
        test::<ActiveSupportOnLoad>().expect_correction(
            indoc! {r#"
                ActiveRecord::Base.extend(MyClass)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `ActiveSupport.on_load(:active_record) { extend MyClass }` instead of `ActiveRecord::Base.extend(MyClass)`.
            "#},
            "ActiveSupport.on_load(:active_record) { extend MyClass }\n",
        );
    }

    #[test]
    fn flags_absolute_const() {
        test::<ActiveSupportOnLoad>().expect_correction(
            indoc! {r#"
                ::ActiveRecord::Base.extend(MyClass)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `ActiveSupport.on_load(:active_record) { extend MyClass }` instead of `::ActiveRecord::Base.extend(MyClass)`.
            "#},
            "ActiveSupport.on_load(:active_record) { extend MyClass }\n",
        );
    }

    #[test]
    fn flags_variable_argument() {
        test::<ActiveSupportOnLoad>().expect_correction(
            indoc! {r#"
                ActiveRecord::Base.extend(my_class)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `ActiveSupport.on_load(:active_record) { extend my_class }` instead of `ActiveRecord::Base.extend(my_class)`.
            "#},
            "ActiveSupport.on_load(:active_record) { extend my_class }\n",
        );
    }

    #[test]
    fn ignores_include_without_arguments() {
        test::<ActiveSupportOnLoad>()
            .expect_no_offenses("ActiveRecord::Base.include\n");
    }

    #[test]
    fn ignores_variable_receiver() {
        test::<ActiveSupportOnLoad>().expect_no_offenses("foo.extend(MyClass)\n");
    }

    #[test]
    fn ignores_on_load_form() {
        test::<ActiveSupportOnLoad>().expect_no_offenses(
            "ActiveSupport.on_load(:active_record) { include MyClass }\n",
        );
    }

    #[test]
    fn ignores_other_method_on_framework() {
        test::<ActiveSupportOnLoad>()
            .expect_no_offenses("ActiveRecord::Base.include_root_in_json = false\n");
    }

    #[test]
    fn ignores_include_predicate() {
        test::<ActiveSupportOnLoad>().expect_no_offenses("name.include?('bob')\n");
    }

    #[test]
    fn ignores_unsupported_class() {
        test::<ActiveSupportOnLoad>().expect_no_offenses("MyClass1.prepend(MyClass)\n");
    }

    #[test]
    fn flags_rails52_hook_by_default() {
        test::<ActiveSupportOnLoad>().expect_correction(
            indoc! {r#"
                ActiveRecord::ConnectionAdapters::SQLite3Adapter.include(MyClass)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `ActiveSupport.on_load(:active_record_sqlite3adapter) { include MyClass }` instead of `ActiveRecord::ConnectionAdapters::SQLite3Adapter.include(MyClass)`.
            "#},
            "ActiveSupport.on_load(:active_record_sqlite3adapter) { include MyClass }\n",
        );
    }

    #[test]
    fn ignores_rails71_hook_below_71() {
        test::<ActiveSupportOnLoad>()
            .with_target_rails_version(5, 2)
            .expect_no_offenses("ActiveRecord::TestFixtures.include(MyClass)\n");
    }

    #[test]
    fn flags_rails71_hook_at_71() {
        test::<ActiveSupportOnLoad>()
            .with_target_rails_version(7, 1)
            .expect_correction(
                indoc! {r#"
                    ActiveRecord::TestFixtures.include(MyClass)
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `ActiveSupport.on_load(:active_record_fixtures) { include MyClass }` instead of `ActiveRecord::TestFixtures.include(MyClass)`.
                "#},
                "ActiveSupport.on_load(:active_record_fixtures) { include MyClass }\n",
            );
    }
}

murphy_plugin_api::submit_cop!(ActiveSupportOnLoad);
