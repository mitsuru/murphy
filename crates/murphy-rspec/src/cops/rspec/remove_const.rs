//! `RSpec/RemoveConst` — do not use `remove_const` in specs.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/RemoveConst
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `remove_const`
//!   (`(send _ {:send | :__send__} (sym :remove_const) _)`) with
//!   `RESTRICT_ON_SEND = [:send, :__send__]`: the method is `send` /
//!   `__send__`, exactly two args, the first a `Sym` `remove_const`.
//!   The offense range is the whole send per `add_offense(node)` with
//!   `Do not use remove_const in specs. Consider using e.g.
//!   `stub_const`.` No autocorrect upstream, none here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with `methods = ["send", "__send__"]`:
//!
//! - `Object.send(:remove_const, :Foo)` — flagged (whole send).
//! - `Foo.__send__(:remove_const, :Bar)` — flagged.
//! - `send(:remove_const, :Foo)` — bare receiver, flagged.
//! - `Object.send(:remove_const)` — one arg, clean.
//! - `Object.send(:remove_const, :A, :B)` — three args, clean.
//! - `Object.send(:foo, :Bar)` — other method, clean.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; stubbing the constant needs human
//! judgement.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct RemoveConst;

#[cop(
    name = "RSpec/RemoveConst",
    description = "Checks that `remove_const` is not used in specs.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl RemoveConst {
    #[on_node(kind = "send", methods = ["send", "__send__"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { args, .. } = *cx.kind(node) else {
            return;
        };
        let arg_ids = cx.list(args);
        if arg_ids.len() != 2 {
            return;
        }
        if !matches!(*cx.kind(arg_ids[0]), NodeKind::Sym(s) if cx.symbol_str(s) == "remove_const") {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            "Do not use remove_const in specs. Consider using e.g. `stub_const`.",
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::RemoveConst;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_send_remove_const() {
        test::<RemoveConst>().expect_offense(indoc! {r#"
                Object.send(:remove_const, :Foo)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not use remove_const in specs. Consider using e.g. `stub_const`.
            "#});
    }

    #[test]
    fn flags_dunder_send() {
        test::<RemoveConst>().expect_offense(indoc! {r#"
                Foo.__send__(:remove_const, :Bar)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not use remove_const in specs. Consider using e.g. `stub_const`.
            "#});
    }

    #[test]
    fn flags_bare_send() {
        test::<RemoveConst>().expect_offense(indoc! {r#"
                send(:remove_const, :Foo)
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Do not use remove_const in specs. Consider using e.g. `stub_const`.
            "#});
    }

    #[test]
    fn does_not_flag_single_arg() {
        test::<RemoveConst>().expect_no_offenses(indoc! {r#"
                Object.send(:remove_const)
            "#});
    }

    #[test]
    fn does_not_flag_three_args() {
        // Upstream pattern arity is exactly two args.
        test::<RemoveConst>().expect_no_offenses(indoc! {r#"
                Object.send(:remove_const, :A, :B)
            "#});
    }

    #[test]
    fn does_not_flag_other_method() {
        test::<RemoveConst>().expect_no_offenses(indoc! {r#"
                Object.send(:foo, :Bar)
            "#});
    }

    #[test]
    fn does_not_flag_plain_send() {
        test::<RemoveConst>().expect_no_offenses(indoc! {r#"
                Object.foo(:remove_const, :Bar)
            "#});
    }
}

murphy_plugin_api::submit_cop!(RemoveConst);
