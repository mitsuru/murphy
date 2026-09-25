//! `RSpec/ReceiveCounts` — prefer `once`/`twice` over `exactly(1).times` etc.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/ReceiveCounts
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `receive_counts`
//!   (`(send $(send _ {:exactly :at_least :at_most} (int {1 2})) :times)`)
//!   with `RESTRICT_ON_SEND = [:times]`, gated by `stub?`
//!   (`(send nil? :receive ...)` searched in the offending node's receiver
//!   chain). The offense range is the dot-to-end span per
//!   `offending_node.loc.dot.with(end_pos: node.source_range.end_pos)`
//!   (`.exactly(1).times` etc) with
//!   `Use `%<alternative>s` instead of `%<original>s`.`
//!   (`matcher_for`: `exactly` → `.once` / `.twice`,
//!   `at_least` / `at_most` → `.at_least(:once)` etc).
//!   Detection is at parity; autocorrect (replace with the short form)
//!   is not ported in this batch — same convention as `RSpec/BeEmpty`
//!   (status: partial, autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with `methods = ["times"]`:
//!
//! - `expect(foo).to receive(:bar).exactly(1).times` — flagged
//!   (`.exactly(1).times` → `.once`).
//! - `expect(foo).to receive(:bar).exactly(2).times` — flagged
//!   (→ `.twice`).
//! - `expect(foo).to receive(:bar).at_least(1).times` — flagged
//!   (→ `.at_least(:once)`).
//! - `expect(foo).to receive(:bar).at_most(2).times` — flagged.
//! - `expect(foo).to receive(:bar).exactly(3).times` — count 3, clean.
//! - `expect(action).to have_published_event.exactly(1).times` —
//!   no `receive` in chain, clean.
//!
//! ## No autocorrect
//!
//! Upstream replaces with `.once` / `.twice` / `.at_least(:once)` etc.
//! This batch reports only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ReceiveCounts;

#[cop(
    name = "RSpec/ReceiveCounts",
    description = "Check for once and twice receive counts matchers usage.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl ReceiveCounts {
    #[on_node(kind = "send", methods = ["times"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send {
            receiver, args, ..
        } = *cx.kind(node)
        else {
            return;
        };
        if !cx.list(args).is_empty() {
            return;
        }
        let Some(inner) = receiver.get() else {
            return;
        };
        let NodeKind::Send {
            receiver: inner_recv,
            method: inner_method,
            args: inner_args,
        } = *cx.kind(inner)
        else {
            return;
        };
        let inner_name = cx.symbol_str(inner_method);
        if !matches!(inner_name, "exactly" | "at_least" | "at_most") {
            return;
        }
        let arg_ids = cx.list(inner_args);
        if arg_ids.len() != 1 {
            return;
        }
        let count = match *cx.kind(arg_ids[0]) {
            NodeKind::Int(n) if n == 1 || n == 2 => n,
            _ => return,
        };
        if !receiver_chain_has_bare_receive(cx, inner) {
            return;
        }
        let _ = inner_recv;
        let alternative = matcher_for(inner_name, count);
        let original = cx
            .call_operator_loc(inner)
            .map(|dot| cx.raw_source(murphy_plugin_api::Range {
                start: dot.start,
                end: cx.range(node).end,
            }))
            .unwrap_or_else(|| cx.raw_source(cx.range(node)));
        // Fallback original when dot lookup fails still yields the
        // `.method(args).times` shape via the full range; message stays
        // `Use ... instead of ...`.
        let message = format!("Use `{alternative}` instead of `{original}`.");
        let range = cx
            .call_operator_loc(inner)
            .map(|dot| murphy_plugin_api::Range {
                start: dot.start,
                end: cx.range(node).end,
            })
            .unwrap_or_else(|| cx.range(node));
        cx.emit_offense(range, &message, None);
    }
}

/// `true` when `node` or any descendant is a bare `receive` send.
///
/// Mirrors upstream `stub?` (`def_node_search` for
/// `(send nil? :receive ...)`), which searches the offending node's
/// receiver chain (the chain lives inside the subtree, e.g.
/// `receive(:bar).with(baz).exactly(1)` parses with `receive` under
/// `exactly`).
fn receiver_chain_has_bare_receive(cx: &Cx<'_>, node: NodeId) -> bool {
    for id in core::iter::once(node).chain(cx.descendants(node)) {
        if let NodeKind::Send {
            receiver, method, ..
        } = *cx.kind(id)
            && receiver == OptNodeId::NONE
            && cx.symbol_str(method) == "receive"
        {
            return true;
        }
    }
    false
}

fn matcher_for(method: &str, count: i64) -> String {
    let short = if count == 1 { "once" } else { "twice" };
    if method == "exactly" {
        format!(".{short}")
    } else {
        format!(".{method}(:{short})")
    }
}

#[cfg(test)]
mod tests {
    use super::ReceiveCounts;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_exactly_once() {
        test::<ReceiveCounts>().expect_offense(indoc! {r#"
                expect(foo).to receive(:bar).exactly(1).times
                                            ^^^^^^^^^^^^^^^^^ Use `.once` instead of `.exactly(1).times`.
            "#});
    }

    #[test]
    fn flags_exactly_twice() {
        test::<ReceiveCounts>().expect_offense(indoc! {r#"
                expect(foo).to receive(:bar).exactly(2).times
                                            ^^^^^^^^^^^^^^^^^ Use `.twice` instead of `.exactly(2).times`.
            "#});
    }

    #[test]
    fn flags_at_least_once() {
        test::<ReceiveCounts>().expect_offense(indoc! {r#"
                expect(foo).to receive(:bar).at_least(1).times
                                            ^^^^^^^^^^^^^^^^^^ Use `.at_least(:once)` instead of `.at_least(1).times`.
            "#});
    }

    #[test]
    fn flags_at_most_twice() {
        test::<ReceiveCounts>().expect_offense(indoc! {r#"
                expect(foo).to receive(:bar).at_most(2).times
                                            ^^^^^^^^^^^^^^^^^ Use `.at_most(:twice)` instead of `.at_most(2).times`.
            "#});
    }

    #[test]
    fn allows_exactly_three_times() {
        test::<ReceiveCounts>().expect_no_offenses(indoc! {r#"
                expect(foo).to receive(:bar).exactly(3).times
            "#});
    }

    #[test]
    fn allows_without_receive() {
        test::<ReceiveCounts>().expect_no_offenses(indoc! {r#"
                expect(action).to have_published_event.exactly(1).times
            "#});
    }

    #[test]
    fn flags_after_with() {
        test::<ReceiveCounts>().expect_offense(indoc! {r#"
                expect(foo).to receive(:bar).with(baz).exactly(1).times
                                                      ^^^^^^^^^^^^^^^^^ Use `.once` instead of `.exactly(1).times`.
            "#});
    }
}

murphy_plugin_api::submit_cop!(ReceiveCounts);
