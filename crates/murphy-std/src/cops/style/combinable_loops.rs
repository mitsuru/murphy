//! `Style/CombinableLoops` — combine consecutive loops over the same collection.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/CombinableLoops
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Flags consecutive looping blocks (`each`/`*_each`) or `for` loops over the
//!   same collection inside a `begin` body, mirroring RuboCop's
//!   `same_collection_looping_block?` / `same_collection_looping_for?`. Block
//!   detection covers all three block flavours (`block`/`numblock`/`itblock`)
//!   via dedicated handlers, matching RuboCop's `on_block`/`on_numblock`/
//!   `on_itblock` aliases. The looping call's method name, receiver, and
//!   arguments must match (compared by source text, since separate node
//!   instances of the same expression have distinct NodeIds).
//!
//!   Autocorrect ports RuboCop's `combine_with_left_sibling`: it removes the
//!   previous loop's closing delimiter, removes the current loop's header up to
//!   its body, and (for blocks only) appends the previous loop's closer (`}` or
//!   ` end`) unless the next sibling is itself a block. The block-parameter /
//!   `for`-variable equality gate matches RuboCop: an offense always fires, but
//!   the fix is skipped when the variable names differ. Single-pass autocorrect
//!   output (the parity oracle) is byte-identical to RuboCop's `-A` output,
//!   verified against rubocop 1.87.0 for brace blocks, `do…end` blocks, `for`
//!   loops, and 3+ chained loops. Under the CLI's multi-pass `--fix` fixpoint
//!   engine the final closer of a 3+ chain lands on its own line (a cosmetic,
//!   idempotent, still-valid-Ruby artifact of the shared fix engine, not this
//!   cop).
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, Range, SourceTokenKind, cop};

const MSG: &str = "Combine this loop with the previous loop.";

#[derive(Default)]
pub struct CombinableLoops;

#[cop(
    name = "Style/CombinableLoops",
    description = "Combine consecutive loops over the same collection.",
    default_severity = "warning",
    default_enabled = true,
    options = murphy_plugin_api::NoOptions
)]
impl CombinableLoops {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check_loop_block(node, cx);
    }

    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        check_loop_block(node, cx);
    }

    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        check_loop_block(node, cx);
    }

    #[on_node(kind = "for")]
    fn check_for(&self, node: NodeId, cx: &Cx<'_>) {
        if !parent_is_begin(node, cx) {
            return;
        }
        let Some(prev) = cx.left_sibling(node).get() else {
            return;
        };
        if !matches!(cx.kind(prev), NodeKind::For { .. }) {
            return;
        }
        // `same_collection_looping_for?`: collections must match by source.
        if !same_source(cx.for_collection(node), cx.for_collection(prev), cx) {
            return;
        }
        // Both loop bodies must exist.
        if cx.for_body(node).get().is_none() || cx.for_body(prev).get().is_none() {
            return;
        }

        cx.emit_offense(cx.range(node), MSG, None);

        // Autocorrect only when the iteration variables match, otherwise the
        // second body would reference an undefined variable.
        if same_source(cx.for_variable(node), cx.for_variable(prev), cx) {
            combine_with_left_sibling(node, prev, cx);
        }
    }
}

/// Shared body for `on_block`/`on_numblock`/`on_itblock`.
fn check_loop_block(node: NodeId, cx: &Cx<'_>) {
    if !parent_is_begin(node, cx) {
        return;
    }
    // `collection_looping_method?`
    let Some(method) = cx.method_name(node) else {
        return;
    };
    if !method.starts_with("each") && !method.ends_with("_each") {
        return;
    }
    let Some(prev) = cx.left_sibling(node).get() else {
        return;
    };
    // `same_collection_looping_block?`: sibling must be a block of any flavour.
    if !is_any_block(prev, cx) {
        return;
    }
    if cx.method_name(prev) != Some(method) {
        return;
    }
    // Receivers must match by source text.
    if !same_source(block_loop_receiver(node, cx), block_loop_receiver(prev, cx), cx) {
        return;
    }
    // Looping call arguments (e.g. `each_slice(2)`) must match by source text.
    if !same_arg_list(node, prev, cx) {
        return;
    }
    // Both loop bodies must exist.
    if cx.block_body(node).get().is_none() || cx.block_body(prev).get().is_none() {
        return;
    }

    cx.emit_offense(cx.range(node), MSG, None);

    // Autocorrect only when the block parameters match, otherwise it is
    // impossible to know which variable name should be prioritized.
    if same_block_params(node, prev, cx) {
        combine_with_left_sibling(node, prev, cx);
    }
}

/// Port of RuboCop's `combine_with_left_sibling`.
fn combine_with_left_sibling(node: NodeId, prev: NodeId, cx: &Cx<'_>) {
    // 1. Remove from the previous loop body's end to the previous loop's
    //    closing delimiter end — i.e. delete the previous loop's closer.
    let (Some(prev_body), Some(prev_close)) = (loop_body(prev, cx), loop_closer(prev, cx)) else {
        return;
    };
    cx.emit_edit(Range { start: prev_body.end, end: prev_close.end }, "");

    // 2. Remove from the current loop's start to its body's start — i.e.
    //    delete `items.each do |item|` (the loop header).
    let Some(cur_body) = loop_body(node, cx) else {
        return;
    };
    cx.emit_edit(Range { start: cx.range(node).start, end: cur_body.start }, "");

    // 3. `correct_end_of_block` — blocks only (a `for` loop has no `braces?`).
    if matches!(cx.kind(prev), NodeKind::For { .. }) {
        return;
    }
    // Skip when the next sibling is itself a block: the closer is supplied by
    // the *following* combine pass, so emitting one here corrupts the chain.
    if cx
        .right_sibling(node)
        .get()
        .is_some_and(|r| is_any_block(r, cx))
    {
        return;
    }
    let Some(cur_close) = loop_closer(node, cx) else {
        return;
    };
    // Remove the current block's own closer and append the previous block's
    // style of closer at the very end of the combined body.
    let closer = if prev_uses_braces(prev, cx) { "}" } else { " end" };
    cx.emit_edit(cur_close, "");
    let node_end = cx.range(node).end;
    cx.emit_edit(Range { start: node_end, end: node_end }, closer);
}

fn parent_is_begin(node: NodeId, cx: &Cx<'_>) -> bool {
    // Mirrors RuboCop's `node.parent&.begin_type?`: the parser `begin`
    // statement-sequence node specifically. `Kwbegin` (`begin … end`) is a
    // distinct kind here and is deliberately NOT matched, so matching
    // `NodeKind::Begin(_)` directly is correct (not a parenthesized-expression
    // unwrap site).
    cx.parent(node)
        .get()
        .is_some_and(|p| matches!(cx.kind(p), NodeKind::Begin(_)))
}

fn is_any_block(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        cx.kind(node),
        NodeKind::Block { .. } | NodeKind::Numblock { .. } | NodeKind::Itblock { .. }
    )
}

/// Compares two optional nodes by their source text. `None`/`None` (e.g. two
/// implicit receivers) is considered equal; `None`/`Some` is not.
fn same_source(
    a: murphy_plugin_api::OptNodeId,
    b: murphy_plugin_api::OptNodeId,
    cx: &Cx<'_>,
) -> bool {
    match (a.get(), b.get()) {
        (None, None) => true,
        (Some(a), Some(b)) => cx.raw_source(cx.range(a)) == cx.raw_source(cx.range(b)),
        _ => false,
    }
}

/// The receiver of a block's looping call (`items` in `items.each { … }`),
/// resolving through the block's call node. `call_receiver` does not delegate
/// through blocks, so the call must be resolved first.
fn block_loop_receiver(node: NodeId, cx: &Cx<'_>) -> murphy_plugin_api::OptNodeId {
    match cx.block_call(node).get() {
        Some(call) => cx.call_receiver(call),
        None => murphy_plugin_api::OptNodeId::NONE,
    }
}

/// Compares the two blocks' parameter lists (`node.arguments`) by source text.
/// The `Args` node's own range is not a reliable parameter span here, so the
/// individual parameter child nodes are compared instead.
fn same_block_params(node: NodeId, prev: NodeId, cx: &Cx<'_>) -> bool {
    let node_params = block_param_nodes(node, cx);
    let prev_params = block_param_nodes(prev, cx);
    if node_params.len() != prev_params.len() {
        return false;
    }
    node_params
        .iter()
        .zip(prev_params.iter())
        .all(|(&a, &b)| cx.raw_source(cx.range(a)) == cx.raw_source(cx.range(b)))
}

/// The parameter child nodes of a block's `Args` node, or an empty slice for a
/// `numblock`/`itblock` (which have no explicit `Args`).
fn block_param_nodes<'a>(node: NodeId, cx: &Cx<'a>) -> &'a [NodeId] {
    match cx.block_arguments(node).get() {
        Some(args) => match cx.kind(args) {
            NodeKind::Args(list) => cx.list(*list),
            _ => &[],
        },
        None => &[],
    }
}

/// Compares the argument lists of the two blocks' looping calls by source text.
fn same_arg_list(node: NodeId, prev: NodeId, cx: &Cx<'_>) -> bool {
    let (Some(node_call), Some(prev_call)) = (cx.block_call(node).get(), cx.block_call(prev).get())
    else {
        return false;
    };
    let node_args = cx.call_arguments(node_call);
    let prev_args = cx.call_arguments(prev_call);
    if node_args.len() != prev_args.len() {
        return false;
    }
    node_args
        .iter()
        .zip(prev_args.iter())
        .all(|(&a, &b)| cx.raw_source(cx.range(a)) == cx.raw_source(cx.range(b)))
}

/// The loop body of a block (any flavour) or a `for` loop.
fn loop_body(node: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let body = match cx.kind(node) {
        NodeKind::For { .. } => cx.for_body(node),
        _ => cx.block_body(node),
    };
    body.get().map(|b| cx.range(b))
}

/// The closing `end` / `}` token range of a loop node.
fn loop_closer(node: NodeId, cx: &Cx<'_>) -> Option<Range> {
    if prev_uses_braces(node, cx) {
        // Last `}` token within the node range.
        let node_range = cx.range(node);
        cx.tokens_in(node_range)
            .iter()
            .rev()
            .find(|t| t.kind == SourceTokenKind::RightBrace)
            .map(|t| t.range)
    } else {
        let end_kw = cx.loc(node).end_keyword();
        (end_kw != Range::ZERO).then_some(end_kw)
    }
}

/// Whether the loop is a brace block (`{ … }`). `do … end` blocks and `for`
/// loops use the `end` keyword. RuboCop's `braces?` is only defined on blocks;
/// `for` loops always use `end`.
fn prev_uses_braces(node: NodeId, cx: &Cx<'_>) -> bool {
    if matches!(cx.kind(node), NodeKind::For { .. }) {
        return false;
    }
    let node_range = cx.range(node);
    cx.tokens_in(node_range)
        .iter()
        .rev()
        .find(|t| {
            matches!(t.kind, SourceTokenKind::RightBrace)
                || (t.kind == SourceTokenKind::Other && cx.raw_source(t.range) == "end")
        })
        .is_some_and(|t| t.kind == SourceTokenKind::RightBrace)
}

#[cfg(test)]
mod tests {
    use super::CombinableLoops;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_combinable_each_blocks() {
        test::<CombinableLoops>().expect_offense(indoc! {"
            def method
              items.each { |item| do_something(item) }
              items.each { |item| do_something_else(item) }
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Combine this loop with the previous loop.
            end
        "});
    }

    #[test]
    fn flags_combinable_implicit_receiver_each_blocks() {
        test::<CombinableLoops>().expect_offense(indoc! {"
            def method
              each { |item| do_something(item) }
              each { |item| do_something_else(item) }
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Combine this loop with the previous loop.
            end
        "});
    }

    #[test]
    fn flags_combinable_for_loops() {
        test::<CombinableLoops>().expect_offense(indoc! {"
            def method
              for item in items do do_something(item) end
              for item in items do do_something_else(item) end
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Combine this loop with the previous loop.
            end
        "});
    }

    #[test]
    fn flags_even_when_block_args_differ() {
        // Offense fires regardless of block-param mismatch (only the
        // autocorrect is gated on matching params).
        test::<CombinableLoops>().expect_offense(indoc! {"
            def method
              items.each { |a| do_something(a) }
              items.each { |b| do_something_else(b) }
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Combine this loop with the previous loop.
            end
        "});
    }

    #[test]
    fn accepts_single_each() {
        test::<CombinableLoops>()
            .expect_no_offenses("items.each { |item| do_something(item) }\n");
    }

    #[test]
    fn accepts_different_collections() {
        test::<CombinableLoops>()
            .expect_no_offenses("items.each { |i| f(i) }\nother.each { |i| g(i) }\n");
    }

    #[test]
    fn accepts_different_looping_call_arguments() {
        // RuboCop's good example: `each_slice` with different args.
        test::<CombinableLoops>().expect_no_offenses(indoc! {"
            def method
              each_slice(2) { |slice| do_something(slice) }
              each_slice(3) { |slice| do_something(slice) }
            end
        "});
    }

    #[test]
    fn accepts_same_looping_call_arguments() {
        // Same args → still combinable, so this DOES flag.
        test::<CombinableLoops>().expect_offense(indoc! {"
            def method
              each_slice(2) { |slice| do_something(slice) }
              each_slice(2) { |slice| do_something_else(slice) }
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Combine this loop with the previous loop.
            end
        "});
    }

    #[test]
    fn accepts_different_for_variables_still_flags() {
        // Different `for` variables → offense fires, autocorrect skipped.
        test::<CombinableLoops>().expect_offense(indoc! {"
            def method
              for a in items do do_something(a) end
              for b in items do do_something_else(b) end
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Combine this loop with the previous loop.
            end
        "});
    }

    #[test]
    fn accepts_non_looping_methods() {
        test::<CombinableLoops>().expect_no_offenses(indoc! {"
            def method
              items.map { |i| f(i) }
              items.map { |i| g(i) }
            end
        "});
    }

    #[test]
    fn corrects_brace_blocks() {
        test::<CombinableLoops>().expect_correction(
            indoc! {"
                def method
                  items.each { |item| do_something(item) }
                  items.each { |item| do_something_else(item) }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Combine this loop with the previous loop.
                end
            "},
            indoc! {"
                def method
                  items.each { |item| do_something(item)
                  do_something_else(item) }
                end
            "},
        );
    }

    #[test]
    fn corrects_do_end_blocks() {
        test::<CombinableLoops>().expect_correction(
            indoc! {"
                def method
                  items.each do |item| do_something(item) end
                  items.each do |item| do_something_else(item) end
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Combine this loop with the previous loop.
                end
            "},
            indoc! {"
                def method
                  items.each do |item| do_something(item)
                  do_something_else(item)  end
                end
            "},
        );
    }

    #[test]
    fn corrects_for_loops() {
        test::<CombinableLoops>().expect_correction(
            indoc! {"
                def method
                  for item in items do do_something(item) end
                  for item in items do do_something_else(item) end
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Combine this loop with the previous loop.
                end
            "},
            indoc! {"
                def method
                  for item in items do do_something(item)
                  do_something_else(item) end
                end
            "},
        );
    }

    #[test]
    fn does_not_correct_when_block_args_differ() {
        test::<CombinableLoops>().expect_correction(
            indoc! {"
                def method
                  items.each { |a| do_something(a) }
                  items.each { |b| do_something_else(b) }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Combine this loop with the previous loop.
                end
            "},
            indoc! {"
                def method
                  items.each { |a| do_something(a) }
                  items.each { |b| do_something_else(b) }
                end
            "},
        );
    }

    #[test]
    fn corrects_three_consecutive_loops() {
        // Each of the 2nd and 3rd loops combines with its predecessor; the
        // `right_sibling` guard ensures only the final block re-emits the closer
        // so the single-pass application yields one combined loop.
        test::<CombinableLoops>().expect_correction(
            indoc! {"
                def method
                  items.each { |item| a(item) }
                  items.each { |item| b(item) }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Combine this loop with the previous loop.
                  items.each { |item| c(item) }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Combine this loop with the previous loop.
                end
            "},
            indoc! {"
                def method
                  items.each { |item| a(item)
                  b(item)
                  c(item) }
                end
            "},
        );
    }

    #[test]
    fn flags_and_corrects_numblock() {
        // `on_numblock` parity: `_1`-style blocks have no explicit params, so the
        // (empty) param lists match and the fix fires.
        test::<CombinableLoops>().expect_correction(
            indoc! {"
                def method
                  items.each { do_something(_1) }
                  items.each { do_something_else(_1) }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Combine this loop with the previous loop.
                end
            "},
            indoc! {"
                def method
                  items.each { do_something(_1)
                  do_something_else(_1) }
                end
            "},
        );
    }
}
murphy_plugin_api::submit_cop!(CombinableLoops);
