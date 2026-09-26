//! `Lint/UselessNumericOperation` — Checks for useless numeric operations
//! on local variables such as `x + 0`, `x * 1`, `x += 0`, etc.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/UselessNumericOperation
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   All RuboCop parity items verified: simple calls, dot-calls, safe-navigation,
//!   abbreviated assignments, and chained calls.
//! ```
//!
//! ## Matched shapes
//!
//! - `x + 0`, `x - 0`, `x * 1`, `x / 1`, `x ** 1` — operator-style calls.
//! - `x.+(0)`, `x.-(0)`, etc. — explicit dot-call form.
//! - `x&.+(0)`, etc. — safe-navigation form.
//! - `x += 0`, `x -= 0`, `x *= 1`, `x /= 1`, `x **= 1` — abbreviated assignments.
//!
//! ## Autocorrect
//!
//! - `x + 0` → `x`
//! - `x += 0` → `x = x`
//!
//! Verbatim port of the call head `(call _ {:+ :- :* :/ :**} ...)`
//! (murphy-s1yc.36): `call` = `{send csend}` covers safe-navigation
//! (`x&.+(0)`), mirroring RuboCop `alias on_csend on_send`;
//! the wildcard receiver binds an absent or present receiver per murphy-if9y;
//! trailing `...` absorbs any argument list. The variable-receiver and
//! useless-value guards below apply separately (upstream `on_send` returns
//! early unless the receiver/arg shape matches). `op_asgn` (`x += 0`)
//! stays a separate handler.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop, def_node_matcher};

// Verbatim port of the call head (murphy-s1yc.36):
// `(call _ {:+ :- :* :/ :**} ...)` — `call` = `{send csend}`
// covers safe-navigation (`x&.+(0)`), mirroring RuboCop
// `alias on_csend on_send`. The `_` receiver binds an absent or present
// receiver per murphy-if9y; trailing `...` absorbs any argument list, so the
// variable-receiver and useless-value guards below apply separately.
def_node_matcher!(useless_numeric_call, "(call _ {:+ :- :* :/ :**} ...)");

#[derive(Default)]
pub struct UselessNumericOperation;

#[cop(
    name = "Lint/UselessNumericOperation",
    description = "Checks for useless numeric operations on variables.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl UselessNumericOperation {
    /// Send path: `x + 0` etc.
    /// Triggered on all sends; the verbatim
    /// `(call _ {:+ :- :* :/ :**} ...)` head filters to the operator methods.
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    /// Safe-navigation send path: `x&.+(0)`.
    /// Mirrors upstream `alias on_csend on_send`; the verbatim head covers
    /// `csend`.
    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "op_asgn")]
    fn check_op_asgn(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::OpAsgn { target, op, value } = *cx.kind(node) else {
            return;
        };
        let op_str = cx.symbol_str(op);
        if !["+", "-", "*", "/", "**"].contains(&op_str) {
            return;
        }
        if !is_useless_op(op_str, value, cx) {
            return;
        }
        let NodeKind::Lvasgn { name, value: _ } = *cx.kind(target) else {
            return;
        };
        let msg = "Do not apply inconsequential numeric operations to variables.";
        cx.emit_offense(cx.range(node), msg, None);
        let var_name = cx.symbol_str(name);
        let replacement = format!("{var_name} = {var_name}");
        cx.emit_edit(cx.range(node), &replacement);
    }
}

/// Check if `node` is a useless numeric operation on a variable.
fn check(node: NodeId, cx: &Cx<'_>) {
    // Verbatim `(call _ {:+ :- :* :/ :**} ...)` head: filters to the operator
    // methods on either send or csend (safe-navigation), with any receiver
    // (absent or present). Without this, an unrelated call with a variable
    // receiver (e.g. `x.map`) would run check on every call node instead of
    // being rejected by the method set up front.
    if !useless_numeric_call(node, cx) {
        return;
    }
    // The receiver must be a variable or implicit call (lvar or bare send).
    let Some(recv_id) = cx.call_receiver(node).get() else {
        return;
    };
    if !is_variable_or_implicit_call(recv_id, cx) {
        return;
    }
    let args_list = cx.call_arguments(node);
    if args_list.len() != 1 {
        return;
    }
    let Some(method_str) = cx.method_name(node) else {
        return;
    };
    if !is_useless_op(method_str, args_list[0], cx) {
        return;
    }
    let msg = "Do not apply inconsequential numeric operations to variables.";
    cx.emit_offense(cx.range(node), msg, None);
    let replacement = cx.raw_source(cx.range(recv_id));
    cx.emit_edit(cx.range(node), replacement);
}

/// Accept a receiver that is either a local-variable read or an implicit-self
/// method call with no args (`x` → `Send(nil, :x, [])`) — matches RuboCop's
/// `(call nil? $_)` pattern.
fn is_variable_or_implicit_call(node: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Lvar(_) => true,
        NodeKind::Send { receiver, args, .. } if receiver == OptNodeId::NONE => {
            let args_list = cx.list(args);
            args_list.is_empty()
        }
        _ => false,
    }
}

fn is_useless_op(op: &str, arg: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Int(n) = *cx.kind(arg) else {
        return false;
    };
    match op {
        "+" | "-" => n == 0,
        "*" | "/" | "**" => n == 1,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::UselessNumericOperation;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_x_plus_0() {
        test::<UselessNumericOperation>().expect_correction(
            indoc! {r#"
                x + 0
                ^^^^^ Do not apply inconsequential numeric operations to variables.
            "#},
            "x\n",
        );
    }

    #[test]
    fn flags_x_minus_0() {
        test::<UselessNumericOperation>().expect_correction(
            indoc! {r#"
                x - 0
                ^^^^^ Do not apply inconsequential numeric operations to variables.
            "#},
            "x\n",
        );
    }

    #[test]
    fn flags_x_times_1() {
        test::<UselessNumericOperation>().expect_correction(
            indoc! {r#"
                x * 1
                ^^^^^ Do not apply inconsequential numeric operations to variables.
            "#},
            "x\n",
        );
    }

    #[test]
    fn flags_x_divided_by_1() {
        test::<UselessNumericOperation>().expect_correction(
            indoc! {r#"
                x / 1
                ^^^^^ Do not apply inconsequential numeric operations to variables.
            "#},
            "x\n",
        );
    }

    #[test]
    fn flags_x_power_1() {
        test::<UselessNumericOperation>().expect_correction(
            indoc! {r#"
                x ** 1
                ^^^^^^ Do not apply inconsequential numeric operations to variables.
            "#},
            "x\n",
        );
    }

    #[test]
    fn flags_x_plus_0_abbrev() {
        test::<UselessNumericOperation>().expect_correction(
            indoc! {r#"
                x += 0
                ^^^^^^ Do not apply inconsequential numeric operations to variables.
            "#},
            "x = x\n",
        );
    }

    #[test]
    fn flags_x_minus_0_abbrev() {
        test::<UselessNumericOperation>().expect_correction(
            indoc! {r#"
                x -= 0
                ^^^^^^ Do not apply inconsequential numeric operations to variables.
            "#},
            "x = x\n",
        );
    }

    #[test]
    fn flags_x_times_1_abbrev() {
        test::<UselessNumericOperation>().expect_correction(
            indoc! {r#"
                x *= 1
                ^^^^^^ Do not apply inconsequential numeric operations to variables.
            "#},
            "x = x\n",
        );
    }

    #[test]
    fn flags_x_divided_by_1_abbrev() {
        test::<UselessNumericOperation>().expect_correction(
            indoc! {r#"
                x /= 1
                ^^^^^^ Do not apply inconsequential numeric operations to variables.
            "#},
            "x = x\n",
        );
    }

    #[test]
    fn flags_x_power_1_abbrev() {
        test::<UselessNumericOperation>().expect_correction(
            indoc! {r#"
                x **= 1
                ^^^^^^^ Do not apply inconsequential numeric operations to variables.
            "#},
            "x = x\n",
        );
    }

    #[test]
    fn flags_dot_plus_0() {
        test::<UselessNumericOperation>().expect_correction(
            indoc! {r#"
                x.+(0)
                ^^^^^^ Do not apply inconsequential numeric operations to variables.
            "#},
            "x\n",
        );
    }

    #[test]
    fn flags_safe_nav_plus_0() {
        test::<UselessNumericOperation>().expect_correction(
            indoc! {r#"
                x&.+(0)
                ^^^^^^^ Do not apply inconsequential numeric operations to variables.
            "#},
            "x\n",
        );
    }

    #[test]
    fn flags_chained_plus_0() {
        test::<UselessNumericOperation>().expect_correction(
            indoc! {r#"
                x.+(0).bar
                ^^^^^^ Do not apply inconsequential numeric operations to variables.
            "#},
            "x.bar\n",
        );
    }

    #[test]
    fn does_not_flag_non_lvar_receiver() {
        test::<UselessNumericOperation>().expect_no_offenses("foo.bar + 0\n");
    }

    #[test]
    fn does_not_flag_wrong_arg_value() {
        test::<UselessNumericOperation>().expect_no_offenses("x + 1\n");
    }

    #[test]
    fn does_not_flag_wrong_arg_type() {
        test::<UselessNumericOperation>().expect_no_offenses("x + \"\"\n");
    }

    // --- Characterization (murphy-s1yc.36): pin the exact node set the
    // send-only methods=[+ - * / **] dispatch matches, so the verbatim
    // `(call _ {:+ :- :* :/ :**} ...)` port can be proven byte-identical
    // (plus csend coverage per upstream `alias on_csend on_send`). `call`
    // covers safe-navigation; the `_` receiver binds an absent or present
    // receiver per murphy-if9y; trailing `...` absorbs any argument list,
    // so the variable-receiver and useless-value guards below apply
    // separately.

    #[test]
    fn s1yc36_flags_csend_corrects() {
        // Safe-navigation: `call` covers `csend` per murphy-if9y,
        // mirroring upstream `alias on_csend on_send`.
        // Pre-port has a csend handler so this already flags; the verbatim
        // head collapses the workaround and keeps it byte-identical.
        test::<UselessNumericOperation>().expect_correction(
            indoc! {r#"
                x&.+(0)
                ^^^^^^^ Do not apply inconsequential numeric operations to variables.
            "#},
            "x\n",
        );
    }

    #[test]
    fn s1yc36_accepts_bare() {
        // Bare `+(0)` parses as unary `:+@`, not binary `:+`, so the verbatim
        // head (method set `:+`) rejects it up front -- pinned here.
        // Verified vs standalone NodePattern: `(call _ {:+ ...} ...)` does not
        // match unary `:+@`.
        test::<UselessNumericOperation>().expect_no_offenses("+(0)\n");
    }

    #[test]
    fn s1yc36_accepts_with_extra_args() {
        // Extra args: trailing `...` absorbs any argument list, so the head
        // matches and the single-arg guard below rejects -- pinned here.
        test::<UselessNumericOperation>().expect_no_offenses("x.+(0, 1)\n");
    }

    #[test]
    fn s1yc36_accepts_unrelated() {
        // Unrelated `%` is not in the head method set, so the verbatim head
        // rejects it up front -- pinned here.
        test::<UselessNumericOperation>().expect_no_offenses("x % 0\n");
    }
}

murphy_plugin_api::submit_cop!(UselessNumericOperation);
