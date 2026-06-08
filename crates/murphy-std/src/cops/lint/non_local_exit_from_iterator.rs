//! `Lint/NonLocalExitFromIterator` — flags `return` inside blocks that cause non-local exit.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/NonLocalExitFromIterator
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Ported from RuboCop Lint/NonLocalExitFromIterator.
//! ```
//!
//! ## Matched shapes
//! - `items.each { |x| return if ... }` — bare return inside blocks with args and chained send.
//! - `items.map { |x| return }` — bare return inside map/collect/select/reject/...
//! - `items&.each { |x| return }` — safe-navigation chained send.
//! - `items.each { return if _1 }` — numblock (numbered parameters).
//! - `items.each { return if it }` — itblock (implicit `it` parameter, Ruby 3.4).
//! - `items.each_with_index { |x, i| return }` — multiple block arguments.
//!
//! ## Why this shape
//!
//! `return` inside a block passed to an iterator method like `each`, `map`,
//! `select`, etc. causes a non-local exit from the enclosing method, not just
//! the block. This is almost always a bug — the developer likely meant `next`
//! or `break`. The cop mirrors RuboCop's logic: it flags bare `return` (no
//! value) inside blocks that (a) have arguments, (b) have a chained receiver
//! (method chain), and (c) are not inside a lambda, `def`, or
//! `define_method`/`define_singleton_method` scope.
//!
//! ## No autocorrect
//!
//! Whether to replace `return` with `next` or `break` depends on developer
//! intent and cannot be determined automatically.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const MSG: &str = "Non-local exit from iterator, without return value. \
    `next`, `break`, `Array#find`, `Array#any?`, etc. is preferred.";

#[derive(Default)]
pub struct NonLocalExitFromIterator;

#[cop(
    name = "Lint/NonLocalExitFromIterator",
    description = "Flags `return` inside blocks that causes non-local exit.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl NonLocalExitFromIterator {
    #[on_node(kind = "return")]
    fn check_return(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Return(value) = *cx.kind(node) else {
            return;
        };

        // Only flag bare `return` (no return value).
        if value.get().is_some() {
            return;
        }

        // Walk ancestors looking for enclosing blocks.
        for ancestor in cx.ancestors(node) {
            // Stop at scope boundaries — `return` is fine inside def/defs/lambda.
            if cx.is_any_def_type(ancestor) || cx.is_lambda(ancestor) {
                break;
            }

            if !cx.is_any_block_type(ancestor) {
                continue;
            }

            // `define_method` / `define_singleton_method` blocks allow return.
            if is_define_method_block(ancestor, cx) {
                break;
            }

            // Blocks without arguments are not iterator blocks.
            if !block_has_args(ancestor, cx) {
                continue;
            }

            // The block must be preceded by a method chain (has a receiver).
            if is_chained_send(ancestor, cx) {
                cx.emit_offense(return_keyword_range(node, cx), MSG, None);
                break;
            }
        }
    }
}

/// Extract the `return` keyword range (first 6 bytes of the return node).
fn return_keyword_range(node: NodeId, cx: &Cx<'_>) -> Range {
    let r = cx.range(node);
    // `return` is exactly 6 ASCII bytes.
    let keyword_end = (r.start + 6).min(r.end);
    Range {
        start: r.start,
        end: keyword_end,
    }
}

/// Returns `true` when the block has arguments (explicit for `Block`,
/// implicit for `Numblock` / `Itblock`).
fn block_has_args(block_id: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(block_id) {
        NodeKind::Block { args, .. } => {
            matches!(*cx.kind(args), NodeKind::Args(list) if !cx.list(list).is_empty())
        }
        NodeKind::Numblock { .. } | NodeKind::Itblock { .. } => true,
        _ => false,
    }
}

/// Returns `true` when the block's call has a receiver (method chain).
fn is_chained_send(block_id: NodeId, cx: &Cx<'_>) -> bool {
    let Some(call) = block_call(block_id, cx) else {
        return false;
    };
    match *cx.kind(call) {
        NodeKind::Send { receiver, .. } => receiver.get().is_some(),
        NodeKind::Csend { .. } => true, // always has a receiver
        _ => false,
    }
}

/// Returns `true` when the block's call is `define_method` or
/// `define_singleton_method`.
fn is_define_method_block(block_id: NodeId, cx: &Cx<'_>) -> bool {
    let Some(call) = block_call(block_id, cx) else {
        return false;
    };
    match *cx.kind(call) {
        NodeKind::Send { method, .. } | NodeKind::Csend { method, .. } => {
            let name = cx.symbol_str(method);
            name == "define_method" || name == "define_singleton_method"
        }
        _ => false,
    }
}

/// Extract the call node from a block (Block.call / Numblock.send / Itblock.send).
fn block_call(block_id: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    match *cx.kind(block_id) {
        NodeKind::Block { call, .. } => Some(call),
        NodeKind::Numblock { send, .. } | NodeKind::Itblock { send, .. } => Some(send),
        _ => None,
    }
}

murphy_plugin_api::submit_cop!(NonLocalExitFromIterator);

#[cfg(test)]
mod tests {
    use super::NonLocalExitFromIterator;
    use murphy_plugin_api::test_support::{indoc, test};

    // ── positive: single-arg block with chained send ─────────────────────

    #[test]
    fn flags_return_in_each_with_block_args() {
        test::<NonLocalExitFromIterator>().expect_offense(indoc! {r#"
            items.each do |item|
              return if item.stock == 0
              ^^^^^^ Non-local exit from iterator, without return value. `next`, `break`, `Array#find`, `Array#any?`, etc. is preferred.
              item.update!(foobar: true)
            end
        "#});
    }

    #[test]
    fn flags_return_with_safe_nav() {
        test::<NonLocalExitFromIterator>().expect_offense(indoc! {r#"
            items&.each do |item|
              return if item.stock == 0
              ^^^^^^ Non-local exit from iterator, without return value. `next`, `break`, `Array#find`, `Array#any?`, etc. is preferred.
              item.update!(foobar: true)
            end
        "#});
    }

    #[test]
    fn flags_return_in_numblock() {
        test::<NonLocalExitFromIterator>().expect_offense(indoc! {r#"
            items.each do
              return if _1.nil?
              ^^^^^^ Non-local exit from iterator, without return value. `next`, `break`, `Array#find`, `Array#any?`, etc. is preferred.
              _1.update!(foobar: true)
            end
        "#});
    }

    #[test]
    fn flags_return_in_itblock() {
        test::<NonLocalExitFromIterator>().expect_offense(indoc! {r#"
            items.each do
              return if it.nil?
              ^^^^^^ Non-local exit from iterator, without return value. `next`, `break`, `Array#find`, `Array#any?`, etc. is preferred.
              it.update!(foobar: true)
            end
        "#});
    }

    #[test]
    fn flags_return_with_multiple_block_args() {
        test::<NonLocalExitFromIterator>().expect_offense(indoc! {r#"
            items.each_with_index do |item, i|
              return if item.stock == 0
              ^^^^^^ Non-local exit from iterator, without return value. `next`, `break`, `Array#find`, `Array#any?`, etc. is preferred.
              item.update!(foobar: true)
            end
        "#});
    }

    // ── nested blocks ────────────────────────────────────────────────────

    #[test]
    fn flags_return_in_nested_block_with_chain() {
        test::<NonLocalExitFromIterator>().expect_offense(indoc! {r#"
            transaction do
              return unless update_necessary?
              items.each do |item|
                return if item.nil?
                ^^^^^^ Non-local exit from iterator, without return value. `next`, `break`, `Array#find`, `Array#any?`, etc. is preferred.
                item.with_lock do
                  return if item.stock == 0
                  ^^^^^^ Non-local exit from iterator, without return value. `next`, `break`, `Array#find`, `Array#any?`, etc. is preferred.
                  item.very_complicated_update_operation!
                end
              end
            end
        "#});
    }

    // ── negative: no block args ──────────────────────────────────────────

    #[test]
    fn allows_return_in_block_without_args() {
        test::<NonLocalExitFromIterator>().expect_no_offenses(indoc! {r#"
            item.with_lock do
              return if item.stock == 0
              item.update!(foobar: true)
            end
        "#});
    }

    // ── negative: no method chain ────────────────────────────────────────

    #[test]
    fn allows_return_without_method_chain() {
        test::<NonLocalExitFromIterator>().expect_no_offenses(indoc! {r#"
            transaction do
              return unless update_necessary?
              find_each do |item|
                return if item.stock == 0 # false-negative...
                item.update!(foobar: true)
              end
            end
        "#});
    }

    // ── negative: return with value ──────────────────────────────────────

    #[test]
    fn allows_return_with_value() {
        test::<NonLocalExitFromIterator>().expect_no_offenses(indoc! {r#"
            def find_first_sold_out_item(items)
              items.each do |item|
                return item if item.stock == 0
                item.foobar!
              end
            end
        "#});
    }

    // ── negative: lambda ─────────────────────────────────────────────────

    #[test]
    fn allows_return_in_lambda_block() {
        test::<NonLocalExitFromIterator>().expect_no_offenses(indoc! {r#"
            items.each(lambda do |item|
              return if item.stock == 0
              item.update!(foobar: true)
            end)
            items.each -> (item) {
              return if item.stock == 0
              item.update!(foobar: true)
            }
        "#});
    }

    #[test]
    fn allows_return_in_lambda_inside_block() {
        test::<NonLocalExitFromIterator>().expect_no_offenses(indoc! {r#"
            RSpec.configure do |config|
              if Gem.loaded_specs["paper_trail"].version < Gem::Version.new("4.0.0")
                current_behavior = ActiveSupport::Deprecation.behavior
                ActiveSupport::Deprecation.behavior = lambda do |message, callstack|
                  return if message =~ /foobar/
                  Array.wrap(current_behavior).each do |behavior|
                    behavior.call(message, callstack)
                  end
                end
              end
            end
        "#});
    }

    // ── negative: define_method ──────────────────────────────────────────

    #[test]
    fn allows_return_in_define_method() {
        test::<NonLocalExitFromIterator>().expect_no_offenses(indoc! {r#"
            [:method_one, :method_two].each do |method_name|
              define_method(method_name) do
                return if predicate?
              end
            end
        "#});
    }

    #[test]
    fn allows_return_in_define_singleton_method() {
        test::<NonLocalExitFromIterator>().expect_no_offenses(indoc! {r#"
            str = 'foo'
            str.define_singleton_method :bar do |baz|
              return unless baz
              replace baz
            end
        "#});
    }

    // ── negative: nested method def ─────────────────────────────────────

    #[test]
    fn allows_return_in_nested_instance_method_def() {
        test::<NonLocalExitFromIterator>().expect_no_offenses(indoc! {r#"
            Foo.configure do |c|
              def bar
                return if baz?
              end
            end
        "#});
    }

    #[test]
    fn allows_return_in_nested_class_method_def() {
        test::<NonLocalExitFromIterator>().expect_no_offenses(indoc! {r#"
            Foo.configure do |c|
              def self.bar
                return if baz?
              end
            end
        "#});
    }
}
