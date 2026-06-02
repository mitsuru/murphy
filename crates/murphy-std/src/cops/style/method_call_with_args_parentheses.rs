//! `Style/MethodCallWithArgsParentheses` — use parentheses for method calls
//! with arguments.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/MethodCallWithArgsParentheses
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Only the default `require_parentheses` EnforcedStyle is implemented.
//!   The `omit_parentheses` style is not implemented (many ambiguity-detection
//!   edge cases; scope-trap; see gap note below).
//!   Guards:
//!     - AllowedMethods/AllowedPatterns: AllowedMethods supported; patterns not.
//!     - IgnoreMacros (default true): receiverless calls in macro scope are
//!       skipped unless they appear in IncludedMacros.
//!     - operator_method?: skipped (e.g. `foo + bar`).
//!     - setter_method?: skipped (e.g. `obj.foo = x`).
//!   Autocorrect: insert `(` between selector and first argument, insert `)`
//!   after last argument (surgical two-edit form).
//!   Gap: omit_parentheses style.
//!   Gap: AllowedPatterns (regex).
//!   Gap: IncludedMacros / IncludedMacroPatterns (force parens on specific macros).
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! some_method arg1, arg2
//! obj.some_method arg1
//!
//! # good
//! some_method(arg1, arg2)
//! obj.some_method(arg1)
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, Range, cop};

const MSG: &str = "Use parentheses for method calls with arguments.";

/// Stateless unit struct.
#[derive(Default)]
pub struct MethodCallWithArgsParentheses;

/// Options for `Style/MethodCallWithArgsParentheses`.
#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "IgnoreMacros",
        default = true,
        description = "When true, receiverless macro-scope calls are not required to have parentheses."
    )]
    pub ignore_macros: bool,

    #[option(
        name = "AllowedMethods",
        default = [],
        description = "Methods that are allowed to omit parentheses."
    )]
    pub allowed_methods: Vec<String>,
}

#[cop(
    name = "Style/MethodCallWithArgsParentheses",
    description = "Use parentheses for method calls with arguments.",
    default_severity = "warning",
    default_enabled = false,
    options = Options,
)]
impl MethodCallWithArgsParentheses {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Must have at least one argument.
    let args = cx.call_arguments(node);
    if args.is_empty() {
        return;
    }

    // Skip if already parenthesized.
    if cx.is_parenthesized(node) {
        return;
    }

    // Skip operator methods (e.g. `foo + bar`).
    if cx.is_operator_method(node) {
        return;
    }

    // Skip setter methods (e.g. `obj.foo = x`).
    if cx.is_setter_method(node) {
        return;
    }

    let opts = cx.options_or_default::<Options>();

    // Skip AllowedMethods.
    if cx.method_name(node).is_some_and(|name| opts.allowed_methods.iter().any(|m| m == name)) {
        return;
    }

    // IgnoreMacros: skip receiverless calls in macro scope.
    if opts.ignore_macros && cx.is_macro(node) {
        return;
    }

    // Offense range = the whole node.
    cx.emit_offense(cx.range(node), MSG, None);

    // Autocorrect: replace the gap between selector end and first arg start
    // with `(`, then insert `)` after the last arg.
    let selector = cx.selector(node);
    if selector == Range::ZERO {
        return;
    }
    let selector_end = selector.end;
    let first_arg_start = cx.range(args[0]).start;
    if selector_end >= first_arg_start {
        return;
    }
    let last_arg_end = cx.range(args[args.len() - 1]).end;

    cx.emit_edit(
        Range {
            start: selector_end,
            end: first_arg_start,
        },
        "(",
    );
    cx.emit_edit(
        Range {
            start: last_arg_end,
            end: last_arg_end,
        },
        ")",
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use murphy_plugin_api::test_support::{indoc, test};

    fn enabled_opts() -> Options {
        Options {
            ignore_macros: false,
            allowed_methods: vec![],
        }
    }

    // ---- detection ----

    #[test]
    fn flags_method_with_arg_no_parens() {
        test::<MethodCallWithArgsParentheses>()
            .with_options(&enabled_opts())
            .expect_offense(indoc! {"
                obj.some_method arg
                ^^^^^^^^^^^^^^^^^^^ Use parentheses for method calls with arguments.
            "});
    }

    #[test]
    fn flags_receiverless_call_with_args_ignore_macros_false() {
        // With IgnoreMacros: false, macro-scope calls are also flagged.
        test::<MethodCallWithArgsParentheses>()
            .with_options(&Options {
                ignore_macros: false,
                allowed_methods: vec![],
            })
            .expect_offense(indoc! {"
                some_method arg
                ^^^^^^^^^^^^^^^ Use parentheses for method calls with arguments.
            "});
    }

    // ---- no offense ----

    #[test]
    fn accepts_parenthesized_call() {
        test::<MethodCallWithArgsParentheses>()
            .with_options(&enabled_opts())
            .expect_no_offenses("obj.some_method(arg)\n");
    }

    #[test]
    fn accepts_operator_method() {
        test::<MethodCallWithArgsParentheses>()
            .with_options(&enabled_opts())
            .expect_no_offenses("foo + bar\n");
    }

    #[test]
    fn accepts_setter_method() {
        test::<MethodCallWithArgsParentheses>()
            .with_options(&enabled_opts())
            .expect_no_offenses("obj.foo = x\n");
    }

    #[test]
    fn accepts_macro_with_ignore_macros_true() {
        // Default: macros in class body are ignored.
        test::<MethodCallWithArgsParentheses>()
            .with_options(&Options {
                ignore_macros: true,
                allowed_methods: vec![],
            })
            .expect_no_offenses(indoc! {"
                class Foo
                  attr_reader :bar
                end
            "});
    }

    #[test]
    fn accepts_no_args_call() {
        test::<MethodCallWithArgsParentheses>()
            .with_options(&enabled_opts())
            .expect_no_offenses("obj.some_method\n");
    }

    #[test]
    fn accepts_allowed_method() {
        test::<MethodCallWithArgsParentheses>()
            .with_options(&Options {
                ignore_macros: false,
                allowed_methods: vec!["some_method".to_string()],
            })
            .expect_no_offenses("obj.some_method arg\n");
    }

    // ---- autocorrect ----

    #[test]
    fn corrects_method_with_single_arg() {
        test::<MethodCallWithArgsParentheses>()
            .with_options(&enabled_opts())
            .expect_correction(
                indoc! {"
                    obj.some_method arg
                    ^^^^^^^^^^^^^^^^^^^ Use parentheses for method calls with arguments.
                "},
                "obj.some_method(arg)\n",
            );
    }

    #[test]
    fn corrects_method_with_multiple_args() {
        test::<MethodCallWithArgsParentheses>()
            .with_options(&enabled_opts())
            .expect_correction(
                indoc! {"
                    obj.some_method arg1, arg2
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^ Use parentheses for method calls with arguments.
                "},
                "obj.some_method(arg1, arg2)\n",
            );
    }

    #[test]
    fn autocorrect_is_idempotent() {
        test::<MethodCallWithArgsParentheses>()
            .with_options(&enabled_opts())
            .expect_no_offenses("obj.some_method(arg)\n");
    }
}

murphy_plugin_api::submit_cop!(MethodCallWithArgsParentheses);
