//! `Rails/EnvironmentVariableAccess` — flag direct access to
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/EnvironmentVariableAccess
//! upstream_version_checked: 2.35.0
//! status: partial
//! gap_issues:
//!   - murphy-j28r
//! notes: >
//!   AllowReads/AllowWrites options, ::ENV handling (cbase-qualified form),
//!   read/write-specific messages, and ENV const offense range implemented
//!   (murphy-dls6). Remaining gap: Rails include/exclude path gating
//!   (no file-path infrastructure yet, murphy-j28r). RuboCop default.yml
//!   restricts this cop to app/**/*.rb, config/initializers/**/*.rb,
//!   lib/**/*.rb (excluding lib/**/*.rake); Murphy fires in all files.
//!   Users can disable per-directory via .murphy.yml.
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
//! - `Const { scope: None }` means the bare top-level `ENV` **or** the
//!   cbase-qualified `::ENV` — the translator folds `::Foo` to
//!   `Const { scope: None }`, identical to bare `Foo` (see
//!   `cx.const_name` docs). A scoped `Foo::ENV` (scope = `Some(_)` where
//!   the scope is not a `Cbase`) is some other namespace's constant and
//!   is intentionally ignored.
//! - The method position is `_` (any) — `ENV[]`, `ENV.fetch`,
//!   `ENV.store`, `ENV.delete`, `ENV.to_h`, etc. all match. This
//!   mirrors upstream RuboCop-rails which casts a wide net here.
//! - **Write methods**: `:[]=` (index-assign) and `:store` are
//!   classified as writes; all other methods are reads.
//!
//! Bare `ENV` (a Const read with no Send around it) is **not** a
//! Send node and is left alone by the dispatcher.
//!
//! ## Options
//!
//! - **`AllowReads`** (default false): when true, skip ENV reads
//!   (`ENV[key]`, `ENV.fetch`, `ENV.to_h`, etc.).
//! - **`AllowWrites`** (default false): when true, skip ENV writes
//!   (`ENV[key] = value`, `ENV.store(key, value)`).
//!
//! ## Offense messages
//!
//! - Read: "Avoid reading directly from `ENV`."
//! - Write: "Avoid writing directly to `ENV`."
//!
//! ## Offense range
//!
//! The offense range covers just the `ENV` constant node, not the full
//! send expression. This mirrors upstream RuboCop-rails.
//!
//! ## Default disabled
//!
//! Upstream RuboCop-rails ships this cop as `Enabled: false` because
//! settings layers are an architectural choice, not a universal rule
//! (some Rails apps prefer direct `ENV` access for 12-factor
//! reasons). Murphy mirrors that default — opt in via
//! `Rails/EnvironmentVariableAccess: {Enabled: true}` in `.murphy.yml`.
//!
//! ## No autocorrect
//!
//! Mechanical rewriting would require knowing the project's settings
//! layer (Rails.application.config? Settings.foo? Figaro.env.FOO?);
//! that's outside the cop's awareness. Detect-only.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop, def_node_matcher};

// RuboCop NodePattern equivalent: `(send (const nil? :ENV) _ ...)`.
// `nil?` on the inner Const requires no scope (top-level). The method
// position is a wildcard so every `ENV.<method>` call matches.
// Note: `::ENV` folds to `Const { scope: None }` at translation time,
// so this matcher also fires for `::ENV[...]`.
def_node_matcher!(is_env_access, "(send (const nil? :ENV) _ ...)");

/// Returns true if the send method is a write operation on ENV.
/// Write methods: `:[]=` (index-assign) and `:store`.
fn is_env_write(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Send { method, .. } = *cx.kind(node) else {
        return false;
    };
    let m = cx.symbol_str(method);
    m == "[]=" || m == "store"
}

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct EnvironmentVariableAccess;

#[derive(CopOptions)]
pub struct EnvironmentVariableAccessOptions {
    #[option(
        name = "AllowReads",
        default = false,
        description = "When true, skip ENV reads (`ENV[key]`, `ENV.fetch`, `ENV.to_h`, etc.)."
    )]
    pub allow_reads: bool,
    #[option(
        name = "AllowWrites",
        default = false,
        description = "When true, skip ENV writes (`ENV[key] = value`, `ENV.store(key, value)`)."
    )]
    pub allow_writes: bool,
}

#[cop(
    name = "Rails/EnvironmentVariableAccess",
    description = "Avoid accessing environment variables directly.",
    default_severity = "warning",
    default_enabled = false,
    options = EnvironmentVariableAccessOptions,
)]
impl EnvironmentVariableAccess {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        if !is_env_access(node, cx) {
            return;
        }

        let opts = cx.options_or_default::<EnvironmentVariableAccessOptions>();
        let write = is_env_write(node, cx);

        if write && opts.allow_writes {
            return;
        }
        if !write && opts.allow_reads {
            return;
        }

        // Offense range: just the `ENV` constant node, not the full send.
        // The receiver is always present here (pattern is gated on it).
        let receiver_id = match *cx.kind(node) {
            NodeKind::Send { receiver, .. } => receiver.get().unwrap(),
            _ => return,
        };
        let env_range = cx.range(receiver_id);

        let msg = if write {
            "Avoid writing directly to `ENV`."
        } else {
            "Avoid reading directly from `ENV`."
        };

        cx.emit_offense(env_range, msg, None);
    }
}

#[cfg(test)]
mod tests {
    use super::{EnvironmentVariableAccess, EnvironmentVariableAccessOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    fn allow_reads() -> EnvironmentVariableAccessOptions {
        EnvironmentVariableAccessOptions {
            allow_reads: true,
            allow_writes: false,
        }
    }

    fn allow_writes() -> EnvironmentVariableAccessOptions {
        EnvironmentVariableAccessOptions {
            allow_reads: false,
            allow_writes: true,
        }
    }

    // === hit cases ===

    #[test]
    fn flags_env_bracket_read() {
        test::<EnvironmentVariableAccess>().expect_offense(indoc! {r#"
                ENV["DATABASE_URL"]
                ^^^ Avoid reading directly from `ENV`.
            "#});
    }

    #[test]
    fn flags_env_fetch() {
        test::<EnvironmentVariableAccess>().expect_offense(indoc! {r#"
                ENV.fetch("DATABASE_URL")
                ^^^ Avoid reading directly from `ENV`.
            "#});
    }

    #[test]
    fn flags_env_fetch_with_default() {
        test::<EnvironmentVariableAccess>().expect_offense(indoc! {r#"
                ENV.fetch("DATABASE_URL", "sqlite::memory:")
                ^^^ Avoid reading directly from `ENV`.
            "#});
    }

    #[test]
    fn flags_env_bracket_write() {
        test::<EnvironmentVariableAccess>().expect_offense(indoc! {r#"
                ENV["DATABASE_URL"] = "postgres://..."
                ^^^ Avoid writing directly to `ENV`.
            "#});
    }

    #[test]
    fn flags_env_store() {
        test::<EnvironmentVariableAccess>().expect_offense(indoc! {r#"
                ENV.store("KEY", "VALUE")
                ^^^ Avoid writing directly to `ENV`.
            "#});
    }

    #[test]
    fn flags_env_to_h() {
        test::<EnvironmentVariableAccess>().expect_offense(indoc! {r#"
                ENV.to_h
                ^^^ Avoid reading directly from `ENV`.
            "#});
    }

    #[test]
    fn flags_cbase_env_bracket_read() {
        // `::ENV` folds to `Const { scope: None }` at translation time,
        // so the same matcher fires. The Const node range covers `::ENV`
        // (5 chars including the leading `::`).
        test::<EnvironmentVariableAccess>().expect_offense(indoc! {r#"
                ::ENV["FOO"]
                ^^^^^ Avoid reading directly from `ENV`.
            "#});
    }

    // === AllowReads option ===

    #[test]
    fn allow_reads_skips_env_read() {
        test::<EnvironmentVariableAccess>()
            .with_options(&allow_reads())
            .expect_no_offenses("ENV[\"DATABASE_URL\"]\n");
    }

    #[test]
    fn allow_reads_skips_env_fetch() {
        test::<EnvironmentVariableAccess>()
            .with_options(&allow_reads())
            .expect_no_offenses("ENV.fetch(\"DATABASE_URL\")\n");
    }

    #[test]
    fn allow_reads_still_flags_env_write() {
        test::<EnvironmentVariableAccess>()
            .with_options(&allow_reads())
            .expect_offense(indoc! {r#"
                ENV["DATABASE_URL"] = "postgres://..."
                ^^^ Avoid writing directly to `ENV`.
            "#});
    }

    #[test]
    fn allow_reads_still_flags_env_store() {
        test::<EnvironmentVariableAccess>()
            .with_options(&allow_reads())
            .expect_offense(indoc! {r#"
                ENV.store("KEY", "VALUE")
                ^^^ Avoid writing directly to `ENV`.
            "#});
    }

    // === AllowWrites option ===

    #[test]
    fn allow_writes_skips_env_bracket_write() {
        test::<EnvironmentVariableAccess>()
            .with_options(&allow_writes())
            .expect_no_offenses("ENV[\"DATABASE_URL\"] = \"postgres://...\"\n");
    }

    #[test]
    fn allow_writes_skips_env_store() {
        test::<EnvironmentVariableAccess>()
            .with_options(&allow_writes())
            .expect_no_offenses("ENV.store(\"KEY\", \"VALUE\")\n");
    }

    #[test]
    fn allow_writes_still_flags_env_read() {
        test::<EnvironmentVariableAccess>()
            .with_options(&allow_writes())
            .expect_offense(indoc! {r#"
                ENV["DATABASE_URL"]
                ^^^ Avoid reading directly from `ENV`.
            "#});
    }

    // === read/write message distinction ===

    #[test]
    fn read_and_write_messages_differ() {
        // Ensures the write message is used for writes, not the read message.
        test::<EnvironmentVariableAccess>().expect_offense(indoc! {r#"
                ENV.store("K", "V")
                ^^^ Avoid writing directly to `ENV`.
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
