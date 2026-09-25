//! `RSpec/InstanceSpy` — use `instance_spy` when the double is checked with `have_received`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/InstanceSpy
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_block` (`example?`: block examples, bare
//!   receiver only; `Numblock` excluded per the `NumblockHandler`
//!   exclusion): `null_double` (`lvasgn` of `instance_double(...)` with
//!   bare receiver and any args, wrapped in a bare `.as_null_object`
//!   with no args) paired with `have_received_usage` on the same
//!   variable (`expect(lvar).to have_received(...)`, bare `expect` with
//!   exactly that lvar, bare `.to` with exactly the `have_received`
//!   matcher). Arity is exact everywhere except the two `...` spots
//!   (`instance_double` args, `have_received` args): `not_to`,
//!   extra `expect` args, and explicit receivers never match (verified
//!   vs 3.7.0). The offense range is the `instance_double` send node per
//!   `add_offense(receiver)`. Detection is at parity; autocorrect
//!   (rewrite to `instance_spy` and drop `.as_null_object`) is not ported
//!   in this batch — same convention as `RSpec/IncludeExamples`
//!   (status: partial, autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` whose call is a bare example:
//!
//! - `it { foo = instance_double(F).as_null_object;
//!   expect(foo).to have_received(:bar) }` — flagged.
//! - `it { foo = instance_double(F);
//!   expect(foo).to have_received(:bar) }` — no `.as_null_object`,
//!   clean.
//! - `it { foo = instance_double(F).as_null_object;
//!   expect(bar).to have_received(:baz) }` — different variable, clean.
//! - `it { foo = instance_double(F).as_null_object;
//!   expect(foo).not_to have_received(:bar) }` — negated, clean.
//!
//! ## No autocorrect
//!
//! Upstream rewrites to `instance_spy(F)` and drops `.as_null_object`.
//! This batch reports only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct InstanceSpy;

#[cop(
    name = "RSpec/InstanceSpy",
    description = "Checks for `instance_double` used with `have_received`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl InstanceSpy {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, .. } = *cx.kind(node) else {
            return;
        };
        if !is_bare_example_call(cx, call) {
            return;
        }
        // Upstream `null_double` / `have_received_usage` search the whole
        // example subtree.
        let searchable: Vec<NodeId> =
            core::iter::once(node).chain(cx.descendants(node)).collect();
        for id in &searchable {
            let Some((var, double_send)) = null_double(cx, *id) else {
                continue;
            };
            if searchable.iter().any(|&cand| {
                have_received_usage(cx, cand).is_some_and(|used| used == var)
            }) {
                cx.emit_offense(
                    cx.range(double_send),
                    "Use `instance_spy` when you check your double with `have_received`.",
                    None,
                );
            }
        }
    }
}

/// `(lvasgn var (send (send nil :instance_double ...) :as_null_object))`.
///
/// Returns the variable name and the inner `instance_double` send.
/// Arity mirrors upstream exactly: `as_null_object` takes no args;
/// `instance_double` args are unconstrained.
fn null_double(cx: &Cx<'_>, node: NodeId) -> Option<(String, NodeId)> {
    let NodeKind::Lvasgn { name, value } = *cx.kind(node) else {
        return None;
    };
    let value_id = value.get()?;
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(value_id)
    else {
        return None;
    };
    if cx.symbol_str(method) != "as_null_object" || !cx.list(args).is_empty() {
        return None;
    }
    let recv_id = receiver.get()?;
    let NodeKind::Send {
        receiver: inner_recv,
        method: inner_method,
        ..
    } = *cx.kind(recv_id)
    else {
        return None;
    };
    if inner_recv != OptNodeId::NONE || cx.symbol_str(inner_method) != "instance_double" {
        return None;
    }
    Some((cx.symbol_str(name).to_owned(), recv_id))
}

/// `(send (send nil :expect (lvar var)) :to (send nil :have_received ...))`.
///
/// Returns the expected variable name. Arity mirrors upstream exactly:
/// `expect` takes exactly the lvar, `.to` takes exactly the matcher.
fn have_received_usage(cx: &Cx<'_>, node: NodeId) -> Option<String> {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(node)
    else {
        return None;
    };
    if cx.symbol_str(method) != "to" {
        return None;
    }
    let args = cx.list(args);
    if args.len() != 1 {
        return None;
    }
    if !matches!(*cx.kind(args[0]), NodeKind::Send { .. }) {
        return None;
    }
    let NodeKind::Send {
        receiver: matcher_recv,
        method: matcher_method,
        ..
    } = *cx.kind(args[0])
    else {
        return None;
    };
    if matcher_recv != OptNodeId::NONE || cx.symbol_str(matcher_method) != "have_received" {
        return None;
    }
    let recv_id = receiver.get()?;
    let NodeKind::Send {
        receiver: expect_recv,
        method: expect_method,
        args: expect_args,
    } = *cx.kind(recv_id)
    else {
        return None;
    };
    if expect_recv != OptNodeId::NONE || cx.symbol_str(expect_method) != "expect" {
        return None;
    }
    let expect_args = cx.list(expect_args);
    if expect_args.len() != 1 {
        return None;
    }
    let NodeKind::Lvar(sym) = *cx.kind(expect_args[0]) else {
        return None;
    };
    Some(cx.symbol_str(sym).to_owned())
}

/// `true` when `call` is a bare example call (`Examples.all`: regular +
/// focused + skipped + pending, including `its`).
fn is_bare_example_call(cx: &Cx<'_>, call: NodeId) -> bool {
    let NodeKind::Send { receiver, method, .. } = *cx.kind(call) else {
        return false;
    };
    if receiver != OptNodeId::NONE {
        return false;
    }
    matches!(
        cx.symbol_str(method),
        "it" | "specify"
            | "example"
            | "scenario"
            | "its"
            | "fit"
            | "fspecify"
            | "fexample"
            | "fscenario"
            | "focus"
            | "xit"
            | "xspecify"
            | "xexample"
            | "xscenario"
            | "skip"
            | "pending"
    )
}

#[cfg(test)]
mod tests {
    use super::InstanceSpy;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_null_double_with_have_received() {
        test::<InstanceSpy>().expect_offense(indoc! {r#"
                it do
                  foo = instance_double(Foo).as_null_object
                        ^^^^^^^^^^^^^^^^^^^^ Use `instance_spy` when you check your double with `have_received`.
                  expect(foo).to have_received(:bar)
                end
            "#});
    }

    #[test]
    fn does_not_flag_without_null_object() {
        test::<InstanceSpy>().expect_no_offenses(indoc! {r#"
                it do
                  foo = instance_double(Foo)
                  expect(foo).to have_received(:bar)
                end
            "#});
    }

    #[test]
    fn does_not_flag_different_variable() {
        test::<InstanceSpy>().expect_no_offenses(indoc! {r#"
                it do
                  foo = instance_double(Foo).as_null_object
                  expect(bar).to have_received(:baz)
                end
            "#});
    }

    #[test]
    fn does_not_flag_without_have_received() {
        test::<InstanceSpy>().expect_no_offenses(indoc! {r#"
                it do
                  foo = instance_double(Foo).as_null_object
                  expect(foo).to eq(bar)
                end
            "#});
    }

    #[test]
    fn does_not_flag_negated_have_received() {
        // Upstream only matches `.to` (verified vs 3.7.0).
        test::<InstanceSpy>().expect_no_offenses(indoc! {r#"
                it do
                  foo = instance_double(Foo).as_null_object
                  expect(foo).not_to have_received(:bar)
                end
            "#});
    }

    #[test]
    fn does_not_flag_expect_with_extra_arg() {
        // Upstream `expect` takes exactly the lvar (verified vs 3.7.0).
        test::<InstanceSpy>().expect_no_offenses(indoc! {r#"
                it do
                  foo = instance_double(Foo).as_null_object
                  expect(foo, "msg").to have_received(:bar)
                end
            "#});
    }

    #[test]
    fn does_not_flag_explicit_receiver_double() {
        // Upstream `instance_double` requires a bare receiver.
        test::<InstanceSpy>().expect_no_offenses(indoc! {r#"
                it do
                  foo = obj.instance_double(Foo).as_null_object
                  expect(foo).to have_received(:bar)
                end
            "#});
    }

    #[test]
    fn does_not_flag_outside_example() {
        test::<InstanceSpy>().expect_no_offenses(indoc! {r#"
                describe "x" do
                  foo = instance_double(Foo).as_null_object
                  expect(foo).to have_received(:bar)
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(InstanceSpy);
