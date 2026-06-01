//! `Style/RedundantSortBy` — replaces `sort_by { |x| x }` (identity block) with
//! `sort`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantSortBy
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Handles the three identity-block forms: normal Block (`|x| x`),
//!   Numblock (`_1`), and Itblock (`it`). Autocorrects by replacing the
//!   `sort_by { ... }` span with `sort`. No config options.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! array.sort_by { |x| x }
//! array.sort_by { _1 }
//! array.sort_by { it }
//! array.sort_by do |var|
//!   var
//! end
//!
//! # good
//! array.sort
//! ```
//!
//! ## Autocorrect
//!
//! Replaces the span from the `sort_by` selector through the block end with
//! `sort` (single edit).

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantSortBy;

const MSG_BLOCK: &str = "Use `sort` instead of `sort_by { |%s| %s }`.";
const MSG_NUMBLOCK: &str = "Use `sort` instead of `sort_by { _1 }`.";
const MSG_ITBLOCK: &str = "Use `sort` instead of `sort_by { it }`.";

#[cop(
    name = "Style/RedundantSortBy",
    description = "Use `sort` instead of `sort_by` with an identity block.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RedundantSortBy {
    /// Normal block: `sort_by { |x| x }`.
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        if let Some((sort_by_call, var_name)) = match_identity_block(node, cx) {
            let offense_range = sort_by_to_block_end(sort_by_call, node, cx);
            let msg = MSG_BLOCK
                .replacen("%s", var_name, 1)
                .replacen("%s", var_name, 1);
            cx.emit_offense(offense_range, &msg, None);
            cx.emit_edit(offense_range, "sort");
        }
    }

    /// Numbered-parameter block: `sort_by { _1 }`.
    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        if let Some(sort_by_call) = match_identity_numblock(node, cx) {
            let offense_range = sort_by_to_block_end(sort_by_call, node, cx);
            cx.emit_offense(offense_range, MSG_NUMBLOCK, None);
            cx.emit_edit(offense_range, "sort");
        }
    }

    /// `it`-parameter block: `sort_by { it }`.
    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        if let Some(sort_by_call) = match_identity_itblock(node, cx) {
            let offense_range = sort_by_to_block_end(sort_by_call, node, cx);
            cx.emit_offense(offense_range, MSG_ITBLOCK, None);
            cx.emit_edit(offense_range, "sort");
        }
    }
}

// ---------------------------------------------------------------------------
// Pattern matchers
// ---------------------------------------------------------------------------

/// Returns `(sort_by_call_node, var_name_str)` when `node` is a Block that
/// matches `<recv>.sort_by { |x| x }` (identity parameter).
fn match_identity_block<'a>(node: NodeId, cx: &'a Cx<'_>) -> Option<(NodeId, &'a str)> {
    let NodeKind::Block { call, args, body } = *cx.kind(node) else {
        return None;
    };

    // Inner call must be `sort_by`.
    if cx.method_name(call)? != "sort_by" {
        return None;
    }

    // The call must have no extra arguments (no block-pass etc.).
    if !cx.call_arguments(call).is_empty() {
        return None;
    }

    // Args must be exactly one plain `Arg(sym)`.
    let args_list = match *cx.kind(args) {
        NodeKind::Args(list) => cx.list(list),
        _ => return None,
    };
    if args_list.len() != 1 {
        return None;
    }
    let NodeKind::Arg(param_sym) = *cx.kind(args_list[0]) else {
        return None;
    };

    // Body must be `Lvar(param_sym)`.
    let body_id = body.get()?;
    let NodeKind::Lvar(body_sym) = *cx.kind(body_id) else {
        return None;
    };
    if body_sym != param_sym {
        return None;
    }

    Some((call, cx.symbol_str(param_sym)))
}

/// Returns the sort_by call node when `node` is a Numblock matching
/// `<recv>.sort_by { _1 }`.
fn match_identity_numblock(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let NodeKind::Numblock { send, max_n, body } = *cx.kind(node) else {
        return None;
    };

    if cx.method_name(send)? != "sort_by" {
        return None;
    }
    if !cx.call_arguments(send).is_empty() {
        return None;
    }
    if max_n != 1 {
        return None;
    }

    // Body must be `Lvar(_1)`.
    let body_id = body.get()?;
    let NodeKind::Lvar(sym) = *cx.kind(body_id) else {
        return None;
    };
    if cx.symbol_str(sym) != "_1" {
        return None;
    }

    Some(send)
}

/// Returns the sort_by call node when `node` is an Itblock matching
/// `<recv>.sort_by { it }`.
fn match_identity_itblock(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let NodeKind::Itblock { send, body } = *cx.kind(node) else {
        return None;
    };

    if cx.method_name(send)? != "sort_by" {
        return None;
    }
    if !cx.call_arguments(send).is_empty() {
        return None;
    }

    // Body must be `Lvar(it)`.
    let body_id = body.get()?;
    let NodeKind::Lvar(sym) = *cx.kind(body_id) else {
        return None;
    };
    if cx.symbol_str(sym) != "it" {
        return None;
    }

    Some(send)
}

// ---------------------------------------------------------------------------
// Offense / autocorrect range
// ---------------------------------------------------------------------------

/// Returns the range from the `sort_by` selector start to the end of the
/// block node (inclusive of `}` or `end`).
fn sort_by_to_block_end(sort_by_call: NodeId, block_node: NodeId, cx: &Cx<'_>) -> Range {
    let selector = cx.selector(sort_by_call);
    let block_end = cx.range(block_node).end;
    Range {
        start: selector.start,
        end: block_end,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::RedundantSortBy;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Block form -----

    #[test]
    fn flags_sort_by_identity_block() {
        test::<RedundantSortBy>().expect_offense(indoc! {"
            array.sort_by { |x| x }
                  ^^^^^^^^^^^^^^^^^ Use `sort` instead of `sort_by { |x| x }`.
        "});
    }

    #[test]
    fn corrects_sort_by_identity_block() {
        test::<RedundantSortBy>().expect_correction(
            indoc! {"
                array.sort_by { |x| x }
                      ^^^^^^^^^^^^^^^^^ Use `sort` instead of `sort_by { |x| x }`.
            "},
            "array.sort\n",
        );
    }

    #[test]
    fn flags_sort_by_identity_block_different_var() {
        test::<RedundantSortBy>().expect_offense(indoc! {"
            array.sort_by { |item| item }
                  ^^^^^^^^^^^^^^^^^^^^^^^ Use `sort` instead of `sort_by { |item| item }`.
        "});
    }

    #[test]
    fn corrects_sort_by_identity_block_different_var() {
        test::<RedundantSortBy>().expect_correction(
            indoc! {"
                array.sort_by { |item| item }
                      ^^^^^^^^^^^^^^^^^^^^^^^ Use `sort` instead of `sort_by { |item| item }`.
            "},
            "array.sort\n",
        );
    }

    // ----- Numblock form -----

    #[test]
    fn flags_sort_by_numblock() {
        test::<RedundantSortBy>().expect_offense(indoc! {"
            array.sort_by { _1 }
                  ^^^^^^^^^^^^^^ Use `sort` instead of `sort_by { _1 }`.
        "});
    }

    #[test]
    fn corrects_sort_by_numblock() {
        test::<RedundantSortBy>().expect_correction(
            indoc! {"
                array.sort_by { _1 }
                      ^^^^^^^^^^^^^^ Use `sort` instead of `sort_by { _1 }`.
            "},
            "array.sort\n",
        );
    }

    // ----- Itblock form -----

    #[test]
    fn flags_sort_by_itblock() {
        test::<RedundantSortBy>().expect_offense(indoc! {"
            array.sort_by { it }
                  ^^^^^^^^^^^^^^ Use `sort` instead of `sort_by { it }`.
        "});
    }

    #[test]
    fn corrects_sort_by_itblock() {
        test::<RedundantSortBy>().expect_correction(
            indoc! {"
                array.sort_by { it }
                      ^^^^^^^^^^^^^^ Use `sort` instead of `sort_by { it }`.
            "},
            "array.sort\n",
        );
    }

    // ----- Negative cases -----

    #[test]
    fn accepts_sort_by_with_transformation() {
        test::<RedundantSortBy>().expect_no_offenses("array.sort_by { |x| x.length }\n");
    }

    #[test]
    fn accepts_sort_by_with_different_var_in_body() {
        test::<RedundantSortBy>().expect_no_offenses("array.sort_by { |x| y }\n");
    }

    #[test]
    fn accepts_plain_sort() {
        test::<RedundantSortBy>().expect_no_offenses("array.sort\n");
    }

    #[test]
    fn accepts_sort_by_two_params() {
        test::<RedundantSortBy>().expect_no_offenses("array.sort_by { |x, y| x }\n");
    }

    #[test]
    fn accepts_numblock_not_identity() {
        test::<RedundantSortBy>().expect_no_offenses("array.sort_by { _1.length }\n");
    }

    #[test]
    fn accepts_sort_by_with_block_pass() {
        // sort_by(&:foo) is not an identity block form
        test::<RedundantSortBy>().expect_no_offenses("array.sort_by(&:foo)\n");
    }
}
murphy_plugin_api::submit_cop!(RedundantSortBy);
