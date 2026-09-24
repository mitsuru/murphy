//! `Style/RedundantStructKeywordInit` — flags redundant `keyword_init` option
//! for `Struct.new`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantStructKeywordInit
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Since Ruby 3.2, `keyword_init` in `Struct.new` defaults to `nil`
//!   behaviour, so `keyword_init: nil` and `keyword_init: true` are
//!   redundant. Matches `Struct.new(..., keyword_init: true|nil)` and the
//!   safe-navigation form `Struct&.new(...)` (RuboCop aliases
//!   `on_csend on_send`). The last argument must be a hash literal; only
//!   `keyword_init` pairs with the symbol key `:keyword_init` are
//!   considered. If ANY `keyword_init: false` pair is present, the cop
//!   emits nothing, even for sibling `true`/`nil` pairs. The receiver must
//!   be `Struct` or `::Struct`; `Foo::Struct` is rejected. The cop is gated
//!   at target Ruby 3.2 or later.
//!
//!   Autocorrect mirrors RuboCop 1.87.0's deletion range: when the hash
//!   argument has a previous call argument, remove from that argument's
//!   end through the redundant pair's end; otherwise remove just the pair.
//!   This includes upstream's range quirks, such as deleting intervening
//!   hash pairs or leaving commas when the hash is the first argument.
//!   Overlapping deletions are coalesced, and nested corrections covered by
//!   an outer deletion are suppressed while both offenses remain reported.
//!   The correction is unsafe (`SafeAutoCorrect: false`): removing
//!   `keyword_init: true` changes `Struct#keyword_init?` and the
//!   initialization contract.
//! ```
//!
//! ## Matched shapes
//!
//! `Send`/`Csend` nodes with method `new`, receiver `Struct`/`::Struct`, and a
//! trailing `Hash` argument containing a `keyword_init: true|nil` pair (and no
//! `keyword_init: false` pair).

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantStructKeywordInit;

#[cop(
    name = "Style/RedundantStructKeywordInit",
    description = "Checks for redundant `keyword_init` option for `Struct.new`.",
    default_severity = "warning",
    default_enabled = false,
    minimum_target_ruby_version = "3.2",
    options = NoOptions,
    safe_autocorrect = false,
)]
impl RedundantStructKeywordInit {
    #[on_node(kind = "send", methods = ["new"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        if cx.method_name(node) == Some("new") {
            check(node, cx);
        }
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Receiver must be exactly `Struct` or `::Struct` (nil / cbase scope).
    let Some(recv_id) = cx.call_receiver(node).get() else {
        return;
    };
    if !cx.is_global_const(recv_id, "Struct") {
        return;
    }

    // The last argument must be a hash literal; only its pairs are inspected.
    let args = cx.call_arguments(node);
    let Some(&last_arg) = args.last() else {
        return;
    };
    let NodeKind::Hash(pairs) = *cx.kind(last_arg) else {
        return;
    };

    let pair_list = cx.list(pairs);

    // RuboCop returns early if ANY `keyword_init: false` pair is present —
    // `false` is not redundant, so the whole call is left alone (even sibling
    // `true`/`nil` pairs). Check this before emitting any offense.
    if has_keyword_init_false(pair_list, cx) {
        return;
    }

    let mut correction_ranges = Vec::new();
    // Flag each `keyword_init: true|nil` pair.
    for &pair in pair_list {
        let Some(value) = keyword_init_value(pair, cx) else {
            continue;
        };
        if matches!(cx.kind(value), NodeKind::True_ | NodeKind::Nil) {
            register_offense(pair, value, cx);
            let range = upstream_correction_range(args, pair, cx);
            if !ancestor_correction_covers(node, range, cx) {
                correction_ranges.push(range);
            }
        }
    }

    // RuboCop's correction ranges can overlap when a hash has multiple
    // redundant pairs. Deletions commute, so coalesce their union before
    // sending edits to the host (which rejects overlapping edits).
    for range in merge_overlapping_ranges(correction_ranges) {
        cx.emit_edit(range, "");
    }
}

fn has_keyword_init_false(pairs: &[NodeId], cx: &Cx<'_>) -> bool {
    pairs.iter().any(|&pair| {
        keyword_init_value(pair, cx).is_some_and(|value| matches!(cx.kind(value), NodeKind::False_))
    })
}

/// RuboCop takes the pair's hash parent and then that hash's last left sibling.
/// For a trailing hash with a preceding call argument, that sibling is the
/// preceding argument; otherwise it is a non-node method symbol and RuboCop
/// falls back to the pair alone.
fn upstream_correction_range(args: &[NodeId], pair: NodeId, cx: &Cx<'_>) -> Range {
    let pair_range = cx.range(pair);
    let Some(previous_argument_index) = args.len().checked_sub(2) else {
        return pair_range;
    };
    let previous_argument = args[previous_argument_index];
    Range {
        start: cx.range(previous_argument).end,
        end: pair_range.end,
    }
}

fn range_contains(outer: Range, inner: Range) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

/// An outer redundant Struct.new correction may delete a nested Struct.new
/// expression too. Keep its offense, but avoid submitting a clobbered edit.
fn ancestor_correction_covers(node: NodeId, range: Range, cx: &Cx<'_>) -> bool {
    let mut ancestor = cx.parent(node).get();
    while let Some(ancestor_id) = ancestor {
        ancestor = cx.parent(ancestor_id).get();
        if cx.method_name(ancestor_id) != Some("new") {
            continue;
        }
        let Some(receiver) = cx.call_receiver(ancestor_id).get() else {
            continue;
        };
        if !cx.is_global_const(receiver, "Struct") {
            continue;
        }
        let args = cx.call_arguments(ancestor_id);
        let Some(&last_arg) = args.last() else {
            continue;
        };
        let NodeKind::Hash(pairs) = *cx.kind(last_arg) else {
            continue;
        };
        let pair_list = cx.list(pairs);
        if has_keyword_init_false(pair_list, cx) {
            continue;
        }

        for &pair in pair_list {
            let Some(value) = keyword_init_value(pair, cx) else {
                continue;
            };
            if matches!(cx.kind(value), NodeKind::True_ | NodeKind::Nil) {
                let outer_range = upstream_correction_range(args, pair, cx);
                if range_contains(outer_range, range) {
                    return true;
                }
            }
        }
    }
    false
}

fn merge_overlapping_ranges(mut ranges: Vec<Range>) -> Vec<Range> {
    if ranges.len() < 2 {
        return ranges;
    }
    ranges.sort_unstable_by_key(|range| (range.start, range.end));
    let mut merged: Vec<Range> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match merged.last_mut() {
            Some(previous) if range.start < previous.end => {
                previous.end = previous.end.max(range.end);
                continue;
            }
            _ => {}
        }
        merged.push(range);
    }
    merged
}

/// If `pair` is a `keyword_init:` pair (symbol key), return its value node.
fn keyword_init_value(pair: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let NodeKind::Pair { key, value } = *cx.kind(pair) else {
        return None;
    };
    matches!(cx.kind(key), NodeKind::Sym(sym) if cx.symbol_str(*sym) == "keyword_init")
        .then_some(value)
}

/// Emit an offense covering the whole `keyword_init: <value>` pair, with the
/// value's source embedded in the message (matching RuboCop verbatim).
fn register_offense(pair: NodeId, value: NodeId, cx: &Cx<'_>) {
    let value_src = cx.raw_source(cx.range(value));
    let msg = format!("Remove the redundant `keyword_init: {value_src}`.");
    cx.emit_offense(cx.range(pair), &msg, None);
}

#[cfg(test)]
mod tests {
    use super::RedundantStructKeywordInit;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_keyword_init_true() {
        test::<RedundantStructKeywordInit>().expect_offense(indoc! {r#"
            Struct.new(:foo, keyword_init: true)
                             ^^^^^^^^^^^^^^^^^^ Remove the redundant `keyword_init: true`.
        "#});
    }

    #[test]
    fn flags_keyword_init_nil() {
        test::<RedundantStructKeywordInit>().expect_offense(indoc! {r#"
            Struct.new(:foo, keyword_init: nil)
                             ^^^^^^^^^^^^^^^^^ Remove the redundant `keyword_init: nil`.
        "#});
    }

    #[test]
    fn flags_keyword_init_true_after_other_pair() {
        test::<RedundantStructKeywordInit>().expect_offense(indoc! {r#"
            Struct.new(:foo, x: 1, keyword_init: true)
                                   ^^^^^^^^^^^^^^^^^^ Remove the redundant `keyword_init: true`.
        "#});
    }

    #[test]
    fn flags_cbase_struct() {
        test::<RedundantStructKeywordInit>().expect_offense(indoc! {r#"
            ::Struct.new(:foo, keyword_init: true)
                               ^^^^^^^^^^^^^^^^^^ Remove the redundant `keyword_init: true`.
        "#});
    }

    #[test]
    fn flags_safe_navigation() {
        test::<RedundantStructKeywordInit>().expect_offense(indoc! {r#"
            Struct&.new(:foo, keyword_init: true)
                              ^^^^^^^^^^^^^^^^^^ Remove the redundant `keyword_init: true`.
        "#});
    }

    #[test]
    fn autocorrects_keyword_init_true() {
        test::<RedundantStructKeywordInit>().expect_correction(
            indoc! {r#"
                Struct.new(:foo, keyword_init: true)
                                 ^^^^^^^^^^^^^^^^^^ Remove the redundant `keyword_init: true`.
            "#},
            "Struct.new(:foo)\n",
        );
    }

    #[test]
    fn autocorrects_keyword_init_nil() {
        test::<RedundantStructKeywordInit>().expect_correction(
            indoc! {r#"
                Struct.new(:foo, keyword_init: nil)
                                 ^^^^^^^^^^^^^^^^^ Remove the redundant `keyword_init: nil`.
            "#},
            "Struct.new(:foo)\n",
        );
    }

    #[test]
    fn autocorrects_pair_without_previous_argument() {
        test::<RedundantStructKeywordInit>().expect_correction(
            indoc! {r#"
                Struct.new(keyword_init: true)
                           ^^^^^^^^^^^^^^^^^^ Remove the redundant `keyword_init: true`.
            "#},
            "Struct.new()\n",
        );
    }

    #[test]
    fn autocorrect_mirrors_upstream_range_over_intervening_pairs() {
        test::<RedundantStructKeywordInit>().expect_correction(
            indoc! {r#"
                Struct.new(:foo, x: 1, keyword_init: true)
                                       ^^^^^^^^^^^^^^^^^^ Remove the redundant `keyword_init: true`.
            "#},
            "Struct.new(:foo)\n",
        );
    }

    #[test]
    fn autocorrect_preserves_pairs_after_target() {
        test::<RedundantStructKeywordInit>().expect_correction(
            indoc! {r#"
                Struct.new(:foo, keyword_init: true, x: 1)
                                 ^^^^^^^^^^^^^^^^^^ Remove the redundant `keyword_init: true`.
            "#},
            "Struct.new(:foo, x: 1)\n",
        );
    }

    #[test]
    fn coalesces_overlapping_ranges_for_multiple_redundant_pairs() {
        test::<RedundantStructKeywordInit>().expect_correction(
            indoc! {r#"
                Struct.new(
                  :foo,
                  keyword_init: true,
                  ^^^^^^^^^^^^^^^^^^ Remove the redundant `keyword_init: true`.
                  x: 1,
                  keyword_init: nil
                  ^^^^^^^^^^^^^^^^^ Remove the redundant `keyword_init: nil`.
                )
            "#},
            "Struct.new(\n  :foo\n)\n",
        );
    }

    #[test]
    fn skips_nested_edit_clobbered_by_outer_correction() {
        test::<RedundantStructKeywordInit>().expect_correction(
            indoc! {r#"
                Struct.new(
                  :foo,
                  x: Struct.new(:bar, keyword_init: true),
                                      ^^^^^^^^^^^^^^^^^^ Remove the redundant `keyword_init: true`.
                  keyword_init: nil
                  ^^^^^^^^^^^^^^^^^ Remove the redundant `keyword_init: nil`.
                )
            "#},
            "Struct.new(\n  :foo\n)\n",
        );
    }

    #[test]
    fn autocorrects_pair_only_inside_explicit_hash() {
        test::<RedundantStructKeywordInit>().expect_correction(
            indoc! {r#"
                Struct.new({ keyword_init: true })
                             ^^^^^^^^^^^^^^^^^^ Remove the redundant `keyword_init: true`.
            "#},
            "Struct.new({  })\n",
        );
    }

    #[test]
    fn autocorrect_mirrors_range_for_explicit_hash_after_argument() {
        test::<RedundantStructKeywordInit>().expect_correction(
            indoc! {r#"
                Struct.new(:foo, { keyword_init: true })
                                   ^^^^^^^^^^^^^^^^^^ Remove the redundant `keyword_init: true`.
            "#},
            "Struct.new(:foo })\n",
        );
    }

    #[test]
    fn autocorrect_mirrors_pair_only_range_for_hash_first_argument() {
        test::<RedundantStructKeywordInit>().expect_correction(
            indoc! {r#"
                Struct.new(x: 1, keyword_init: true)
                                 ^^^^^^^^^^^^^^^^^^ Remove the redundant `keyword_init: true`.
            "#},
            "Struct.new(x: 1, )\n",
        );
    }

    #[test]
    fn accepts_keyword_init_false() {
        test::<RedundantStructKeywordInit>().expect_no_offenses("Struct.new(:foo, keyword_init: false)\n");
    }

    #[test]
    fn accepts_keyword_init_false_with_sibling_true() {
        // RuboCop returns early if any `keyword_init: false` is present,
        // suppressing even sibling redundant pairs.
        test::<RedundantStructKeywordInit>()
            .expect_no_offenses("Struct.new(:foo, keyword_init: false, keyword_init: true)\n");
    }

    #[test]
    fn accepts_keyword_init_false_with_sibling_nil() {
        test::<RedundantStructKeywordInit>()
            .expect_no_offenses("Struct.new(:foo, keyword_init: false, keyword_init: nil)\n");
    }

    #[test]
    fn accepts_namespaced_struct() {
        test::<RedundantStructKeywordInit>()
            .expect_no_offenses("Foo::Struct.new(:foo, keyword_init: true)\n");
    }

    #[test]
    fn accepts_plain_struct_new() {
        test::<RedundantStructKeywordInit>().expect_no_offenses("Struct.new(:foo)\n");
    }

    #[test]
    fn accepts_no_hash_argument() {
        test::<RedundantStructKeywordInit>().expect_no_offenses("Struct.new(:foo, :bar)\n");
    }

    #[test]
    fn accepts_string_keyword_init_key() {
        // Upstream matches `(sym :keyword_init)` only; a string/rocket key
        // is a different shape and is not flagged.
        test::<RedundantStructKeywordInit>()
            .expect_no_offenses("Struct.new(:foo, \"keyword_init\" => true)\n");
    }

    #[test]
    fn accepts_non_struct_receiver() {
        test::<RedundantStructKeywordInit>()
            .expect_no_offenses("Klass.new(:foo, keyword_init: true)\n");
    }

    #[test]
    fn minimum_target_ruby_version_is_set() {
        use murphy_plugin_api::{Cop, RubyVersion};
        assert_eq!(
            <RedundantStructKeywordInit as Cop>::MINIMUM_TARGET_RUBY_VERSION,
            Some(RubyVersion::new(3, 2)),
        );
        assert_eq!(
            <RedundantStructKeywordInit as Cop>::SAFE_AUTOCORRECT,
            Some(false),
        );
    }
}

murphy_plugin_api::submit_cop!(RedundantStructKeywordInit);
