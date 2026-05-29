//! `Rails/EnvironmentVariableAccess` — flag direct access to
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/EnvironmentVariableAccess
//! upstream_version_checked: 2.35.0
//! status: partial
//! gap_issues:
//!   - murphy-dls6
//! notes: >
//!   Known gaps remain around AllowReads/AllowWrites, ::ENV, read/write messages, ranges, and file gating.
//! ```
//!
//! environment variables through the top-level `ENV` constant
//! (`ENV[key]`, `ENV.fetch(key)`, `ENV["A"] = "B"`, `ENV.store(...)`,
//! `ENV.to_h`, …). Rails projects typically prefer a settings layer
//! (Settings, Figaro, dotenv, Rails 6 credentials, anyway, …) so the
//! configuration surface stays type-checked and discoverable.
//!
//! ## Matched shape (Send node)
//!
//! `Send(receiver=Const{scope=None, name="ENV"}, method=_, args=...)` —
//! any method called on the top-level `ENV` constant.
//!
//! - `Const { scope: None }` means the bare top-level `ENV`. A
//!   scoped `Foo::ENV` (scope = `Some(_)`) is some other namespace's
//!   constant and is intentionally ignored.
//! - The method position is `_` (any) — `ENV[]`, `ENV.fetch`,
//!   `ENV.store`, `ENV.delete`, `ENV.to_h`, etc. all match. This
//!   mirrors upstream RuboCop-rails which casts a wide net here.
//!
//! Bare `ENV` (a Const read with no Send around it) is **not** a
//! Send node and is left alone by the dispatcher.
//!
//! ## Default disabled
//!
//! Upstream RuboCop-rails ships this cop as `Enabled: false` because
//! settings layers are an architectural choice, not a universal rule
//! (some Rails apps prefer direct `ENV` access for 12-factor
//! reasons). Murphy mirrors that default — opt in via `[cops.rules.
//! "Rails/EnvironmentVariableAccess"] enabled = true` in
//! `murphy.toml`.
//!
//! ## No autocorrect
//!
//! Mechanical rewriting would require knowing the project's settings
//! layer (Rails.application.config? Settings.foo? Figaro.env.FOO?);
//! that's outside the cop's awareness. Detect-only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, cop, def_node_matcher};

// RuboCop NodePattern equivalent: `(send (const nil? :ENV) _ ...)`.
// `nil?` on the inner Const requires no scope (top-level). The method
// position is a wildcard so every `ENV.<method>` call matches.
def_node_matcher!(is_env_access, "(send (const nil? :ENV) _ ...)");

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct EnvironmentVariableAccess;

#[cop(
    name = "Rails/EnvironmentVariableAccess",
    description = "Don't access environment variables directly; use a settings layer instead.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl EnvironmentVariableAccess {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        if !is_env_access(node, cx) {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            "Don't access environment variables directly; use a settings layer instead.",
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::EnvironmentVariableAccess;
    use murphy_plugin_api::test_support::{indoc, test};

    // === hit cases ===

    #[test]
    fn flags_env_bracket_read() {
        test::<EnvironmentVariableAccess>().expect_offense(indoc! {r#"
                ENV["DATABASE_URL"]
                ^^^^^^^^^^^^^^^^^^^ Don't access environment variables directly; use a settings layer instead.
            "#});
    }

    #[test]
    fn flags_env_fetch() {
        test::<EnvironmentVariableAccess>().expect_offense(indoc! {r#"
                ENV.fetch("DATABASE_URL")
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Don't access environment variables directly; use a settings layer instead.
            "#});
    }

    #[test]
    fn flags_env_fetch_with_default() {
        test::<EnvironmentVariableAccess>().expect_offense(indoc! {r#"
                ENV.fetch("DATABASE_URL", "sqlite::memory:")
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Don't access environment variables directly; use a settings layer instead.
            "#});
    }

    #[test]
    fn flags_env_bracket_write() {
        test::<EnvironmentVariableAccess>().expect_offense(indoc! {r#"
                ENV["DATABASE_URL"] = "postgres://..."
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Don't access environment variables directly; use a settings layer instead.
            "#});
    }

    #[test]
    fn flags_env_store() {
        test::<EnvironmentVariableAccess>().expect_offense(indoc! {r#"
                ENV.store("KEY", "VALUE")
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Don't access environment variables directly; use a settings layer instead.
            "#});
    }

    #[test]
    fn flags_env_to_h() {
        test::<EnvironmentVariableAccess>().expect_offense(indoc! {r#"
                ENV.to_h
                ^^^^^^^^ Don't access environment variables directly; use a settings layer instead.
            "#});
    }

    // === no-hit cases ===

    #[test]
    fn does_not_flag_scoped_env_const() {
        // `Foo::ENV` is a namespaced constant — not the top-level
        // `ENV` we want to flag.
        test::<EnvironmentVariableAccess>().expect_no_offenses("Foo::ENV.fetch(\"KEY\")\n");
    }

    #[test]
    fn does_not_flag_lvar_env() {
        // `env` (lowercase, local variable) is not the `ENV` const.
        test::<EnvironmentVariableAccess>().expect_no_offenses("env.fetch(\"KEY\")\n");
    }

    #[test]
    fn does_not_flag_other_const() {
        // A different top-level constant with `fetch`/`[]` is fine.
        test::<EnvironmentVariableAccess>().expect_no_offenses("MyEnv.fetch(\"KEY\")\n");
    }

    #[test]
    fn does_not_flag_bare_env_read() {
        // `puts ENV` references the const directly without a Send —
        // the dispatcher never visits the Const node from this cop.
        test::<EnvironmentVariableAccess>().expect_no_offenses("puts ENV\n");
    }
}
