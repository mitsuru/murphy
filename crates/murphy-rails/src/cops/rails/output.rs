//! `Rails/Output` — flag receiver-less debug-output method calls
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/Output
//! upstream_version_checked: 2.35.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   output?/io_output? method-gating split and parent/block/hash/block_pass
//!   guards are complete. Cbase forms (::STDOUT/::STDERR) fold to scope=None
//!   in Murphy's AST, so they are matched identically to bare STDOUT/STDERR.
//!   Autocorrect (logger rewrite) is intentionally absent — the safe logger
//!   receiver cannot be synthesised by the cop (ADR 0006). RuboCop Rails/Output
//!   is global (no Include globs); Murphy matches that behaviour.
//! ```
//!
//! (`puts`/`p`/`pp`/`print`/`pretty_print`/`ap` — bare only) and
//! (`binwrite`/`syswrite`/`write`/`write_nonblock` — stdio receiver only).
//! Rails apps should route debug output through `Rails.logger` so it ends
//! up in the configured log sink instead of stdout.
//!
//! Mirrors RuboCop's two-pattern split:
//! - `output?`:    nil? receiver + {ap p pp pretty_print print puts}
//! - `io_output?`: $stdout/$stderr/STDOUT/STDERR receiver +
//!   {binwrite syswrite write write_nonblock}
//!
//! ## Matched shapes (Send node)
//!
//! - **bare call only** for output methods: `receiver == OptNodeId::NONE`.
//! - **stdio receiver** for io_output methods.
//! - **Not flagged** when the node is the *receiver* of a parent Send/Csend
//!   (e.g. `p.do_something` — the `p` is chained, not a bare debug call),
//!   or when the node IS the call of its parent Block/Numblock/Itblock
//!   (DSL usage like `p { 'text' }`), or when any argument is a Hash or
//!   BlockPass. A debug call used as an *argument* (e.g. `foo(p)`) IS
//!   flagged.
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
            receiver,
            method,
            args,
            ..
        } = *cx.kind(node)
        else {
            return;
        };

        // Gate 1 (split into two branches mirroring RuboCop):
        //
        // Branch A — output? pattern: nil receiver + output-method names.
        let is_output = receiver.get().is_none()
            && matches!(
                cx.symbol_str(method),
                "ap" | "p" | "pp" | "pretty_print" | "print" | "puts"
            );

        // Branch B — io_output? pattern: stdio receiver + write-family names.
        let is_io_output = !is_output
            && matches!(
                cx.symbol_str(method),
                "binwrite" | "syswrite" | "write" | "write_nonblock"
            )
            && receiver_is_stdio(cx, receiver);

        if !is_output && !is_io_output {
            return;
        }

        // Gate 2: skip in two cases —
        //
        // (a) This node is the *call* child of a parent Block/Numblock/Itblock
        //     (DSL block call like `p { 'text' }` — mirrors RuboCop's
        //     `node.block_node` check). Note: a send that is merely in the
        //     *body* of a block (e.g. `foo do; puts "x"; end`) is not the
        //     block's call — it still fires.
        //
        // (b) This node is the *receiver* of a parent Send/Csend
        //     (e.g. `p.do_something` or `p&.do_something`). A debug call
        //     used as an *argument* — e.g. `foo(p)` or
        //     `logger.info(puts "x")` — is still flagged.
        if let Some(pid) = cx.parent(node).get() {
            let suppress = match *cx.kind(pid) {
                NodeKind::Block { call, .. } => call == node,
                NodeKind::Numblock { send, .. } => send == node,
                NodeKind::Itblock { send, .. } => send == node,
                // Narrowed: only suppress when the debug call is the
                // *receiver* (p.foo), not when it is an argument (foo(p)).
                NodeKind::Send { receiver, .. } => receiver.get() == Some(node),
                NodeKind::Csend { receiver, .. } => receiver == node,
                _ => false,
            };
            if suppress {
                return;
            }
        }

        // Gate 3: skip when any argument is a Hash (DSL keyword args
        // like `p(class: 'foo')`) or a BlockPass (`p(&:method)`).
        if cx
            .list(args)
            .iter()
            .any(|&a| matches!(*cx.kind(a), NodeKind::Hash(_) | NodeKind::BlockPass(_)))
        {
            return;
        }

        cx.emit_offense(
            cx.range(node),
            "Do not write to stdout. Use Rails's logger if you want to log.",
            None,
        );
    }
}

/// `true` if `receiver` is one of the standard-output stream aliases —
/// `$stdout` / `$stderr` (`Gvar`), or top-level `STDOUT` / `STDERR`
/// (`Const` with no scope).
///
/// Note: a bare receiver (None) returns `false` here — bare calls are
/// handled by Branch A (is_output) which only applies to output-method
/// names, not by Branch B (io_output?) which requires an explicit stdio
/// receiver.
fn receiver_is_stdio(cx: &Cx<'_>, receiver: OptNodeId) -> bool {
    let Some(rid) = receiver.get() else {
        return false; // bare call is not a stdio receiver for io_output?
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

    // === puts in a block body still flags (it is the body, not the call) ===

    #[test]
    fn flags_puts_in_block_body() {
        test::<Output>().expect_offense(indoc! {r#"
                foo do
                  puts "x"
                  ^^^^^^^^ Do not write to stdout. Use Rails's logger if you want to log.
                end
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

    // === stdio-alias receiver + write-family cases (should flag) ===

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

    // === parent-is-call guard (false positives) ===

    #[test]
    fn does_not_flag_p_dot_do_something() {
        // `p` is used as the receiver of a method call — not a bare
        // debug-output call. RuboCop skips when parent is call_type?.
        test::<Output>().expect_no_offenses("p.do_something\n");
    }

    #[test]
    fn does_not_flag_p_safe_nav_do_something() {
        // Same as above but with safe-navigation (`p&.do_something`).
        test::<Output>().expect_no_offenses("p&.do_something\n");
    }

    #[test]
    fn flags_debug_call_as_method_argument() {
        // `puts "x"` is an argument to another method — it is still a
        // bare debug-output call and must fire even though its parent is
        // a Send node. The old guard was too broad: it skipped any
        // Send/Csend parent, causing a false negative here.
        test::<Output>().expect_offense(indoc! {r#"
                logger.info(puts "x")
                            ^^^^^^^^ Do not write to stdout. Use Rails's logger if you want to log.
            "#});
    }

    #[test]
    fn flags_p_as_method_argument() {
        // `p` passed as an argument — not a receiver chain, still a debug call.
        test::<Output>().expect_offense(indoc! {r#"
                foo(p)
                    ^ Do not write to stdout. Use Rails's logger if you want to log.
            "#});
    }

    // === block-call guard (node IS the block's call, not body) ===

    #[test]
    fn does_not_flag_p_with_block() {
        // `p { 'text' }` — `p` is the *call* of the block node, so it
        // is a DSL usage. RuboCop uses `node.block_node` for this check.
        test::<Output>().expect_no_offenses(indoc! {r#"
                div do
                  p { 'text' }
                end
            "#});
    }

    // === hash/block_pass argument guard (false positives) ===

    #[test]
    fn does_not_flag_p_with_hash_arg() {
        // `p(class: 'DSL')` — the argument is a Hash; this is a DSL
        // call, not debug output.
        test::<Output>().expect_no_offenses("p(class: 'this `p` method is a DSL')\n");
    }

    #[test]
    fn does_not_flag_p_with_block_pass() {
        // `p(&:dsl)` — the argument is a BlockPass; DSL pattern.
        test::<Output>().expect_no_offenses("p(&:this_p_method_is_a_dsl)\n");
    }

    // === method-gating split: bare write-family should NOT flag ===

    #[test]
    fn does_not_flag_bare_write() {
        // Bare `write` is not in RuboCop's output? group (nil receiver
        // + output names). io_output? requires an explicit stdio
        // receiver. Neither pattern matches.
        test::<Output>().expect_no_offenses("write \"x\"\n");
    }

    // === method-gating split: $stdout + output-family should NOT flag ===

    #[test]
    fn does_not_flag_stdout_puts() {
        // `$stdout.puts` — output? requires nil? receiver; io_output?
        // does not list `puts`. Neither pattern matches RuboCop.
        test::<Output>().expect_no_offenses("$stdout.puts \"x\"\n");
    }

    // === cbase qualified forms (::STDOUT / ::STDERR) ===

    #[test]
    fn flags_cbase_stderr_write() {
        // `::STDERR.write` — the cbase prefix (::) folds to scope=None
        // in Murphy's AST, so ::STDERR is indistinguishable from bare
        // STDERR and must trigger io_output?.
        test::<Output>().expect_offense(indoc! {r#"
                ::STDERR.write "x"
                ^^^^^^^^^^^^^^^^^^ Do not write to stdout. Use Rails's logger if you want to log.
            "#});
    }

    #[test]
    fn flags_cbase_stdout_syswrite() {
        // `::STDOUT.syswrite` — same cbase-folding reasoning as above.
        test::<Output>().expect_offense(indoc! {r#"
                ::STDOUT.syswrite "x"
                ^^^^^^^^^^^^^^^^^^^^^ Do not write to stdout. Use Rails's logger if you want to log.
            "#});
    }

    // === local-variable receiver: false-positive guard ===

    #[test]
    fn does_not_flag_local_variable_receiver_write() {
        // `io.write(x)` where `io` is a local variable — the receiver
        // is an Lvar node, not a Gvar or Const, so receiver_is_stdio
        // returns false and no offense is emitted.
        test::<Output>().expect_no_offenses("io = $stdout\nio.write(\"x\")\n");
    }
}
murphy_plugin_api::submit_cop!(Output);
