//! `Style/SlicingWithRange` — flags redundant or inefficient array slicing ranges.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/SlicingWithRange
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Three cases are implemented:
//!   1. Useless range: `ary[0..-1]` or `ary[0..]` → remove `[...]` entirely.
//!   2. Endless range: `ary[n..-1]` → remove just the `-1` end, producing `ary[n..]`.
//!   3. Beginless range: `ary[nil..n]` (explicit nil begin) → remove the nil,
//!      producing `ary[..n]`.
//!   Already-optimal forms (`ary[n..]`, `ary[..n]`) are not flagged.
//!   Gated at minimum_target_ruby_version = "2.7" (beginless-range syntax
//!   requires Ruby >= 2.7).
//!   This cop is marked Safe: false upstream because `x..-1` and `x..` are only
//!   equivalent for Array/String; Murphy emits the autocorrect regardless, matching
//!   the upstream behaviour under the default settings.
//!   Safe-navigation `ary&.[](range)` (csend) is also checked, mirroring
//!   RuboCop's `alias on_csend on_send`.
//!   The offense range covers the bracket-and-argument portion (`[range]`),
//!   matching the RuboCop offense range for the bracket-notation form.
//! ```
//!
//! ## Matched shapes
//!
//! `Send` (or `Csend`) nodes with method `[]` and exactly one argument that is
//! a `RangeExpr`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct SlicingWithRange;

#[cop(
    name = "Style/SlicingWithRange",
    description = "Checks array slicing is done with redundant, endless, and beginless ranges when suitable.",
    default_severity = "warning",
    default_enabled = true,
    minimum_target_ruby_version = "2.7",
    options = NoOptions,
)]
impl SlicingWithRange {
    #[on_node(kind = "send", methods = ["[]"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        if cx.method_name(node) == Some("[]") {
            check(node, cx);
        }
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Extract receiver and args regardless of Send/Csend.
    let (receiver_id, args) = match cx.kind(node) {
        NodeKind::Send { receiver, args, .. } => (receiver.get(), *args),
        NodeKind::Csend { receiver, args, .. } => (Some(*receiver), *args),
        _ => return,
    };

    // Must have exactly one argument (the range).
    let arg_list = cx.list(args);
    if arg_list.len() != 1 {
        return;
    }
    let range_node = arg_list[0];

    let NodeKind::RangeExpr {
        begin_,
        end_,
        exclusive,
    } = *cx.kind(range_node)
    else {
        return;
    };

    // Compute the bracket region: from end of receiver to end of node.
    // This is the `[range]` portion we will report on and edit.
    let node_range = cx.range(node);
    let bracket_region = if let Some(recv_id) = receiver_id {
        Range {
            start: cx.range(recv_id).end,
            end: node_range.end,
        }
    } else {
        node_range
    };

    // Pattern 1 (highest precedence): useless range.
    // `ary[0..-1]`, `ary[0..]`, `ary[0...]` — begin=0, end=-1 or absent.
    if is_zero(begin_, cx) && (is_minus_one(end_, cx) || end_.get().is_none()) {
        let bracket_src = cx.raw_source(bracket_region);
        let msg = format!("Remove the useless `{bracket_src}`.");
        cx.emit_offense(bracket_region, &msg, None);
        cx.emit_edit(bracket_region, "");
        return;
    }

    // Pattern 2: endless range opportunity.
    // `ary[n..-1]` where begin is present and not a nil literal, end is -1.
    // Already-endless (`ary[n..]`) is not flagged (end absent).
    if let Some(begin_id) = begin_.get() {
        if !is_nil_literal(begin_id, cx) && is_minus_one(end_, cx) {
            let begin_src = cx.raw_source(cx.range(begin_id));
            let op = if exclusive { "..." } else { ".." };
            let prefer_bracket = format!("[{begin_src}{op}]");
            let current_bracket = cx.raw_source(bracket_region);
            let msg = format!("Prefer `{prefer_bracket}` over `{current_bracket}`.");
            cx.emit_offense(bracket_region, &msg, None);
            // Autocorrect: remove the `-1` end node.
            let end_id = end_.get().unwrap();
            cx.emit_edit(cx.range(end_id), "");
            return;
        }
    }

    // Pattern 3: beginless range with explicit nil begin.
    // `ary[nil..n]` — begin is a nil literal node, end is present.
    // The form `ary[..n]` (begin absent) is already optimal and not flagged.
    if let Some(begin_id) = begin_.get() {
        if is_nil_literal(begin_id, cx) {
            if let Some(end_id) = end_.get() {
                let end_src = cx.raw_source(cx.range(end_id));
                let op = if exclusive { "..." } else { ".." };
                let prefer_bracket = format!("[{op}{end_src}]");
                let current_bracket = cx.raw_source(bracket_region);
                let msg = format!("Prefer `{prefer_bracket}` over `{current_bracket}`.");
                cx.emit_offense(bracket_region, &msg, None);
                // Autocorrect: remove the nil begin node.
                cx.emit_edit(cx.range(begin_id), "");
            }
        }
    }
}

/// Returns `true` if `opt` holds an integer literal `0`.
fn is_zero(opt: OptNodeId, cx: &Cx<'_>) -> bool {
    let Some(id) = opt.get() else {
        return false;
    };
    matches!(cx.kind(id), NodeKind::Int(n) if *n == 0)
}

/// Returns `true` if `opt` holds an integer literal `-1`.
fn is_minus_one(opt: OptNodeId, cx: &Cx<'_>) -> bool {
    let Some(id) = opt.get() else {
        return false;
    };
    matches!(cx.kind(id), NodeKind::Int(n) if *n == -1)
}

/// Returns `true` if `id` is a `nil` literal node.
fn is_nil_literal(id: NodeId, cx: &Cx<'_>) -> bool {
    matches!(cx.kind(id), NodeKind::Nil)
}

#[cfg(test)]
mod tests {
    use super::SlicingWithRange;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- useless range (begin=0, end=-1 or absent) ---

    #[test]
    fn flags_zero_to_minus_one() {
        test::<SlicingWithRange>().expect_correction(
            indoc! {r#"
                ary[0..-1]
                   ^^^^^^^ Remove the useless `[0..-1]`.
            "#},
            "ary\n",
        );
    }

    #[test]
    fn flags_zero_to_endless() {
        test::<SlicingWithRange>().expect_correction(
            indoc! {r#"
                ary[0..]
                   ^^^^^ Remove the useless `[0..]`.
            "#},
            "ary\n",
        );
    }

    #[test]
    fn flags_zero_to_exclusive_endless() {
        test::<SlicingWithRange>().expect_correction(
            indoc! {r#"
                ary[0...]
                   ^^^^^^ Remove the useless `[0...]`.
            "#},
            "ary\n",
        );
    }

    // --- endless range opportunity (begin=n non-zero/non-nil, end=-1) ---

    #[test]
    fn flags_partial_to_minus_one() {
        test::<SlicingWithRange>().expect_correction(
            indoc! {r#"
                ary[1..-1]
                   ^^^^^^^ Prefer `[1..]` over `[1..-1]`.
            "#},
            "ary[1..]\n",
        );
    }

    #[test]
    fn flags_partial_exclusive_to_minus_one() {
        test::<SlicingWithRange>().expect_correction(
            indoc! {r#"
                ary[2...-1]
                   ^^^^^^^^ Prefer `[2...]` over `[2...-1]`.
            "#},
            "ary[2...]\n",
        );
    }

    // --- beginless range (begin=nil literal, end=present) ---

    #[test]
    fn flags_nil_begin() {
        test::<SlicingWithRange>().expect_correction(
            indoc! {r#"
                ary[nil..2]
                   ^^^^^^^^ Prefer `[..2]` over `[nil..2]`.
            "#},
            "ary[..2]\n",
        );
    }

    // --- no offense cases ---

    #[test]
    fn accepts_already_endless() {
        test::<SlicingWithRange>().expect_no_offenses("ary[1..]\n");
    }

    #[test]
    fn accepts_already_beginless() {
        test::<SlicingWithRange>().expect_no_offenses("ary[..2]\n");
    }

    #[test]
    fn accepts_normal_range() {
        test::<SlicingWithRange>().expect_no_offenses("ary[1..3]\n");
    }

    #[test]
    fn accepts_integer_index() {
        test::<SlicingWithRange>().expect_no_offenses("ary[0]\n");
    }

    #[test]
    fn accepts_two_arg_slice() {
        test::<SlicingWithRange>().expect_no_offenses("ary[0, 2]\n");
    }

    #[test]
    fn minimum_target_ruby_version_is_set() {
        use murphy_plugin_api::{Cop, RubyVersion};
        assert_eq!(
            <SlicingWithRange as Cop>::MINIMUM_TARGET_RUBY_VERSION,
            Some(RubyVersion::new(2, 7)),
        );
    }
}

murphy_plugin_api::submit_cop!(SlicingWithRange);
