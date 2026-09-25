//! `Rails/TimeZoneAssignment` — use `Time.use_zone` with block instead of `Time.zone=`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/TimeZoneAssignment
//! upstream_version_checked: 2.35.0
//! version_added: "2.10"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `time_zone_assignment?`
//!   (`(send (const {nil? cbase} :Time) :zone= ...)`): bare `Time`
//!   and `::Time` receivers flag on the whole send; namespaced
//!   `Foo::Time` does not. No autocorrect. Disabled upstream by
//!   default (`Enabled: pending`). File scope (`Include:
//!   ['**/spec/**/*.rb', '**/test/**/*.rb']`) is enforced via the
//!   murphy-rails pack default.yml (engine `cop_applies_to_file` gate).
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct TimeZoneAssignment;

#[cop(
    name = "Rails/TimeZoneAssignment",
    description = "Use `Time.use_zone` with block instead of `Time.zone=`.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl TimeZoneAssignment {
    #[on_node(kind = "send", methods = ["zone="])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
        return;
    }
    if cx.method_name(node) != Some("zone=") {
        return;
    }
    let Some(recv) = cx.call_receiver(node).get() else {
        return;
    };
    // Upstream `(const {nil? cbase} :Time)`.
    if !cx.is_global_const(recv, "Time") {
        return;
    }
    cx.emit_offense(
        cx.range(node),
        "Use `Time.use_zone` with block instead of `Time.zone=`.",
        None,
    );
}

#[cfg(test)]
mod tests {
    use super::TimeZoneAssignment;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_time_zone_assignment() {
        test::<TimeZoneAssignment>().expect_offense(indoc! {r#"
            Time.zone = 'EST'
            ^^^^^^^^^^^^^^^^^ Use `Time.use_zone` with block instead of `Time.zone=`.
        "#});
    }

    #[test]
    fn flags_cbase_time_zone_assignment() {
        test::<TimeZoneAssignment>().expect_offense(indoc! {r#"
            ::Time.zone = 'EST'
            ^^^^^^^^^^^^^^^^^^^ Use `Time.use_zone` with block instead of `Time.zone=`.
        "#});
    }

    #[test]
    fn does_not_flag_namespaced_time() {
        test::<TimeZoneAssignment>().expect_no_offenses("Foo::Time.zone = 'EST'\n");
    }

    #[test]
    fn does_not_flag_use_zone() {
        test::<TimeZoneAssignment>().expect_no_offenses("Time.use_zone('EST') { }\n");
    }

    #[test]
    fn does_not_flag_other_assignment() {
        test::<TimeZoneAssignment>().expect_no_offenses("Time.foo = 'EST'\n");
    }
}
murphy_plugin_api::submit_cop!(TimeZoneAssignment);
