//! `Style/SignalException` — enforces consistent use of `fail` vs `raise`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/SignalException
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   EnforcedStyle: only_raise (default) -- always use `raise`, never `fail`.
//!   EnforcedStyle: only_fail -- always use `fail`, never `raise`.
//!   EnforcedStyle: semantic -- use `fail` to signal exceptions initially,
//!   use `raise` to rethrow inside rescue handlers.
//!
//!   Covered:
//!     - only_raise: flags bare `fail` (no receiver) and `Kernel.fail`.
//!     - only_fail: flags bare `raise` (no receiver) and `Kernel.raise`.
//!     - semantic: flags bare `raise` outside a rescue handler body;
//!       flags bare `fail` inside a rescue handler body.
//!     - Autocorrect: replaces the selector (`fail` <-> `raise`).
//!
//!   Gaps:
//!     - only_raise: custom_fail_defined? check -- when a method named `fail`
//!       is defined in the file, RuboCop skips the only_raise enforcement.
//!       Murphy does not scan for custom `fail` definitions.
//!     - explicit non-Kernel receivers (e.g. `obj.fail`) are never flagged
//!       (consistent with RuboCop).
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct SignalException;

const FAIL_MSG: &str = "Use `fail` instead of `raise` to signal exceptions.";
const RAISE_MSG: &str = "Use `raise` instead of `fail` to rethrow exceptions.";

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    #[default]
    #[option(value = "only_raise")]
    OnlyRaise,
    #[option(value = "only_fail")]
    OnlyFail,
    #[option(value = "semantic")]
    Semantic,
}

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "EnforcedStyle",
        default = "only_raise",
        description = "When `only_raise`, always use `raise`. When `only_fail`, always use `fail`. When `semantic`, use `fail` to signal new exceptions and `raise` to rethrow inside rescue handlers."
    )]
    pub enforced_style: EnforcedStyle,
}

#[cop(
    name = "Style/SignalException",
    description = "Checks for proper usage of `fail` and `raise`.",
    default_severity = "warning",
    default_enabled = true,
    options = Options,
)]
impl SignalException {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<Options>();

        let NodeKind::Send { receiver, method, .. } = *cx.kind(node) else {
            return;
        };

        let method_str = cx.symbol_str(method);
        let is_fail = method_str == "fail";
        let is_raise = method_str == "raise";

        if !is_fail && !is_raise {
            return;
        }

        // Only flag bare calls (no receiver) or Kernel.fail / Kernel.raise.
        if let Some(recv_id) = receiver.get() {
            if !is_kernel_const(recv_id, cx) {
                return;
            }
        }

        match opts.enforced_style {
            EnforcedStyle::OnlyRaise => {
                if is_fail {
                    let selector = cx.selector(node);
                    cx.emit_offense(selector, "Always use `raise` to signal exceptions.", None);
                    cx.emit_edit(selector, "raise");
                }
            }
            EnforcedStyle::OnlyFail => {
                if is_raise {
                    let selector = cx.selector(node);
                    cx.emit_offense(selector, "Always use `fail` to signal exceptions.", None);
                    cx.emit_edit(selector, "fail");
                }
            }
            EnforcedStyle::Semantic => {
                let in_rescue = is_inside_resbody(node, cx);
                if is_raise && !in_rescue {
                    // raise used outside rescue handler -- should be `fail`
                    let selector = cx.selector(node);
                    cx.emit_offense(selector, FAIL_MSG, None);
                    cx.emit_edit(selector, "fail");
                } else if is_fail && in_rescue {
                    // fail used inside rescue handler -- should be `raise` to rethrow
                    let selector = cx.selector(node);
                    cx.emit_offense(selector, RAISE_MSG, None);
                    cx.emit_edit(selector, "raise");
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns `true` if `node` is a `Const` with name `:Kernel` and nil scope.
fn is_kernel_const(node: NodeId, cx: &Cx<'_>) -> bool {
    match cx.kind(node) {
        NodeKind::Const { scope, name } => {
            cx.symbol_str(*name) == "Kernel" && scope.get().is_none()
        }
        _ => false,
    }
}

/// Returns `true` if `node` is inside a `Resbody` body (i.e. inside
/// a rescue handler body). Walks ancestors until a `Resbody` is found.
/// Stops at `Def`/`Defs`/`Block`/`Numblock` boundaries that reset rescue scope.
///
/// When a `Resbody` is found, `child_id` is the direct child of `Resbody` on
/// the path from `node` to the root. If that child equals `body`, we are in
/// the handler body; otherwise (exceptions list, var binding) we are not.
fn is_inside_resbody(node: NodeId, cx: &Cx<'_>) -> bool {
    let mut child_id = node;
    for ancestor in cx.ancestors(node) {
        match cx.kind(ancestor) {
            NodeKind::Resbody { body, .. } => {
                return body.get() == Some(child_id);
            }
            // Method/block boundaries reset rescue scope.
            NodeKind::Def { .. }
            | NodeKind::Defs { .. }
            | NodeKind::Block { .. }
            | NodeKind::Numblock { .. } => {
                return false;
            }
            _ => {}
        }
        child_id = ancestor;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{EnforcedStyle, Options, SignalException};
    use murphy_plugin_api::test_support::{indoc, test};

    fn only_fail_opts() -> Options {
        Options { enforced_style: EnforcedStyle::OnlyFail }
    }

    fn semantic_opts() -> Options {
        Options { enforced_style: EnforcedStyle::Semantic }
    }

    // -------------------------------------------------------------------------
    // only_raise (default)
    // -------------------------------------------------------------------------

    #[test]
    fn flags_fail_in_begin_only_raise() {
        test::<SignalException>().expect_offense(indoc! {"
            begin
              fail
              ^^^^ Always use `raise` to signal exceptions.
            rescue Exception
              # handle
            end
        "});
    }

    #[test]
    fn corrects_fail_to_raise() {
        test::<SignalException>().expect_correction(
            indoc! {"
                begin
                  fail
                  ^^^^ Always use `raise` to signal exceptions.
                rescue Exception
                  # handle
                end
            "},
            indoc! {"
                begin
                  raise
                rescue Exception
                  # handle
                end
            "},
        );
    }

    #[test]
    fn flags_kernel_fail_only_raise() {
        test::<SignalException>().expect_offense(indoc! {"
            Kernel.fail
                   ^^^^ Always use `raise` to signal exceptions.
        "});
    }

    #[test]
    fn no_offense_raise_only_raise() {
        test::<SignalException>().expect_no_offenses("raise\n");
    }

    #[test]
    fn no_offense_kernel_raise_only_raise() {
        test::<SignalException>().expect_no_offenses("Kernel.raise\n");
    }

    #[test]
    fn no_offense_explicit_non_kernel_receiver() {
        // `obj.fail` has an explicit non-Kernel receiver -- not flagged.
        test::<SignalException>().expect_no_offenses("obj.fail\n");
    }

    #[test]
    fn no_offense_explicit_raise_with_message() {
        test::<SignalException>().expect_no_offenses("raise RuntimeError, 'msg'\n");
    }

    #[test]
    fn flags_fail_with_message_only_raise() {
        test::<SignalException>().expect_offense(indoc! {"
            fail RuntimeError, 'msg'
            ^^^^ Always use `raise` to signal exceptions.
        "});
    }

    // -------------------------------------------------------------------------
    // only_fail style
    // -------------------------------------------------------------------------

    #[test]
    fn flags_raise_only_fail() {
        test::<SignalException>()
            .with_options(&only_fail_opts())
            .expect_offense(indoc! {"
                raise
                ^^^^^ Always use `fail` to signal exceptions.
            "});
    }

    #[test]
    fn corrects_raise_to_fail() {
        test::<SignalException>()
            .with_options(&only_fail_opts())
            .expect_correction(
                indoc! {"
                    raise
                    ^^^^^ Always use `fail` to signal exceptions.
                "},
                "fail\n",
            );
    }

    #[test]
    fn flags_kernel_raise_only_fail() {
        test::<SignalException>()
            .with_options(&only_fail_opts())
            .expect_offense(indoc! {"
                Kernel.raise
                       ^^^^^ Always use `fail` to signal exceptions.
            "});
    }

    #[test]
    fn no_offense_fail_only_fail() {
        test::<SignalException>()
            .with_options(&only_fail_opts())
            .expect_no_offenses("fail\n");
    }

    // -------------------------------------------------------------------------
    // semantic style
    // -------------------------------------------------------------------------

    #[test]
    fn semantic_flags_raise_outside_rescue() {
        test::<SignalException>()
            .with_options(&semantic_opts())
            .expect_offense(indoc! {"
                begin
                  raise
                  ^^^^^ Use `fail` instead of `raise` to signal exceptions.
                rescue Exception
                  # handle
                end
            "});
    }

    #[test]
    fn semantic_flags_fail_inside_rescue() {
        test::<SignalException>()
            .with_options(&semantic_opts())
            .expect_offense(indoc! {"
                begin
                  fail
                rescue Exception
                  fail
                  ^^^^ Use `raise` instead of `fail` to rethrow exceptions.
                end
            "});
    }

    #[test]
    fn semantic_no_offense_fail_outside_rescue() {
        test::<SignalException>()
            .with_options(&semantic_opts())
            .expect_no_offenses(indoc! {"
                begin
                  fail
                rescue Exception
                  # handle
                end
            "});
    }

    #[test]
    fn semantic_no_offense_raise_inside_rescue() {
        test::<SignalException>()
            .with_options(&semantic_opts())
            .expect_no_offenses(indoc! {"
                begin
                  fail
                rescue Exception
                  raise
                end
            "});
    }

    #[test]
    fn semantic_corrects_raise_outside_rescue() {
        test::<SignalException>()
            .with_options(&semantic_opts())
            .expect_correction(
                indoc! {"
                    begin
                      raise
                      ^^^^^ Use `fail` instead of `raise` to signal exceptions.
                    rescue Exception
                      raise
                    end
                "},
                indoc! {"
                    begin
                      fail
                    rescue Exception
                      raise
                    end
                "},
            );
    }
}

murphy_plugin_api::submit_cop!(SignalException);
