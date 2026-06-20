//! `Style/CollectionQuerying` — prefer predicate methods over `count` comparisons.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/CollectionQuerying
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Handles count.positive?, count > 0, count != 0 -> any?,
//!   count.zero?, count == 0 -> none?,
//!   count == 1 -> one?, for bare `count`, `count(&:blk)`, and `count { }`.
//!   Offense range starts at the `count` selector and ends at the predicate
//!   (RuboCop `count_node.loc.selector.join(node.source_range.end)`).
//!   Autocorrect renames the selector and removes the predicate.
//!   Parenthesized receivers (`(x.count).positive?`) are not matched, matching
//!   RuboCop (the `begin` node breaks its node pattern).
//!   RESIDUAL GAP: `count > 1` -> `many?` is gated behind RuboCop's
//!   `active_support_extensions_enabled?` (default off) and is not implemented;
//!   Murphy has no ActiveSupportExtensions toggle.
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, Range, cop};

#[derive(Default)]
pub struct CollectionQuerying;

#[cop(
    name = "Style/CollectionQuerying",
    description = "Use predicate methods instead of `count` comparisons.",
    default_severity = "warning",
    default_enabled = true,
    options = murphy_plugin_api::NoOptions
)]
impl CollectionQuerying {
    #[on_node(kind = "send", methods = ["positive?", ">", "!=", "zero?", "=="])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // The matched node is the predicate send (`positive?`, `==`, etc.).
        let predicate = cx.method_name(node).unwrap_or("");

        // The predicate's raw receiver: a `count` send, or a block wrapping it.
        // A parenthesized receiver (`(x.count).positive?`) is a `Begin` node;
        // RuboCop's node pattern does not match it. No explicit reject is
        // needed — `Begin` carries no `method_name`, so the `count` selector
        // check below already returns for it.
        let Some(recv_id) = cx.call_receiver(node).get() else {
            return;
        };

        // Resolve the inner `count` send, delegating through a block form.
        let count_send = match cx.block_call(recv_id).get() {
            Some(call) => call,
            None => recv_id,
        };
        if cx.method_name(count_send) != Some("count") {
            return;
        }
        // `count` must have an explicit receiver (RuboCop `!nil?`).
        if cx.call_receiver(count_send).get().is_none() {
            return;
        }
        // Allow `count` with no args or exactly one block-pass arg; reject
        // positional args (RuboCop matches `(call !nil? :count (block-pass _)?)`).
        let count_args = cx.call_arguments(count_send);
        match count_args {
            [] => {}
            [single] if matches!(*cx.kind(*single), NodeKind::BlockPass(_)) => {}
            _ => return,
        }

        let replacement = match (predicate, predicate_literal_int(node, cx)) {
            ("positive?", _) => "any?",
            ("zero?", _) => "none?",
            (">" | "!=", Some("0")) => "any?",
            ("==", Some("0")) => "none?",
            ("==", Some("1")) => "one?",
            _ => return,
        };

        // Offense range: from the `count` selector to the end of the predicate
        // expression (RuboCop `count_node.loc.selector.join(node.source_range.end)`).
        let selector = cx.selector(count_send);
        let offense_range = Range { start: selector.start, end: cx.range(node).end };

        cx.emit_offense(offense_range, &format!("Use `{replacement}` instead."), None);

        // Edit 1: rename the `count` selector to the predicate method.
        cx.emit_edit(selector, replacement);
        // Edit 2: remove everything after the predicate's receiver (the
        // `.positive?` / ` > 0` tail). Anchoring to the receiver's end (not the
        // count call's end) preserves any block: `arr.count { }.positive?` ->
        // `arr.any? { }`.
        cx.emit_edit(Range { start: cx.range(recv_id).end, end: cx.range(node).end }, "");
    }
}

/// The integer-literal source of the sole argument to a comparison predicate
/// (`> 0`, `== 1`, …), or `None` when absent / not an int literal.
fn predicate_literal_int<'a>(node: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    let args = cx.call_arguments(node);
    let [arg] = args else { return None };
    if !matches!(*cx.kind(*arg), NodeKind::Int(_)) {
        return None;
    }
    Some(cx.raw_source(cx.range(*arg)))
}

#[cfg(test)]
mod tests {
    use super::CollectionQuerying;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_count_positive() {
        test::<CollectionQuerying>().expect_offense(indoc! {"
            x.count.positive?
              ^^^^^^^^^^^^^^^ Use `any?` instead.
        "});
    }

    #[test]
    fn flags_count_gt_zero() {
        test::<CollectionQuerying>().expect_offense(indoc! {"
            x.count > 0
              ^^^^^^^^^ Use `any?` instead.
        "});
    }

    #[test]
    fn flags_count_zero() {
        test::<CollectionQuerying>().expect_offense(indoc! {"
            x.count.zero?
              ^^^^^^^^^^^ Use `none?` instead.
        "});
    }

    #[test]
    fn flags_count_eq_zero() {
        test::<CollectionQuerying>().expect_offense(indoc! {"
            x.count == 0
              ^^^^^^^^^^ Use `none?` instead.
        "});
    }

    #[test]
    fn flags_count_eq_one() {
        test::<CollectionQuerying>().expect_offense(indoc! {"
            x.count == 1
              ^^^^^^^^^^ Use `one?` instead.
        "});
    }

    #[test]
    fn flags_count_neq_zero() {
        test::<CollectionQuerying>().expect_offense(indoc! {"
            x.count != 0
              ^^^^^^^^^^ Use `any?` instead.
        "});
    }

    #[test]
    fn flags_count_block_pass() {
        test::<CollectionQuerying>().expect_offense(indoc! {"
            x.count(&:foo?).positive?
              ^^^^^^^^^^^^^^^^^^^^^^^ Use `any?` instead.
        "});
    }

    #[test]
    fn flags_count_with_block() {
        test::<CollectionQuerying>().expect_offense(indoc! {"
            arr.count { |x| x }.positive?
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Use `any?` instead.
        "});
    }

    #[test]
    fn does_not_flag_parenthesized_count() {
        // RuboCop's node pattern does not match a parenthesized receiver.
        test::<CollectionQuerying>().expect_no_offenses("(x.count).positive?\n");
    }

    #[test]
    fn does_not_flag_count_with_positional_arg() {
        test::<CollectionQuerying>().expect_no_offenses("x.count(5).positive?\n");
    }

    #[test]
    fn does_not_flag_bare_count() {
        test::<CollectionQuerying>().expect_no_offenses("count.positive?\n");
    }

    #[test]
    fn does_not_flag_gt_one() {
        // `count > 1` -> `many?` is ActiveSupport-gated (default off).
        test::<CollectionQuerying>().expect_no_offenses("x.count > 1\n");
    }

    #[test]
    fn accepts_any() {
        test::<CollectionQuerying>().expect_no_offenses("x.any?\n");
    }

    #[test]
    fn autocorrects_count_positive() {
        test::<CollectionQuerying>().expect_correction(
            indoc! {"
                x.count.positive?
                  ^^^^^^^^^^^^^^^ Use `any?` instead.
            "},
            "x.any?\n",
        );
    }

    #[test]
    fn autocorrects_count_gt_zero() {
        test::<CollectionQuerying>().expect_correction(
            indoc! {"
                x.count > 0
                  ^^^^^^^^^ Use `any?` instead.
            "},
            "x.any?\n",
        );
    }

    #[test]
    fn autocorrects_count_eq_one() {
        test::<CollectionQuerying>().expect_correction(
            indoc! {"
                x.count == 1
                  ^^^^^^^^^^ Use `one?` instead.
            "},
            "x.one?\n",
        );
    }

    #[test]
    fn autocorrects_count_block_pass() {
        test::<CollectionQuerying>().expect_correction(
            indoc! {"
                x.count(&:foo?).positive?
                  ^^^^^^^^^^^^^^^^^^^^^^^ Use `any?` instead.
            "},
            "x.any?(&:foo?)\n",
        );
    }

    #[test]
    fn autocorrects_count_with_block() {
        test::<CollectionQuerying>().expect_correction(
            indoc! {"
                arr.count { |x| x }.positive?
                    ^^^^^^^^^^^^^^^^^^^^^^^^^ Use `any?` instead.
            "},
            "arr.any? { |x| x }\n",
        );
    }
}
murphy_plugin_api::submit_cop!(CollectionQuerying);
