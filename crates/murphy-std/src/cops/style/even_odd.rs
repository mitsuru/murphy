//! `Style/EvenOdd` — favor `Integer#even?` and `Integer#odd?` over modulo comparisons.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/EvenOdd
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   All patterns from RuboCop are covered:
//!     - `x % 2 == 0` -> `x.even?`
//!     - `x % 2 != 0` -> `x.odd?`
//!     - `x % 2 == 1` -> `x.odd?`
//!     - `x % 2 != 1` -> `x.even?`
//!   The receiver of `%` may be wrapped in `(begin ...)` (e.g. `(x + y) % 2 == 0`).
//!   Autocorrect: whole-node replacement using interpolation form (AST shuffle).
//!   No configurable options -- matches RuboCop.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! x % 2 == 0   # -> x.even?
//! x % 2 != 0   # -> x.odd?
//! x % 2 == 1   # -> x.odd?
//! x % 2 != 1   # -> x.even?
//!
//! # also bad (parenthesized receiver)
//! (x + y) % 2 == 0   # -> (x + y).even?
//!
//! # good
//! x.even?
//! x.odd?
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct EvenOdd;

const MSG: &str = "Replace with `Integer#%<method>s?`.";

#[cop(
    name = "Style/EvenOdd",
    description = "Favor the use of `Integer#even?` && `Integer#odd?`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl EvenOdd {
    #[on_node(kind = "send", methods = ["==", "!="])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Send { receiver, method, args } = *cx.kind(node) else {
        return;
    };
    let Some(lhs) = receiver.get() else {
        return;
    };
    let op = cx.symbol_str(method);
    let args = cx.list(args);
    if args.len() != 1 {
        return;
    }
    let rhs = args[0];

    // rhs must be `int 0` or `int 1`
    let NodeKind::Int(int_val) = *cx.kind(rhs) else {
        return;
    };
    if int_val != 0 && int_val != 1 {
        return;
    }

    // lhs may be `(x % 2)` directly or wrapped in Begin `((x) % 2)`
    let modulo_node = unwrap_begin(lhs, cx);

    let NodeKind::Send { receiver: mod_receiver, method: mod_method, args: mod_args } =
        *cx.kind(modulo_node)
    else {
        return;
    };
    if cx.symbol_str(mod_method) != "%" {
        return;
    }
    let Some(base_node) = mod_receiver.get() else {
        return;
    };
    let mod_args = cx.list(mod_args);
    if mod_args.len() != 1 {
        return;
    }
    let NodeKind::Int(two) = *cx.kind(mod_args[0]) else {
        return;
    };
    if two != 2 {
        return;
    }

    // Determine replacement method
    let replacement_method = replacement_method(int_val, op);
    let message = MSG.replace("%<method>s", replacement_method);

    cx.emit_offense(cx.range(node), &message, None);

    // Autocorrect: `<base>.even?` or `<base>.odd?`
    // Use raw_source of base_node (the receiver of %)
    let base_src = cx.raw_source(cx.range(base_node));
    let correction = format!("{}.{}?", base_src, replacement_method);
    cx.emit_edit(cx.range(node), &correction);
}

/// If `node` is a `Begin` wrapper with a single child, return that child.
fn unwrap_begin(node: NodeId, cx: &Cx<'_>) -> NodeId {
    if let NodeKind::Begin(children) = cx.kind(node) {
        let children = cx.list(*children);
        if children.len() == 1 {
            return children[0];
        }
    }
    node
}

/// Map (int_val, comparison_op) -> replacement method name.
fn replacement_method(int_val: i64, op: &str) -> &'static str {
    match (int_val, op) {
        (0, "==") => "even",
        (0, "!=") => "odd",
        (1, "==") => "odd",
        (1, "!=") => "even",
        _ => "even",
    }
}

#[cfg(test)]
mod tests {
    use super::EvenOdd;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn accepts_even_predicate() {
        test::<EvenOdd>().expect_no_offenses("x.even?\n");
    }

    #[test]
    fn accepts_odd_predicate() {
        test::<EvenOdd>().expect_no_offenses("x.odd?\n");
    }

    #[test]
    fn accepts_modulo_three() {
        test::<EvenOdd>().expect_no_offenses("x % 3 == 0\n");
    }

    #[test]
    fn accepts_different_int() {
        test::<EvenOdd>().expect_no_offenses("x % 2 == 2\n");
    }

    #[test]
    fn flags_modulo_two_eq_zero() {
        test::<EvenOdd>().expect_offense(indoc! {"
            x % 2 == 0
            ^^^^^^^^^^ Replace with `Integer#even?`.
        "});
    }

    #[test]
    fn flags_modulo_two_ne_zero() {
        test::<EvenOdd>().expect_offense(indoc! {"
            x % 2 != 0
            ^^^^^^^^^^ Replace with `Integer#odd?`.
        "});
    }

    #[test]
    fn flags_modulo_two_eq_one() {
        test::<EvenOdd>().expect_offense(indoc! {"
            x % 2 == 1
            ^^^^^^^^^^ Replace with `Integer#odd?`.
        "});
    }

    #[test]
    fn flags_modulo_two_ne_one() {
        test::<EvenOdd>().expect_offense(indoc! {"
            x % 2 != 1
            ^^^^^^^^^^ Replace with `Integer#even?`.
        "});
    }

    #[test]
    fn corrects_eq_zero_to_even() {
        test::<EvenOdd>().expect_correction(
            indoc! {"
                x % 2 == 0
                ^^^^^^^^^^ Replace with `Integer#even?`.
            "},
            "x.even?\n",
        );
    }

    #[test]
    fn corrects_ne_zero_to_odd() {
        test::<EvenOdd>().expect_correction(
            indoc! {"
                x % 2 != 0
                ^^^^^^^^^^ Replace with `Integer#odd?`.
            "},
            "x.odd?\n",
        );
    }

    #[test]
    fn corrects_eq_one_to_odd() {
        test::<EvenOdd>().expect_correction(
            indoc! {"
                x % 2 == 1
                ^^^^^^^^^^ Replace with `Integer#odd?`.
            "},
            "x.odd?\n",
        );
    }

    #[test]
    fn corrects_ne_one_to_even() {
        test::<EvenOdd>().expect_correction(
            indoc! {"
                x % 2 != 1
                ^^^^^^^^^^ Replace with `Integer#even?`.
            "},
            "x.even?\n",
        );
    }

    #[test]
    fn corrects_with_method_receiver() {
        test::<EvenOdd>().expect_correction(
            indoc! {"
                some_method % 2 == 0
                ^^^^^^^^^^^^^^^^^^^^ Replace with `Integer#even?`.
            "},
            "some_method.even?\n",
        );
    }
}

murphy_plugin_api::submit_cop!(EvenOdd);
