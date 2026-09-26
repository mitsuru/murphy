//! `Style/MapJoin` — removes redundant `map(&:to_s)` before `join`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/MapJoin
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Verbatim port of the call head `(call _ :join ...)`
//!   (murphy-s1yc.16): `call` = `{send csend}` covers safe-navigation
//!   (`array.map(&:to_s)&.join(...)`), mirroring RuboCop
//!   `alias on_csend on_send` plus `RESTRICT_ON_SEND`; the wildcard
//!   receiver binds an absent or present receiver per murphy-if9y;
//!   trailing `...` absorbs any argument list. The receiver plus inner
//!   `map` shape guards below apply separately (upstream inner is
//!   `(call _ {:map :collect} (block_pass (sym :to_s)))`, which also
//!   covers inner csend like `array&.map(&:to_s).join`).
//!   `map(&:to_s).join(...)` handled. Uses hand-written shape check
//!   (def_node_matcher does not support block_pass in v1).
//!   Block form and numblock/itblock forms are v1 gaps. Receiverless
//!   inner `map(&:to_s).join` stays accepted (upstream corrects via
//!   `no_receiver_range`; murphy keeps the map-receiver guard).
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, cop, def_node_matcher};

// Verbatim port of the call head (murphy-s1yc.16):
// `(call _ :join ...)` — `call` = `{send csend}` covers safe-navigation
// (`array.map(&:to_s)&.join(...)`), mirroring RuboCop
// `alias on_csend on_send` plus `RESTRICT_ON_SEND`. The `_` receiver binds
// an absent or present receiver per murphy-if9y; trailing `...` absorbs any
// argument list, so the receiver plus inner-`map` shape guards below apply
// separately (upstream inner is `(call _ {:map :collect}
// (block_pass (sym :to_s)))`; block/numblock/itblock map forms stay gaps).
def_node_matcher!(
    map_join_call,
    "(call _ :join ...)"
);

const MSG: &str = "Remove redundant `map(&:to_s)` before `join`.";

#[derive(Default)]
pub struct MapJoin;

#[cop(
    name = "Style/MapJoin",
    description = "Remove redundant `map(&:to_s)` before `join`.",
    default_severity = "warning",
    default_enabled = true,
    options = murphy_plugin_api::NoOptions
)]
impl MapJoin {
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
    // Verbatim `(call _ :join ...)` head: filters to `join` calls on
    // either send or csend (safe-navigation), with any receiver
    // (absent or present). Without this, an unrelated call on an
    // array receiver (e.g. `array.map(&:to_s).sort`) would run check
    // on every call node instead of being rejected by the method set
    // up front.
    if !map_join_call(node, cx) {
        return;
    }
    // Must have a receiver. Mirrors the pre-port hand-rolled guard;
    // `_` binds an absent receiver, so bare `join(', ')` is accepted
    // here.
    let Some(recv_id) = cx.call_receiver(node).get() else {
        return;
    };
    let map_call_id = unwrap_begin(recv_id, cx);
    // Inner `map`/`collect` also covers `csend`
    // (`array&.map(&:to_s).join`), mirroring upstream inner
    // `(call _ {:map :collect} ...)`.
    if !matches!(
        cx.kind(map_call_id),
        NodeKind::Send { .. } | NodeKind::Csend { .. }
    ) {
        return;
    }
    let Some(method_str) = cx.method_name(map_call_id) else {
        return;
    };
    if method_str != "map" && method_str != "collect" {
        return;
    }
    // Trailing `...` absorbs any argument list, so the inner
    // zero/multi-argument cases are accepted here (upstream inner
    // requires exactly `(block_pass (sym :to_s))`).
    let map_arg_list = cx.call_arguments(map_call_id);
    if map_arg_list.len() != 1 {
        return;
    }
    let bp_arg = map_arg_list[0];
    let opt_sym = match *cx.kind(bp_arg) {
        murphy_plugin_api::NodeKind::BlockPass(sym) => sym,
        _ => return,
    };
    let Some(sym_id) = opt_sym.get() else {
        return;
    };
    if !matches!(cx.kind(sym_id), murphy_plugin_api::NodeKind::Sym(_)) {
        return;
    }
    let sym_src = cx.raw_source(cx.range(sym_id));
    if sym_src != ":to_s" {
        return;
    }
    let map_range = cx.range(map_call_id);
    // Must have a map receiver (upstream handles receiverless inner
    // `map(&:to_s).join` via `no_receiver_range`; murphy keeps the
    // guard as a complementary gap).
    let Some(map_recv_id) = cx.call_receiver(map_call_id).get() else {
        return;
    };

    let method_name_len = method_str.len() as u32;
    cx.emit_offense(
        murphy_plugin_api::Range {
            start: map_range.start,
            end: map_range.start + method_name_len,
        },
        MSG,
        None,
    );

    let map_recv_src = cx.raw_source(cx.range(map_recv_id));
    let node_src = cx.raw_source(cx.range(node));
    let dot_pos = cx.range(recv_id).end - cx.range(node).start;
    let after_dot = &node_src[dot_pos as usize..];
    cx.emit_edit(cx.range(node), &format!("{}{}", map_recv_src, after_dot));
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
    use super::MapJoin;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_map_to_s_join() {
        test::<MapJoin>().expect_correction(
            indoc! {"
                array.map(&:to_s).join(', ')
                ^^^ Remove redundant `map(&:to_s)` before `join`.
            "},
            "array.join(', ')\n",
        );
    }

    #[test]
    fn flags_parenthesized_map_to_s_join() {
        test::<MapJoin>().expect_correction(
            indoc! {"
                (array.map(&:to_s)).join(', ')
                 ^^^ Remove redundant `map(&:to_s)` before `join`.
            "},
            "array.join(', ')\n",
        );
    }

    #[test]
    fn flags_collect_to_s_join() {
        test::<MapJoin>().expect_correction(
            indoc! {"
                array.collect(&:to_s).join
                ^^^^^^^ Remove redundant `map(&:to_s)` before `join`.
            "},
            "array.join\n",
        );
    }

    #[test]
    fn flags_collect_to_s_join_map_range() {
        // Verify offense range covers full "collect", not just first 3 chars.
        test::<MapJoin>().expect_offense(indoc! {"
            array.collect(&:to_s).join
            ^^^^^^^ Remove redundant `map(&:to_s)` before `join`.
        "});
    }

    #[test]
    fn accepts_plain_join() {
        test::<MapJoin>().expect_no_offenses("array.join(', ')\n");
    }

    #[test]
    fn accepts_map_without_to_s() {
        test::<MapJoin>().expect_no_offenses("array.map(&:foo).join\n");
    }

    // --- Characterization (murphy-s1yc.16): pin the exact node set the
    // hand-rolled send-only dispatch matches, so the verbatim
    // `(call _ :join ...)` port can be proven byte-identical.
    // `call` covers safe-navigation (mirroring upstream `alias on_csend
    // on_send` plus `RESTRICT_ON_SEND`); trailing `...` absorbs any
    // argument list, so the receiver plus inner-`map` shape guards below
    // apply separately (upstream inner is
    // `(call _ {:map :collect} (block_pass (sym :to_s)))`; block form and
    // numblock/itblock forms stay gaps).

    #[test]
    fn s1yc16_flags_csend_join() {
        // Safe navigation on outer `join`: `call` covers `csend` per
        // murphy-if9y, mirroring upstream `alias on_csend on_send`.
        // (Pre-port the `csend` handler is missing: `check_join`
        // destructures `NodeKind::Send` only and
        // `#[on_node(kind = "send", methods = ["join"])]` never visits
        // csend.)
        test::<MapJoin>().expect_correction(
            indoc! {"
                array.map(&:to_s)&.join(', ')
                ^^^ Remove redundant `map(&:to_s)` before `join`.
            "},
            "array&.join(', ')\n",
        );
    }

    #[test]
    fn s1yc16_flags_inner_csend_join() {
        // Safe navigation on inner `map`: upstream inner
        // `(call _ {:map :collect} ...)` covers `csend`; murphy's generic
        // call accessors match it. (Pre-port the inner `NodeKind::Send`
        // destructure rejects csend.)
        test::<MapJoin>().expect_correction(
            indoc! {"
                array&.map(&:to_s).join(', ')
                ^^^ Remove redundant `map(&:to_s)` before `join`.
            "},
            "array.join(', ')\n",
        );
    }

    #[test]
    fn s1yc16_accepts_bare_join() {
        // Bare receiver: `_` binds an absent receiver per murphy-if9y, so
        // the head matches and the complementary receiver guard accepts.
        test::<MapJoin>().expect_no_offenses("join(', ')\n");
    }

    #[test]
    fn s1yc16_accepts_no_arg_inner_map() {
        // Inner `map` without arguments: trailing `...` absorbs the outer
        // argument list so the head matches, and the complementary
        // one-`block_pass`-arg guard accepts (upstream inner requires
        // exactly `(block_pass (sym :to_s))`).
        test::<MapJoin>().expect_no_offenses("array.map.join(', ')\n");
    }

    #[test]
    fn s1yc16_accepts_unrelated_method() {
        // `sort` is outside the verbatim method set, so the head rejects.
        test::<MapJoin>().expect_no_offenses("array.map(&:to_s).sort\n");
    }
}
murphy_plugin_api::submit_cop!(MapJoin);
