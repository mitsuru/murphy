//! `Rails/ScopeArgs` — pass a lambda/proc to `scope` instead of a plain method call.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/ScopeArgs
//! upstream_version_checked: 2.35.0
//! version_added: "0.19"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: `(send nil? :scope _ $send)` — bare
//!   `scope` whose second argument is a plain `send`. Any receiver on
//!   `scope` suppresses, and a second arg that is not a `send` (block,
//!   lambda, proc call with block, symbol, etc.) never flags. Offense is
//!   the second argument; autocorrect wraps it as `-> { src }`. File
//!   scope (`Include: ['**/app/models/**/*.rb']`) is enforced via the
//!   murphy-rails pack default.yml (engine `cop_applies_to_file` gate,
//!   verified vs rubocop-rails 2.38.0 default.yml, murphy-4gd.1.15).
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ScopeArgs;

#[cop(
    name = "Rails/ScopeArgs",
    description = "Checks for scope calls where a method is passed instead of a lambda/proc.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl ScopeArgs {
    // Mirrors upstream `RESTRICT_ON_SEND = %i[scope]`.
    #[on_node(kind = "send", methods = ["scope"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
        return;
    }
    if cx.method_name(node) != Some("scope") {
        return;
    }
    // Upstream `(send nil? :scope ...)`: bare call only.
    if cx.call_receiver(node).get().is_some() {
        return;
    }
    let args = cx.call_arguments(node);
    if args.len() != 2 {
        return;
    }
    let second = args[1];
    // Upstream `$send`: second arg must be a plain `send` (not csend/block/etc).
    if !matches!(*cx.kind(second), NodeKind::Send { .. }) {
        return;
    }
    cx.emit_offense(
        cx.range(second),
        "Use `lambda`/`proc` instead of a plain method call.",
        None,
    );
    let src = cx.raw_source(cx.range(second)).to_owned();
    cx.emit_edit(cx.range(second), &format!("-> {{ {src} }}"));
}

#[cfg(test)]
mod tests {
    use super::ScopeArgs;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_plain_method_call() {
        test::<ScopeArgs>().expect_correction(
            indoc! {r#"
                scope :something, where(something: true)
                                  ^^^^^^^^^^^^^^^^^^^^^^ Use `lambda`/`proc` instead of a plain method call.
            "#},
            "scope :something, -> { where(something: true) }
",
        );
    }

    #[test]
    fn allows_lambda() {
        test::<ScopeArgs>()
            .expect_no_offenses("scope :something, -> { where(something: true) }
");
    }

    #[test]
    fn allows_proc_with_block() {
        test::<ScopeArgs>()
            .expect_no_offenses("scope :something, proc { where(something: true) }
");
    }

    #[test]
    fn allows_symbol() {
        test::<ScopeArgs>().expect_no_offenses("scope :something, :active
");
    }

    #[test]
    fn allows_receiver_scope() {
        test::<ScopeArgs>()
            .expect_no_offenses("foo.scope :something, where(something: true)
");
    }

    #[test]
    fn allows_single_arg() {
        test::<ScopeArgs>().expect_no_offenses("scope :something
");
    }
}
murphy_plugin_api::submit_cop!(ScopeArgs);
