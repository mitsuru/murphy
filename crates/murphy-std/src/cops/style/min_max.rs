//! `Style/MinMax` — use `minmax` instead of separate `min` and `max` calls.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/MinMax
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detects `[recv.min, recv.max]` and `return recv.min, recv.max` patterns
//!   and suggests using `recv.minmax` instead.
//!
//!   For the `return` form, prism parses `return a, b` as
//!   `Return(Array([a, b]))`. The offense range covers the two arguments
//!   (from the first argument to the last), matching RuboCop's `argument_range`.
//!
//!   The `array` handler skips arrays whose parent is a `return` node, to avoid
//!   double-reporting with the `return` handler.
//!
//!   Covered patterns:
//!     - `[recv.min, recv.max]` array literals (offense on the whole array)
//!     - `return recv.min, recv.max` (offense on the argument span)
//!   Both forms carry an autocorrect.
//!
//!   Gap: The receiver must be a simple receiver (non-nil). Chained calls
//!   like `(a + b).min` are supported as long as both sides share the same
//!   receiver source text.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! bar = [foo.min, foo.max]
//! return foo.min, foo.max
//!
//! # good
//! bar = foo.minmax
//! return foo.minmax
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct MinMax;

#[cop(
    name = "Style/MinMax",
    description = "Use `minmax` instead of `min` and `max` in conjunction.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl MinMax {
    #[on_node(kind = "array")]
    fn check_array(&self, node: NodeId, cx: &Cx<'_>) {
        // Skip if this array is the direct value of a `return` node — the
        // `return` handler covers that case to get the correct offense range.
        if let Some(parent) = cx.parent(node).get()
            && matches!(cx.kind(parent), NodeKind::Return(_)) {
                return;
            }
        check_array_node(node, cx);
    }

    #[on_node(kind = "return")]
    fn check_return(&self, node: NodeId, cx: &Cx<'_>) {
        check_return_node(node, cx);
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// For a `Send` node, return `(receiver_id, "min" | "max")`, or `None`.
fn send_min_or_max(id: NodeId, cx: &Cx<'_>) -> Option<(NodeId, &'static str)> {
    let recv = cx.call_receiver(id).get()?;
    if !cx.call_arguments(id).is_empty() {
        return None;
    }
    match cx.method_name(id)? {
        "min" => Some((recv, "min")),
        "max" => Some((recv, "max")),
        _ => None,
    }
}

/// Check a two-element array node for `[recv.min, recv.max]`.
/// Returns `(recv1_id, [elem0, elem1])` if it matches, or `None`.
fn match_min_max_array(array_node: NodeId, cx: &Cx<'_>) -> Option<(NodeId, [NodeId; 2])> {
    let NodeKind::Array(elems_list) = *cx.kind(array_node) else {
        return None;
    };
    let elems = cx.list(elems_list);
    if elems.len() != 2 {
        return None;
    }
    let (recv1, method1) = send_min_or_max(elems[0], cx)?;
    let (recv2, method2) = send_min_or_max(elems[1], cx)?;

    // Must be one `min` and one `max`, in that order.
    if method1 != "min" || method2 != "max" {
        return None;
    }

    // Both receivers must have the same source text.
    let recv1_src = cx.raw_source(cx.range(recv1));
    let recv2_src = cx.raw_source(cx.range(recv2));
    if recv1_src != recv2_src {
        return None;
    }

    Some((recv1, [elems[0], elems[1]]))
}

/// Offense range for an array node: the full array literal.
fn check_array_node(node: NodeId, cx: &Cx<'_>) {
    let Some((recv1, _)) = match_min_max_array(node, cx) else {
        return;
    };

    let offense_range = cx.range(node);
    let receiver_src = cx.raw_source(cx.range(recv1));
    let offender_src = cx.raw_source(offense_range);
    let msg = format!("Use `{receiver_src}.minmax` instead of `{offender_src}`.");

    cx.emit_offense(offense_range, &msg, None);
    // Autocorrect: replace entire array with `recv.minmax`
    let replacement = format!("{}.minmax", receiver_src);
    cx.emit_edit(offense_range, &replacement);
}

/// For a `return` node, check if its value is `[recv.min, recv.max]`.
fn check_return_node(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Return(value) = *cx.kind(node) else {
        return;
    };
    let Some(val_id) = value.get() else {
        return;
    };
    // `return a, b` parses as Return(Array([a, b])) in prism.
    let Some((recv1, elems)) = match_min_max_array(val_id, cx) else {
        return;
    };

    // Offense range: from start of first arg to end of last arg
    // (matches RuboCop's `argument_range`).
    let offense_range = Range {
        start: cx.range(elems[0]).start,
        end: cx.range(elems[1]).end,
    };
    let receiver_src = cx.raw_source(cx.range(recv1));
    let offender_src = cx.raw_source(offense_range);
    let msg = format!("Use `{receiver_src}.minmax` instead of `{offender_src}`.");

    cx.emit_offense(offense_range, &msg, None);
    // Autocorrect: replace the argument span with `recv.minmax`
    let replacement = format!("{}.minmax", receiver_src);
    cx.emit_edit(offense_range, &replacement);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::MinMax;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_array_min_max() {
        test::<MinMax>().expect_offense(indoc! {"
            bar = [foo.min, foo.max]
                  ^^^^^^^^^^^^^^^^^^ Use `foo.minmax` instead of `[foo.min, foo.max]`.
        "});
    }

    #[test]
    fn corrects_array_min_max() {
        test::<MinMax>().expect_correction(
            indoc! {"
                bar = [foo.min, foo.max]
                      ^^^^^^^^^^^^^^^^^^ Use `foo.minmax` instead of `[foo.min, foo.max]`.
            "},
            "bar = foo.minmax\n",
        );
    }

    #[test]
    fn flags_return_min_max() {
        test::<MinMax>().expect_offense(indoc! {"
            def x
              return foo.min, foo.max
                     ^^^^^^^^^^^^^^^^ Use `foo.minmax` instead of `foo.min, foo.max`.
            end
        "});
    }

    #[test]
    fn corrects_return_min_max() {
        test::<MinMax>().expect_correction(
            indoc! {"
                def x
                  return foo.min, foo.max
                         ^^^^^^^^^^^^^^^^ Use `foo.minmax` instead of `foo.min, foo.max`.
                end
            "},
            "def x\n  return foo.minmax\nend\n",
        );
    }

    #[test]
    fn accepts_already_minmax() {
        test::<MinMax>().expect_no_offenses("foo.minmax\n");
    }

    #[test]
    fn accepts_different_receivers() {
        test::<MinMax>().expect_no_offenses("[foo.min, bar.max]\n");
    }

    #[test]
    fn accepts_max_before_min() {
        // reversed order — not the pattern
        test::<MinMax>().expect_no_offenses("[foo.max, foo.min]\n");
    }

    #[test]
    fn accepts_single_element_array() {
        test::<MinMax>().expect_no_offenses("[foo.min]\n");
    }

    #[test]
    fn accepts_min_with_args() {
        test::<MinMax>().expect_no_offenses("[foo.min(1), foo.max]\n");
    }

    #[test]
    fn accepts_max_with_args() {
        test::<MinMax>().expect_no_offenses("[foo.min, foo.max(1)]\n");
    }

    #[test]
    fn accepts_three_element_array() {
        test::<MinMax>().expect_no_offenses("[foo.min, foo.max, foo.sum]\n");
    }
}
murphy_plugin_api::submit_cop!(MinMax);
