//! `Style/ConcatArrayLiterals` — enforces `push(item)` over `concat([item])`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ConcatArrayLiterals
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Verbatim port of the call head `(call _ :concat ...)` (murphy-s1yc.13):
//!   `call` covers safe-navigation (`list&.concat`), mirroring RuboCop
//!   `alias on_csend on_send` plus `RESTRICT_ON_SEND concat`; the wildcard
//!   receiver binds an absent or present receiver per murphy-if9y; trailing
//!   `...` absorbs any argument list. The receiver plus non-empty plus
//!   all-array plus non-empty-array guards below apply separately (upstream
//!   has no receiver guard and flags bare `concat([1])` plus empty
//!   `concat([])`; murphy preserves bare-accept plus empty-array-accept as
//!   complementary guards). v1 gap: percent-literal args are not transformed
//!   to `push(...)` form.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop, def_node_matcher};

// Verbatim port of the call head (murphy-s1yc.13):
// `(call _ :concat ...)` — `call` = `{send csend}` covers safe-navigation
// (`list&.concat`), mirroring RuboCop `alias on_csend on_send` plus
// `RESTRICT_ON_SEND concat`. The `_` receiver binds an absent or present
// receiver per murphy-if9y; trailing `...` absorbs any argument list, so the
// receiver plus non-empty plus all-array plus non-empty-array guards below
// apply separately (upstream flags bare plus empty; murphy preserves
// bare-accept plus empty-array-accept).
def_node_matcher!(concat_array_literals_call, "(call _ :concat ...)");

#[derive(Default)]
pub struct ConcatArrayLiterals;

#[cop(
    name = "Style/ConcatArrayLiterals",
    description = "Use `push(item)` instead of `concat([item])`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl ConcatArrayLiterals {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Verbatim `(call _ :concat ...)` head: filters to `concat` calls
    // on either send or csend (safe-navigation), with any receiver
    // (absent or present). Without this, an unrelated call over an array
    // argument (e.g. `list.append([foo])`) would run check on every call
    // node instead of being rejected by the method set up front.
    if !concat_array_literals_call(node, cx) {
        return;
    }

    // Must have a receiver. Mirrors the pre-port hand-rolled guard;
    // `_` binds an absent receiver, so bare `concat([foo])` is accepted
    // here (upstream flags bare; murphy preserves bare-accept).
    if cx.call_receiver(node).get().is_none() {
        return;
    }
    // Trailing `...` absorbs any argument list, so the no-argument case
    // (`list.concat`) is accepted here.
    let arg_list = cx.call_arguments(node);
    if arg_list.is_empty() {
        return;
    }
    let all_arrays = arg_list.iter().all(|&a| matches!(cx.kind(unwrap_begin(a, cx)), NodeKind::Array(_)));
    if !all_arrays {
        return;
    }
    // Empty array (`list.concat([])`) is accepted here (upstream flags it
    // to `push()`; murphy preserves empty-array-accept).
    let empty_array = arg_list.iter().any(|&a| {
        if let NodeKind::Array(elements) = cx.kind(unwrap_begin(a, cx)) {
            cx.list(*elements).is_empty()
        } else {
            false
        }
    });
    if empty_array {
        return;
    }
    cx.emit_offense(
        cx.range(node),
        "Use `push` with elements as arguments instead of `concat` with an array literal.",
        None,
    );
    cx.emit_edit(cx.range(node), &build_push_call(node, cx));
}

fn build_push_call(node: NodeId, cx: &Cx<'_>) -> String {
    // Generic over send/csend: `call` covers safe-navigation, so preserve
    // `&.` in the correction (`list&.concat([foo])` -> `list&.push(foo)`,
    // mirroring upstream). The receiver guard above guarantees `Some`.
    let Some(recv_id) = cx.call_receiver(node).get() else {
        return String::new();
    };
    let recv_src = cx.raw_source(cx.range(recv_id)).to_string();
    let arg_list = cx.call_arguments(node);
    let push_args: Vec<String> = arg_list.iter().map(|&a| {
        let NodeKind::Array(elements) = *cx.kind(unwrap_begin(a, cx)) else {
            return cx.raw_source(cx.range(a)).to_string();
        };
        let elems: Vec<String> = cx.list(elements).iter()
            .map(|&e| cx.raw_source(cx.range(e)).to_string())
            .collect();
        if elems.is_empty() {
            return String::new();
        }
        elems.join(", ")
    }).collect();
    let op = if cx.is_safe_navigation(node) { "&." } else { "." };
    format!("{}{}push({})", recv_src, op, push_args.join(", "))
}

fn unwrap_begin(mut node: NodeId, cx: &Cx<'_>) -> NodeId {
    while let NodeKind::Begin(children) = cx.kind(node) {
        let child_list = cx.list(*children);
        if child_list.len() != 1 {
            break;
        }
        node = child_list[0];
    }
    node
}

#[cfg(test)]
mod tests {
    use super::ConcatArrayLiterals;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_concat_single_element() {
        test::<ConcatArrayLiterals>().expect_correction(
            indoc! {"
                list.concat([foo])
                ^^^^^^^^^^^^^^^^^^ Use `push` with elements as arguments instead of `concat` with an array literal.
            "},
            "list.push(foo)\n",
        );
    }

    #[test]
    fn flags_parenthesized_array_arg() {
        test::<ConcatArrayLiterals>().expect_correction(
            indoc! {"
                list.concat(([foo]))
                ^^^^^^^^^^^^^^^^^^^^ Use `push` with elements as arguments instead of `concat` with an array literal.
            "},
            "list.push(foo)\n",
        );
    }

    #[test]
    fn flags_concat_multiple_elements() {
        test::<ConcatArrayLiterals>().expect_correction(
            indoc! {"
                list.concat([bar, baz])
                ^^^^^^^^^^^^^^^^^^^^^^^ Use `push` with elements as arguments instead of `concat` with an array literal.
            "},
            "list.push(bar, baz)\n",
        );
    }

    #[test]
    fn accepts_push() {
        test::<ConcatArrayLiterals>().expect_no_offenses("list.push(foo)\n");
    }

    #[test]
    fn accepts_concat_non_array() {
        test::<ConcatArrayLiterals>().expect_no_offenses("list.concat(other)\n");
    }

    // --- Characterization (murphy-s1yc.13): pin the exact node set the
    // hand-rolled send-only dispatch matches, so the verbatim
    // `(call _ :concat ...)` port can be proven byte-identical. `call`
    // covers safe-navigation (mirroring upstream `alias on_csend on_send`
    // plus `RESTRICT_ON_SEND concat`); trailing `...` absorbs any argument
    // list, so the receiver plus non-empty plus all-array plus non-empty-array
    // guards below apply separately (upstream has no receiver guard and flags
    // bare `concat([1])` plus empty `concat([])`; murphy preserves bare-accept
    // plus empty-array-accept as complementary guards).

    #[test]
    fn s1yc13_flags_csend_corrects() {
        // Safe navigation: `call` covers `csend` per murphy-if9y, mirroring
        // upstream `alias on_csend on_send`. (Pre-port the `csend` handler
        // is missing: `check_concat` destructures `NodeKind::Send` only and
        // `#[on_node(kind = "send")]` never visits csend.)
        test::<ConcatArrayLiterals>().expect_correction(
            indoc! {r#"
                list&.concat([foo])
                ^^^^^^^^^^^^^^^^^^^ Use `push` with elements as arguments instead of `concat` with an array literal.
            "#},
            "list&.push(foo)\n",
        );
    }

    #[test]
    fn s1yc13_accepts_bare_concat() {
        // Bare receiver: `_` binds an absent receiver per murphy-if9y, so
        // the head matches and the complementary receiver guard accepts.
        // (Upstream flags bare `concat([1])`; murphy preserves bare-accept.)
        test::<ConcatArrayLiterals>().expect_no_offenses("concat([foo])\n");
    }

    #[test]
    fn s1yc13_accepts_no_arg_concat() {
        // No arguments: trailing `...` absorbs the empty list, so the head
        // matches and the complementary non-empty-arg guard accepts.
        test::<ConcatArrayLiterals>().expect_no_offenses("list.concat\n");
    }

    #[test]
    fn s1yc13_accepts_unrelated_method() {
        // `append` is outside the verbatim method set, so the head rejects.
        test::<ConcatArrayLiterals>().expect_no_offenses("list.append([foo])\n");
    }
}
murphy_plugin_api::submit_cop!(ConcatArrayLiterals);
