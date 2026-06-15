//! `Style/DigChain` — collapses chained `dig` calls into a single `dig`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/DigChain
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Flags chained dig calls and suggests a single combined dig call.
//!   Both `send` and `csend` (safe navigation) are checked.
//!   The offense is reported at the outermost dig node in the chain.
//!   Hash and block_pass args are excluded matching DigHelp's dig? matcher.
//!   The comments_in_range preservation from RuboCop's autocorrect is omitted
//!   (edge case; comments between chained digs are lost in autocorrect).
//!   Safe: false is noted — cannot guarantee the receiver implements dig
//!   in the standard way.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! x.dig(:foo).dig(:bar).dig(:baz)
//! x.dig(:foo, :bar).dig(:baz)
//! x.dig(:foo, :bar)&.dig(:baz)
//!
//! # good - digs cannot be combined
//! x.dig(:foo).bar.dig(:baz)
//! ```
//!
//! ## Autocorrect
//!
//! Replaces the range from the deepest inner dig's selector start to the
//! outermost dig's expression end with `dig(joined_args)`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

#[derive(Default)]
pub struct DigChain;

#[cop(
    name = "Style/DigChain",
    description = "Use `dig` with multiple parameters instead of chaining multiple calls.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl DigChain {
    #[on_node(kind = "send", methods = ["dig"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// Returns true if `node` is a dig call with a receiver and valid args.
/// Mirrors RuboCop's `dig?` matcher: `(call _ :dig !{hash block_pass}+)`.
fn is_valid_dig(node: NodeId, cx: &Cx<'_>) -> bool {
    if !matches!(
        cx.kind(node),
        NodeKind::Send { .. } | NodeKind::Csend { .. }
    ) {
        return false;
    }
    if cx.method_name(node) != Some("dig") {
        return false;
    }
    if cx.call_receiver(node).get().is_none() {
        return false;
    }
    let args = cx.call_arguments(node);
    if args.is_empty() {
        return false;
    }
    args.iter().all(|&a| {
        !matches!(cx.kind(a), NodeKind::Hash(_) | NodeKind::BlockPass(_))
    })
}

/// Check if this node is a dig that is the direct receiver of another dig.
fn is_inner_dig(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    if !matches!(cx.kind(parent), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        return false;
    }
    if cx.method_name(parent) != Some("dig") {
        return false;
    }
    cx.call_receiver(parent).get() == Some(node)
}

/// Collect all arguments from the dig chain, walking the receiver chain.
/// Returns (innermost_selector_start, all_args_in_order).
fn collect_chain(node: NodeId, cx: &Cx<'_>) -> Option<(u32, Vec<NodeId>)> {
    let mut all_args: Vec<NodeId> = cx.call_arguments(node).to_vec();
    let mut current = node;

    loop {
        let recv = cx.call_receiver(current).get()?;
        if !is_valid_dig(recv, cx) {
            break;
        }
        let mut recv_args: Vec<NodeId> = cx.call_arguments(recv).to_vec();
        recv_args.append(&mut all_args);
        all_args = recv_args;
        current = recv;
    }

    // `current` is the deepest dig. Its selector is the replacement start.
    let sel_range = cx.loc(current).name;
    if sel_range == Range::ZERO {
        return None;
    }

    Some((sel_range.start, all_args))
}

/// Returns true if ForwardedArgs appears before the last argument position.
fn has_invalid_forwarded_args(args: &[NodeId], cx: &Cx<'_>) -> bool {
    if args.is_empty() {
        return false;
    }
    let last_idx = args.len() - 1;
    args[..last_idx]
        .iter()
        .any(|&a| matches!(cx.kind(a), NodeKind::ForwardedArgs))
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // `check_csend` registers on every csend node (the cop macro does not support
    // a `methods` filter on csend), so guard the method name here. Without this,
    // a single `&.dig` followed by other csends — e.g.
    // `foo[bar]&.dig('a','b','c')&.to_i&.positive?` — runs `check` on `&.to_i`,
    // whose receiver is the lone `&.dig`, and a one-element chain gets flagged.
    if cx.method_name(node) != Some("dig") {
        return;
    }

    // Only report at the outermost dig in a chain.
    if is_inner_dig(node, cx) {
        return;
    }

    let Some(recv) = cx.call_receiver(node).get() else {
        return;
    };
    if !is_valid_dig(recv, cx) {
        return;
    }

    let Some((innermost_start, all_args)) = collect_chain(node, cx) else {
        return;
    };

    if has_invalid_forwarded_args(&all_args, cx) {
        return;
    }

    let outer_end = cx.range(node).end;
    let replacement_range = Range {
        start: innermost_start,
        end: outer_end,
    };

    let args_src: Vec<&str> = all_args
        .iter()
        .map(|&a| cx.raw_source(cx.range(a)))
        .collect();
    let replacement = format!("dig({})", args_src.join(", "));

    let message = format!("Use `{replacement}` instead of chaining.");
    cx.emit_offense(replacement_range, &message, None);
    cx.emit_edit(replacement_range, &replacement);
}

#[cfg(test)]
mod tests {
    use super::DigChain;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_two_dig_chain() {
        test::<DigChain>().expect_correction(
            indoc! {r#"
                x.dig(:foo, :bar).dig(:baz)
                  ^^^^^^^^^^^^^^^^^^^^^^^^^ Use `dig(:foo, :bar, :baz)` instead of chaining.
            "#},
            "x.dig(:foo, :bar, :baz)\n",
        );
    }

    #[test]
    fn flags_three_dig_chain() {
        test::<DigChain>().expect_correction(
            indoc! {r#"
                x.dig(:foo).dig(:bar).dig(:baz)
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `dig(:foo, :bar, :baz)` instead of chaining.
            "#},
            "x.dig(:foo, :bar, :baz)\n",
        );
    }

    #[test]
    fn flags_csend_dig_chain() {
        test::<DigChain>().expect_correction(
            indoc! {r#"
                x.dig(:foo, :bar)&.dig(:baz)
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `dig(:foo, :bar, :baz)` instead of chaining.
            "#},
            "x.dig(:foo, :bar, :baz)\n",
        );
    }

    #[test]
    fn accepts_single_dig() {
        test::<DigChain>().expect_no_offenses("x.dig(:foo, :bar)\n");
    }

    #[test]
    fn accepts_chain_broken_by_other_method() {
        test::<DigChain>().expect_no_offenses("x.dig(:foo).bar.dig(:baz)\n");
    }

    #[test]
    fn accepts_no_receiver() {
        test::<DigChain>().expect_no_offenses("dig(:key)\n");
    }

    #[test]
    fn accepts_single_csend_dig_followed_by_other_csends() {
        // `check_csend` fires on every csend (the cop macro can't filter csend by
        // method name). Here `&.to_i`'s receiver is the single `&.dig`, but there
        // is only one dig in the chain, so there is nothing to collapse.
        test::<DigChain>()
            .expect_no_offenses("x = foo.get_settings[bar]&.dig('a', 'b', 'c')&.to_i&.positive?\n");
    }
}
murphy_plugin_api::submit_cop!(DigChain);
