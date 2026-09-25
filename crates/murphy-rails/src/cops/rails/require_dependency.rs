//! `Rails/RequireDependency` — do not use `require_dependency` with Zeitwerk.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/RequireDependency
//! upstream_version_checked: 2.35.0
//! version_added: "2.10"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND [:require_dependency]
//!   with nil or bare `Kernel` / `::Kernel` receiver and at least one arg,
//!   gated on `TargetRailsVersion >= 6.0` (unset means newest). Whole-node
//!   offense, no autocorrect. Upstream ships `Enabled: false`; Murphy maps
//!   that to `default_enabled = false`.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct RequireDependency;

#[cop(
    name = "Rails/RequireDependency",
    description = "Do not use `require_dependency` with Zeitwerk mode.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl RequireDependency {
    // Mirrors upstream `RESTRICT_ON_SEND`.
    #[on_node(kind = "send", methods = ["require_dependency"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Upstream `minimum_target_rails_version 6.0` — unset means newest.
    if !cx.rails_version_at_least(6, 0) {
        return;
    }
    if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
        return;
    }
    // Upstream pattern `(send {nil? (const {nil? cbase} :Kernel)} :require_dependency _)`.
    // Exactly: no receiver, or `Kernel` / `::Kernel`; at least one arg.
    match cx.call_receiver(node).get() {
        None => {}
        Some(recv) => {
            if cx.const_name(recv).as_deref() != Some("Kernel") {
                return;
            }
        }
    }
    if cx.call_arguments(node).is_empty() {
        return;
    }
    cx.emit_offense(
        cx.range(node),
        "Do not use `require_dependency` with Zeitwerk mode.",
        None,
    );
}

#[cfg(test)]
mod tests {
    use super::RequireDependency;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_bare() {
        test::<RequireDependency>().expect_offense(indoc! {r#"
            require_dependency 'foo'
            ^^^^^^^^^^^^^^^^^^^^^^^^ Do not use `require_dependency` with Zeitwerk mode.
        "#});
    }

    #[test]
    fn flags_kernel_receiver() {
        test::<RequireDependency>().expect_offense(indoc! {r#"
            Kernel.require_dependency 'foo'
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not use `require_dependency` with Zeitwerk mode.
        "#});
    }

    #[test]
    fn flags_cbase_kernel_receiver() {
        test::<RequireDependency>().expect_offense(indoc! {r#"
            ::Kernel.require_dependency 'foo'
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not use `require_dependency` with Zeitwerk mode.
        "#});
    }

    #[test]
    fn allows_namespaced_kernel() {
        test::<RequireDependency>()
            .expect_no_offenses("Foo::Kernel.require_dependency 'foo'\n");
    }

    #[test]
    fn allows_require() {
        test::<RequireDependency>().expect_no_offenses("require 'foo'\n");
    }

    #[test]
    fn allows_require_relative() {
        test::<RequireDependency>().expect_no_offenses("require_relative 'foo'\n");
    }

    #[test]
    fn allows_below_rails_6() {
        test::<RequireDependency>()
            .with_target_rails_version(5, 2)
            .expect_no_offenses("require_dependency 'foo'\n");
    }

    #[test]
    fn fires_at_rails_6() {
        test::<RequireDependency>()
            .with_target_rails_version(6, 0)
            .expect_offense(indoc! {r#"
                require_dependency 'foo'
                ^^^^^^^^^^^^^^^^^^^^^^^^ Do not use `require_dependency` with Zeitwerk mode.
            "#});
    }
}
murphy_plugin_api::submit_cop!(RequireDependency);
