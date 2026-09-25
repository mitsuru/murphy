//! `Rails/MailerName` — mailer class names must end with `Mailer`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/MailerName
//! upstream_version_checked: 2.35.0
//! version_added: "2.7"
//! safe: false
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: class form `(class $(const _ !suffix)
//!   #mailer_base ...)` plus `Class.new(base)` / `::Class.new(base)`
//!   assigned via the nearest ancestor casgn. Offense is the class short
//!   name (class form) or the casgn name (Class.new form); autocorrect
//!   appends `Mailer` preserving any `Foo::` scope or `::` prefix on the
//!   base. Upstream Include path gating (`**/app/mailers/**/*.rb`) is
//!   enforced via the murphy-rails pack default.yml (engine
//!   `cop_applies_to_file` gate, verified vs rubocop-rails 2.38.0
//!   default.yml, murphy-4gd.1.15).
//! ```
//!
//! Enforces that mailer names end with `Mailer` suffix.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct MailerName;

#[cop(
    name = "Rails/MailerName",
    description = "Mailer should end with `Mailer` suffix.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl MailerName {
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Class { name, superclass, .. } = *cx.kind(node) else {
            return;
        };
        let Some(super_id) = superclass.get() else {
            return;
        };
        if !is_mailer_base(cx, super_id) {
            return;
        }
        let Some(short) = short_const_name(cx, name) else {
            return;
        };
        if short.ends_with("Mailer") {
            return;
        }
        // `Const` carries no `loc.name` (Range::ZERO); the whole const
        // range is the offense (mirrors upstream `add_offense(name_node)`).
        let name_range = cx.range(name);
        cx.emit_offense(
            name_range,
            "Mailer should end with `Mailer` suffix.",
            None,
        );
        cx.emit_edit(name_range, &format!("{short}Mailer"));
    }

    #[on_node(kind = "send", methods = ["new"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
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
        if !is_mailer_base(cx, args[0]) {
            return;
        }
        // Nearest ancestor casgn (mirrors upstream `each_ancestor(:casgn).first`).
        let Some(casgn) = cx.ancestors(node).find(|&a| matches!(*cx.kind(a), NodeKind::Casgn { .. })) else {
            return;
        };
        let NodeKind::Casgn { name, .. } = *cx.kind(casgn) else {
            return;
        };
        let short = cx.symbol_str(name).to_owned();
        if short.ends_with("Mailer") {
            return;
        }
        // `Casgn` carries no `loc.name`; derive the `User` slice from the
        // LHS (`Foo::User = ...` keeps only the short name).
        let name_range = casgn_name_range(cx, casgn, &short);
        cx.emit_offense(
            name_range,
            "Mailer should end with `Mailer` suffix.",
            None,
        );
        cx.emit_edit(name_range, &format!("{short}Mailer"));
    }
}

fn is_mailer_base(cx: &Cx<'_>, id: NodeId) -> bool {
    matches!(
        cx.const_name(id).as_deref(),
        Some("ActionMailer::Base") | Some("ApplicationMailer")
    )
}

fn short_const_name(cx: &Cx<'_>, id: NodeId) -> Option<String> {
    match *cx.kind(id) {
        NodeKind::Const { name, .. } => Some(cx.symbol_str(name).to_owned()),
        _ => cx
            .const_name(id)
            .map(|full| full.rsplit("::").next().unwrap_or("").to_owned()),
    }
}

fn casgn_name_range(cx: &Cx<'_>, casgn: NodeId, short: &str) -> murphy_plugin_api::Range {
    let whole = cx.range(casgn);
    let src = cx.raw_source(whole);
    // LHS is everything before the first `=`.
    let eq = src.find('=').unwrap_or(src.len());
    let lhs = src[..eq].trim_end();
    let start_offset = lhs.len().saturating_sub(short.len());
    // Fallback to the whole start when the short name is not a suffix
    // (should not happen for well-formed `Name = ...`).
    if src.len() >= start_offset + short.len()
        && &lhs[start_offset..] == short
    {
        murphy_plugin_api::Range {
            start: whole.start + start_offset as u32,
            end: whole.start + (start_offset + short.len()) as u32,
        }
    } else {
        murphy_plugin_api::Range {
            start: whole.start,
            end: whole.start + short.len() as u32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::MailerName;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_class_with_action_mailer_base() {
        test::<MailerName>().expect_offense(indoc! {r#"
            class User < ActionMailer::Base
                  ^^^^ Mailer should end with `Mailer` suffix.
            end
        "#});
    }

    #[test]
    fn corrects_class_with_action_mailer_base() {
        test::<MailerName>().expect_correction(
            indoc! {r#"
                class User < ActionMailer::Base
                      ^^^^ Mailer should end with `Mailer` suffix.
                end
            "#},
            "class UserMailer < ActionMailer::Base\nend\n",
        );
    }

    #[test]
    fn flags_class_with_application_mailer() {
        test::<MailerName>().expect_offense(indoc! {r#"
            class User < ApplicationMailer
                  ^^^^ Mailer should end with `Mailer` suffix.
            end
        "#});
    }

    #[test]
    fn flags_class_with_cbase_action_mailer() {
        test::<MailerName>().expect_offense(indoc! {r#"
            class User < ::ActionMailer::Base
                  ^^^^ Mailer should end with `Mailer` suffix.
            end
        "#});
    }

    #[test]
    fn flags_class_with_cbase_application_mailer() {
        test::<MailerName>().expect_offense(indoc! {r#"
            class User < ::ApplicationMailer
                  ^^^^ Mailer should end with `Mailer` suffix.
            end
        "#});
    }

    #[test]
    fn allows_class_with_suffix() {
        test::<MailerName>().expect_no_offenses("class UserMailer < ActionMailer::Base\nend\n");
    }

    #[test]
    fn allows_non_mailer_superclass() {
        test::<MailerName>().expect_no_offenses("class User < Foo\nend\n");
    }

    #[test]
    fn flags_class_new_assignment() {
        test::<MailerName>().expect_offense(indoc! {r#"
            User = Class.new(ActionMailer::Base)
            ^^^^ Mailer should end with `Mailer` suffix.
        "#});
    }

    #[test]
    fn corrects_class_new_assignment() {
        test::<MailerName>().expect_correction(
            indoc! {r#"
                User = Class.new(ActionMailer::Base)
                ^^^^ Mailer should end with `Mailer` suffix.
            "#},
            "UserMailer = Class.new(ActionMailer::Base)\n",
        );
    }

    #[test]
    fn flags_cbase_class_new_assignment() {
        test::<MailerName>().expect_offense(indoc! {r#"
            User = ::Class.new(ApplicationMailer)
            ^^^^ Mailer should end with `Mailer` suffix.
        "#});
    }

    #[test]
    fn allows_class_new_with_suffix() {
        test::<MailerName>()
            .expect_no_offenses("UserMailer = Class.new(ActionMailer::Base)\n");
    }

    #[test]
    fn does_not_flag_bare_class_new() {
        test::<MailerName>().expect_no_offenses("Class.new(ActionMailer::Base)\n");
    }
}
murphy_plugin_api::submit_cop!(MailerName);
