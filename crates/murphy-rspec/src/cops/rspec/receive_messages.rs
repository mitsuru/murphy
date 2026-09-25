//! `RSpec/ReceiveMessages` — prefer `receive_messages` over repeated `receive`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/ReceiveMessages
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_begin`: `allow_receive_message?`
//!   (`(send (send nil? :allow ...) :to (send (send nil? :receive (sym _))
//!   :and_return !heredoc_or_splat?))`) grouped by `allow_argument`
//!   (the `allow(...)` object). Groups with a single stub stay clean;
//!   duplicate message names (`receive(:foo)` twice) stay clean via
//!   `uniq_items`; `and_return` with multiple args, splat, heredoc
//!   (`<<~`), or trailing chain (`.once` / `.ordered` — top node is not
//!   `to`) stays clean. Each remaining item flags whole per
//!   `add_offense(item)` with
//!   `Use `receive_messages` instead of multiple stubs on lines [...]`.
//!   Heredoc is detected via `<<` in source (upstream checks
//!   `heredoc?`). Detection is at parity; autocorrect (merge into
//!   `receive_messages`) is not ported in this batch — same convention
//!   as `RSpec/ReceiveCounts` (status: partial, autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Begin` (e.g. the body of `before do ... end`):
//!
//! - Two `allow(Service).to receive(:foo).and_return` stubs — both flagged.
//! - Same object, different messages — flagged.
//! - Different `allow` objects — separate groups, clean.
//! - Same message twice (`:foo` twice) — clean.
//! - `and_return(1, 2)` (multiple) — clean.
//! - `and_return(*arr)` (splat) — clean.
//! - Heredoc return — clean.
//! - `.once` / `.ordered` trailing — clean (top is not `to`).
//!
//! ## No autocorrect
//!
//! Upstream merges into `receive_messages(...)`. This batch reports only.

use std::collections::BTreeMap;

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::line_index_of_offset;

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ReceiveMessages;

#[cop(
    name = "RSpec/ReceiveMessages",
    description = "Prefer receive_messages over multiple receives.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl ReceiveMessages {
    #[on_node(kind = "begin")]
    fn check_begin(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Begin(members) = *cx.kind(node) else {
            return;
        };
        let children = cx.list(members).to_vec();
        let mut groups: BTreeMap<String, Vec<(NodeId, String)>> = BTreeMap::new();
        for &child in &children {
            let Some((allow_key, recv_name)) = allow_receive_parts(cx, child) else {
                continue;
            };
            groups
                .entry(allow_key)
                .or_default()
                .push((child, recv_name));
        }
        for items in groups.values() {
            if items.len() < 2 {
                continue;
            }
            // `uniq_items`: keep only stubs whose message name is unique
            // in the group (same message twice stays clean).
            let uniq: Vec<(NodeId, String)> = items
                .iter()
                .filter(|(_, recv)| {
                    items
                        .iter()
                        .filter(|(_, other)| other == recv)
                        .count()
                        == 1
                })
                .cloned()
                .collect();
            if uniq.len() < 2 {
                continue;
            }
            let src = cx.source();
            let lines: Vec<usize> = uniq
                .iter()
                .map(|(id, _)| line_index_of_offset(src, cx.range(*id).start) + 1)
                .collect();
            for (idx, (item, _)) in uniq.iter().enumerate() {
                let others: Vec<usize> = lines
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != idx)
                    .map(|(_, l)| *l)
                    .collect();
                let msg = format!(
                    "Use `receive_messages` instead of multiple stubs on lines [{}].",
                    others
                        .iter()
                        .map(|n| n.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                cx.emit_offense(cx.range(*item), &msg, None);
            }
        }
    }
}

/// `(allow_key, receive_name)` when `node` is
/// `allow(X).to receive(:sym).and_return(single)` with no trailing chain.
fn allow_receive_parts(cx: &Cx<'_>, node: NodeId) -> Option<(String, String)> {
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
    let allow_id = receiver.get()?;
    let NodeKind::Send {
        receiver: allow_recv,
        method: allow_method,
        args: allow_args,
    } = *cx.kind(allow_id)
    else {
        return None;
    };
    if allow_recv != OptNodeId::NONE || cx.symbol_str(allow_method) != "allow" {
        return None;
    }
    let allow_arg_ids = cx.list(allow_args);
    if allow_arg_ids.is_empty() {
        return None;
    }
    let allow_key = cx.raw_source(cx.range(allow_arg_ids[0])).to_owned();
    let to_args = cx.list(args);
    if to_args.len() != 1 {
        return None;
    }
    let NodeKind::Send {
        receiver: ar_recv,
        method: ar_method,
        args: ar_args,
    } = *cx.kind(to_args[0])
    else {
        return None;
    };
    if cx.symbol_str(ar_method) != "and_return" {
        return None;
    }
    let recv_id = ar_recv.get()?;
    let NodeKind::Send {
        receiver: r_recv,
        method: r_method,
        args: r_args,
    } = *cx.kind(recv_id)
    else {
        return None;
    };
    if r_recv != OptNodeId::NONE || cx.symbol_str(r_method) != "receive" {
        return None;
    }
    let recv_args = cx.list(r_args);
    if recv_args.len() != 1 {
        return None;
    }
    let NodeKind::Sym(sym) = *cx.kind(recv_args[0]) else {
        return None;
    };
    let recv_name = cx.symbol_str(sym).to_owned();
    let ret_args = cx.list(ar_args);
    if ret_args.len() != 1 {
        return None;
    }
    if matches!(*cx.kind(ret_args[0]), NodeKind::Splat(_)) {
        return None;
    }
    let ret_src = cx.raw_source(cx.range(ret_args[0]));
    if ret_src.contains("<<") {
        return None;
    }
    Some((allow_key, recv_name))
}

#[cfg(test)]
mod tests {
    use super::ReceiveMessages;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_multiple_stubs_same_object() {
        test::<ReceiveMessages>().expect_offense(indoc! {r#"
                before do
                  allow(Service).to receive(:foo).and_return(baz)
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `receive_messages` instead of multiple stubs on lines [3].
                  allow(Service).to receive(:bar).and_return(qux)
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `receive_messages` instead of multiple stubs on lines [2].
                end
            "#});
    }

    #[test]
    fn ignores_different_objects() {
        test::<ReceiveMessages>().expect_no_offenses(indoc! {r#"
                before do
                  allow(Service).to receive(:foo).and_return(1)
                  allow(user).to receive(:bar).and_return(2)
                end
            "#});
    }

    #[test]
    fn ignores_same_message() {
        test::<ReceiveMessages>().expect_no_offenses(indoc! {r#"
                before do
                  allow(Service).to receive(:foo).and_return(bar)
                  allow(Service).to receive(:foo).and_return(qux)
                end
            "#});
    }

    #[test]
    fn ignores_multiple_return_values() {
        test::<ReceiveMessages>().expect_no_offenses(indoc! {r#"
                before do
                  allow(Service).to receive(:foo).and_return(1, 2)
                  allow(Service).to receive(:bar).and_return(3, 4)
                end
            "#});
    }

    #[test]
    fn ignores_splat_return() {
        test::<ReceiveMessages>().expect_no_offenses(indoc! {r#"
                before do
                  allow(Service).to receive(:foo).and_return(*array)
                  allow(Service).to receive(:bar).and_return(*array)
                end
            "#});
    }

    #[test]
    fn ignores_receive_counts() {
        test::<ReceiveMessages>().expect_no_offenses(indoc! {r#"
                before do
                  allow(Service).to receive(:foo).and_return(1).once
                  allow(Service).to receive(:bar).and_return(2).twice
                end
            "#});
    }

    #[test]
    fn ignores_single_stub() {
        test::<ReceiveMessages>().expect_no_offenses(indoc! {r#"
                before do
                  allow(Service).to receive(:foo).and_return(1)
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(ReceiveMessages);
