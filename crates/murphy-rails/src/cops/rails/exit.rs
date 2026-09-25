//! `Rails/Exit` — flag `exit`/`exit!`/`abort` calls in Rails applications.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/Exit
//! upstream_version_checked: 2.35.0
//! version_added: "0.0"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND [:exit, :exit!, :abort]
//!   gating, the `arguments.size <= 1` gate (a two-argument `exit` is some
//!   other implementation), and the receiver gate (nil receiver, or a
//!   `Kernel`/`Process` constant matched on the name segment, so `::Kernel`
//!   and any scope's `Kernel`/`Process` match — mirroring the upstream const
//!   destructure). Offense on the selector range; no autocorrect. Upstream
//!   Include/Exclude path gating (`app/`, `config/`, `lib/` minus rake
//!   files) has no file-path infrastructure in Murphy yet, so the cop fires
//!   in all files; users can scope it per-directory via `.murphy.yml`.
//! ```
//!
//! ## Matched shape (Send node)
//!
//! `Send(receiver=None | Const{name in {Kernel, Process}}, method in
//! {exit, exit!, abort}, args.len() <= 1)` — e.g. `exit(0)`,
//! `Kernel.exit(1)`, `Process.abort("boom")`. `exit(0, 1)` (two args) and
//! `foo.exit` (other receiver) do not flag.
//!
//! ## No autocorrect
//!
//! Stopping execution has no single mechanical replacement (`raise`,
//! `break`, `return` all depend on context); upstream is detect-only too.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct Exit;

#[cop(
    name = "Rails/Exit",
    description = "Do not use `exit` in Rails applications.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl Exit {
    // Mirrors upstream `RESTRICT_ON_SEND = %i[exit exit! abort]`.
    #[on_node(kind = "send", methods = ["exit", "exit!", "abort"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // Upstream `right_argument_count?` — more than one argument likely
        // means a different `exit` implementation.
        if cx.call_arguments(node).len() > 1 {
            return;
        }
        // Upstream `right_receiver?` — nil receiver, or an explicit
        // `Kernel`/`Process` constant (matched on the name segment of the
        // const node, so `::Kernel` folds in identically).
        let NodeKind::Send { receiver, .. } = *cx.kind(node) else {
            return;
        };
        if let Some(recv) = receiver.get() {
            let NodeKind::Const { name, .. } = *cx.kind(recv) else {
                return;
            };
            let name = cx.symbol_str(name);
            if name != "Kernel" && name != "Process" {
                return;
            }
        }
        let Some(method) = cx.method_name(node) else {
            return;
        };
        cx.emit_offense(
            cx.selector(node),
            &format!("Do not use `{method}` in Rails applications."),
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::Exit;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_bare_exit() {
        test::<Exit>().expect_offense(indoc! {r#"
            exit
            ^^^^ Do not use `exit` in Rails applications.
        "#});
    }

    #[test]
    fn flags_exit_with_status() {
        test::<Exit>().expect_offense(indoc! {r#"
            exit(0)
            ^^^^ Do not use `exit` in Rails applications.
        "#});
    }

    #[test]
    fn flags_exit_without_parens() {
        test::<Exit>().expect_offense(indoc! {r#"
            exit 0
            ^^^^ Do not use `exit` in Rails applications.
        "#});
    }

    #[test]
    fn flags_kernel_exit() {
        test::<Exit>().expect_offense(indoc! {r#"
            Kernel.exit(1)
                   ^^^^ Do not use `exit` in Rails applications.
        "#});
    }

    #[test]
    fn flags_process_exit() {
        test::<Exit>().expect_offense(indoc! {r#"
            Process.exit(1)
                    ^^^^ Do not use `exit` in Rails applications.
        "#});
    }

    #[test]
    fn flags_cbase_kernel_exit() {
        test::<Exit>().expect_offense(indoc! {r#"
            ::Kernel.exit(1)
                     ^^^^ Do not use `exit` in Rails applications.
        "#});
    }

    #[test]
    fn flags_exit_bang() {
        test::<Exit>().expect_offense(indoc! {r#"
            exit!
            ^^^^^ Do not use `exit!` in Rails applications.
        "#});
    }

    #[test]
    fn flags_abort_with_message() {
        test::<Exit>().expect_offense(indoc! {r#"
            abort('msg')
            ^^^^^ Do not use `abort` in Rails applications.
        "#});
    }

    #[test]
    fn flags_kernel_abort() {
        test::<Exit>().expect_offense(indoc! {r#"
            Kernel.abort('x')
                   ^^^^^ Do not use `abort` in Rails applications.
        "#});
    }

    #[test]
    fn flags_exit_with_block() {
        // A block does not change the call shape upstream either.
        test::<Exit>().expect_offense(indoc! {r#"
            exit { foo }
            ^^^^ Do not use `exit` in Rails applications.
        "#});
    }

    #[test]
    fn flags_kernel_exit_without_parens() {
        test::<Exit>().expect_offense(indoc! {r#"
            Kernel.exit 1
                   ^^^^ Do not use `exit` in Rails applications.
        "#});
    }

    #[test]
    fn does_not_flag_two_arguments() {
        test::<Exit>().expect_no_offenses("exit(0, 1)\n");
    }

    #[test]
    fn does_not_flag_other_receiver() {
        test::<Exit>().expect_no_offenses("foo.exit\n");
    }

    #[test]
    fn does_not_flag_literal_receiver() {
        test::<Exit>().expect_no_offenses("'x'.exit\n");
    }
}
murphy_plugin_api::submit_cop!(Exit);
