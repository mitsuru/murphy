//! `Style/StderrPuts` — flags `$stderr.puts(…)` / `STDERR.puts(…)` in favor
//! of `warn(…)`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/StderrPuts
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Matches `$stderr.puts(args)` and `STDERR.puts(args)` (including
//!   `::STDERR.puts(args)`). `$stderr.puts` with no arguments is NOT flagged
//!   (matches RuboCop's pattern which requires ≥1 argument via `$_ ...`).
//!   The offense range covers from the start of the receiver through the end
//!   of the `puts` selector (i.e. `$stderr.puts`), mirroring RuboCop's
//!   `stderr_puts_range`. Autocorrect replaces that range with `warn`,
//!   leaving the argument list intact.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! $stderr.puts('hello')
//! STDERR.puts('hello')
//! ::STDERR.puts('hello')
//!
//! # good
//! warn('hello')
//! $stderr.puts        # no args — not flagged
//! ```
//!
//! ## Autocorrect
//!
//! Replaces `$stderr.puts` (the receiver + dot + selector span) with `warn`,
//! leaving the parenthesised arguments unchanged.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop, def_node_matcher};

// RuboCop parity: `Style/StderrPuts` receiver guards are `STDERR` top-level
// (`(const {nil? cbase} :STDERR)`) and `$stderr` gvar (`(gvar :$stderr)`),
// method `:puts`.
// In Murphy `::STDERR` collapses to `Const{scope:None}`: `nil?` covers bare +
// `::` for `STDERR` (namespaced `Foo::STDERR` still accepts — pinned by
// `boundary_accepts_namespaced_stderr_puts`; `::STDERR` flags — pinned by
// pre-existing `flags_stderr_qualified_const_puts`). `send` covers `Send`
// only (not `Csend`), matching the `#[on_node(kind = "send")]` dispatch
// (pinned by `boundary_accepts_csend_stderr_puts`). Argument-count guard
// (`$_ ...`, ≥1 arg) stays hand-rolled below.
def_node_matcher!(stderr_const, "(send (const nil? :STDERR) :puts ...)");
def_node_matcher!(stderr_gvar, "(send (gvar :$stderr) :puts ...)");

/// Stateless unit struct.
#[derive(Default)]
pub struct StderrPuts;

#[cop(
    name = "Style/StderrPuts",
    description = "Use `warn` instead of `$stderr.puts`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl StderrPuts {
    #[on_node(kind = "send", methods = ["puts"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Send {
        receiver,
        method: _,
        args,
    } = *cx.kind(node)
    else {
        return;
    };

    // `(send (const nil? :STDERR) :puts ...)` (`STDERR` / `::STDERR`,
    // top-level only) or `(send (gvar :$stderr) :puts ...)` (`$stderr`).
    // `send` covers `Send` only (not `Csend`).
    if !stderr_const(node, cx) && !stderr_gvar(node, cx) {
        return;
    }

    // Must be `puts` with at least one argument (RuboCop's `$_ ...`).
    if cx.list(args).is_empty() {
        return;
    }

    // Receiver for the offense range. Always present when a matcher above
    // passes.
    let Some(recv_id) = receiver.get() else {
        return;
    };

    // Offense range: from start of receiver to end of selector (`puts`).
    let recv_range = cx.range(recv_id);
    let selector_range = cx.selector(node);
    let offense_range = Range {
        start: recv_range.start,
        end: selector_range.end,
    };

    let recv_src = cx.raw_source(recv_range);
    let message =
        format!("Use `warn` instead of `{recv_src}.puts` to allow such output to be disabled.");

    cx.emit_offense(offense_range, &message, None);
    cx.emit_edit(offense_range, "warn");
}

#[cfg(test)]
mod tests {
    use super::StderrPuts;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Basic offense cases -----

    #[test]
    fn flags_stderr_gvar_puts_with_string_arg() {
        test::<StderrPuts>().expect_correction(
            indoc! {r#"
                $stderr.puts('hello')
                ^^^^^^^^^^^^ Use `warn` instead of `$stderr.puts` to allow such output to be disabled.
            "#},
            "warn('hello')\n",
        );
    }

    #[test]
    fn flags_stderr_const_puts_with_string_arg() {
        test::<StderrPuts>().expect_correction(
            indoc! {r#"
                STDERR.puts('hello')
                ^^^^^^^^^^^ Use `warn` instead of `STDERR.puts` to allow such output to be disabled.
            "#},
            "warn('hello')\n",
        );
    }

    #[test]
    fn flags_stderr_qualified_const_puts() {
        test::<StderrPuts>().expect_correction(
            indoc! {r#"
                ::STDERR.puts('hello')
                ^^^^^^^^^^^^^ Use `warn` instead of `::STDERR.puts` to allow such output to be disabled.
            "#},
            "warn('hello')\n",
        );
    }

    #[test]
    fn flags_stderr_puts_with_multiple_args() {
        test::<StderrPuts>().expect_correction(
            indoc! {r#"
                $stderr.puts('a', 'b')
                ^^^^^^^^^^^^ Use `warn` instead of `$stderr.puts` to allow such output to be disabled.
            "#},
            "warn('a', 'b')\n",
        );
    }

    #[test]
    fn flags_stderr_puts_with_no_parens() {
        test::<StderrPuts>().expect_correction(
            indoc! {r#"
                $stderr.puts 'hello'
                ^^^^^^^^^^^^ Use `warn` instead of `$stderr.puts` to allow such output to be disabled.
            "#},
            "warn 'hello'\n",
        );
    }

    // ----- Negative cases -----

    #[test]
    fn accepts_stderr_puts_with_no_args() {
        // No arguments: RuboCop does not flag `$stderr.puts` (the matcher
        // requires at least one argument via `$_ ...`).
        test::<StderrPuts>().expect_no_offenses("$stderr.puts\n");
    }

    #[test]
    fn accepts_warn_directly() {
        test::<StderrPuts>().expect_no_offenses("warn('hello')\n");
    }

    #[test]
    fn accepts_stdout_puts() {
        test::<StderrPuts>().expect_no_offenses("$stdout.puts('hello')\n");
        test::<StderrPuts>().expect_no_offenses("STDOUT.puts('hello')\n");
    }

    #[test]
    fn accepts_plain_puts() {
        test::<StderrPuts>().expect_no_offenses("puts('hello')\n");
    }

    #[test]
    fn accepts_stderr_other_method() {
        test::<StderrPuts>().expect_no_offenses("$stderr.print('hello')\n");
    }

    // --- Boundary characterization (murphy-ft88.7): pin the exact node set
    // the hand-rolled `STDERR` guard matches, so the verbatim
    // `(send (const nil? :STDERR) :puts ...)` +
    // `(send (gvar :$stderr) :puts ...)` refactor can be proven equivalent.
    // `::STDERR` collapses to `Const{scope:None}`: `nil?` covers bare + `::`
    // (pinned by pre-existing `flags_stderr_qualified_const_puts`). `send`
    // covers `Send` only (not `Csend`), matching the
    // `#[on_node(kind = "send")]` dispatch.

    #[test]
    fn boundary_accepts_namespaced_stderr_puts() {
        // `Foo::STDERR` is scoped: `is_global_const` rejects.
        test::<StderrPuts>().expect_no_offenses("Foo::STDERR.puts('hello')\n");
    }

    #[test]
    fn boundary_accepts_csend_stderr_puts() {
        // `&.` is a `csend` node; `send` covers `Send` only.
        test::<StderrPuts>().expect_no_offenses("STDERR&.puts('hello')\n");
        test::<StderrPuts>().expect_no_offenses("$stderr&.puts('hello')\n");
    }
}
murphy_plugin_api::submit_cop!(StderrPuts);
