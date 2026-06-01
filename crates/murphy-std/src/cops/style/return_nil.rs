//! `Style/ReturnNil` — enforces consistency between `return nil` and `return`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ReturnNil
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Disabled by default (matches RuboCop's default).
//!
//!   EnforcedStyle: return (default) — flags `return nil`, suggests `return`.
//!   EnforcedStyle: return_nil — flags bare `return`, suggests `return nil`.
//!
//!   Ancestor guard: a `return` inside a block that has at least one argument
//!   and whose block's send has a receiver (i.e. a chained send like `arr.each
//!   do |x| ... end`) is skipped before any def/lambda/define_method boundary
//!   is reached — this mirrors RuboCop's `chained_send?` check.
//!
//!   Lambda boundary: `-> { return nil }` terminates ancestor walking (lambda
//!   is treated as a scoping def-like node).
//!
//!   define_method/define_singleton_method calls in block position also
//!   terminate ancestor walking without emitting an offense.
//!
//!   Numblock (`arr.each_with_index { _1 }`) is treated like Block: has
//!   implicit arguments, so it is treated as args-present. Skipped when
//!   the block call has a receiver (chained). This is conservative: RuboCop's
//!   `each_ancestor(:block, :any_def)` may not match `:numblock` at all and
//!   could flag `return nil` inside numbered-parameter chained blocks. Murphy's
//!   behavior here is a deliberate conservative choice (no false positives).
//!
//!   Gaps:
//!     - `return(nil)` is not handled; the parenthesised form parses as Unknown
//!       in Murphy's translator and no offense is emitted.
//!     - Numblock handling may diverge from RuboCop for numbered-param blocks
//!       (conservative, see note above).
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct ReturnNil;

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    #[default]
    #[option(value = "return")]
    Return,
    #[option(value = "return_nil")]
    ReturnNil,
}

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "EnforcedStyle",
        default = "return",
        description = "When `return`, prefer bare `return` over `return nil`. When `return_nil`, prefer `return nil` over bare `return`."
    )]
    pub enforced_style: EnforcedStyle,
}

const RETURN_MSG: &str = "Use `return` instead of `return nil`.";
const RETURN_NIL_MSG: &str = "Use `return nil` instead of `return`.";

#[cop(
    name = "Style/ReturnNil",
    description = "Enforces consistency between `return nil` and `return`.",
    default_severity = "warning",
    default_enabled = false,
    options = Options,
)]
impl ReturnNil {
    #[on_node(kind = "return")]
    fn check_return(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<Options>();

        let NodeKind::Return(value_opt) = *cx.kind(node) else {
            return;
        };

        let is_return_nil = value_opt.get().is_some_and(|v| matches!(cx.kind(v), NodeKind::Nil));
        let is_bare_return = value_opt.get().is_none();

        // Check whether this return is in a method def scope (skipping blocks).
        // If it's in a chained-send block (with args) before hitting a def,
        // skip the offense.
        if should_skip_due_to_ancestor(node, cx) {
            return;
        }

        match opts.enforced_style {
            EnforcedStyle::Return => {
                if is_return_nil {
                    cx.emit_offense(cx.range(node), RETURN_MSG, None);
                    // Autocorrect: replace whole `return nil` with `return`
                    cx.emit_edit(cx.range(node), "return");
                }
            }
            EnforcedStyle::ReturnNil => {
                if is_bare_return {
                    cx.emit_offense(cx.range(node), RETURN_NIL_MSG, None);
                    // Autocorrect: replace bare `return` with `return nil`
                    cx.emit_edit(cx.range(node), "return nil");
                }
            }
        }
    }
}

/// Returns `true` if the return node should be skipped.
///
/// Walk ancestors from innermost to outermost:
/// - Hit a `Def` or `Defs` -> we are inside a method; do NOT skip (flag it).
/// - Hit a `Block` or `Numblock` where the send node is a lambda -> stop,
///   do NOT flag (lambdas are scoping).
/// - Hit a `Block` or `Numblock` where the block's send is `define_method` /
///   `define_singleton_method` -> stop, do NOT flag.
/// - Hit a `Block` or `Numblock` that has at least one argument AND whose
///   send-receiver is present (chained send) -> skip (return true).
/// - Otherwise continue walking up.
/// - If no Def/Block reached -> do NOT skip (top-level return is flagged).
fn should_skip_due_to_ancestor(node: NodeId, cx: &Cx<'_>) -> bool {
    for ancestor in cx.ancestors(node) {
        match cx.kind(ancestor) {
            NodeKind::Def { .. } | NodeKind::Defs { .. } => {
                // We are inside a method -- flag the offense.
                return false;
            }
            NodeKind::Block { call, args, .. } => {
                // Lambda: acts as a scoping boundary (like def), stop walking but
                // DO flag (RuboCop's scoped_node? breaks the loop, not returns nil).
                if cx.is_lambda(ancestor) {
                    return false;
                }
                // define_method / define_singleton_method: stop walking but DO flag.
                if is_define_method(*call, cx) {
                    return false;
                }
                // Block with args and chained send (receiver present) -> skip.
                let has_args = block_has_args(*args, cx);
                if has_args && block_has_receiver(*call, cx) {
                    return true;
                }
            }
            NodeKind::Numblock { send, .. } => {
                // Numblock always has implicit numbered params -> treat as having args.
                // Lambda: acts as a scoping boundary, DO flag.
                if cx.is_lambda(ancestor) {
                    return false;
                }
                if is_define_method(*send, cx) {
                    return false;
                }
                // Numblock always has a receiver (chained) by nature.
                if block_has_receiver(*send, cx) {
                    return true;
                }
            }
            _ => {}
        }
    }
    // Top-level return -- flag it.
    false
}

/// Returns `true` if `call_id` is a Send whose method is `define_method` or
/// `define_singleton_method`.
fn is_define_method(call_id: NodeId, cx: &Cx<'_>) -> bool {
    cx.method_name(call_id)
        .is_some_and(|name| name == "define_method" || name == "define_singleton_method")
}

/// Returns `true` if `call_id` is a Send with a non-None receiver.
fn block_has_receiver(call_id: NodeId, cx: &Cx<'_>) -> bool {
    cx.call_receiver(call_id).get().is_some()
}

/// Returns `true` if the `Args` node at `args_id` has at least one parameter.
fn block_has_args(args_id: NodeId, cx: &Cx<'_>) -> bool {
    match cx.kind(args_id) {
        NodeKind::Args(list) => !cx.list(*list).is_empty(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{EnforcedStyle, Options, ReturnNil};
    use murphy_plugin_api::test_support::{indoc, test};

    fn return_nil_opts() -> Options {
        Options { enforced_style: EnforcedStyle::ReturnNil }
    }

    // -------------------------------------------------------------------------
    // EnforcedStyle: return (default) -- flags `return nil`
    // -------------------------------------------------------------------------

    #[test]
    fn flags_return_nil_in_method() {
        test::<ReturnNil>().expect_offense(indoc! {"
            def foo(arg)
              return nil if arg
              ^^^^^^^^^^ Use `return` instead of `return nil`.
            end
        "});
    }

    #[test]
    fn no_offense_bare_return_in_method() {
        test::<ReturnNil>().expect_no_offenses(indoc! {"
            def foo(arg)
              return if arg
            end
        "});
    }

    #[test]
    fn no_offense_return_value_in_method() {
        test::<ReturnNil>().expect_no_offenses(indoc! {"
            def foo(arg)
              return 42 if arg
            end
        "});
    }

    #[test]
    fn corrects_return_nil_to_return() {
        test::<ReturnNil>().expect_correction(
            indoc! {"
                def foo(arg)
                  return nil if arg
                  ^^^^^^^^^^ Use `return` instead of `return nil`.
                end
            "},
            indoc! {"
                def foo(arg)
                  return if arg
                end
            "},
        );
    }

    #[test]
    fn no_offense_return_nil_in_chained_block_with_args() {
        // arr.each do |x| return nil end -- RuboCop skips this.
        test::<ReturnNil>().expect_no_offenses(indoc! {"
            [1, 2].each do |x|
              return nil if x
            end
        "});
    }

    #[test]
    fn flags_return_nil_in_method_body() {
        test::<ReturnNil>().expect_offense(indoc! {"
            def foo
              return nil
              ^^^^^^^^^^ Use `return` instead of `return nil`.
            end
        "});
    }

    #[test]
    fn flags_return_nil_in_lambda() {
        // Lambda bodies are flagged just like method bodies.
        test::<ReturnNil>().expect_offense(indoc! {"
            -> { return nil }
                 ^^^^^^^^^^ Use `return` instead of `return nil`.
        "});
    }

    // -------------------------------------------------------------------------
    // EnforcedStyle: return_nil -- flags bare `return`
    // -------------------------------------------------------------------------

    #[test]
    fn flags_bare_return_in_method() {
        test::<ReturnNil>()
            .with_options(&return_nil_opts())
            .expect_offense(indoc! {"
                def foo(arg)
                  return if arg
                  ^^^^^^ Use `return nil` instead of `return`.
                end
            "});
    }

    #[test]
    fn no_offense_return_nil_style_return_nil() {
        test::<ReturnNil>()
            .with_options(&return_nil_opts())
            .expect_no_offenses(indoc! {"
                def foo(arg)
                  return nil if arg
                end
            "});
    }

    #[test]
    fn corrects_bare_return_to_return_nil() {
        test::<ReturnNil>()
            .with_options(&return_nil_opts())
            .expect_correction(
                indoc! {"
                    def foo(arg)
                      return if arg
                      ^^^^^^ Use `return nil` instead of `return`.
                    end
                "},
                indoc! {"
                    def foo(arg)
                      return nil if arg
                    end
                "},
            );
    }

    #[test]
    fn no_offense_bare_return_in_chained_block_with_args() {
        test::<ReturnNil>()
            .with_options(&return_nil_opts())
            .expect_no_offenses(indoc! {"
                [1, 2].each do |x|
                  return if x
                end
            "});
    }

    #[test]
    fn flags_return_nil_in_singleton_method() {
        test::<ReturnNil>().expect_offense(indoc! {"
            def self.foo(arg)
              return nil if arg
              ^^^^^^^^^^ Use `return` instead of `return nil`.
            end
        "});
    }
}

murphy_plugin_api::submit_cop!(ReturnNil);
