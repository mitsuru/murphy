//! `Style/ArrayIntersect` — use `Array#intersect?` instead of intersection
//! checks.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ArrayIntersect
//! upstream_version_checked: 1.86.2
//! version_added: "1.40"
//! safe: false
//! supports_autocorrect: true
//! status: partial
//! gap_issues:
//!   - murphy-m3sf
//! notes: >
//!   Two of the three RuboCop shapes are implemented with full autocorrect:
//!
//!   1. array1.intersection(array2).<predicate> — predicates (any?, empty?,
//!      none?) plus count/size/length comparisons (count > 0,
//!      count.positive?, count != 0, count == 0, count.zero?).
//!   2. array1.any? { |e| array2.member?(e) } — also none?, numblock,
//!      and itblock forms.
//!
//!   Not implemented: the (array1 & array2).any? paren-& family.
//!   prism's ParenthesesNode translates to NodeKind::Unknown in Murphy's
//!   AST (same blocker as Style/RedundantParentheses, see gap issue murphy-m3sf).
//!   The inner & call is unreachable via the cop API.
//!
//!   ActiveSupportExtensionsEnabled (present? / blank?) is not implemented
//!   because Murphy has no AllCops.ActiveSupportExtensionsEnabled config key.
//!   csend (&.) forms are detected and corrected, using &.intersect?.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad (intersection-receiver form)
//! array1.intersection(array2).any?
//! array1.intersection(array2).empty?
//! array1.intersection(array2).none?
//! array1.intersection(array2).count > 0
//! array1.intersection(array2).count.positive?
//! array1.intersection(array2).count != 0
//! array1.intersection(array2).count == 0
//! array1.intersection(array2).count.zero?
//! array1.intersection(array2).size > 0
//! array1.intersection(array2).length != 0
//!
//! # bad (block member? form)
//! array1.any? { |elem| array2.member?(elem) }
//! array1.none? { |elem| array2.member?(elem) }
//! array1.any? { array2.member?(_1) }
//! array1.any? { array2.member?(it) }
//!
//! # good
//! array1.intersect?(array2)
//! !array1.intersect?(array2)
//! ```
//!
//! ## Autocorrect
//!
//! Whole-node replacement because the structure must be rearranged:
//! `recv.intersection(arg).any?` → `recv.intersect?(arg)`.
//! The negating predicates (`empty?`, `none?`, `== 0`, `zero?`)
//! produce `!recv.intersect?(arg)`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

/// Negating predicates: result maps to `!intersect?`.
const NEGATING_PREDICATES: &[&str] = &["empty?", "none?", "==", "zero?"];

/// Size methods whose return value is compared.
const SIZE_METHODS: &[&str] = &["count", "size", "length"];

#[derive(Default)]
pub struct ArrayIntersect;

#[cop(
    name = "Style/ArrayIntersect",
    description = "Use `Array#intersect?` instead of intersection-emptiness checks.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl ArrayIntersect {
    /// Check `send` nodes for the intersection-receiver and count-comparison shapes.
    #[on_node(kind = "send", methods = [
        "any?", "empty?", "none?",
        "positive?", "zero?", ">", "!=", "=="
    ])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check_call(node, cx);
    }

    /// Check `csend` nodes for the safe-navigation variants, e.g.
    /// `array1&.intersection(array2)&.any?`.
    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Csend { method, .. } = *cx.kind(node) else {
            return;
        };
        let method_name = cx.symbol_str(method);
        if !matches!(
            method_name,
            "any?" | "empty?" | "none?" | "positive?" | "zero?" | ">" | "!=" | "=="
        ) {
            return;
        }
        check_call(node, cx);
    }

    /// Check `block` nodes for `array1.any? { |e| array2.member?(e) }`.
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, args, body } = *cx.kind(node) else {
            return;
        };
        let Some(body_id) = body.get() else {
            return;
        };
        let NodeKind::Send {
            receiver: call_recv,
            method: call_method,
            ..
        } = *cx.kind(call)
        else {
            return;
        };
        let call_method_name = cx.symbol_str(call_method);
        if call_method_name != "any?" && call_method_name != "none?" {
            return;
        }
        let Some(array1) = call_recv.get() else {
            return;
        };

        // Block args: exactly one named Arg.
        let arg_children = cx.children(args);
        if arg_children.len() != 1 {
            return;
        }
        let NodeKind::Arg(param_sym) = *cx.kind(arg_children[0]) else {
            return;
        };

        if let Some(array2) = extract_member_param(body_id, param_sym, cx) {
            let dot = dot_str(call, cx);
            let bang = if call_method_name == "none?" { "!" } else { "" };
            let array1_src = cx.raw_source(cx.range(array1));
            let array2_src = cx.raw_source(cx.range(array2));
            let replacement = format!("{bang}{array1_src}{dot}intersect?({array2_src})");
            emit(node, &replacement, cx);
        }
    }

    /// Check `numblock` nodes for `array1.any? { array2.member?(_1) }`.
    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Numblock { send, max_n, body } = *cx.kind(node) else {
            return;
        };
        if max_n != 1 {
            return;
        }
        let Some(body_id) = body.get() else {
            return;
        };
        let NodeKind::Send {
            receiver: call_recv,
            method: call_method,
            ..
        } = *cx.kind(send)
        else {
            return;
        };
        let call_method_name = cx.symbol_str(call_method);
        if call_method_name != "any?" && call_method_name != "none?" {
            return;
        }
        let Some(array1) = call_recv.get() else {
            return;
        };

        if let Some(array2) = extract_member_lvar(body_id, "_1", cx) {
            let dot = dot_str(send, cx);
            let bang = if call_method_name == "none?" { "!" } else { "" };
            let array1_src = cx.raw_source(cx.range(array1));
            let array2_src = cx.raw_source(cx.range(array2));
            let replacement = format!("{bang}{array1_src}{dot}intersect?({array2_src})");
            emit(node, &replacement, cx);
        }
    }

    /// Check `itblock` nodes for `array1.any? { array2.member?(it) }`.
    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Itblock { send, body } = *cx.kind(node) else {
            return;
        };
        let Some(body_id) = body.get() else {
            return;
        };
        let NodeKind::Send {
            receiver: call_recv,
            method: call_method,
            ..
        } = *cx.kind(send)
        else {
            return;
        };
        let call_method_name = cx.symbol_str(call_method);
        if call_method_name != "any?" && call_method_name != "none?" {
            return;
        }
        let Some(array1) = call_recv.get() else {
            return;
        };

        if let Some(array2) = extract_member_lvar(body_id, "it", cx) {
            let dot = dot_str(send, cx);
            let bang = if call_method_name == "none?" { "!" } else { "" };
            let array1_src = cx.raw_source(cx.range(array1));
            let array2_src = cx.raw_source(cx.range(array2));
            let replacement = format!("{bang}{array1_src}{dot}intersect?({array2_src})");
            emit(node, &replacement, cx);
        }
    }
}

// ---------------------------------------------------------------------------
// Shared call-check logic
// ---------------------------------------------------------------------------

/// Core check for both `send` and `csend` call nodes.
fn check_call(node: NodeId, cx: &Cx<'_>) {
    let (receiver, method, args) = match *cx.kind(node) {
        NodeKind::Send { receiver, method, args } => (receiver, method, args),
        NodeKind::Csend { receiver, method, args } => (OptNodeId::some(receiver), method, args),
        _ => return,
    };
    let method_name = cx.symbol_str(method);

    // Skip if this call has a block literal attached (RuboCop does the same).
    if cx.parent(node).get().is_some_and(|p| {
        matches!(*cx.kind(p), NodeKind::Block { call, .. } if call == node)
            || matches!(*cx.kind(p), NodeKind::Numblock { send, .. } if send == node)
            || matches!(*cx.kind(p), NodeKind::Itblock { send, .. } if send == node)
    }) {
        return;
    }

    let Some(recv_id) = receiver.get() else {
        return;
    };

    let outer_args = cx.list(args);

    // Comparison methods: `> 0`, `!= 0`, `== 0`.
    if matches!(method_name, ">" | "!=" | "==") {
        if let Some(replacement) = check_size_check(recv_id, method_name, outer_args, cx) {
            emit(node, &replacement, cx);
        }
        return;
    }

    // Zero-arg predicates only.
    if !outer_args.is_empty() {
        return;
    }

    // Check if receiver is `intersection(arg)` directly.
    if let Some(replacement) = check_intersection_predicate(recv_id, method_name, cx) {
        emit(node, &replacement, cx);
        return;
    }

    // Check if this is `positive?` / `zero?` on a count/size/length of intersection
    // (`array1.intersection(array2).count.positive?` etc.).
    // Guard for size predicates only to avoid false positives on integer-returning methods.
    if matches!(method_name, "positive?" | "zero?") &&
        let Some(replacement) = check_size_predicate(recv_id, method_name, cx)
    {
        emit(node, &replacement, cx);
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns `Some(replacement)` if `recv_id` is an `intersection(arg)` send and
/// the outer method is a predicate.
fn check_intersection_predicate(
    recv_id: NodeId,
    outer_method: &str,
    cx: &Cx<'_>,
) -> Option<String> {
    let (array1, array2) = extract_intersection(recv_id, cx)?;
    let dot = dot_str(recv_id, cx);
    let bang = if is_negating(outer_method) { "!" } else { "" };
    let array1_src = cx.raw_source(cx.range(array1));
    let array2_src = cx.raw_source(cx.range(array2));
    Some(format!("{bang}{array1_src}{dot}intersect?({array2_src})"))
}

/// Returns `Some(replacement)` for `recv.count > 0`, `recv.count != 0`, `recv.count == 0`.
fn check_size_check(
    recv_id: NodeId,
    outer_method: &str,
    outer_args: &[NodeId],
    cx: &Cx<'_>,
) -> Option<String> {
    if outer_args.len() != 1 {
        return None;
    }
    let NodeKind::Int(0) = *cx.kind(outer_args[0]) else {
        return None;
    };
    let size_method_name = cx.method_name(recv_id)?;
    if !SIZE_METHODS.contains(&size_method_name) {
        return None;
    }
    let size_recv = cx.call_receiver(recv_id).get()?;
    let (array1, array2) = extract_intersection(size_recv, cx)?;

    let dot = dot_str(size_recv, cx);
    let bang = if is_negating(outer_method) { "!" } else { "" };
    let array1_src = cx.raw_source(cx.range(array1));
    let array2_src = cx.raw_source(cx.range(array2));
    Some(format!("{bang}{array1_src}{dot}intersect?({array2_src})"))
}

/// Returns `Some(replacement)` for `positive?` / `zero?` on count/size/length
/// of an intersection: `array1.intersection(array2).count.positive?`.
fn check_size_predicate(recv_id: NodeId, outer_method: &str, cx: &Cx<'_>) -> Option<String> {
    let size_method_name = cx.method_name(recv_id)?;
    if !SIZE_METHODS.contains(&size_method_name) {
        return None;
    }
    let size_recv = cx.call_receiver(recv_id).get()?;
    let (array1, array2) = extract_intersection(size_recv, cx)?;

    let dot = dot_str(size_recv, cx);
    let bang = if is_negating(outer_method) { "!" } else { "" };
    let array1_src = cx.raw_source(cx.range(array1));
    let array2_src = cx.raw_source(cx.range(array2));
    Some(format!("{bang}{array1_src}{dot}intersect?({array2_src})"))
}

/// Returns `(array1, array2)` if `node` is a `send :intersection` or
/// `csend :intersection` with exactly one argument and a non-nil receiver.
fn extract_intersection(node: NodeId, cx: &Cx<'_>) -> Option<(NodeId, NodeId)> {
    let method_name = cx.method_name(node)?;
    if method_name != "intersection" {
        return None;
    }
    let array1 = cx.call_receiver(node).get()?;
    let args = cx.call_arguments(node);
    if args.len() != 1 {
        return None;
    }
    Some((array1, args[0]))
}

/// Returns `.` or `&.` based on the call node kind.
fn dot_str(call_id: NodeId, cx: &Cx<'_>) -> &'static str {
    match cx.kind(call_id) {
        NodeKind::Csend { .. } => "&.",
        _ => ".",
    }
}

/// Returns `true` if the method name is negating (maps to `!intersect?`).
fn is_negating(method_name: &str) -> bool {
    NEGATING_PREDICATES.contains(&method_name)
}

/// Emit an offense + autocorrect for the node.
fn emit(node: NodeId, replacement: &str, cx: &Cx<'_>) {
    let existing = cx.raw_source(cx.range(node));
    let msg = format!("Use `{replacement}` instead of `{existing}`.");
    cx.emit_offense(cx.range(node), &msg, None);
    cx.emit_edit(cx.range(node), replacement);
}

/// Returns `Some(array2)` if `body` is `array2.member?(param_sym)`.
fn extract_member_param(
    body: NodeId,
    param_sym: murphy_plugin_api::Symbol,
    cx: &Cx<'_>,
) -> Option<NodeId> {
    let NodeKind::Send {
        receiver: body_recv,
        method: body_method,
        args: body_args,
    } = *cx.kind(body)
    else {
        return None;
    };
    if cx.symbol_str(body_method) != "member?" {
        return None;
    }
    let array2 = body_recv.get()?;
    let args = cx.list(body_args);
    if args.len() != 1 {
        return None;
    }
    let NodeKind::Lvar(lvar_sym) = *cx.kind(args[0]) else {
        return None;
    };
    if lvar_sym != param_sym {
        return None;
    }
    Some(array2)
}

/// Returns `Some(array2)` if `body` is `array2.member?(lvar_name)`.
fn extract_member_lvar(body: NodeId, lvar_name: &str, cx: &Cx<'_>) -> Option<NodeId> {
    let NodeKind::Send {
        receiver: body_recv,
        method: body_method,
        args: body_args,
    } = *cx.kind(body)
    else {
        return None;
    };
    if cx.symbol_str(body_method) != "member?" {
        return None;
    }
    let array2 = body_recv.get()?;
    let args = cx.list(body_args);
    if args.len() != 1 {
        return None;
    }
    let NodeKind::Lvar(lvar_sym) = *cx.kind(args[0]) else {
        return None;
    };
    if cx.symbol_str(lvar_sym) != lvar_name {
        return None;
    }
    Some(array2)
}

#[cfg(test)]
mod tests {
    use super::ArrayIntersect;
    use murphy_plugin_api::test_support::{indoc, test};

    // -------------------------------------------------------------------------
    // Shape 1: intersection receiver — predicates
    // -------------------------------------------------------------------------

    #[test]
    fn flags_intersection_any() {
        test::<ArrayIntersect>().expect_correction(
            indoc! {r#"
                array1.intersection(array2).any?
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `array1.intersect?(array2)` instead of `array1.intersection(array2).any?`.
            "#},
            "array1.intersect?(array2)\n",
        );
    }

    #[test]
    fn flags_intersection_empty() {
        test::<ArrayIntersect>().expect_correction(
            indoc! {r#"
                array1.intersection(array2).empty?
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `!array1.intersect?(array2)` instead of `array1.intersection(array2).empty?`.
            "#},
            "!array1.intersect?(array2)\n",
        );
    }

    #[test]
    fn flags_intersection_none() {
        test::<ArrayIntersect>().expect_correction(
            indoc! {r#"
                array1.intersection(array2).none?
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `!array1.intersect?(array2)` instead of `array1.intersection(array2).none?`.
            "#},
            "!array1.intersect?(array2)\n",
        );
    }

    // -------------------------------------------------------------------------
    // Shape 2: intersection receiver — count/size/length comparisons
    // -------------------------------------------------------------------------

    #[test]
    fn flags_intersection_count_gt_zero() {
        test::<ArrayIntersect>().expect_correction(
            indoc! {r#"
                array1.intersection(array2).count > 0
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `array1.intersect?(array2)` instead of `array1.intersection(array2).count > 0`.
            "#},
            "array1.intersect?(array2)\n",
        );
    }

    #[test]
    fn flags_intersection_count_positive() {
        test::<ArrayIntersect>().expect_correction(
            indoc! {r#"
                array1.intersection(array2).count.positive?
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `array1.intersect?(array2)` instead of `array1.intersection(array2).count.positive?`.
            "#},
            "array1.intersect?(array2)\n",
        );
    }

    #[test]
    fn flags_intersection_count_ne_zero() {
        test::<ArrayIntersect>().expect_correction(
            indoc! {r#"
                array1.intersection(array2).count != 0
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `array1.intersect?(array2)` instead of `array1.intersection(array2).count != 0`.
            "#},
            "array1.intersect?(array2)\n",
        );
    }

    #[test]
    fn flags_intersection_count_eq_zero() {
        test::<ArrayIntersect>().expect_correction(
            indoc! {r#"
                array1.intersection(array2).count == 0
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `!array1.intersect?(array2)` instead of `array1.intersection(array2).count == 0`.
            "#},
            "!array1.intersect?(array2)\n",
        );
    }

    #[test]
    fn flags_intersection_count_zero() {
        test::<ArrayIntersect>().expect_correction(
            indoc! {r#"
                array1.intersection(array2).count.zero?
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `!array1.intersect?(array2)` instead of `array1.intersection(array2).count.zero?`.
            "#},
            "!array1.intersect?(array2)\n",
        );
    }

    #[test]
    fn flags_intersection_size_gt_zero() {
        test::<ArrayIntersect>().expect_correction(
            indoc! {r#"
                array1.intersection(array2).size > 0
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `array1.intersect?(array2)` instead of `array1.intersection(array2).size > 0`.
            "#},
            "array1.intersect?(array2)\n",
        );
    }

    #[test]
    fn flags_intersection_length_ne_zero() {
        test::<ArrayIntersect>().expect_correction(
            indoc! {r#"
                array1.intersection(array2).length != 0
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `array1.intersect?(array2)` instead of `array1.intersection(array2).length != 0`.
            "#},
            "array1.intersect?(array2)\n",
        );
    }

    // -------------------------------------------------------------------------
    // Shape 3: block member? form
    // -------------------------------------------------------------------------

    #[test]
    fn flags_block_any_member() {
        test::<ArrayIntersect>().expect_correction(
            indoc! {r#"
                array1.any? { |elem| array2.member?(elem) }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `array1.intersect?(array2)` instead of `array1.any? { |elem| array2.member?(elem) }`.
            "#},
            "array1.intersect?(array2)\n",
        );
    }

    #[test]
    fn flags_block_none_member() {
        test::<ArrayIntersect>().expect_correction(
            indoc! {r#"
                array1.none? { |elem| array2.member?(elem) }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `!array1.intersect?(array2)` instead of `array1.none? { |elem| array2.member?(elem) }`.
            "#},
            "!array1.intersect?(array2)\n",
        );
    }

    #[test]
    fn flags_numblock_any_member() {
        test::<ArrayIntersect>().expect_correction(
            indoc! {r#"
                array1.any? { array2.member?(_1) }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `array1.intersect?(array2)` instead of `array1.any? { array2.member?(_1) }`.
            "#},
            "array1.intersect?(array2)\n",
        );
    }

    #[test]
    fn flags_itblock_any_member() {
        test::<ArrayIntersect>().expect_correction(
            indoc! {r#"
                array1.any? { array2.member?(it) }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `array1.intersect?(array2)` instead of `array1.any? { array2.member?(it) }`.
            "#},
            "array1.intersect?(array2)\n",
        );
    }

    // -------------------------------------------------------------------------
    // Negative cases
    // -------------------------------------------------------------------------

    #[test]
    fn accepts_intersect_already() {
        test::<ArrayIntersect>().expect_no_offenses("array1.intersect?(array2)\n");
    }

    #[test]
    fn accepts_negated_intersect_already() {
        test::<ArrayIntersect>().expect_no_offenses("!array1.intersect?(array2)\n");
    }

    #[test]
    fn accepts_intersection_with_multiple_args() {
        test::<ArrayIntersect>().expect_no_offenses("array1.intersection(array2, array3).any?\n");
    }

    #[test]
    fn accepts_count_gt_nonzero() {
        test::<ArrayIntersect>().expect_no_offenses("array1.intersection(array2).count > 1\n");
    }

    #[test]
    fn accepts_block_with_wrong_arg() {
        test::<ArrayIntersect>()
            .expect_no_offenses("array1.any? { |elem| array2.member?(other) }\n");
    }

    #[test]
    fn accepts_block_any_without_receiver() {
        test::<ArrayIntersect>().expect_no_offenses("any? { |e| array2.member?(e) }\n");
    }

    #[test]
    fn accepts_any_with_extra_block_on_intersection() {
        test::<ArrayIntersect>()
            .expect_no_offenses("array1.intersection(array2).any? { |x| x > 0 }\n");
    }

    // -------------------------------------------------------------------------
    // csend (safe navigation) forms
    // -------------------------------------------------------------------------

    #[test]
    fn flags_csend_intersection_with_dot_any() {
        // array1&.intersection(array2).any? — inner csend, outer regular send.
        test::<ArrayIntersect>().expect_correction(
            indoc! {r#"
                array1&.intersection(array2).any?
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `array1&.intersect?(array2)` instead of `array1&.intersection(array2).any?`.
            "#},
            "array1&.intersect?(array2)\n",
        );
    }

    #[test]
    fn flags_csend_intersection_with_csend_any() {
        // array1&.intersection(array2)&.any? — both inner and outer csend.
        test::<ArrayIntersect>().expect_correction(
            indoc! {r#"
                array1&.intersection(array2)&.any?
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `array1&.intersect?(array2)` instead of `array1&.intersection(array2)&.any?`.
            "#},
            "array1&.intersect?(array2)\n",
        );
    }

    #[test]
    fn flags_csend_intersection_count_gt_zero() {
        // array1&.intersection(array2).count > 0 — csend inner, regular outer comparison.
        test::<ArrayIntersect>().expect_correction(
            indoc! {r#"
                array1&.intersection(array2).count > 0
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `array1&.intersect?(array2)` instead of `array1&.intersection(array2).count > 0`.
            "#},
            "array1&.intersect?(array2)\n",
        );
    }
}

murphy_plugin_api::submit_cop!(ArrayIntersect);
