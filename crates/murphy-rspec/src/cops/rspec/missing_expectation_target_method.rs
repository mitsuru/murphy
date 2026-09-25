//! `RSpec/MissingExpectationTargetMethod` — expectations need `.to` / `.not_to`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/MissingExpectationTargetMethod
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_send` (`RESTRICT_ON_SEND`: `expect` /
//!   `is_expected`): the trigger send's parent block (for the
//!   `expect { ... }` shape per `node = node.parent if
//!   node.parent&.block_type?`) must satisfy
//!   `expectation_without_runner?` (`(send {#expect? #expect_block?}
//!   !#Runners.all ...)` — receiver is a bare `expect` (any args) or a
//!   bare `is_expected` (no args), or an arg-less `expect { ... }`
//!   block per `expect_block?`, and the outer method is not a runner
//!   (`to` / `to_not` / `not_to`)). Dispatch here is on every `Send`
//!   instead (the offense sits on the outer call), gating on the same
//!   receiver shape; the offense is the outer selector per
//!   `add_offense(node.parent.loc.selector)` with `Use `.to`, `.not_to`
//!   or `.to_not` to set an expectation.` No autocorrect upstream, none
//!   here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` whose method is not a runner and whose receiver
//! is an expectation target:
//!
//! - `expect(something).kind_of? Foo` — flagged (the `kind_of?`
//!   selector).
//! - `is_expected == 42` — flagged (the `==` selector).
//! - `expect { something }.eq? BarError` — block target, flagged.
//! - `expect(x).foo.bar` — only the first non-runner (`foo`) flagged.
//! - `expect(something).to be_a Foo` — runner, clean.
//! - `is_expected.to eq 42` — runner, clean.
//! - `expect { something }.to raise_error BarError` — runner, clean.
//! - `obj.expect(x).foo` — non-bare `expect`, clean.
//! - `is_expected(x).foo` — `is_expected` takes no args, clean.
//! - `expect { |x| y }.foo` — block with params, clean.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; picking the matcher needs human
//! judgement about what is asserted.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct MissingExpectationTargetMethod;

#[cop(
    name = "RSpec/MissingExpectationTargetMethod",
    description = "Checks if `.to`, `not_to` or `to_not` are used.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl MissingExpectationTargetMethod {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, method, .. } = *cx.kind(node) else {
            return;
        };
        // Runners satisfy the cop on their own.
        if matches!(cx.symbol_str(method), "to" | "to_not" | "not_to") {
            return;
        }
        let Some(recv) = receiver.get() else {
            return;
        };
        if !is_expectation_target(cx, recv) {
            return;
        }
        cx.emit_offense(
            cx.node(node).loc.name,
            "Use `.to`, `.not_to` or `.to_not` to set an expectation.",
            None,
        );
    }
}

/// `true` when `id` is an expectation target: a bare `expect` / arg-less
/// `is_expected` send, or an arg-less `expect { ... }` block.
///
/// Mirrors upstream `expect?` (`(send nil? :expect ...)` /
/// `(send nil? :is_expected)`) plus `expect_block?` (`(block #expect?
/// (args) _body)`).
fn is_expectation_target(cx: &Cx<'_>, id: NodeId) -> bool {
    match *cx.kind(id) {
        NodeKind::Send { .. } => is_expect_send(cx, id),
        NodeKind::Block { call, args, .. } => {
            is_expect_send(cx, call) && block_args_empty(cx, args)
        }
        _ => false,
    }
}

/// `true` when `call` is a bare `expect` (any args) or a bare,
/// arg-less `is_expected`.
fn is_expect_send(cx: &Cx<'_>, call: NodeId) -> bool {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(call)
    else {
        return false;
    };
    if receiver != OptNodeId::NONE {
        return false;
    }
    match cx.symbol_str(method) {
        "expect" => true,
        "is_expected" => cx.list(args).is_empty(),
        _ => false,
    }
}

/// `true` when the block parameter list holds no parameters
/// (`expect { ... }`, not `expect { |x| ... }`).
fn block_args_empty(cx: &Cx<'_>, args_id: NodeId) -> bool {
    let NodeKind::Args(params) = *cx.kind(args_id) else {
        return false;
    };
    cx.list(params).is_empty()
}

#[cfg(test)]
mod tests {
    use super::MissingExpectationTargetMethod;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_non_runner_on_expect() {
        test::<MissingExpectationTargetMethod>().expect_offense(indoc! {r#"
                expect(something).kind_of? Foo
                                  ^^^^^^^^ Use `.to`, `.not_to` or `.to_not` to set an expectation.
            "#});
    }

    #[test]
    fn flags_operator_on_is_expected() {
        test::<MissingExpectationTargetMethod>().expect_offense(indoc! {r#"
                is_expected == 42
                            ^^ Use `.to`, `.not_to` or `.to_not` to set an expectation.
            "#});
    }

    #[test]
    fn flags_non_runner_on_expect_block() {
        test::<MissingExpectationTargetMethod>().expect_offense(indoc! {r#"
                expect { something }.eq? BarError
                                     ^^^ Use `.to`, `.not_to` or `.to_not` to set an expectation.
            "#});
    }

    #[test]
    fn flags_only_first_non_runner_in_chain() {
        // `.bar`'s receiver is `.foo`, not an expectation target.
        test::<MissingExpectationTargetMethod>().expect_offense(indoc! {r#"
                expect(x).foo.bar
                          ^^^ Use `.to`, `.not_to` or `.to_not` to set an expectation.
            "#});
    }

    #[test]
    fn ignores_to_runner() {
        test::<MissingExpectationTargetMethod>().expect_no_offenses(indoc! {r#"
                expect(something).to be_a Foo
            "#});
    }

    #[test]
    fn ignores_is_expected_to_runner() {
        test::<MissingExpectationTargetMethod>().expect_no_offenses(indoc! {r#"
                is_expected.to eq 42
            "#});
    }

    #[test]
    fn ignores_to_runner_on_expect_block() {
        test::<MissingExpectationTargetMethod>().expect_no_offenses(indoc! {r#"
                expect { something }.to raise_error BarError
            "#});
    }

    #[test]
    fn ignores_not_to_runner() {
        test::<MissingExpectationTargetMethod>().expect_no_offenses(indoc! {r#"
                expect(x).not_to receive(:foo)
            "#});
    }

    #[test]
    fn ignores_expect_with_receiver() {
        // `expect?` requires a bare receiver (verified vs 3.7.0).
        test::<MissingExpectationTargetMethod>().expect_no_offenses(indoc! {r#"
                obj.expect(x).foo
            "#});
    }

    #[test]
    fn ignores_is_expected_with_args() {
        // `(send nil? :is_expected)` takes no arguments.
        test::<MissingExpectationTargetMethod>().expect_no_offenses(indoc! {r#"
                is_expected(x).foo
            "#});
    }

    #[test]
    fn ignores_expect_block_with_params() {
        // `expect_block?` requires `(args)` — empty params.
        test::<MissingExpectationTargetMethod>().expect_no_offenses(indoc! {r#"
                expect { |x| y }.foo
            "#});
    }
}

murphy_plugin_api::submit_cop!(MissingExpectationTargetMethod);
