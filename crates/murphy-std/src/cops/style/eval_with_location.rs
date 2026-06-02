//! `Style/EvalWithLocation` — pass `__FILE__` and `__LINE__` to `eval` methods.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/EvalWithLocation
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Flags `eval`, `instance_eval`, `class_eval`, and `module_eval` calls that
//!   pass a string literal as the code argument but omit `__FILE__` and/or
//!   `__LINE__`. Only string literals are checked (not string variables), matching
//!   upstream.
//!
//!   Murphy represents `__FILE__` and `__LINE__` as `Unknown` nodes (they have no
//!   `NodeKind` mapping yet). The cop checks for their presence by looking for the
//!   raw source text `"__FILE__"` and `"__LINE__"` among the argument nodes.
//!
//!   For `eval`: expects (code, binding, __FILE__, __LINE__) — 4 arguments.
//!   For `instance_eval`/`class_eval`/`module_eval`: expects (code, __FILE__,
//!   __LINE__) — 3 arguments.
//!
//!   Autocorrect is not implemented (the binding argument for `eval` makes
//!   safe auto-insertion ambiguous).
//!
//!   Covered:
//!     - Missing location args: flags the call
//!     - `__FILE__` and `__LINE__` present in correct positions: no offense
//!   Deferred (gaps):
//!     - Verification that the literal `__LINE__` matches the actual call line
//!       (requires source-line tracking not yet available in the cop ABI).
//!     - Auto-correction.
//!     - Validation that receiver is nil or `Kernel`.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! eval('code')
//! eval('code', binding)
//! instance_eval('code')
//! class_eval('code')
//! module_eval('code')
//!
//! # good
//! eval('code', binding, __FILE__, __LINE__)
//! instance_eval('code', __FILE__, __LINE__)
//! class_eval('code', __FILE__, __LINE__)
//! module_eval('code', __FILE__, __LINE__)
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

const MSG_MISSING: &str = "Pass `__FILE__` and `__LINE__` to `%<method>s`.";
const MSG_MISSING_EVAL: &str = "Pass a binding, `__FILE__`, and `__LINE__` to `eval`.";

/// Stateless unit struct.
#[derive(Default)]
pub struct EvalWithLocation;

#[cop(
    name = "Style/EvalWithLocation",
    description = "Pass `__FILE__` and `__LINE__` to `eval` method, as they are used by backtraces.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl EvalWithLocation {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Send { method, args, .. } = *cx.kind(node) else {
        return;
    };

    let method_str = cx.symbol_str(method);
    let arg_list = cx.list(args);

    match method_str {
        "eval" => check_eval(node, arg_list, cx),
        "instance_eval" | "class_eval" | "module_eval" => {
            check_instance_eval(node, method_str, arg_list, cx);
        }
        _ => {}
    }
}

/// Check `eval(code, binding, __FILE__, __LINE__)`.
/// The code argument (arg 0) must be a string literal.
fn check_eval(node: NodeId, arg_list: &[NodeId], cx: &Cx<'_>) {
    // No args or first arg not a string literal → skip.
    let Some(&code_arg) = arg_list.first() else {
        return;
    };
    if !is_str_literal(code_arg, cx) {
        return;
    }

    // eval needs 4 arguments: code, binding, __FILE__, __LINE__
    if arg_list.len() >= 4 && is_file_keyword(arg_list[2], cx) && is_line_keyword(arg_list[3], cx) {
        // OK — has file and line.
        return;
    }

    cx.emit_offense(cx.range(node), MSG_MISSING_EVAL, None);
}

/// Check `instance_eval/class_eval/module_eval(code, __FILE__, __LINE__)`.
fn check_instance_eval(node: NodeId, method_name: &str, arg_list: &[NodeId], cx: &Cx<'_>) {
    // No args or first arg not a string literal → skip.
    let Some(&code_arg) = arg_list.first() else {
        return;
    };
    if !is_str_literal(code_arg, cx) {
        return;
    }

    // Needs 3 arguments: code, __FILE__, __LINE__
    if arg_list.len() >= 3 && is_file_keyword(arg_list[1], cx) && is_line_keyword(arg_list[2], cx) {
        // OK — has file and line.
        return;
    }

    let msg = MSG_MISSING.replace("%<method>s", method_name);
    cx.emit_offense(cx.range(node), &msg, None);
}

/// Returns `true` if `node` is a string literal (`Str` or `Dstr`).
fn is_str_literal(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(cx.kind(node), NodeKind::Str(_) | NodeKind::Dstr(_))
}

/// Returns `true` if `node` represents `__FILE__` (raw source text).
fn is_file_keyword(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.raw_source(cx.range(node)) == "__FILE__"
}

/// Returns `true` if `node` represents `__LINE__` (raw source text), possibly
/// with an integer offset like `__LINE__ + 1`.
fn is_line_keyword(node: NodeId, cx: &Cx<'_>) -> bool {
    let src = cx.raw_source(cx.range(node));
    src.starts_with("__LINE__")
}

#[cfg(test)]
mod tests {
    use super::EvalWithLocation;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_eval_missing_location() {
        test::<EvalWithLocation>().expect_offense(indoc! {"
            eval('code')
            ^^^^^^^^^^^^ Pass a binding, `__FILE__`, and `__LINE__` to `eval`.
        "});
    }

    #[test]
    fn flags_eval_with_binding_only() {
        test::<EvalWithLocation>().expect_offense(indoc! {"
            eval('code', binding)
            ^^^^^^^^^^^^^^^^^^^^^ Pass a binding, `__FILE__`, and `__LINE__` to `eval`.
        "});
    }

    #[test]
    fn accepts_eval_with_all_args() {
        test::<EvalWithLocation>()
            .expect_no_offenses("eval('code', binding, __FILE__, __LINE__)\n");
    }

    #[test]
    fn flags_instance_eval_missing_location() {
        test::<EvalWithLocation>().expect_offense(indoc! {"
            instance_eval('code')
            ^^^^^^^^^^^^^^^^^^^^^ Pass `__FILE__` and `__LINE__` to `instance_eval`.
        "});
    }

    #[test]
    fn accepts_instance_eval_with_location() {
        test::<EvalWithLocation>()
            .expect_no_offenses("instance_eval('code', __FILE__, __LINE__)\n");
    }

    #[test]
    fn flags_class_eval_missing_location() {
        test::<EvalWithLocation>().expect_offense(indoc! {"
            Foo.class_eval('code')
            ^^^^^^^^^^^^^^^^^^^^^^ Pass `__FILE__` and `__LINE__` to `class_eval`.
        "});
    }

    #[test]
    fn accepts_class_eval_with_location() {
        test::<EvalWithLocation>()
            .expect_no_offenses("Foo.class_eval('code', __FILE__, __LINE__)\n");
    }

    #[test]
    fn flags_module_eval_missing_location() {
        test::<EvalWithLocation>().expect_offense(indoc! {"
            module_eval('code')
            ^^^^^^^^^^^^^^^^^^^ Pass `__FILE__` and `__LINE__` to `module_eval`.
        "});
    }

    #[test]
    fn accepts_module_eval_with_location() {
        test::<EvalWithLocation>().expect_no_offenses("module_eval('code', __FILE__, __LINE__)\n");
    }

    #[test]
    fn accepts_eval_with_non_string_code() {
        // Variable as code arg → not checked.
        test::<EvalWithLocation>().expect_no_offenses("eval(code_str)\n");
    }

    #[test]
    fn accepts_instance_eval_with_non_string_code() {
        test::<EvalWithLocation>().expect_no_offenses("obj.instance_eval(&block)\n");
    }
}

murphy_plugin_api::submit_cop!(EvalWithLocation);
