//! `Style/HashFetchChain` — use `dig` instead of chaining `fetch` calls.
//!
//! When `fetch(key, default)` calls are chained on a hash, the expectation
//! is that each step returns either `nil` or another hash. These can be
//! simplified with a single call to `dig` with multiple arguments.
//!
//! If a non-nil default value (`{}` or `Hash.new`) is given for an earlier
//! call in the chain, the offense still fires as long as the **final** call
//! has a `nil` default. If the final call has a non-nil default, the chain
//! is not flagged (the default value cannot be safely expressed with `dig`).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/HashFetchChain
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Both Send and Csend (safe-navigation) variants are handled.
//!   Known v1 limitation: no per-cop file-pattern gating — the cop fires
//!   on all `.rb` files, not only those that use hash patterns.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! hash.fetch('foo', nil).fetch('bar', nil)
//! hash.fetch('foo', {}).fetch('bar', nil)
//! hash.fetch('foo', Hash.new).fetch('bar', nil)
//! hash.fetch('foo', nil)&.fetch('bar', nil)
//!
//! # good
//! hash.dig('foo', 'bar')
//!
//! # ok — final fetch has non-nil default
//! hash.fetch('foo', nil).fetch('bar', {})
//! ```
//!
//! ## Autocorrect
//!
//! Replaces the chained `fetch` calls with a single `dig` call combining
//! all keys. The replacement range runs from the innermost `.fetch`
//! selector to the outermost expression's end, producing
//! `receiver.dig(key1, key2, ...)`.
//!
//! ## Safety
//!
//! This cop is **unsafe** — it cannot be guaranteed that the receiver is
//! a `Hash` or that `fetch` or `dig` have the expected standard
//! implementation.

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind, Range};

#[derive(Default)]
pub struct HashFetchChain;

#[cop(
    name = "Style/HashFetchChain",
    description = "Use `dig` instead of chaining `fetch` calls.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl HashFetchChain {
    #[on_node(kind = "send", methods = ["fetch"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        if cx.method_name(node) != Some("fetch") {
            return;
        }
        check(node, cx);
    }
}

/// Check if a node is a `fetch` call with an acceptable default value.
/// Mirrors RuboCop's `diggable?` pattern:
/// `(call _ :fetch $_arg {nil (hash) (send (const {nil? cbase} :Hash) :new)})`
fn is_diggable_fetch(node: NodeId, cx: &Cx<'_>) -> bool {
    if cx.method_name(node) != Some("fetch") {
        return false;
    }
    if cx.call_receiver(node).get().is_none() {
        return false;
    }
    let args = cx.call_arguments(node);
    args.len() >= 2 && is_acceptable_default(args[1], cx)
}

/// Check if a node is an acceptable default value: `nil`, `{}`, `Hash.new`, or `::Hash.new`.
fn is_acceptable_default(node: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Nil => true,
        NodeKind::Hash(list) => cx.list(list).is_empty(),
        NodeKind::Send { method, args, .. } => {
            cx.symbol_str(method) == "new"
                && cx.list(args).is_empty()
                && cx
                    .call_receiver(node)
                    .get()
                    .and_then(|r| cx.const_name(r))
                    .as_deref()
                    == Some("Hash")
        }
        _ => false,
    }
}

/// Check if this node's parent is also a diggable fetch call that would
/// absorb this node into a larger chain.
fn is_inner_chain_fetch(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    if cx.method_name(parent) != Some("fetch") {
        return false;
    }
    let Some(recv) = cx.call_receiver(parent).get() else {
        return false;
    };
    recv == node && is_diggable_fetch(parent, cx)
}

/// Check if the outermost fetch in the chain has a non-nil default value
/// or is missing a default entirely (which would raise on missing keys).
fn last_fetch_non_nil(node: NodeId, cx: &Cx<'_>) -> bool {
    if cx.method_name(node) != Some("fetch") {
        return false;
    }
    let args = cx.call_arguments(node);
    args.len() < 2 || !matches!(cx.kind(args[1]), NodeKind::Nil)
}

/// Walk the fetch chain from outermost inward, collecting keys.
/// Returns `(innermost_selector_start, keys_in_source_order)`.
fn collect_chain(node: NodeId, cx: &Cx<'_>) -> Option<(u32, Vec<NodeId>)> {
    let mut keys: Vec<NodeId> = Vec::new();

    let args = cx.call_arguments(node);
    if args.is_empty() {
        return None;
    }
    keys.push(args[0]);

    let mut current = node;
    while let Some(recv) = cx.call_receiver(current).get() {
        if !is_diggable_fetch(recv, cx) {
            break;
        }
        let recv_args = cx.call_arguments(recv);
        keys.insert(0, recv_args.first().copied()?);
        current = recv;
    }

    if keys.len() < 2 {
        return None;
    }

    let sel_range = cx.loc(current).name;
    if sel_range == Range::ZERO {
        return None;
    }

    Some((sel_range.start, keys))
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if is_inner_chain_fetch(node, cx) {
        return;
    }

    if last_fetch_non_nil(node, cx) {
        return;
    }

    let Some((innermost_start, keys)) = collect_chain(node, cx) else {
        return;
    };

    let outer_end = cx.range(node).end;
    let replacement_range = Range {
        start: innermost_start,
        end: outer_end,
    };

    let key_srcs: Vec<&str> = keys.iter().map(|&id| cx.raw_source(cx.range(id))).collect();
    let replacement = format!("dig({})", key_srcs.join(", "));

    let message = format!("Use `{replacement}` instead.");
    cx.emit_offense(replacement_range, &message, None);
    cx.emit_edit(replacement_range, &replacement);
}

#[cfg(test)]
mod tests {
    use super::HashFetchChain;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_two_fetch_chain() {
        test::<HashFetchChain>().expect_correction(
            indoc! {r#"
                h.fetch('foo', nil).fetch('bar', nil)
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `dig('foo', 'bar')` instead.
            "#},
            "h.dig('foo', 'bar')\n",
        );
    }

    #[test]
    fn flags_three_fetch_chain() {
        test::<HashFetchChain>().expect_correction(
            indoc! {r#"
                h.fetch('foo', nil).fetch('bar', nil).fetch('baz', nil)
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `dig('foo', 'bar', 'baz')` instead.
            "#},
            "h.dig('foo', 'bar', 'baz')\n",
        );
    }

    #[test]
    fn flags_chain_with_empty_hash_intermediate() {
        test::<HashFetchChain>().expect_correction(
            indoc! {r#"
                h.fetch('foo', {}).fetch('bar', nil)
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `dig('foo', 'bar')` instead.
            "#},
            "h.dig('foo', 'bar')\n",
        );
    }

    #[test]
    fn flags_csend_chain() {
        test::<HashFetchChain>().expect_correction(
            indoc! {r#"
                h.fetch('foo', nil)&.fetch('bar', nil)
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `dig('foo', 'bar')` instead.
            "#},
            "h.dig('foo', 'bar')\n",
        );
    }

    #[test]
    fn flags_chain_with_hash_new_intermediate() {
        test::<HashFetchChain>().expect_correction(
            indoc! {r#"
                h.fetch('foo', Hash.new).fetch('bar', nil)
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `dig('foo', 'bar')` instead.
            "#},
            "h.dig('foo', 'bar')\n",
        );
    }

    #[test]
    fn flags_chain_with_global_hash_new_intermediate() {
        test::<HashFetchChain>().expect_correction(
            indoc! {r#"
                h.fetch('foo', ::Hash.new).fetch('bar', nil)
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `dig('foo', 'bar')` instead.
            "#},
            "h.dig('foo', 'bar')\n",
        );
    }

    #[test]
    fn accepts_single_fetch() {
        test::<HashFetchChain>().expect_no_offenses("h.fetch('foo', nil)\n");
    }

    #[test]
    fn accepts_last_fetch_non_nil() {
        test::<HashFetchChain>().expect_no_offenses("h.fetch('foo', nil).fetch('bar', {})\n");
    }

    #[test]
    fn accepts_outermost_without_default() {
        test::<HashFetchChain>().expect_no_offenses(
            "h.fetch(:a, nil).fetch(:b)\n",
        );
    }

    #[test]
    fn accepts_no_receiver() {
        test::<HashFetchChain>().expect_no_offenses("fetch('foo', nil)\n");
    }

    #[test]
    fn accepts_non_fetch_method() {
        test::<HashFetchChain>().expect_no_offenses("h.fetch('foo', nil).bar('baz')\n");
    }
}
murphy_plugin_api::submit_cop!(HashFetchChain);
