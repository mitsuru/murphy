//! `RSpec/EmptyOutput` — avoid matching on an empty output string.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/EmptyOutput
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `matching_empty_output`
//!   (`(send (block (send nil? :expect) ...) #Runners.all
//!   (send $(send nil? :output (str empty?)) ...))`) with
//!   `RESTRICT_ON_SEND = Runners.all` (`to` / `to_not` / `not_to`).
//!   The receiver must be an `expect { ... }` block (bare `expect` with no
//!   args); `expect(foo)` sends do not match. Each `to` / `not_to` arg that
//!   is a `Send` wrapping a bare `output('')` (single empty-string arg)
//!   flags the inner `output('')` per `add_offense(node)` with
//!   `Use `%<runner>s` instead of matching on an empty output`
//!   (`not_to` for `to`, `to` otherwise). Detection is at parity;
//!   autocorrect (flip the runner + replace with bare `output`) is not
//!   ported in this batch — same convention as `RSpec/Eq`
//!   (status: partial, autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with `methods = ["to", "to_not", "not_to"]`.
//!
//! - `expect { foo }.to output('').to_stdout` — flagged (range is
//!   `output('')`).
//! - `expect { foo }.not_to output('').to_stderr` — flagged (`to`
//!   message).
//! - `expect { foo }.to output('')` — bare matcher without chaining,
//!   not flagged per upstream.
//! - `expect { foo }.to output('hi').to_stdout` — non-empty, not flagged.
//! - `expect { foo }.to output.to_stdout` — no arg, not flagged.
//! - `expect(foo).to output('').to_stdout` — `expect` with arg (no block),
//!   not flagged.
//!
//! ## No autocorrect
//!
//! Upstream flips the runner and replaces `output('')` with bare `output`.
//! This batch reports only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct EmptyOutput;

#[cop(
    name = "RSpec/EmptyOutput",
    description = "Check that the output matcher is not called with an empty string.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl EmptyOutput {
    #[on_node(kind = "send", methods = ["to", "to_not", "not_to"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send {
            receiver, method, args,
        } = *cx.kind(node)
        else {
            return;
        };
        let Some(block_id) = receiver.get() else {
            return;
        };
        if !is_expect_block(cx, block_id) {
            return;
        }
        let runner = if cx.symbol_str(method) == "to" {
            "not_to"
        } else {
            "to"
        };
        for arg in cx.list(args).iter().copied() {
            let NodeKind::Send {
                receiver: outer_recv,
                ..
            } = *cx.kind(arg)
            else {
                continue;
            };
            let Some(inner_id) = outer_recv.get() else {
                continue;
            };
            if !is_empty_output(cx, inner_id) {
                continue;
            }
            cx.emit_offense(
                cx.range(inner_id),
                &format!("Use `{runner}` instead of matching on an empty output."),
                None,
            );
        }
    }
}

/// `true` when `id` is an `expect { ... }` block: call is bare `expect`
/// with no args.
fn is_expect_block(cx: &Cx<'_>, id: NodeId) -> bool {
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

/// `true` when `id` is bare `output('')`: nil receiver, single empty-string
/// arg.
fn is_empty_output(cx: &Cx<'_>, id: NodeId) -> bool {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(id)
    else {
        return false;
    };
    if receiver != OptNodeId::NONE {
        return false;
    }
    if cx.symbol_str(method) != "output" {
        return false;
    }
    let arg_ids = cx.list(args);
    if arg_ids.len() != 1 {
        return false;
    }
    let NodeKind::Str(sid) = *cx.kind(arg_ids[0]) else {
        return false;
    };
    cx.string_str(sid).is_empty()
}

#[cfg(test)]
mod tests {
    use super::EmptyOutput;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_to_output_empty_to_stdout() {
        test::<EmptyOutput>().expect_offense(indoc! {r#"
                expect { foo }.to output('').to_stdout
                                  ^^^^^^^^^^ Use `not_to` instead of matching on an empty output.
            "#});
    }

    #[test]
    fn flags_not_to_output_empty() {
        test::<EmptyOutput>().expect_offense(indoc! {r#"
                expect { foo }.not_to output('').to_stderr
                                      ^^^^^^^^^^ Use `to` instead of matching on an empty output.
            "#});
    }

    #[test]
    fn flags_to_not_output_empty() {
        test::<EmptyOutput>().expect_offense(indoc! {r#"
                expect { foo }.to_not output('').to_stdout
                                      ^^^^^^^^^^ Use `to` instead of matching on an empty output.
            "#});
    }

    #[test]
    fn does_not_flag_bare_output_without_chain() {
        test::<EmptyOutput>().expect_no_offenses(indoc! {r#"
                expect { foo }.to output('')
            "#});
    }

    #[test]
    fn does_not_flag_non_empty_string() {
        test::<EmptyOutput>().expect_no_offenses(indoc! {r#"
                expect { foo }.to output('hi').to_stdout
            "#});
    }

    #[test]
    fn does_not_flag_bare_output() {
        test::<EmptyOutput>().expect_no_offenses(indoc! {r#"
                expect { foo }.to output.to_stdout
            "#});
    }

    #[test]
    fn does_not_flag_expect_with_arg() {
        test::<EmptyOutput>().expect_no_offenses(indoc! {r#"
                expect(foo).to output('').to_stdout
            "#});
    }
}

murphy_plugin_api::submit_cop!(EmptyOutput);
