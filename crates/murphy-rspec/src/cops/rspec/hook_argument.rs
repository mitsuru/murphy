//! `RSpec/HookArgument` — consistent hook scope arguments.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/HookArgument
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `scoped_hook`
//!   (`(any_block $(send _ #Hooks.all (sym ${:each :example})) ...)`)
//!   and `unscoped_hook` (`(any_block $(send _ #Hooks.all) ...)`) with
//!   `EnforcedStyle` (`implicit` default, `each` / `example` alternates).
//!   The receiver is unconstrained upstream (`_`), so explicit-receiver
//!   hooks flag too; any other arity (e.g. `:all`, `:suite`, two args)
//!   matches neither arm and is clean. A scoped hook whose scope equals
//!   the style is clean; otherwise the whole `Send` is flagged (trimmed
//!   to exclude a wrapping block via `send_without_block_range`, since
//!   Murphy's `Send` range covers the block while RuboCop's does not).
//!   A bare hook under an explicit style flags the selector only per
//!   `add_offense(method_send.loc.selector)`. Messages mirror
//!   `IMPLICIT_MSG` / `EXPLICIT_MSG` (`%p` renders `:each` / `:example`).
//!   Detection is at parity; autocorrect (add/remove the scope argument)
//!   is not ported in this batch — same convention as
//!   `RSpec/NotToNot` (status: partial, autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` and `Numblock`. Default style `implicit`:
//!
//! - `before(:each) do ... end` — flagged (whole `before(:each)`).
//! - `before(:example) do ... end` — flagged.
//! - `before do ... end` — preferred spelling, not flagged.
//! - `before(:all) do ... end` — other scope, not flagged.
//! - `around(:each) { }` — flagged (all hook selectors apply).
//!
//! Alternate style `each` (`EnforcedStyle: each`) mirrors:
//! `before(:example)` flags (whole send), bare `before` flags
//! (selector only), `before(:each)` passes. Style `example` mirrors
//! with the roles swapped.
//!
//! ## No autocorrect
//!
//! Upstream adds or removes the scope argument. This batch reports only.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, cop};

use crate::cops::rspec_helpers::{is_hook_name, send_without_block_range};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct HookArgument;

#[derive(CopOptions)]
pub struct HookArgumentOptions {
    #[option(
        name = "EnforcedStyle",
        default = "implicit",
        description = "Whether hooks use implicit, each, or example scope style."
    )]
    pub enforced_style: HookArgumentStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum HookArgumentStyle {
    #[option(value = "implicit")]
    Implicit,
    #[option(value = "each")]
    Each,
    #[option(value = "example")]
    Example,
}

#[cop(
    name = "RSpec/HookArgument",
    description = "Checks the arguments passed to `before`, `around`, and `after`.",
    default_severity = "warning",
    default_enabled = true,
    options = HookArgumentOptions,
)]
impl HookArgument {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, .. } = *cx.kind(node) else {
            return;
        };
        self.check_hook_call(call, cx);
    }

    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Numblock { send, .. } = *cx.kind(node) else {
            return;
        };
        self.check_hook_call(send, cx);
    }

    fn check_hook_call(&self, call: NodeId, cx: &Cx<'_>) {
        // Any receiver upstream (`_`), so only the method name is gated.
        let NodeKind::Send { method, args, .. } = *cx.kind(call) else {
            return;
        };
        if !is_hook_name(cx.symbol_str(method)) {
            return;
        }
        let opts = cx.options_or_default::<HookArgumentOptions>();
        let arg_ids = cx.list(args);
        match arg_ids {
            [] => {
                // Unscoped hook: clean under implicit, selector offense
                // under an explicit style.
                if opts.enforced_style == HookArgumentStyle::Implicit {
                    return;
                }
                let style = match opts.enforced_style {
                    HookArgumentStyle::Each => "each",
                    HookArgumentStyle::Example => "example",
                    HookArgumentStyle::Implicit => unreachable!(),
                };
                cx.emit_offense(
                    cx.node(call).loc.name,
                    &format!("Use `:{style}` for RSpec hooks."),
                    None,
                );
            }
            [scope_arg] => {
                let NodeKind::Sym(sym) = *cx.kind(*scope_arg) else {
                    return;
                };
                let scope = cx.symbol_str(sym);
                if scope != "each" && scope != "example" {
                    return;
                }
                let desired = match opts.enforced_style {
                    HookArgumentStyle::Implicit => None,
                    HookArgumentStyle::Each => Some("each"),
                    HookArgumentStyle::Example => Some("example"),
                };
                if desired == Some(scope) {
                    return;
                }
                let msg = match desired {
                    None => format!("Omit the default `:{scope}` argument for RSpec hooks."),
                    Some(style) => format!("Use `:{style}` for RSpec hooks."),
                };
                cx.emit_offense(send_without_block_range(cx, call), &msg, None);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{HookArgument, HookArgumentOptions, HookArgumentStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn each_style() -> HookArgumentOptions {
        HookArgumentOptions {
            enforced_style: HookArgumentStyle::Each,
        }
    }

    fn example_style() -> HookArgumentOptions {
        HookArgumentOptions {
            enforced_style: HookArgumentStyle::Example,
        }
    }

    #[test]
    fn flags_each_scope_in_implicit_style() {
        test::<HookArgument>().expect_offense(indoc! {r#"
                before(:each) do
                ^^^^^^^^^^^^^ Omit the default `:each` argument for RSpec hooks.
                end
            "#});
    }

    #[test]
    fn flags_example_scope_in_implicit_style() {
        test::<HookArgument>().expect_offense(indoc! {r#"
                before(:example) do
                ^^^^^^^^^^^^^^^^ Omit the default `:example` argument for RSpec hooks.
                end
            "#});
    }

    #[test]
    fn does_not_flag_bare_hook_in_implicit_style() {
        test::<HookArgument>().expect_no_offenses(indoc! {r#"
                before do
                end
            "#});
    }

    #[test]
    fn does_not_flag_other_scope_in_implicit_style() {
        // Only `:each` / `:example` participate; `:all` matches neither arm.
        test::<HookArgument>().expect_no_offenses(indoc! {r#"
                before(:all) do
                end
            "#});
    }

    #[test]
    fn flags_around_each_in_implicit_style() {
        test::<HookArgument>().expect_offense(indoc! {r#"
                around(:each) { }
                ^^^^^^^^^^^^^ Omit the default `:each` argument for RSpec hooks.
            "#});
    }

    #[test]
    fn does_not_flag_each_scope_in_each_style() {
        test::<HookArgument>()
            .with_options(&each_style())
            .expect_no_offenses(indoc! {r#"
                before(:each) do
                end
            "#});
    }

    #[test]
    fn flags_bare_hook_in_each_style() {
        // Unscoped under an explicit style flags the selector only.
        test::<HookArgument>()
            .with_options(&each_style())
            .expect_offense(indoc! {r#"
                before do
                ^^^^^^ Use `:each` for RSpec hooks.
                end
            "#});
    }

    #[test]
    fn flags_example_scope_in_each_style() {
        test::<HookArgument>()
            .with_options(&each_style())
            .expect_offense(indoc! {r#"
                before(:example) do
                ^^^^^^^^^^^^^^^^ Use `:each` for RSpec hooks.
                end
            "#});
    }

    #[test]
    fn flags_each_scope_in_example_style() {
        test::<HookArgument>()
            .with_options(&example_style())
            .expect_offense(indoc! {r#"
                before(:each) do
                ^^^^^^^^^^^^^ Use `:example` for RSpec hooks.
                end
            "#});
    }

    #[test]
    fn does_not_flag_example_scope_in_example_style() {
        test::<HookArgument>()
            .with_options(&example_style())
            .expect_no_offenses(indoc! {r#"
                before(:example) do
                end
            "#});
    }

    #[test]
    fn flags_explicit_receiver_hook() {
        // Upstream matches any receiver (`_`), including explicit ones.
        test::<HookArgument>().expect_offense(indoc! {r#"
                RSpec.before(:each) do
                ^^^^^^^^^^^^^^^^^^^ Omit the default `:each` argument for RSpec hooks.
                end
            "#});
    }

    #[test]
    fn does_not_flag_multi_arg_hook() {
        // Two args match neither the scoped nor the unscoped arm.
        test::<HookArgument>().expect_no_offenses(indoc! {r#"
                before(:each, :foo) do
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(HookArgument);
