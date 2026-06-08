//! `Lint/UselessOr` — Checks for useless `||` (and `or`) where the left side
//! always returns a truthy value, so the right side never evaluates.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/UselessOr
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Truthy-return methods (nil.to_X always succeeds): to_a, to_c, to_d, to_i,
//!   to_f, to_h, to_r, to_s, to_sym, intern, inspect, hash, object_id, __id__.
//!   Safe navigation (x&.to_s) is accepted. Chained or-expressions are handled
//!   by walking up the Or-parent chain.
//! ```
//!
//! ## Matched shapes
//!
//! - `x.method || fallback` where method always returns truthy.
//! - `x.method or fallback` (keyword `or`).
//! - `foo || x.method || fallback` — flags the inner `||`.
//! - `(foo || x.method) || fallback` — flags the outer `||`.
//!
//! ## Autocorrect
//!
//! Replace the whole `or` expression with just the truthy side.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct UselessOr;

#[cop(
    name = "Lint/UselessOr",
    description = "Checks for useless `||` (or `or`) where the left side always returns a truthy value.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl UselessOr {
    #[on_node(kind = "or")]
    fn check_or(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Or { lhs, rhs } = *cx.kind(node) else {
            return;
        };

        // Case 1: lhs is a Send with a truthy-return method
        // (e.g. `x.to_s || fallback`).
        if is_truthy_return_method(lhs, cx) {
            let target = walk_up_or_chain(node, cx);
            report_offense(target, lhs, cx, true);
            return;
        }

        // Case 2: rhs is a truthy-return method and parent is an Or
        // (e.g. `foo || x.to_s || fallback` → inner Or's RHS is `x.to_s`).
        if is_truthy_return_method(rhs, cx) {
            // Skip Begin nodes (parenthesized expressions like `(foo || x.to_s)`).
            let parent = cx.parent(node).get().and_then(|p| {
                if matches!(*cx.kind(p), NodeKind::Begin(_)) {
                    cx.parent(p).get()
                } else {
                    Some(p)
                }
            });
            if let Some(parent_id) = parent
                && matches!(*cx.kind(parent_id), NodeKind::Or { .. }) {
                    report_offense(parent_id, rhs, cx, false);
                }
        }
    }
}

/// Returns `true` if `node` is a `Send` (not `Csend`) with an explicit
/// receiver whose method is known to always return a truthy value even
/// when called on `nil` (e.g., `nil.to_s` returns `""`).
/// Bare calls (implicit-self, e.g. `to_s || fallback`) are NOT flagged
/// because they could be user-defined methods.
fn is_truthy_return_method(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Send { receiver, method, .. } = *cx.kind(node) else {
        return false;
    };
    // Require an explicit receiver (bare `to_s` may be user-defined).
    if receiver.is_none() {
        return false;
    }
    let name = cx.symbol_str(method);
    matches!(
        name,
        "to_a"
            | "to_c"
            | "to_d"
            | "to_i"
            | "to_f"
            | "to_h"
            | "to_r"
            | "to_s"
            | "to_sym"
            | "intern"
            | "inspect"
            | "hash"
            | "object_id"
            | "__id__"
    )
}

/// Walk up the `Or` chain while `node` is the LHS of its parent `Or`,
/// returning the topmost `Or` in the chain. For `x.to_s || a || b` this
/// walks from the inner `(x.to_s || a)` to the outer `((x.to_s || a) || b)`,
/// so a single edit can replace the entire chain with just the truthy call.
fn walk_up_or_chain(mut node: NodeId, cx: &Cx<'_>) -> NodeId {
    loop {
        let parent = match cx.parent(node).get() {
            Some(p) if matches!(*cx.kind(p), NodeKind::Or { .. }) => p,
            _ => return node,
        };
        let NodeKind::Or { lhs: parent_lhs, .. } = *cx.kind(parent) else {
            return node;
        };
        if parent_lhs != node {
            return node;
        }
        node = parent;
    }
}

/// Emit an offense on `or_node` and an autocorrect.
///
/// When `replace_with_truthy` is true (Case 1: LHS is the truthy method),
/// replaces the entire `or_node` with just the truthy call's source.
/// When false (Case 2: RHS is the truthy method, parent is Or), replaces
/// with the `or_node`'s LHS source, preserving the left chain.
fn report_offense(
    or_node: NodeId,
    truthy_node: NodeId,
    cx: &Cx<'_>,
    replace_with_truthy: bool,
) {
    let NodeKind::Or { lhs, rhs } = *cx.kind(or_node) else {
        return;
    };
    let truthy_source = cx.raw_source(cx.range(truthy_node));
    let never_evaluated_source = cx.raw_source(cx.range(rhs));
    let replacement = if replace_with_truthy {
        truthy_source.to_string()
    } else {
        cx.raw_source(cx.range(lhs)).to_string()
    };
    let msg = format!(
        "`{}` will never evaluate because `{}` always returns a truthy value.",
        never_evaluated_source, truthy_source
    );
    cx.emit_offense(cx.range(or_node), &msg, None);
    cx.emit_edit(cx.range(or_node), &replacement);
}

#[cfg(test)]
mod tests {
    use super::UselessOr;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_to_s_or_fallback() {
        test::<UselessOr>().expect_correction(
            indoc! {r#"
                x.to_s || fallback
                ^^^^^^^^^^^^^^^^^^ `fallback` will never evaluate because `x.to_s` always returns a truthy value.
            "#},
            "x.to_s\n",
        );
    }

    #[test]
    fn flags_to_s_or_fallback_keyword_or() {
        test::<UselessOr>().expect_correction(
            indoc! {r#"
                x.to_s or fallback
                ^^^^^^^^^^^^^^^^^^ `fallback` will never evaluate because `x.to_s` always returns a truthy value.
            "#},
            "x.to_s\n",
        );
    }

    #[test]
    fn flags_to_s_with_and_chain() {
        test::<UselessOr>().expect_correction(
            indoc! {r#"
                x.to_s || fallback || other_fallback
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `other_fallback` will never evaluate because `x.to_s` always returns a truthy value.
            "#},
            "x.to_s\n",
        );
    }

    #[test]
    fn flags_truthy_rhs() {
        test::<UselessOr>().expect_correction(
            indoc! {r#"
                foo || x.to_s || fallback
                ^^^^^^^^^^^^^^^^^^^^^^^^^ `fallback` will never evaluate because `x.to_s` always returns a truthy value.
            "#},
            "foo || x.to_s\n",
        );
    }

    #[test]
    fn accepts_safe_navigation() {
        test::<UselessOr>().expect_no_offenses("x&.to_s || fallback\n");
    }

    #[test]
    fn accepts_plain_x_to_s() {
        test::<UselessOr>().expect_no_offenses("x.to_s\n");
    }

    #[test]
    fn accepts_unknown_method() {
        test::<UselessOr>().expect_no_offenses("x.foo || fallback\n");
    }
}
murphy_plugin_api::submit_cop!(UselessOr);
