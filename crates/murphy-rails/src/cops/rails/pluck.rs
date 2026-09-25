//! `Rails/Pluck` — prefer `pluck` over `map`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/Pluck
//! upstream_version_checked: 2.35.0
//! version_added: "2.7"
//! safe: false
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: any_block over map/collect, single
//!   block-arg gate (Block Args with one Arg, Numblock max_n == 1, Itblock),
//!   body `(send lvar :[] key)` with exactly one key arg, regexp-key
//!   suppression, block-arg-not-in-key gate (key source equality plus no
//!   descendant lvar with the same name), iteration guard (nearest ancestor
//!   any_block with a receiver suppresses), and TargetRailsVersion >= 5.0
//!   gating (unset means newest). Offense is map/collect selector to block
//!   end; autocorrect replaces it with `pluck(key)` preserving safe
//!   navigation (`&.`). Receiver lvar check is intentionally permissive
//!   like upstream (any lvar, even when it differs from the block arg).
//! ```
//!
//! Enforces the use of `pluck` over `map`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct Pluck;

#[cop(
    name = "Rails/Pluck",
    description = "Prefer `pluck` over `map { ... }`.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl Pluck {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check_block(node, cx);
    }

    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        check_numblock(node, cx);
    }

    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        check_itblock(node, cx);
    }
}

fn check_block(node: NodeId, cx: &Cx<'_>) {
    if !cx.rails_version_at_least(5, 0) {
        return;
    }
    if in_iteration(cx, node) {
        return;
    }
    let NodeKind::Block { call, args, body } = *cx.kind(node) else {
        return;
    };
    if !is_map_collect_call(cx, call) {
        return;
    }
    let Some(block_arg) = single_block_arg_name(cx, args) else {
        return;
    };
    let Some(body_id) = body.get() else {
        return;
    };
    let Some(key) = bracket_key(cx, body_id) else {
        return;
    };
    if is_regexp_key(cx, key) {
        return;
    }
    if !block_arg_not_in_key(cx, &block_arg, key) {
        return;
    }
    emit(cx, node, call, key);
}

fn check_numblock(node: NodeId, cx: &Cx<'_>) {
    if !cx.rails_version_at_least(5, 0) {
        return;
    }
    if in_iteration(cx, node) {
        return;
    }
    let NodeKind::Numblock { send, max_n, body } = *cx.kind(node) else {
        return;
    };
    if max_n != 1 {
        return;
    }
    if !is_map_collect_call(cx, send) {
        return;
    }
    let Some(body_id) = body.get() else {
        return;
    };
    let Some(key) = bracket_key(cx, body_id) else {
        return;
    };
    if is_regexp_key(cx, key) {
        return;
    }
    if !block_arg_not_in_key(cx, "_1", key) {
        return;
    }
    emit(cx, node, send, key);
}

fn check_itblock(node: NodeId, cx: &Cx<'_>) {
    if !cx.rails_version_at_least(5, 0) {
        return;
    }
    if in_iteration(cx, node) {
        return;
    }
    let NodeKind::Itblock { send, body } = *cx.kind(node) else {
        return;
    };
    if !is_map_collect_call(cx, send) {
        return;
    }
    let Some(body_id) = body.get() else {
        return;
    };
    let Some(key) = bracket_key(cx, body_id) else {
        return;
    };
    if is_regexp_key(cx, key) {
        return;
    }
    if !block_arg_not_in_key(cx, "it", key) {
        return;
    }
    emit(cx, node, send, key);
}

fn in_iteration(cx: &Cx<'_>, node: NodeId) -> bool {
    // `node.each_ancestor(:any_block).first&.receiver` — nearest ancestor
    // block with a receiver suppresses (iteration suspected).
    for anc in cx.ancestors(node) {
        let call_opt = match *cx.kind(anc) {
            NodeKind::Block { call, .. } => Some(call),
            NodeKind::Numblock { send, .. } => Some(send),
            NodeKind::Itblock { send, .. } => Some(send),
            _ => None,
        };
        if let Some(call) = call_opt {
            return cx.call_receiver(call).get().is_some();
        }
    }
    false
}

fn is_map_collect_call(cx: &Cx<'_>, call: NodeId) -> bool {
    if !matches!(*cx.kind(call), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        return false;
    }
    let m = cx.method_name(call);
    if m != Some("map") && m != Some("collect") {
        return false;
    }
    cx.call_arguments(call).is_empty()
}

fn single_block_arg_name(cx: &Cx<'_>, args: NodeId) -> Option<String> {
    let NodeKind::Args(list) = *cx.kind(args) else {
        return None;
    };
    let params = cx.list(list);
    if params.len() != 1 {
        return None;
    }
    if let NodeKind::Arg(s) = *cx.kind(params[0]) {
        return Some(cx.symbol_str(s).to_owned());
    }
    None
}

/// `(send lvar :[] key)` with exactly one key argument. Receiver must be
/// an lvar (any name, mirroring upstream permissiveness) for Block/Numblock;
/// for Itblock the receiver is `(send nil :it)` which is also accepted.
fn bracket_key(cx: &Cx<'_>, body: NodeId) -> Option<NodeId> {
    if !matches!(*cx.kind(body), NodeKind::Send { .. }) {
        return None;
    }
    if cx.method_name(body) != Some("[]") {
        return None;
    }
    let args = cx.call_arguments(body);
    if args.len() != 1 {
        return None;
    }
    let recv = cx.call_receiver(body).get()?;
    let recv_ok = match *cx.kind(recv) {
        NodeKind::Lvar(_) => true,
        _ => {
            // `it` receiver: `(send nil :it)` with no args
            matches!(*cx.kind(recv), NodeKind::Send { .. })
                && cx.method_name(recv) == Some("it")
                && cx.call_receiver(recv).get().is_none()
                && cx.call_arguments(recv).is_empty()
        }
    };
    if !recv_ok {
        return None;
    }
    Some(args[0])
}

fn is_regexp_key(cx: &Cx<'_>, key: NodeId) -> bool {
    matches!(*cx.kind(key), NodeKind::Regexp { .. })
}

fn block_arg_not_in_key(cx: &Cx<'_>, block_arg: &str, key: NodeId) -> bool {
    // `return false if block_argument == key.source`
    let key_src = cx.raw_source(cx.range(key));
    if block_arg == key_src {
        return false;
    }
    // `key.each_descendant(:lvar).none? { |lvar| block_argument == lvar.source }`
    for desc in cx.descendants(key) {
        if let NodeKind::Lvar(s) = *cx.kind(desc)
            && cx.symbol_str(s) == block_arg
        {
            return false;
        }
    }
    true
}

fn emit(cx: &Cx<'_>, node: NodeId, call: NodeId, key: NodeId) {
    // Offense: `node.send_node.loc.selector.join(node.loc.end)` — from
    // map/collect selector start to block end.
    let offense = Range {
        start: cx.loc(call).name.start,
        end: cx.range(node).end,
    };
    let key_src = cx.raw_source(cx.range(key));
    let replacement = format!("pluck({key_src})");
    let current = cx.raw_source(offense);
    let msg = format!("Prefer `{replacement}` over `{current}`.");
    cx.emit_offense(offense, &msg, None);
    // Offense starts at the selector, so the `.` / `&.` before it is
    // preserved; replacing the offense with `pluck(key)` keeps safe
    // navigation (`x&.map` -> `x&.pluck`).
    cx.emit_edit(offense, &replacement);
}

#[cfg(test)]
mod tests {
    use super::Pluck;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_map_sym() {
        test::<Pluck>().expect_correction(
            indoc! {r#"
                x.map { |a| a[:foo] }
                  ^^^^^^^^^^^^^^^^^^^ Prefer `pluck(:foo)` over `map { |a| a[:foo] }`.
            "#},
            "x.pluck(:foo)\n",
        );
    }

    #[test]
    fn flags_collect_sym() {
        test::<Pluck>().expect_correction(
            indoc! {r#"
                x.collect { |a| a[:foo] }
                  ^^^^^^^^^^^^^^^^^^^^^^^ Prefer `pluck(:foo)` over `collect { |a| a[:foo] }`.
            "#},
            "x.pluck(:foo)\n",
        );
    }

    #[test]
    fn flags_csend_map() {
        test::<Pluck>().expect_correction(
            indoc! {r#"
                x&.map { |a| a[:foo] }
                   ^^^^^^^^^^^^^^^^^^^ Prefer `pluck(:foo)` over `map { |a| a[:foo] }`.
            "#},
            "x&.pluck(:foo)\n",
        );
    }

    #[test]
    fn flags_string_key() {
        test::<Pluck>().expect_correction(
            indoc! {r#"
                x.map { |a| a['foo'] }
                  ^^^^^^^^^^^^^^^^^^^^ Prefer `pluck('foo')` over `map { |a| a['foo'] }`.
            "#},
            "x.pluck('foo')\n",
        );
    }

    #[test]
    fn flags_numblock() {
        test::<Pluck>().expect_correction(
            indoc! {r#"
                x.map { _1[:foo] }
                  ^^^^^^^^^^^^^^^^ Prefer `pluck(:foo)` over `map { _1[:foo] }`.
            "#},
            "x.pluck(:foo)\n",
        );
    }

    #[test]
    fn flags_itblock() {
        test::<Pluck>().expect_correction(
            indoc! {r#"
                x.map { it[:foo] }
                  ^^^^^^^^^^^^^^^^ Prefer `pluck(:foo)` over `map { it[:foo] }`.
            "#},
            "x.pluck(:foo)\n",
        );
    }

    #[test]
    fn allows_unused_block_arg() {
        test::<Pluck>().expect_no_offenses("x.map { |a| b[:foo] }\n");
    }

    #[test]
    fn allows_block_arg_in_key() {
        test::<Pluck>().expect_no_offenses("x.map { |a| a[foo...a.to_something] }\n");
    }

    #[test]
    fn allows_multiple_block_args() {
        test::<Pluck>().expect_no_offenses("x.map { |_, obj| obj['id'] }\n");
    }

    #[test]
    fn allows_regexp_key() {
        test::<Pluck>().expect_no_offenses("x.map { |a| a[/regexp/] }\n");
    }

    #[test]
    fn allows_inside_iteration() {
        test::<Pluck>().expect_no_offenses("n.each do |x|\n  x.map { |a| a[:foo] }\nend\n");
    }

    #[test]
    fn allows_numblock_non_one() {
        test::<Pluck>().expect_no_offenses("x.map { _2['id'] }\n");
    }

    #[test]
    fn allows_key_same_as_arg() {
        test::<Pluck>().expect_no_offenses("lvar = do_something\nx.map { |id| lvar[id] }\n");
    }

    #[test]
    fn allows_bare_block_receiver() {
        // `foo do; x.map {...}; end` — ancestor block has no receiver, so flag.
        test::<Pluck>().expect_correction(
            indoc! {r#"
                foo do
                  x.map { |a| a[:foo] }
                    ^^^^^^^^^^^^^^^^^^^ Prefer `pluck(:foo)` over `map { |a| a[:foo] }`.
                end
            "#},
            "foo do\n  x.pluck(:foo)\nend\n",
        );
    }

    #[test]
    fn allows_rails_42() {
        test::<Pluck>()
            .with_target_rails_version(4, 2)
            .expect_no_offenses("x.map { |a| a[:foo] }\n");
    }

    #[test]
    fn flags_rails_50() {
        test::<Pluck>()
            .with_target_rails_version(5, 0)
            .expect_correction(
                indoc! {r#"
                x.map { |a| a[:foo] }
                  ^^^^^^^^^^^^^^^^^^^ Prefer `pluck(:foo)` over `map { |a| a[:foo] }`.
            "#},
                "x.pluck(:foo)\n",
            );
    }
}
murphy_plugin_api::submit_cop!(Pluck);
