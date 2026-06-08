//! `Lint/NextWithoutAccumulator` — flags `next` without value in `reduce`/`inject` blocks.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/NextWithoutAccumulator
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Ported from RuboCop Lint/NextWithoutAccumulator.
//! ```
//!
//! ## Matched shapes
//! - `[1,2,3].reduce(0) { |acc, n| next if n.even? }` — next without accumulator value
//! - `[1,2,3].inject { |acc, n| next if n.odd? }` — next without value in inject
//! - `[1,2,3]&.reduce(0) { |acc, n| next if n.even? }` — safe-navigation chained send
//! - `[1,2,3].reduce(0) { next if _2.odd? }` — numblock (numbered parameters)
//! - `[1,2,3].reduce(0) { next if it.odd? }` — itblock (implicit `it` parameter)
//!
//! ## No autocorrect
//!
//! There is no safe mechanical rewrite: the fix depends on developer intent
//! for what accumulator value should be passed to `next`.

use murphy_plugin_api::{Cx, NodeId, NodeKind, NoOptions, Range, cop};

const MSG: &str = "Use `next` with an accumulator argument in a `reduce`.";

#[derive(Default)]
pub struct NextWithoutAccumulator;

#[cop(
    name = "Lint/NextWithoutAccumulator",
    description = "Flags `next` without value inside `reduce`/`inject` blocks.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl NextWithoutAccumulator {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check_reduce_block(node, cx);
    }

    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        check_reduce_block(node, cx);
    }

    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        check_reduce_block(node, cx);
    }
}

fn check_reduce_block(block_id: NodeId, cx: &Cx<'_>) {
    let method = cx.method_name(block_id);
    if method != Some("reduce") && method != Some("inject") {
        return;
    }

    // Exclude reduce(:+) style calls (symbol-to-proc).
    if has_symbol_arg(block_id, cx) {
        return;
    }

    let Some(body_id) = block_body(block_id, cx) else {
        return;
    };

    find_void_next(body_id, block_id, cx);
}

/// Returns `true` when the block's call has a `Sym` as its first argument.
fn has_symbol_arg(block_id: NodeId, cx: &Cx<'_>) -> bool {
    let Some(call_id) = block_call(block_id, cx) else {
        return false;
    };
    match *cx.kind(call_id) {
        NodeKind::Send { args, .. } | NodeKind::Csend { args, .. } => {
            let args_list = cx.list(args);
            if args_list.is_empty() {
                return false;
            }
            matches!(*cx.kind(args_list[0]), NodeKind::Sym(_))
        }
        _ => false,
    }
}

/// Extract the call node from a block.
fn block_call(block_id: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    match *cx.kind(block_id) {
        NodeKind::Block { call, .. } => Some(call),
        NodeKind::Numblock { send, .. } | NodeKind::Itblock { send, .. } => Some(send),
        _ => None,
    }
}

/// Extract the body node from a block.
fn block_body(block_id: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    match *cx.kind(block_id) {
        NodeKind::Block { body, .. } => body.get(),
        NodeKind::Numblock { body, .. } => body.get(),
        NodeKind::Itblock { body, .. } => body.get(),
        _ => None,
    }
}

/// Walk the block body looking for the first bare `next` (no value).
fn find_void_next(body_id: NodeId, block_id: NodeId, cx: &Cx<'_>) {
    // Check the body node itself (e.g. single-statement block).
    if check_node(body_id, block_id, cx) {
        return;
    }
    // Check all descendants.
    for &d in cx.descendants(body_id).iter() {
        if check_node(d, block_id, cx) {
            return;
        }
    }
}

/// Emit an offense for `node` if it is a bare `next` directly inside the reduce block.
fn check_node(node: NodeId, block_id: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Next(val) = *cx.kind(node) else {
        return false;
    };
    // Only bare `next` (no value passed to accumulate).
    if val.get().is_some() {
        return false;
    }
    // Only flag if directly inside the reduce block, not inside a nested block.
    if inside_inner_block(node, block_id, cx) {
        return false;
    }
    cx.emit_offense(next_keyword_range(node, cx), MSG, None);
    true
}

/// Returns `true` when `node` is inside an inner block (not the target reduce block).
fn inside_inner_block(node: NodeId, outer_block: NodeId, cx: &Cx<'_>) -> bool {
    for ancestor in cx.ancestors(node) {
        if ancestor == outer_block {
            return false;
        }
        if matches!(
            *cx.kind(ancestor),
            NodeKind::Block { .. }
                | NodeKind::Numblock { .. }
                | NodeKind::Itblock { .. }
        ) {
            return true;
        }
    }
    false
}

/// Extract the `next` keyword range (first 4 bytes of the node).
fn next_keyword_range(node: NodeId, cx: &Cx<'_>) -> Range {
    let r = cx.range(node);
    let keyword_end = (r.start + 4).min(r.end);
    Range {
        start: r.start,
        end: keyword_end,
    }
}

murphy_plugin_api::submit_cop!(NextWithoutAccumulator);

#[cfg(test)]
mod tests {
    use super::NextWithoutAccumulator;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_bare_next_in_reduce() {
        test::<NextWithoutAccumulator>().expect_offense(indoc! {r#"
            (1..4).reduce(0) do |acc, i|
              next if i.odd?
              ^^^^ Use `next` with an accumulator argument in a `reduce`.
              acc + i
            end
        "#});
    }

    #[test]
    fn flags_bare_next_in_inject() {
        test::<NextWithoutAccumulator>().expect_offense(indoc! {r#"
            (1..4).inject(0) do |acc, i|
              next if i.odd?
              ^^^^ Use `next` with an accumulator argument in a `reduce`.
              acc + i
            end
        "#});
    }

    #[test]
    fn flags_bare_next_in_reduce_with_safe_navigation() {
        test::<NextWithoutAccumulator>().expect_offense(indoc! {r#"
            (1..4)&.reduce(0) do |acc, i|
              next if i.odd?
              ^^^^ Use `next` with an accumulator argument in a `reduce`.
              acc + i
            end
        "#});
    }

    #[test]
    fn accepts_next_with_value() {
        test::<NextWithoutAccumulator>().expect_no_offenses(indoc! {"
            (1..4).reduce(0) do |acc, i|
              next acc if i.odd?
              acc + i
            end
        "});
    }

    #[test]
    fn accepts_next_in_nested_block() {
        test::<NextWithoutAccumulator>().expect_no_offenses(indoc! {"
            [(1..3), (4..6)].reduce(0) do |acc, elems|
              elems.each_with_index do |elem, i|
                next if i == 1
                acc << elem
              end
              acc
            end
        "});
    }

    #[test]
    fn flags_bare_next_in_numblock() {
        test::<NextWithoutAccumulator>().expect_offense(indoc! {r#"
            (1..4).reduce(0) do
              next if _2.odd?
              ^^^^ Use `next` with an accumulator argument in a `reduce`.
              _1 + i
            end
        "#});
    }

    #[test]
    fn flags_bare_next_in_numblock_with_safe_navigation() {
        test::<NextWithoutAccumulator>().expect_offense(indoc! {r#"
            (1..4)&.reduce(0) do
              next if _2.odd?
              ^^^^ Use `next` with an accumulator argument in a `reduce`.
              _1 + i
            end
        "#});
    }

    #[test]
    fn flags_bare_next_in_itblock() {
        test::<NextWithoutAccumulator>().expect_offense(indoc! {r#"
            (1..4).reduce(0) do
              next if it.odd?
              ^^^^ Use `next` with an accumulator argument in a `reduce`.
              it + 1
            end
        "#});
    }

    #[test]
    fn accepts_bare_next_in_unrelated_block() {
        test::<NextWithoutAccumulator>().expect_no_offenses(indoc! {"
            (1..4).foo(0) do |acc, i|
              next if i.odd?
              acc + i
            end
        "});
    }

    #[test]
    fn accepts_next_with_value_in_unrelated_block() {
        test::<NextWithoutAccumulator>().expect_no_offenses(indoc! {"
            (1..4).foo(0) do |acc, i|
              next acc if i.odd?
              acc + i
            end
        "});
    }
}
