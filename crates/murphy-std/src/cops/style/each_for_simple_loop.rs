//! `Style/EachForSimpleLoop` — use `Integer#times` for simple fixed-count loops.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/EachForSimpleLoop
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detection: flags `(int..int).each {}` and `(int...int).each {}` when
//!   the block has no arguments (matches RuboCop's `node.arguments.empty?` guard).
//!   In Murphy the parenthesized range receiver is represented as
//!   `NodeKind::Unknown` (prism's ParenthesesNode). We match the receiver by
//!   inspecting the raw source text of the `Unknown` node to extract the integer
//!   endpoints. Bare unparenthesized range receivers (e.g. `0...10.each`) parse
//!   differently in Ruby and are not valid, so only parenthesized receivers occur
//!   in practice.
//!   Autocorrect: replaces `(range).each` selector span with `N.times`.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # offense — no args, any integer range
//! (1..5).each { }
//! (0...10).each {}
//!
//! # good
//! 5.times { }
//! 10.times {}
//!
//! # no offense — block has args (and range doesn't start at 0)
//! (1..5).each { |n| puts n }
//!
//! # offense — block has args but range starts at 0
//! (0..5).each { |n| puts n }
//! ```
//!
//! ## Autocorrect
//!
//! Replaces the send node `(range).each` with `N.times`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const MSG: &str =
    "Use `Integer#times` for a simple loop which iterates a fixed number of times.";

/// Stateless unit struct.
#[derive(Default)]
pub struct EachForSimpleLoop;

#[cop(
    name = "Style/EachForSimpleLoop",
    description = "Use `Integer#times` for a simple loop which iterates a fixed number of times.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl EachForSimpleLoop {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check_each_block(node, cx);
    }
}

fn check_each_block(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Block { call, args, .. } = *cx.kind(node) else {
        return;
    };

    // Must be a `.each` call with no send arguments.
    let NodeKind::Send { method, .. } = *cx.kind(call) else {
        return;
    };
    if cx.symbol_str(method) != "each" {
        return;
    }
    if !cx.call_arguments(call).is_empty() {
        return;
    }

    // Receiver must exist.
    let Some(recv) = cx.call_receiver(call).get() else {
        return;
    };

    // Extract range info from the receiver.
    let Some((exclusive, min, max)) = extract_int_range(recv, cx) else {
        return;
    };

    // Check block args.
    let NodeKind::Args(list) = *cx.kind(args) else {
        return;
    };
    let has_block_args = !cx.list(list).is_empty();

    // Only flag when block has no arguments.
    // RuboCop returns immediately if node.arguments is non-empty.
    if has_block_args {
        return;
    }

    // Compute the iteration count.
    let count = if exclusive { max - min } else { max - min + 1 };

    // Compute the range covering only `(range).each` (not the block body).
    // `cx.range(call)` for a block-attached send includes the block body,
    // so we use receiver.start..selector.end to get just the call part.
    let call_only_range = {
        let recv_start = cx.range(recv).start;
        let selector_end = cx.node(call).loc.name.end;
        Range { start: recv_start, end: selector_end }
    };

    // Offense on the `(range).each` call span.
    cx.emit_offense(call_only_range, MSG, None);

    // Autocorrect: replace `(range).each` with `count.times`.
    let replacement = format!("{}.times", count);
    cx.emit_edit(call_only_range, &replacement);
}

/// Extract `(exclusive, min, max)` from an integer range node.
///
/// Handles both:
/// - `NodeKind::RangeExpr` (bare unparenthesized range, rare in `.each` context)
/// - `NodeKind::Unknown` (parenthesized range like `(0...10)`)
fn extract_int_range(recv: NodeId, cx: &Cx<'_>) -> Option<(bool, i64, i64)> {
    match *cx.kind(recv) {
        NodeKind::RangeExpr {
            begin_,
            end_,
            exclusive,
        } => {
            let begin_node = begin_.get()?;
            let end_node = end_.get()?;
            let NodeKind::Int(min) = *cx.kind(begin_node) else {
                return None;
            };
            let NodeKind::Int(max) = *cx.kind(end_node) else {
                return None;
            };
            Some((exclusive, min, max))
        }
        NodeKind::Unknown => {
            // Parenthesized range: parse the raw source text.
            parse_parenthesized_range(recv, cx)
        }
        _ => None,
    }
}

/// Parse a parenthesized range from raw source text.
///
/// Matches patterns like `(N..M)` and `(N...M)` where N and M are integer
/// literals (with optional sign).
fn parse_parenthesized_range(node: NodeId, cx: &Cx<'_>) -> Option<(bool, i64, i64)> {
    let src = cx.raw_source(cx.range(node));
    let src = src.trim();

    // Must be wrapped in parentheses.
    let inner = src.strip_prefix('(')?.strip_suffix(')')?;

    // Find `...` or `..` operator.
    let (exclusive, idx) = if let Some(i) = inner.find("...") {
        (true, i)
    } else if let Some(i) = inner.find("..") {
        (false, i)
    } else {
        return None;
    };

    let lhs = inner[..idx].trim();
    let rhs = if exclusive {
        inner[idx + 3..].trim()
    } else {
        inner[idx + 2..].trim()
    };

    let min: i64 = lhs.parse().ok()?;
    let max: i64 = rhs.parse().ok()?;

    Some((exclusive, min, max))
}

#[cfg(test)]
mod tests {
    use super::EachForSimpleLoop;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Positive cases (offense) ------------------------------------

    #[test]
    fn flags_exclusive_range_no_args() {
        test::<EachForSimpleLoop>().expect_offense(indoc! {"
            (0...10).each {}
            ^^^^^^^^^^^^^ Use `Integer#times` for a simple loop which iterates a fixed number of times.
        "});
    }

    #[test]
    fn flags_inclusive_range_no_args() {
        test::<EachForSimpleLoop>().expect_offense(indoc! {"
            (1..5).each { }
            ^^^^^^^^^^^ Use `Integer#times` for a simple loop which iterates a fixed number of times.
        "});
    }

    #[test]
    fn accepts_zero_origin_range_with_block_args() {
        // RuboCop only flags when block has no args (node.arguments.empty?).
        test::<EachForSimpleLoop>().expect_no_offenses("(0..5).each { |n| puts n }\n");
    }

    // ----- Negative cases (no offense) --------------------------------

    #[test]
    fn accepts_non_zero_range_with_block_args() {
        test::<EachForSimpleLoop>().expect_no_offenses("(1..5).each { |n| puts n }\n");
    }

    #[test]
    fn accepts_times_already() {
        test::<EachForSimpleLoop>().expect_no_offenses("5.times {}\n");
    }

    #[test]
    fn accepts_each_without_integer_range() {
        test::<EachForSimpleLoop>().expect_no_offenses("[1, 2, 3].each {}\n");
    }

    // ----- Autocorrect -----------------------------------------------

    #[test]
    fn corrects_exclusive_range_to_times() {
        test::<EachForSimpleLoop>().expect_correction(
            indoc! {"
                (0...10).each {}
                ^^^^^^^^^^^^^ Use `Integer#times` for a simple loop which iterates a fixed number of times.
            "},
            "10.times {}\n",
        );
    }

    #[test]
    fn corrects_inclusive_range_to_times() {
        test::<EachForSimpleLoop>().expect_correction(
            indoc! {"
                (1..5).each { }
                ^^^^^^^^^^^ Use `Integer#times` for a simple loop which iterates a fixed number of times.
            "},
            "5.times { }\n",
        );
    }

    #[test]
    fn corrects_non_zero_exclusive_range_to_times() {
        test::<EachForSimpleLoop>().expect_correction(
            indoc! {"
                (3...7).each {}
                ^^^^^^^^^^^^ Use `Integer#times` for a simple loop which iterates a fixed number of times.
            "},
            "4.times {}\n",
        );
    }
}
murphy_plugin_api::submit_cop!(EachForSimpleLoop);
