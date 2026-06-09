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
//!   `map(&:to_s).join(...)` handled. Uses hand-written shape check
//!   (def_node_matcher does not support block_pass in v1).
//!   Block form and numblock/itblock forms are v1 gaps.
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, cop};

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
    #[on_node(kind = "send", methods = ["join"])]
    fn check_join(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, .. } = *cx.kind(node) else {
            return;
        };
        let Some(recv_id) = receiver.get() else {
            return;
        };
        let map_call_id = unwrap_begin(recv_id, cx);
        let NodeKind::Send { method, args: map_args, .. } = *cx.kind(map_call_id) else {
            return;
        };
        let method_str = cx.symbol_str(method);
        if method_str != "map" && method_str != "collect" {
            return;
        }
        let map_arg_list = cx.list(map_args);
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
        let recv_of_map = match cx.kind(map_call_id) {
            NodeKind::Send { receiver: r, .. } => r,
            _ => return,
        };
        let Some(map_recv_id) = recv_of_map.get() else {
            return;
        };

        let method_name_len = cx.symbol_str(method).len() as u32;
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
}
murphy_plugin_api::submit_cop!(MapJoin);
