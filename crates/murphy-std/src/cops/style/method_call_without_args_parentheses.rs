//! `Style/MethodCallWithoutArgsParentheses` — do not use parentheses for
//! method calls with no arguments.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/MethodCallWithoutArgsParentheses
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Flags Send/Csend nodes that have no arguments but are parenthesized.
//!   Guards:
//!     - CamelCase method names are skipped (`is_camel_case_method`).
//!     - Implicit calls (`foo.()`) are skipped (no method name).
//!     - `not` prefix form is skipped (`is_prefix_not`).
//!     - Default-argument context: skip when direct parent is `Optarg` or
//!       `Kwoptarg` (matches RuboCop's `default_argument?`).
//!     - same-name-assignment: skip receiverless calls whose method name
//!       matches a local-variable assignment ancestor, to prevent
//!       `foo = foo()` -> `foo = foo` (which would read the variable).
//!     - `it()` in an empty block argument list is skipped (Ruby 3.4 `it`
//!       semantics, consistent with `Lint/ItWithoutArgumentsInBlock`).
//!     - AllowedMethods list (default empty).
//!   Autocorrect: remove the `(` and `)` tokens (surgical, two edits).
//!   Gap: AllowedPatterns (regex) is not implemented.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! object.some_method()
//!
//! # good
//! object.some_method
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};

const MSG: &str = "Do not use parentheses for method calls with no arguments.";

/// Stateless unit struct.
#[derive(Default)]
pub struct MethodCallWithoutArgsParentheses;

/// Options for `Style/MethodCallWithoutArgsParentheses`.
#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "AllowedMethods",
        default = [],
        description = "Methods that are allowed to have empty parentheses."
    )]
    pub allowed_methods: Vec<String>,
}

#[cop(
    name = "Style/MethodCallWithoutArgsParentheses",
    description = "Do not use parentheses for method calls with no arguments.",
    default_severity = "warning",
    default_enabled = true,
    options = Options,
)]
impl MethodCallWithoutArgsParentheses {
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
    // Must have no arguments.
    if !cx.call_arguments(node).is_empty() {
        return;
    }

    // Must be parenthesized.
    if !cx.is_parenthesized(node) {
        return;
    }

    // Must have a method name (implicit call `foo.()` has no name).
    let Some(method_name) = cx.method_name(node) else {
        return;
    };

    // Skip camelcase methods (e.g. `SomeClass()`).
    if cx.is_camel_case_method(node) {
        return;
    }

    // Skip `not()` prefix form.
    if cx.is_prefix_not(node) {
        return;
    }

    // Skip when parent is an optional-argument node (`def foo(x = bar())`).
    if is_default_argument(node, cx) {
        return;
    }

    // Skip `it()` in an empty-block-param context (Ruby 3.4 `it` semantics).
    if is_parenthesized_it_in_empty_block(node, cx) {
        return;
    }

    // Skip receiverless calls whose name collides with a local assignment
    // ancestor (`foo = foo()` must not autocorrect to `foo = foo`).
    if is_same_name_assignment(node, method_name, cx) {
        return;
    }

    // Skip AllowedMethods.
    let opts = cx.options_or_default::<Options>();
    if opts.allowed_methods.iter().any(|m| m == method_name) {
        return;
    }

    // Offense: the range of `(` through `)`.
    let begin_tok = cx.loc(node).begin();
    let end_tok = cx.loc(node).end();
    if begin_tok == Range::ZERO || end_tok == Range::ZERO {
        // Should not happen for is_parenthesized nodes, but guard defensively.
        return;
    }

    let offense_range = Range {
        start: begin_tok.start,
        end: end_tok.end,
    };
    cx.emit_offense(offense_range, MSG, None);

    // Autocorrect: delete `(` and `)`.
    cx.emit_edit(begin_tok, "");
    cx.emit_edit(end_tok, "");
}

/// Returns `true` when the direct parent of `node` is an `Optarg` or
/// `Kwoptarg` node (i.e. the call is a default-argument expression).
fn is_default_argument(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    matches!(
        cx.kind(parent),
        NodeKind::Optarg { .. } | NodeKind::Kwoptarg { .. }
    )
}

/// Returns `true` for `it()` with no receiver inside a block whose parameter
/// list is empty (Ruby 3.4 `it` semantics).
fn is_parenthesized_it_in_empty_block(node: NodeId, cx: &Cx<'_>) -> bool {
    // Only receiverless `it`.
    if cx.call_receiver(node).get().is_some() {
        return false;
    }
    if cx.method_name(node) != Some("it") {
        return false;
    }

    // Walk ancestors looking for an enclosing block.
    for ancestor in cx.ancestors(node) {
        match cx.kind(ancestor) {
            NodeKind::Block { args, .. } => {
                // args node must be an Args node with no children.
                let NodeKind::Args(list) = *cx.kind(*args) else {
                    return false;
                };
                return cx.list(list).is_empty();
            }
            // Stop at scope boundaries.
            NodeKind::Def { .. }
            | NodeKind::Defs { .. }
            | NodeKind::Class { .. }
            | NodeKind::Module { .. } => {
                return false;
            }
            _ => {}
        }
    }
    false
}

/// Returns `true` when the call is receiverless and an ancestor assignment
/// has a left-hand side whose name equals the call's method name. Prevents
/// `foo = foo()` autocorrecting to `foo = foo` (which reads the variable).
fn is_same_name_assignment(node: NodeId, method_name: &str, cx: &Cx<'_>) -> bool {
    // Only applies to receiverless calls (Csend always has a receiver).
    if cx.call_receiver(node).get().is_some() {
        return false;
    }

    for ancestor in cx.ancestors(node) {
        match cx.kind(ancestor) {
            NodeKind::Lvasgn { name, .. } if cx.symbol_str(*name) == method_name => {
                return true;
            }
            NodeKind::Masgn { lhs, .. } if masgn_has_name(*lhs, method_name, cx) => {
                return true;
            }
            NodeKind::OrAsgn { target, .. } | NodeKind::AndAsgn { target, .. } => {
                if let NodeKind::Lvasgn { name, .. } = *cx.kind(*target) {
                    if cx.symbol_str(name) == method_name {
                        return true;
                    }
                }
            }
            NodeKind::OpAsgn { target, .. } => {
                if let NodeKind::Lvasgn { name, .. } = *cx.kind(*target) {
                    if cx.symbol_str(name) == method_name {
                        return true;
                    }
                }
            }
            // Stop at scope-creating boundaries.
            NodeKind::Def { .. }
            | NodeKind::Defs { .. }
            | NodeKind::Class { .. }
            | NodeKind::Module { .. }
            | NodeKind::Block { .. } => break,
            _ => {}
        }
    }
    false
}

/// Walk the `mlhs` node of a `Masgn` and return `true` if any non-send target
/// has the given name.
fn masgn_has_name(lhs: NodeId, name: &str, cx: &Cx<'_>) -> bool {
    let NodeKind::Mlhs(list) = *cx.kind(lhs) else {
        return false;
    };
    cx.list(list).iter().any(|&target| {
        matches!(*cx.kind(target), NodeKind::Lvasgn { name: n, .. } if cx.symbol_str(n) == name)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use murphy_plugin_api::test_support::{indoc, test};

    // ---- detection ----

    #[test]
    fn flags_no_args_with_parens() {
        test::<MethodCallWithoutArgsParentheses>().expect_offense(indoc! {"
            object.some_method()
                              ^^ Do not use parentheses for method calls with no arguments.
        "});
    }

    #[test]
    fn flags_csend_no_args_with_parens() {
        test::<MethodCallWithoutArgsParentheses>().expect_offense(indoc! {"
            object&.some_method()
                               ^^ Do not use parentheses for method calls with no arguments.
        "});
    }

    #[test]
    fn flags_receiverless_call_with_parens() {
        test::<MethodCallWithoutArgsParentheses>().expect_offense(indoc! {"
            some_method()
                       ^^ Do not use parentheses for method calls with no arguments.
        "});
    }

    // ---- no offense ----

    #[test]
    fn accepts_no_args_without_parens() {
        test::<MethodCallWithoutArgsParentheses>().expect_no_offenses("object.some_method\n");
    }

    #[test]
    fn accepts_call_with_args() {
        test::<MethodCallWithoutArgsParentheses>().expect_no_offenses("object.some_method(x)\n");
    }

    #[test]
    fn accepts_camelcase_with_parens() {
        test::<MethodCallWithoutArgsParentheses>().expect_no_offenses("SomeClass()\n");
    }

    #[test]
    fn accepts_default_argument_context() {
        test::<MethodCallWithoutArgsParentheses>().expect_no_offenses(indoc! {"
            def foo(x = bar())
            end
        "});
    }

    #[test]
    fn accepts_same_name_assignment() {
        test::<MethodCallWithoutArgsParentheses>().expect_no_offenses("foo = foo()\n");
    }

    #[test]
    fn accepts_mass_assignment_same_name() {
        test::<MethodCallWithoutArgsParentheses>()
            .expect_no_offenses("foo, bar = foo(), bar()\n");
    }

    #[test]
    fn accepts_allowed_method() {
        test::<MethodCallWithoutArgsParentheses>()
            .with_options(&Options {
                allowed_methods: vec!["some_method".to_string()],
            })
            .expect_no_offenses("object.some_method()\n");
    }

    // ---- autocorrect ----

    #[test]
    fn corrects_no_args_with_parens() {
        test::<MethodCallWithoutArgsParentheses>().expect_correction(
            indoc! {"
                object.some_method()
                                  ^^ Do not use parentheses for method calls with no arguments.
            "},
            "object.some_method\n",
        );
    }

    #[test]
    fn corrects_receiverless_call() {
        test::<MethodCallWithoutArgsParentheses>().expect_correction(
            indoc! {"
                some_method()
                           ^^ Do not use parentheses for method calls with no arguments.
            "},
            "some_method\n",
        );
    }

    #[test]
    fn autocorrect_is_idempotent() {
        test::<MethodCallWithoutArgsParentheses>().expect_no_offenses("object.some_method\n");
    }
}

murphy_plugin_api::submit_cop!(MethodCallWithoutArgsParentheses);
