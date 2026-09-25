//! `RSpec/UnspecifiedException` — specify the exception being captured.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/UnspecifiedException
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_send` (`RESTRICT_ON_SEND = [:raise_error,
//!   :raise_exception]`) with `empty_exception_matcher?` (no args, no
//!   block literal) and `find_expect_to` (nearest ancestor `Send` matching
//!   `expect_to?`: `(send (block (send nil? :expect) ...) :to ...)` — a
//!   bare zero-arg `expect` block running `.to`). The walk stops at the
//!   first `Block` / `Numblock` / `Itblock` ancestor (upstream
//!   `break if ancestor.block_type?`), so `raise_error` calls inside the
//!   `expect { }` body never flag. `expect_to.block_node` with block args
//!   (`expect { }.to raise_error do |e| ... end`) also stays clean.
//!   Chained matchers (`.to raise_error.and change`, `.to change.and
//!   raise_error`) flag via the ancestor `.to`, and ternary args
//!   (`.to (cond ? raise_error : other)`) flag by skipping non-`Send`
//!   ancestors. The offense is the matcher node per `add_offense(node)`
//!   with `Specify the exception being captured`. Detection is at parity
//!   (verified vs 3.7.0, including chains, ternaries, block forms, and
//!   the `not_to` clean case); upstream ships no autocorrect and none is
//!   added here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with `methods = ["raise_error", "raise_exception"]`:
//!
//! - `expect { raise }.to raise_error` — flagged (the matcher).
//! - `expect { raise }.to raise_error(StandardError)` — has args, clean.
//! - `expect { raise }.to raise_error { |e| e }` — block literal, clean.
//! - `expect { raise }.not_to raise_error` — `not_to`, clean.
//! - `expect { raise_error }.to raise_error(StandardError)` — inner call
//!   inside the `expect` block, clean (walk breaks on `Block`).
//! - `expect { foo }.to raise_error.and change { bar }` — chained,
//!   flagged.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; choosing the exception needs human
//! judgement.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct UnspecifiedException;

#[cop(
    name = "RSpec/UnspecifiedException",
    description = "Checks for a specified error in checking raised errors.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl UnspecifiedException {
    #[on_node(kind = "send", methods = ["raise_error", "raise_exception"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // `return false if node.arguments? || node.block_literal?`
        if cx.has_call_arguments(node) {
            return;
        }
        if cx.block_node(node).get().is_some() {
            return;
        }
        let Some(expect_to) = find_expect_to(cx, node) else {
            return;
        };
        // `return false if expect_to.block_node&.arguments?`
        if let Some(block_id) = cx.block_node(expect_to).get() {
            let NodeKind::Block { args, .. } = *cx.kind(block_id) else {
                return;
            };
            let NodeKind::Args(list) = *cx.kind(args) else {
                return;
            };
            if !cx.list(list).is_empty() {
                return;
            }
        }
        cx.emit_offense(
            cx.range(node),
            "Specify the exception being captured",
            None,
        );
    }
}

/// `find_expect_to`: nearest ancestor `Send` matching `expect_to?`,
/// stopping at the first block-like ancestor.
///
/// `expect_to?` is `(send (block (send nil? :expect) ...) :to ...)`:
/// a `.to` send whose receiver is a `Block` whose call is a bare
/// zero-arg `expect`.
fn find_expect_to(cx: &Cx<'_>, node: NodeId) -> Option<NodeId> {
    for anc in cx.ancestors(node) {
        match *cx.kind(anc) {
            NodeKind::Block { .. } | NodeKind::Numblock { .. } | NodeKind::Itblock { .. } => {
                break;
            }
            NodeKind::Send {
                receiver,
                method,
                ..
            } => {
                if cx.symbol_str(method) != "to" {
                    continue;
                }
                let Some(recv) = receiver.get() else {
                    continue;
                };
                if !is_bare_expect_block(cx, recv) {
                    continue;
                }
                return Some(anc);
            }
            _ => continue,
        }
    }
    None
}

/// `true` when `id` is a `Block` whose call is bare `expect` with no args.
fn is_bare_expect_block(cx: &Cx<'_>, id: NodeId) -> bool {
    let NodeKind::Block { call, .. } = *cx.kind(id) else {
        return false;
    };
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
    if cx.symbol_str(method) != "expect" {
        return false;
    }
    cx.list(args).is_empty()
}

#[cfg(test)]
mod tests {
    use super::UnspecifiedException;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_bare_raise_error() {
        test::<UnspecifiedException>().expect_offense(indoc! {r#"
                expect {
                  raise StandardError
                }.to raise_error
                     ^^^^^^^^^^^ Specify the exception being captured
            "#});
    }

    #[test]
    fn flags_bare_raise_exception() {
        test::<UnspecifiedException>().expect_offense(indoc! {r#"
                expect {
                  raise StandardError
                }.to raise_exception
                     ^^^^^^^^^^^^^^^ Specify the exception being captured
            "#});
    }

    #[test]
    fn allows_not_to() {
        test::<UnspecifiedException>().expect_no_offenses(indoc! {r#"
                expect {
                  raise StandardError
                }.not_to raise_error
            "#});
    }

    #[test]
    fn allows_with_class() {
        test::<UnspecifiedException>().expect_no_offenses(indoc! {r#"
                expect {
                  raise StandardError
                }.to raise_error(StandardError)
            "#});
    }

    #[test]
    fn allows_with_message() {
        test::<UnspecifiedException>().expect_no_offenses(indoc! {r#"
                expect {
                  raise StandardError.new('error')
                }.to raise_error('error')
            "#});
    }

    #[test]
    fn allows_with_block_braces() {
        test::<UnspecifiedException>().expect_no_offenses(indoc! {r#"
                expect {
                  raise StandardError.new('error')
                }.to raise_error { |err| err.data }
            "#});
    }

    #[test]
    fn allows_with_block_do_end() {
        test::<UnspecifiedException>().expect_no_offenses(indoc! {r#"
                expect {
                  raise StandardError.new('error')
                }.to raise_error do |error|
                  error.data
                end
            "#});
    }

    #[test]
    fn does_not_flag_inside_expect_body() {
        // Inner `raise_error` (no args) sits inside the `expect { }`
        // block body; the ancestor walk breaks on `Block`.
        test::<UnspecifiedException>().expect_no_offenses(indoc! {r#"
                expect {
                  raise_error
                }.to raise_error(StandardError)
            "#});
    }

    #[test]
    fn flags_chained() {
        test::<UnspecifiedException>().expect_offense(indoc! {r#"
                expect {
                  foo
                }.to raise_error.and change { bar }
                     ^^^^^^^^^^^ Specify the exception being captured
            "#});
    }

    #[test]
    fn does_not_confuse_blocks_with_chains() {
        test::<UnspecifiedException>().expect_no_offenses(indoc! {r#"
                expect do
                  expect { foo }.not_to raise_error
                end.to change(Foo, :count).by(3)
            "#});
    }
}

murphy_plugin_api::submit_cop!(UnspecifiedException);
