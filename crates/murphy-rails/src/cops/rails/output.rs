//! `Rails/Output` — flag receiver-less debug-output method calls
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
        // Gate 1: an explicit receiver is intentional output.
        if receiver != OptNodeId::NONE {
            return;
        }
        // Gate 2: only the bare debug-output method names. Mirrors the
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

#[cfg(test)]
mod tests {
    use super::Output;
    use murphy_plugin_api::test_support::{expect_no_offenses, expect_offense, indoc};

    // === bare-call cases (should flag) ===

    #[test]
    fn flags_bare_puts() {
        expect_offense!(
            Output,
            indoc! {r#"
                puts "debug"
                ^^^^^^^^^^^^ Do not write to stdout. Use Rails's logger if you want to log.
            "#}
        );
    }

    #[test]
    fn flags_bare_p() {
        expect_offense!(
            Output,
            indoc! {r#"
                p obj
                ^^^^^ Do not write to stdout. Use Rails's logger if you want to log.
            "#}
        );
    }

    #[test]
    fn flags_bare_print() {
        expect_offense!(
            Output,
            indoc! {r#"
                print "x"
                ^^^^^^^^^ Do not write to stdout. Use Rails's logger if you want to log.
            "#}
        );
    }

    #[test]
    fn flags_bare_pp() {
        expect_offense!(
            Output,
            indoc! {r#"
                pp data
                ^^^^^^^ Do not write to stdout. Use Rails's logger if you want to log.
            "#}
        );
    }

    #[test]
    fn flags_bare_pretty_print() {
        expect_offense!(
            Output,
            indoc! {r#"
                pretty_print value
                ^^^^^^^^^^^^^^^^^^ Do not write to stdout. Use Rails's logger if you want to log.
            "#}
        );
    }

    #[test]
    fn flags_bare_ap() {
        expect_offense!(
            Output,
            indoc! {r#"
                ap hash
                ^^^^^^^ Do not write to stdout. Use Rails's logger if you want to log.
            "#}
        );
    }

    #[test]
    fn flags_bare_binwrite() {
        // binwrite/syswrite/write/write_nonblock are filesystem-output
        // methods that, on a bare call, write to whatever Rails has
        // bound to `$stdout` — same problem as puts.
        expect_offense!(
            Output,
            indoc! {r#"
                binwrite data
                ^^^^^^^^^^^^^ Do not write to stdout. Use Rails's logger if you want to log.
            "#}
        );
    }

    // === explicit-receiver cases (should NOT flag) ===

    #[test]
    fn does_not_flag_logger_call() {
        expect_no_offenses!(Output, "logger.info \"x\"\n");
    }

    #[test]
    fn does_not_flag_rails_logger_call() {
        expect_no_offenses!(Output, "Rails.logger.info \"x\"\n");
    }

    #[test]
    fn does_not_flag_const_receiver() {
        expect_no_offenses!(Output, "Foo.puts \"x\"\n");
    }

    #[test]
    fn does_not_flag_self_receiver() {
        expect_no_offenses!(Output, "self.puts \"x\"\n");
    }

    #[test]
    fn does_not_flag_method_chain_pp() {
        // `obj.pp` is fine — the offense is *bare* `pp`, not the
        // ActiveSupport `Object#pp` instance method.
        expect_no_offenses!(Output, "obj.pp\n");
    }

    // === unrelated-method cases (should NOT flag) ===

    #[test]
    fn does_not_flag_unrelated_method() {
        expect_no_offenses!(Output, "do_something\n");
    }

    #[test]
    fn does_not_flag_local_variable_named_like_method() {
        // `puts = "x"` parses as a local assignment, not a send, so it
        // must not trip the cop.
        expect_no_offenses!(Output, "puts = \"x\"\n");
    }
}
