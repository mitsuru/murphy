//! `Style/ArrayCoercion` — prefer `Array()` over `[*var]` or explicit Array check.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ArrayCoercion
//! upstream_version_checked: 1.86.2
//! version_added: "0.88"
//! safe: false
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Disabled by default (matches RuboCop's Enabled: false) because the cop
//!   is unsafe: Array(nil) => [], Array({a: 'b'}) => [[:a, 'b']], etc.
//!   Both shapes are autocorrected.
//!
//!   Shape 1 ([*var] -> Array(var)):
//!     Fires on square-bracket array literals with exactly one element that is
//!     a splat. Captures whatever the splat wraps as the argument source.
//!     RuboCop's node.square_brackets? guard is mirrored via cx.is_square_brackets.
//!
//!   Shape 2 (unless var.is_a?(Array); var = [var]; end -> var = Array(var)):
//!     Fires on `unless` modifier and block forms where:
//!       - the condition is `lvar.is_a?(Array)`,
//!       - the else_ branch is `lvar = [lvar]`,
//!       - all three variable names are the same local variable.
//!     Only `lvar` receivers are matched (not method calls / send), matching
//!     RuboCop's (lvar $name) pattern. The multiline unless...end block form
//!     uses a vcall send AST node for the condition receiver (not lvar), so it
//!     correctly does NOT fire -- this is expected parity with RuboCop.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad (Shape 1)
//! [*paths].each { |p| do_something(p) }
//!
//! # bad (Shape 2)
//! paths = [paths] unless paths.is_a?(Array)
//!
//! # good
//! Array(paths).each { |p| do_something(p) }
//! paths = Array(paths)
//! ```
//!
//! ## Autocorrect
//!
//! Shape 1: replaces the whole array literal `[*var]` with `Array(var_source)`.
//! Shape 2: replaces the whole `unless` expression with `var = Array(var)`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, Symbol, cop};

#[derive(Default)]
pub struct ArrayCoercion;

#[cop(
    name = "Style/ArrayCoercion",
    description = "Use Array() instead of explicit Array check or [*var].",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl ArrayCoercion {
    #[on_node(kind = "array")]
    fn check_array(&self, node: NodeId, cx: &Cx<'_>) {
        check_array_splat(node, cx);
    }

    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        check_unless_array(node, cx);
    }
}

/// Shape 1: `[*arg]` -> `Array(arg)`.
fn check_array_splat(node: NodeId, cx: &Cx<'_>) {
    // Must be a square-bracket array (not %w / %i / etc.)
    if !cx.is_square_brackets(node) {
        return;
    }

    let elements = cx.array_elements(node);
    // Must have exactly one element.
    if elements.len() != 1 {
        return;
    }

    let elem = elements[0];

    // The single element must be a Splat node.
    let NodeKind::Splat(inner_opt) = *cx.kind(elem) else {
        return;
    };

    // The splat must wrap a value (not a bare `*`).
    let Some(inner) = inner_opt.get() else {
        return;
    };

    let arg_src = cx.raw_source(cx.range(inner));
    let message = format!("Use `Array({arg_src})` instead of `[*{arg_src}]`.");
    cx.emit_offense(cx.range(node), &message, None);
    cx.emit_edit(cx.range(node), &format!("Array({arg_src})"));
}

/// Shape 2: `paths = [paths] unless paths.is_a?(Array)` -> `paths = Array(paths)`.
fn check_unless_array(node: NodeId, cx: &Cx<'_>) {
    // Must be an `unless` form.
    if !cx.is_unless(node) {
        return;
    }

    let NodeKind::If { cond, then_, else_ } = *cx.kind(node) else {
        return;
    };

    // `unless` in the modifier/block form: then_ is absent (the "true" branch
    // is nil), else_ holds the body.
    if then_ != OptNodeId::NONE {
        return;
    }
    let Some(else_node) = else_.get() else {
        return;
    };

    // Condition must be `(send (lvar var_a) :is_a? (const nil? :Array))`.
    let Some(var_a) = match_is_a_array_cond(cond, cx) else {
        return;
    };

    // Else branch must be `(lvasgn var_b (array (lvar var_c)))`.
    let Some((var_b, var_c)) = match_lvasgn_array_lvar(else_node, cx) else {
        return;
    };

    // All three variable names must be the same.
    let name_a = cx.symbol_str(var_a);
    let name_b = cx.symbol_str(var_b);
    let name_c = cx.symbol_str(var_c);
    if name_a != name_b || name_b != name_c {
        return;
    }

    let message = format!("Use `Array({name_a})` instead of explicit `Array` check.");
    cx.emit_offense(cx.range(node), &message, None);
    cx.emit_edit(cx.range(node), &format!("{name_a} = Array({name_a})"));
}

/// Matches `(send (lvar var_a) :is_a? (const nil? :Array))` and returns the
/// captured symbol for `var_a`, or `None` if the shape doesn't match.
fn match_is_a_array_cond(cond: NodeId, cx: &Cx<'_>) -> Option<Symbol> {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(cond)
    else {
        return None;
    };

    // Method must be `is_a?`.
    if cx.symbol_str(method) != "is_a?" {
        return None;
    }

    // Receiver must be a local variable.
    let recv_id = receiver.get()?;
    let NodeKind::Lvar(var_a) = *cx.kind(recv_id) else {
        return None;
    };

    // Must have exactly one argument.
    let arg_list = cx.list(args);
    if arg_list.len() != 1 {
        return None;
    }

    // The argument must be `(const nil? :Array)`.
    let arg = arg_list[0];
    let NodeKind::Const { scope, name } = *cx.kind(arg) else {
        return None;
    };
    // scope must be absent (nil?) -- top-level `Array`.
    if scope != OptNodeId::NONE {
        return None;
    }
    if cx.symbol_str(name) != "Array" {
        return None;
    }

    Some(var_a)
}

/// Matches `(lvasgn var_b (array (lvar var_c)))` and returns `(var_b, var_c)`,
/// or `None` if the shape doesn't match.
fn match_lvasgn_array_lvar(node: NodeId, cx: &Cx<'_>) -> Option<(Symbol, Symbol)> {
    let NodeKind::Lvasgn { name: var_b, value } = *cx.kind(node) else {
        return None;
    };

    let value_id = value.get()?;

    // Value must be a square-bracket array with exactly one element.
    if !cx.is_square_brackets(value_id) {
        return None;
    }
    let elements = cx.array_elements(value_id);
    if elements.len() != 1 {
        return None;
    }

    // The single element must be an `lvar`.
    let NodeKind::Lvar(var_c) = *cx.kind(elements[0]) else {
        return None;
    };

    Some((var_b, var_c))
}

murphy_plugin_api::submit_cop!(ArrayCoercion);

#[cfg(test)]
mod tests {
    use super::ArrayCoercion;
    use murphy_plugin_api::test_support::{indoc, test};

    // ── Shape 1: [*var] ────────────────────────────────────────────────────

    #[test]
    fn flags_and_corrects_splat_array() {
        test::<ArrayCoercion>().expect_correction(
            indoc! {r#"
                [*paths]
                ^^^^^^^^ Use `Array(paths)` instead of `[*paths]`.
            "#},
            "Array(paths)\n",
        );
    }

    #[test]
    fn flags_splat_array_in_method_chain() {
        test::<ArrayCoercion>().expect_correction(
            indoc! {r#"
                [*paths].each { |p| p }
                ^^^^^^^^ Use `Array(paths)` instead of `[*paths]`.
            "#},
            "Array(paths).each { |p| p }\n",
        );
    }

    #[test]
    fn accepts_array_without_splat() {
        test::<ArrayCoercion>().expect_no_offenses("[paths]\n");
    }

    #[test]
    fn accepts_empty_array() {
        test::<ArrayCoercion>().expect_no_offenses("[]\n");
    }

    #[test]
    fn accepts_multi_element_array_with_splat() {
        // More than one element: not the single-splat shape.
        test::<ArrayCoercion>().expect_no_offenses("[*paths, other]\n");
    }

    #[test]
    fn accepts_percent_literal_array() {
        test::<ArrayCoercion>().expect_no_offenses("%w[foo bar]\n");
    }

    // ── Shape 2: unless var.is_a?(Array); var = [var]; end ─────────────────

    #[test]
    fn flags_and_corrects_unless_is_a_array_modifier() {
        // Modifier form -- condition receiver is lvar, fires.
        test::<ArrayCoercion>().expect_correction(
            indoc! {r#"
                paths = [paths] unless paths.is_a?(Array)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Array(paths)` instead of explicit `Array` check.
            "#},
            "paths = Array(paths)\n",
        );
    }

    #[test]
    fn accepts_mismatched_variable_names() {
        // var_a != var_b: `paths = [other] unless paths.is_a?(Array)`
        test::<ArrayCoercion>().expect_no_offenses(
            "paths = [other] unless paths.is_a?(Array)\n",
        );
    }

    #[test]
    fn accepts_unless_with_different_class() {
        // Condition checks a class other than Array.
        test::<ArrayCoercion>().expect_no_offenses(
            "paths = [paths] unless paths.is_a?(String)\n",
        );
    }

    #[test]
    fn accepts_if_not_unless() {
        // The condition is an `if`, not `unless` -- should not fire.
        test::<ArrayCoercion>().expect_no_offenses(
            "paths = [paths] if paths.is_a?(Array)\n",
        );
    }

    #[test]
    fn accepts_array_coerce_already() {
        test::<ArrayCoercion>().expect_no_offenses("Array(paths)\n");
    }
}
