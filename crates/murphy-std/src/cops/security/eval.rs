//! `Security/Eval` — flag uses of `eval` with a non-string-literal argument.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Security/Eval
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's `def_node_matcher :eval?`:
//!   `(send {nil? (send nil? :binding) (const {cbase nil?} :Kernel)} :eval
//!   $!str ...)`. The cop fires on `eval(x)`, `binding.eval(x)`, and
//!   `Kernel.eval(x)` (and `::Kernel.eval(x)` — Murphy normalises `::Kernel`
//!   to a scope-less `Const`). The first argument must NOT be a string
//!   literal; a `dstr` whose interpolation is fully recursive-literal is also
//!   accepted, matching `code.dstr_type? && code.recursive_literal?`. The
//!   offense highlights the `eval` selector (`loc.name`), matching
//!   `node.loc.selector`. No autocorrect (parity with RuboCop).
//! ```
//!
//! ## Matched shapes
//!
//! - **Implicit receiver**: `eval(something)`
//! - **`binding` receiver**: `binding.eval(something)`
//! - **`Kernel` receiver**: `Kernel.eval(something)` / `::Kernel.eval(something)`
//!
//! ## Accepted (not flagged)
//!
//! - `eval("1 + 1")` — string literal argument
//! - `binding.eval("foo")` — string literal argument
//! - `eval("#{1}")` — interpolation is fully recursive-literal
//! - `obj.eval(something)` — receiver is not `nil`/`binding`/`Kernel`
//!
//! ## Message
//!
//! `` The use of `eval` is a serious security risk. `` (matches RuboCop).

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind};

#[derive(Default)]
pub struct Eval;

#[cop(
    name = "Security/Eval",
    description = "The use of eval represents a serious security risk.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl Eval {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        if cx.method_name(node) != Some("eval") {
            return;
        }
        if !receiver_is_eval_target(node, cx) {
            return;
        }
        let args = cx.call_arguments(node);
        let Some(&code) = args.first() else {
            return;
        };
        // `$!str` — a bare string literal argument is acceptable.
        if matches!(*cx.kind(code), NodeKind::Str(_)) {
            return;
        }
        // `code.dstr_type? && code.recursive_literal?` — an interpolated
        // string whose parts are all literal is also acceptable.
        if matches!(*cx.kind(code), NodeKind::Dstr(_)) && cx.is_recursive_literal(code) {
            return;
        }
        cx.emit_offense(
            cx.node(node).loc.name,
            "The use of `eval` is a serious security risk.",
            None,
        );
    }
}

/// True when the receiver of an `eval` call matches RuboCop's
/// `{nil? (send nil? :binding) (const {cbase nil?} :Kernel)}`:
/// an implicit (`nil`) receiver, a bare `binding` call, or the `Kernel`
/// constant (with or without a `::` prefix).
fn receiver_is_eval_target(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(receiver) = cx.call_receiver(node).get() else {
        // Implicit receiver (`nil?`).
        return true;
    };
    match *cx.kind(receiver) {
        // `(send nil? :binding)` — a bare `binding` call.
        NodeKind::Send { .. } => {
            cx.call_receiver(receiver).get().is_none()
                && cx.method_name(receiver) == Some("binding")
        }
        // `(const {cbase nil?} :Kernel)` — `Kernel` / `::Kernel`.
        // Murphy normalises `::Kernel` to a scope-less `Const`, so
        // `is_global_const` matches both forms.
        NodeKind::Const { .. } => cx.is_global_const(receiver, "Kernel"),
        _ => false,
    }
}

murphy_plugin_api::submit_cop!(Eval);

#[cfg(test)]
mod tests {
    use super::Eval;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_implicit_receiver() {
        test::<Eval>().expect_offense(indoc! {r#"
            eval(something)
            ^^^^ The use of `eval` is a serious security risk.
        "#});
    }

    #[test]
    fn flags_binding_receiver() {
        test::<Eval>().expect_offense(indoc! {r#"
            binding.eval(something)
                    ^^^^ The use of `eval` is a serious security risk.
        "#});
    }

    #[test]
    fn flags_kernel_receiver() {
        test::<Eval>().expect_offense(indoc! {r#"
            Kernel.eval(something)
                   ^^^^ The use of `eval` is a serious security risk.
        "#});
    }

    #[test]
    fn flags_cbase_kernel_receiver() {
        test::<Eval>().expect_offense(indoc! {r#"
            ::Kernel.eval(something)
                     ^^^^ The use of `eval` is a serious security risk.
        "#});
    }

    #[test]
    fn flags_non_literal_interpolation() {
        test::<Eval>().expect_offense(indoc! {r##"
            eval("#{something}")
            ^^^^ The use of `eval` is a serious security risk.
        "##});
    }

    #[test]
    fn accepts_string_literal() {
        test::<Eval>().expect_no_offenses("eval(\"1 + 1\")\n");
    }

    #[test]
    fn accepts_binding_string_literal() {
        test::<Eval>().expect_no_offenses("binding.eval(\"foo\")\n");
    }

    #[test]
    fn accepts_recursive_literal_dstr() {
        // `#{1}` → dstr whose only part is an int literal → recursive_literal.
        test::<Eval>().expect_no_offenses("eval(\"#{1}\")\n");
    }

    #[test]
    fn flags_arithmetic_interpolation() {
        // `+` is not a `recursive_literal?` operator (only `== === != <= >=
        // > < * ! <=>`), so `#{1 + 1}` is NOT recursive-literal → flagged.
        // Pins the surprising RuboCop boundary.
        test::<Eval>().expect_offense(indoc! {r##"
            eval("#{1 + 1}")
            ^^^^ The use of `eval` is a serious security risk.
        "##});
    }

    #[test]
    fn accepts_other_receiver() {
        test::<Eval>().expect_no_offenses("obj.eval(something)\n");
    }

    #[test]
    fn accepts_non_eval_method() {
        test::<Eval>().expect_no_offenses("instance_eval { foo }\n");
    }
}
