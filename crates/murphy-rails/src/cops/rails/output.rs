//! `Rails/Output` — flag receiver-less debug-output method calls
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/Output
//! upstream_version_checked: 2.35.0
//! status: partial
//! gap_issues:
//!   - murphy-hfcg
//! notes: >
//!   Known gaps remain around guards, STDOUT/STDERR coverage, autocorrect, and file gating.
//! ```
//!
//! (`puts`/`p`/`pp`/`print`/`pretty_print`/`ap`/`binwrite`/`syswrite`/
//! `write`/`write_nonblock`). Rails apps should route debug output
//! through `Rails.logger` so it ends up in the configured log sink
//! instead of stdout.
//!
//! Same pattern as `murphy-std`'s `Murphy/NoReceiverPuts`, expanded
//! with the longer method-name table that upstream `rubocop-rails`
//! `Rails/Output` covers (see the call-dispatch table at
//! `git show 46a1de6^:crates/murphy-rails/src/lib.rs` for the
//! pre-9cr.22 method list).
//!
//! ## Matched shapes (Send node)
//!
//! - **bare call only**: `receiver == OptNodeId::NONE`. An explicit
//!   receiver (`logger.info "x"`, `Foo.puts "x"`, `self.puts "x"`) is
//!   intentional output and is left alone.
//! - **method ∈ debug-output names**: the table above.
//!
//! ## No autocorrect
//!
//! Rewriting `puts` to a logger call requires a logger receiver in
//! scope that the cop cannot synthesise safely (the `Rails.logger`
//! singleton is only available inside the Rails runtime). Reported
//! offense; manual fix.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct Output;

#[cop(
    name = "Rails/Output",
    description = "Do not write to stdout. Use Rails's logger if you want to log.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl Output {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // Defensive pattern-match: the dispatcher feeds us only Send
        // nodes today (`KINDS = [send]`), but the `let-else` is free
        // insurance against a future kind-aliasing accident.
        let NodeKind::Send {
            receiver, method, ..
        } = *cx.kind(node)
        else {
            return;
        };
        // Gate 1: the receiver must be either bare (no receiver) or
        // one of the standard-output stream aliases (`$stdout`,
        // `$stderr`, `STDOUT`, `STDERR`). Any other receiver
        // (`logger`, `Rails.logger`, an arbitrary Const like
        // `Foo.puts`, a chain like `obj.pp`) is intentional output and
        // is left alone. roborev review (job 1124) flagged that an
        // earlier "receiver-less only" gate let through
        // `$stdout.puts "x"` / `STDOUT.write "x"` /
        // `$stderr.print "x"`, which are exactly the stdio-bypass
        // shapes this cop wants to catch.
        if !receiver_targets_stdout(cx, receiver) {
            return;
        }
        // Gate 2: only the debug-output method names. Mirrors the
        // pre-9cr.22 `output_dispatch` table.
        if !matches!(
            cx.symbol_str(method),
            "puts"
                | "print"
                | "p"
                | "pp"
                | "pretty_print"
                | "ap"
                | "binwrite"
                | "syswrite"
                | "write"
                | "write_nonblock"
        ) {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            "Do not write to stdout. Use Rails's logger if you want to log.",
            None,
        );
    }
}

/// `true` if `receiver` is one of the standard-output stream aliases
/// — bare (None), `$stdout` / `$stderr` (`Gvar`), or top-level
/// `STDOUT` / `STDERR` (`Const` with no scope). These are exactly the
/// receivers whose `puts` / `write` / etc. calls bypass `Rails.logger`
/// and write directly to the process's stdio fds.
///
/// Any other receiver (logger object, `Rails.logger`, custom Const,
/// chained expression) is intentional output for this cop's purposes
/// and returns `false`.
fn receiver_targets_stdout(cx: &Cx<'_>, receiver: OptNodeId) -> bool {
    let Some(rid) = receiver.get() else {
        return true; // bare call
    };
    match *cx.kind(rid) {
        NodeKind::Gvar(name) => {
            matches!(cx.symbol_str(name), "$stdout" | "$stderr")
        }
        NodeKind::Const { scope, name } => {
            scope == OptNodeId::NONE && matches!(cx.symbol_str(name), "STDOUT" | "STDERR")
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::Output;
    use murphy_plugin_api::test_support::{indoc, test};

    // === bare-call cases (should flag) ===

    #[test]
    fn flags_bare_puts() {
        test::<Output>().expect_offense(indoc! {r#"
                puts "debug"
                ^^^^^^^^^^^^ Do not write to stdout. Use Rails's logger if you want to log.
            "#});
    }

    #[test]
    fn flags_bare_p() {
        test::<Output>().expect_offense(indoc! {r#"
                p obj
                ^^^^^ Do not write to stdout. Use Rails's logger if you want to log.
            "#});
    }

    #[test]
    fn flags_bare_print() {
        test::<Output>().expect_offense(indoc! {r#"
                print "x"
                ^^^^^^^^^ Do not write to stdout. Use Rails's logger if you want to log.
            "#});
    }

    #[test]
    fn flags_bare_pp() {
        test::<Output>().expect_offense(indoc! {r#"
                pp data
                ^^^^^^^ Do not write to stdout. Use Rails's logger if you want to log.
            "#});
    }

    #[test]
    fn flags_bare_pretty_print() {
        test::<Output>().expect_offense(indoc! {r#"
                pretty_print value
                ^^^^^^^^^^^^^^^^^^ Do not write to stdout. Use Rails's logger if you want to log.
            "#});
    }

    #[test]
    fn flags_bare_ap() {
        test::<Output>().expect_offense(indoc! {r#"
                ap hash
                ^^^^^^^ Do not write to stdout. Use Rails's logger if you want to log.
            "#});
    }

    #[test]
    fn flags_bare_binwrite() {
        // binwrite/syswrite/write/write_nonblock are filesystem-output
        // methods that, on a bare call, write to whatever Rails has
        // bound to `$stdout` — same problem as puts.
        test::<Output>().expect_offense(indoc! {r#"
                binwrite data
                ^^^^^^^^^^^^^ Do not write to stdout. Use Rails's logger if you want to log.
            "#});
    }

    // === explicit-receiver cases (should NOT flag) ===

    #[test]
    fn does_not_flag_logger_call() {
        test::<Output>().expect_no_offenses("logger.info \"x\"\n");
    }

    #[test]
    fn does_not_flag_rails_logger_call() {
        test::<Output>().expect_no_offenses("Rails.logger.info \"x\"\n");
    }

    #[test]
    fn does_not_flag_const_receiver() {
        test::<Output>().expect_no_offenses("Foo.puts \"x\"\n");
    }

    #[test]
    fn does_not_flag_self_receiver() {
        test::<Output>().expect_no_offenses("self.puts \"x\"\n");
    }

    #[test]
    fn does_not_flag_method_chain_pp() {
        // `obj.pp` is fine — the offense is *bare* `pp`, not the
        // ActiveSupport `Object#pp` instance method.
        test::<Output>().expect_no_offenses("obj.pp\n");
    }

    // === unrelated-method cases (should NOT flag) ===

    #[test]
    fn does_not_flag_unrelated_method() {
        test::<Output>().expect_no_offenses("do_something\n");
    }

    #[test]
    fn does_not_flag_local_variable_named_like_method() {
        // `puts = "x"` parses as a local assignment, not a send, so it
        // must not trip the cop.
        test::<Output>().expect_no_offenses("puts = \"x\"\n");
    }

    // === stdio-alias receiver cases (should flag, added per roborev review 1124) ===

    #[test]
    fn flags_stdout_gvar_puts() {
        test::<Output>().expect_offense(indoc! {r#"
                $stdout.puts "x"
                ^^^^^^^^^^^^^^^^ Do not write to stdout. Use Rails's logger if you want to log.
            "#});
    }

    #[test]
    fn flags_stderr_gvar_print() {
        test::<Output>().expect_offense(indoc! {r#"
                $stderr.print "x"
                ^^^^^^^^^^^^^^^^^ Do not write to stdout. Use Rails's logger if you want to log.
            "#});
    }

    #[test]
    fn flags_stdout_const_write() {
        test::<Output>().expect_offense(indoc! {r#"
                STDOUT.write "x"
                ^^^^^^^^^^^^^^^^ Do not write to stdout. Use Rails's logger if you want to log.
            "#});
    }

    #[test]
    fn flags_stderr_const_write_nonblock() {
        test::<Output>().expect_offense(indoc! {r#"
                STDERR.write_nonblock "x"
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Do not write to stdout. Use Rails's logger if you want to log.
            "#});
    }

    #[test]
    fn does_not_flag_other_gvar_puts() {
        // Only `$stdout` / `$stderr` are stdio aliases — a custom
        // global like `$log` is not.
        test::<Output>().expect_no_offenses("$log.puts \"x\"\n");
    }

    #[test]
    fn does_not_flag_scoped_const_stdout_puts() {
        // `Foo::STDOUT` is a namespaced constant, not the top-level
        // stdio alias.
        test::<Output>().expect_no_offenses("Foo::STDOUT.puts \"x\"\n");
    }
}
