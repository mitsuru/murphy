//! `Style/NestedParenthesizedCalls` — parenthesize method calls nested inside
//! another parenthesized method call's argument list.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/NestedParenthesizedCalls
//! upstream_version_checked: 1.84.2
//! version_added: "0.36"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Full parity with RuboCop's Style/NestedParenthesizedCalls.
//!   AllowedMethods is ported with the same 17-entry default list.
//!   The autocorrect_incompatible_with [Style::MethodCallWithArgsParentheses]
//!   declaration has no Murphy equivalent — not a behavioral gap; Murphy's fix
//!   scheduler handles conflicting edits via idempotency.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad — nested call lacks parentheses
//! method1(method2 arg)
//!
//! # good — nested call is already parenthesized
//! method1(method2(arg))
//!
//! # good — nested call has no arguments
//! method1(method2)
//!
//! # good — allowed method with single arg on each side
//! method1(eq arg)
//! ```
//!
//! ## Why this shape
//!
//! RuboCop's `on_send` (plus `alias on_csend on_send`) fires on every `Send`
//! or `Csend` node. For each parenthesized outer call, the cop checks the
//! direct-argument children that are themselves call nodes (`each_child_node(:call)`).
//! Murphy replicates this with `#[on_node(kind = "send")]` and
//! `#[on_node(kind = "csend")]` dispatching to a shared `check` function.
//! Iterating `cx.call_arguments(outer)` and filtering to `Send`/`Csend` kinds
//! is equivalent to RuboCop's `:call` child iteration.
//!
//! ## Autocorrect
//!
//! Two surgical edits (per `.claude/rules/autocorrect-pattern.md`):
//!
//! 1. Replace the whitespace gap between the nested selector's end and its
//!    first argument's start with `(`.
//! 2. Insert `)` after the last argument.
//!
//! This mirrors RuboCop's `replace(leading_space, '(')` +
//! `insert_after(last_arg, ')')`.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};

/// Options for `Style/NestedParenthesizedCalls`.
#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "AllowedMethods",
        default = [
            "be",
            "be_a",
            "be_an",
            "be_between",
            "be_falsey",
            "be_kind_of",
            "be_instance_of",
            "be_truthy",
            "be_within",
            "eq",
            "eql",
            "end_with",
            "include",
            "match",
            "raise_error",
            "respond_to",
            "start_with",
        ],
        description = "Methods allowed to omit parentheses when they are the sole argument \
                       of a parenthesized call and themselves take exactly one argument."
    )]
    pub allowed_methods: Vec<String>,
}

/// Stateless unit struct.
#[derive(Default)]
pub struct NestedParenthesizedCalls;

#[cop(
    name = "Style/NestedParenthesizedCalls",
    description = "Parenthesize method calls which are nested inside the argument list \
                   of another parenthesized method call.",
    default_severity = "warning",
    default_enabled = true,
    options = Options,
)]
impl NestedParenthesizedCalls {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(outer: NodeId, cx: &Cx<'_>) {
    // Only inspect parenthesized outer calls.
    if !cx.is_parenthesized(outer) {
        return;
    }

    let outer_args = cx.call_arguments(outer);
    let opts = cx.options_or_default::<Options>();

    for &nested in outer_args {
        // Only consider direct children that are themselves call nodes.
        if !matches!(cx.kind(nested), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
            continue;
        }

        if allowed_omission(nested, outer_args, &opts, cx) {
            continue;
        }

        let source = cx.raw_source(cx.range(nested));
        let msg = format!("Add parentheses to nested method call `{source}`.");
        cx.emit_offense(cx.range(nested), &msg, None);

        // Autocorrect: insert `(` before first arg and `)` after last arg.
        autocorrect(nested, cx);
    }
}

/// Mirrors RuboCop's `allowed_omission?`.
fn allowed_omission(nested: NodeId, outer_args: &[NodeId], opts: &Options, cx: &Cx<'_>) -> bool {
    // No arguments on the nested call — nothing to parenthesize.
    if cx.call_arguments(nested).is_empty() {
        return true;
    }

    // Already parenthesized — no offense.
    if cx.is_parenthesized(nested) {
        return true;
    }

    // Setter methods (e.g. `obj.foo = x`) — skip.
    if cx.is_setter_method(nested) {
        return true;
    }

    // Operator methods (e.g. `a + b`) — skip.
    if cx.is_operator_method(nested) {
        return true;
    }

    // AllowedMethods special case: skip iff
    //   - the outer call has exactly one argument (the nested call is the sole arg),
    //   - the nested method name is in AllowedMethods, and
    //   - the nested call also has exactly one argument.
    if outer_args.len() == 1
        && cx.call_arguments(nested).len() == 1
        && cx
            .method_name(nested)
            .is_some_and(|name| opts.allowed_methods.iter().any(|m| m == name))
    {
        return true;
    }

    false
}

/// Emit two surgical edits: replace the space before first arg with `(`,
/// insert `)` after the last arg.
fn autocorrect(nested: NodeId, cx: &Cx<'_>) {
    let selector = cx.selector(nested);
    if selector == Range::ZERO {
        return;
    }

    let nested_args = cx.call_arguments(nested);
    if nested_args.is_empty() {
        return;
    }

    let first_arg = nested_args[0];
    let last_arg = nested_args[nested_args.len() - 1];

    let selector_end = selector.end;
    let first_arg_start = cx.range(first_arg).start;
    let last_arg_end = cx.range(last_arg).end;

    // Guard: selector must end before first arg starts.
    if selector_end >= first_arg_start {
        return;
    }

    // Edit 1: replace the whitespace gap between selector and first arg with `(`.
    cx.emit_edit(
        Range {
            start: selector_end,
            end: first_arg_start,
        },
        "(",
    );

    // Edit 2: insert `)` immediately after the last argument.
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

    fn default_opts() -> Options {
        Options::default()
    }

    fn no_allowed_opts() -> Options {
        Options {
            allowed_methods: vec![],
        }
    }

    // ---- positive: flags unparenthesized nested calls ----

    #[test]
    fn flags_nested_call_with_arg_no_parens() {
        test::<NestedParenthesizedCalls>()
            .with_options(&no_allowed_opts())
            .expect_offense(indoc! {r#"
                method1(method2 arg)
                        ^^^^^^^^^^^ Add parentheses to nested method call `method2 arg`.
            "#});
    }

    #[test]
    fn flags_nested_call_with_multiple_args() {
        test::<NestedParenthesizedCalls>()
            .with_options(&no_allowed_opts())
            .expect_offense(indoc! {r#"
                method1(method2 arg1, arg2)
                        ^^^^^^^^^^^^^^^^^^ Add parentheses to nested method call `method2 arg1, arg2`.
            "#});
    }

    #[test]
    fn flags_nested_receiver_call() {
        test::<NestedParenthesizedCalls>()
            .with_options(&no_allowed_opts())
            .expect_offense(indoc! {r#"
                method1(obj.method2 arg)
                        ^^^^^^^^^^^^^^^ Add parentheses to nested method call `obj.method2 arg`.
            "#});
    }

    #[test]
    fn flags_nested_csend_call() {
        test::<NestedParenthesizedCalls>()
            .with_options(&no_allowed_opts())
            .expect_offense(indoc! {r#"
                method1(obj&.method2 arg)
                        ^^^^^^^^^^^^^^^^ Add parentheses to nested method call `obj&.method2 arg`.
            "#});
    }

    // ---- negative: no offense ----

    #[test]
    fn accepts_nested_call_already_parenthesized() {
        test::<NestedParenthesizedCalls>()
            .with_options(&no_allowed_opts())
            .expect_no_offenses("method1(method2(arg))\n");
    }

    #[test]
    fn accepts_outer_call_not_parenthesized() {
        test::<NestedParenthesizedCalls>()
            .with_options(&no_allowed_opts())
            .expect_no_offenses("method1 method2 arg\n");
    }

    #[test]
    fn accepts_nested_call_with_no_args() {
        test::<NestedParenthesizedCalls>()
            .with_options(&no_allowed_opts())
            .expect_no_offenses("method1(method2)\n");
    }

    #[test]
    fn accepts_nested_operator_method() {
        test::<NestedParenthesizedCalls>()
            .with_options(&no_allowed_opts())
            .expect_no_offenses("method1(a + b)\n");
    }

    #[test]
    fn accepts_nested_setter_method() {
        test::<NestedParenthesizedCalls>()
            .with_options(&no_allowed_opts())
            .expect_no_offenses("method1(obj.foo = x)\n");
    }

    // ---- AllowedMethods guard ----

    #[test]
    fn allows_allowed_method_with_single_arg_on_each_side() {
        // Outer has one arg (eq), nested has one arg — allowed omission.
        test::<NestedParenthesizedCalls>()
            .with_options(&default_opts())
            .expect_no_offenses("expect(eq 1)\n");
    }

    #[test]
    fn flags_allowed_method_when_nested_has_multiple_args_with_symbol() {
        // `method1(eq 1, other)` parses as `method1(eq(1, other))` — nested `eq`
        // has two args so the AllowedMethods guard does NOT apply.
        test::<NestedParenthesizedCalls>()
            .with_options(&default_opts())
            .expect_offense(indoc! {r#"
                method1(eq 1, other)
                        ^^^^^^^^^^^ Add parentheses to nested method call `eq 1, other`.
            "#});
    }

    #[test]
    fn flags_allowed_method_when_nested_has_multiple_args() {
        // `method1(eq 1, 2)` parses as `method1(eq(1, 2))` — nested `eq`
        // has two args so the AllowedMethods guard does NOT apply.
        test::<NestedParenthesizedCalls>()
            .with_options(&default_opts())
            .expect_offense(indoc! {r#"
                method1(eq 1, 2)
                        ^^^^^^^ Add parentheses to nested method call `eq 1, 2`.
            "#});
    }

    #[test]
    fn flags_non_allowed_method_even_with_single_args() {
        // Not in AllowedMethods — must be flagged.
        test::<NestedParenthesizedCalls>()
            .with_options(&default_opts())
            .expect_offense(indoc! {r#"
                method1(custom_method arg)
                        ^^^^^^^^^^^^^^^^^ Add parentheses to nested method call `custom_method arg`.
            "#});
    }

    #[test]
    fn allows_configured_allowed_method() {
        test::<NestedParenthesizedCalls>()
            .with_options(&Options {
                allowed_methods: vec!["foo".to_string()],
            })
            .expect_no_offenses("method1(foo arg)\n");
    }

    // ---- default AllowedMethods entries ----

    #[test]
    fn allows_default_be_method() {
        test::<NestedParenthesizedCalls>()
            .with_options(&default_opts())
            .expect_no_offenses("expect(be true)\n");
    }

    #[test]
    fn allows_default_include_method() {
        test::<NestedParenthesizedCalls>()
            .with_options(&default_opts())
            .expect_no_offenses("expect(include :x)\n");
    }

    #[test]
    fn allows_default_respond_to_method() {
        test::<NestedParenthesizedCalls>()
            .with_options(&default_opts())
            .expect_no_offenses("expect(respond_to :to_s)\n");
    }

    // ---- autocorrect ----

    #[test]
    fn corrects_nested_call_with_single_arg() {
        test::<NestedParenthesizedCalls>()
            .with_options(&no_allowed_opts())
            .expect_correction(
                indoc! {r#"
                    method1(method2 arg)
                            ^^^^^^^^^^^ Add parentheses to nested method call `method2 arg`.
                "#},
                "method1(method2(arg))\n",
            );
    }

    #[test]
    fn corrects_nested_call_with_multiple_args() {
        test::<NestedParenthesizedCalls>()
            .with_options(&no_allowed_opts())
            .expect_correction(
                indoc! {r#"
                    method1(method2 arg1, arg2)
                            ^^^^^^^^^^^^^^^^^^ Add parentheses to nested method call `method2 arg1, arg2`.
                "#},
                "method1(method2(arg1, arg2))\n",
            );
    }

    #[test]
    fn corrects_nested_receiver_call() {
        test::<NestedParenthesizedCalls>()
            .with_options(&no_allowed_opts())
            .expect_correction(
                indoc! {r#"
                    method1(obj.method2 arg)
                            ^^^^^^^^^^^^^^^ Add parentheses to nested method call `obj.method2 arg`.
                "#},
                "method1(obj.method2(arg))\n",
            );
    }

    #[test]
    fn autocorrect_is_idempotent() {
        test::<NestedParenthesizedCalls>()
            .with_options(&no_allowed_opts())
            .expect_no_offenses("method1(method2(arg))\n");
    }
}

murphy_plugin_api::submit_cop!(NestedParenthesizedCalls);
