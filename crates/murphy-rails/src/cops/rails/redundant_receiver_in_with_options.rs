//! `Rails/RedundantReceiverInWithOptions` — redundant receiver in `with_options`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/RedundantReceiverInWithOptions
//! upstream_version_checked: 2.35.0
//! version_added: "0.52"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: `on_block` / `on_numblock` / `on_itblock`
//!   for `with_options` with a body containing no nested blocks, all sends
//!   sharing the block-arg (`|assoc|`), `_1`, or `it` receiver, receiver-only
//!   offense with receiver+dot removal plus single block-arg (`|...|`)
//!   removal for explicit blocks.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct RedundantReceiverInWithOptions;

#[cop(
    name = "Rails/RedundantReceiverInWithOptions",
    description = "Checks for redundant receiver in `with_options`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RedundantReceiverInWithOptions {
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

fn is_with_options_call(cx: &Cx<'_>, call: NodeId) -> bool {
    matches!(*cx.kind(call), NodeKind::Send { .. } | NodeKind::Csend { .. })
        && cx.method_name(call) == Some("with_options")
}

fn check_block(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Block { call, args, body } = *cx.kind(node) else {
        return;
    };
    if !is_with_options_call(cx, call) {
        return;
    }
    let Some(body_id) = body.get() else {
        return;
    };
    if contains_block(cx, body_id) {
        return;
    }
    let Some(arg_name) = single_block_arg(cx, args) else {
        return;
    };
    let sends = collect_sends(cx, body_id);
    if sends.is_empty() {
        return;
    }
    let mut offenders = Vec::new();
    for &s in &sends {
        let Some(recv) = cx.call_receiver(s).get() else {
            return;
        };
        if !is_lvar_named(cx, recv, &arg_name) {
            return;
        }
        offenders.push(s);
    }
    for &s in &offenders {
        let recv = cx.call_receiver(s).get().unwrap();
        cx.emit_offense(
            cx.range(recv),
            "Redundant receiver in `with_options`.",
            None,
        );
        cx.emit_edit(cx.range(recv), "");
        if let Some(dot) = cx.call_operator_loc(s) {
            cx.emit_edit(dot, "");
        }
    }
    if let Some(arg_range) = block_arg_pipes_range(cx, node, call, body_id) {
        cx.emit_edit(arg_range, "");
    }
}

fn check_numblock(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Numblock { send, body, .. } = *cx.kind(node) else {
        return;
    };
    if !is_with_options_call(cx, send) {
        return;
    }
    let Some(body_id) = body.get() else {
        return;
    };
    if contains_block(cx, body_id) {
        return;
    }
    let sends = collect_sends(cx, body_id);
    if sends.is_empty() {
        return;
    }
    let mut offenders = Vec::new();
    for &s in &sends {
        let Some(recv) = cx.call_receiver(s).get() else {
            return;
        };
        if !is_lvar_named(cx, recv, "_1") {
            return;
        }
        offenders.push(s);
    }
    for &s in &offenders {
        let recv = cx.call_receiver(s).get().unwrap();
        cx.emit_offense(
            cx.range(recv),
            "Redundant receiver in `with_options`.",
            None,
        );
        cx.emit_edit(cx.range(recv), "");
        if let Some(dot) = cx.call_operator_loc(s) {
            cx.emit_edit(dot, "");
        }
    }
}

fn check_itblock(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Itblock { send, body } = *cx.kind(node) else {
        return;
    };
    if !is_with_options_call(cx, send) {
        return;
    }
    let Some(body_id) = body.get() else {
        return;
    };
    if contains_block(cx, body_id) {
        return;
    }
    let all = collect_sends(cx, body_id);
    // Prism/Murphy translates bare `it` to `(send nil :it)`; parser-gem
    // treats it as lvar, so upstream never sees these as sends. Exclude
    // them to match upstream `all?` semantics.
    let sends: Vec<NodeId> = all
        .into_iter()
        .filter(|&s| !is_bare_it(cx, s))
        .collect();
    if sends.is_empty() {
        return;
    }
    let mut offenders = Vec::new();
    for &s in &sends {
        let Some(recv) = cx.call_receiver(s).get() else {
            return;
        };
        if !is_it_receiver(cx, recv) {
            return;
        }
        offenders.push(s);
    }
    for &s in &offenders {
        let recv = cx.call_receiver(s).get().unwrap();
        cx.emit_offense(
            cx.range(recv),
            "Redundant receiver in `with_options`.",
            None,
        );
        cx.emit_edit(cx.range(recv), "");
        if let Some(dot) = cx.call_operator_loc(s) {
            cx.emit_edit(dot, "");
        }
    }
}

fn contains_block(cx: &Cx<'_>, id: NodeId) -> bool {
    if matches!(
        *cx.kind(id),
        NodeKind::Block { .. } | NodeKind::Numblock { .. } | NodeKind::Itblock { .. }
    ) {
        return true;
    }
    for child in cx.children(id) {
        if contains_block(cx, child) {
            return true;
        }
    }
    false
}

fn collect_sends(cx: &Cx<'_>, id: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    collect_sends_into(cx, id, &mut out);
    out
}

fn collect_sends_into(cx: &Cx<'_>, id: NodeId, out: &mut Vec<NodeId>) {
    if matches!(*cx.kind(id), NodeKind::Send { .. }) {
        out.push(id);
    }
    for child in cx.children(id) {
        // Body has no nested blocks (checked), so no need to prune.
        collect_sends_into(cx, child, out);
    }
}

fn single_block_arg(cx: &Cx<'_>, args: NodeId) -> Option<String> {
    let NodeKind::Args(list) = *cx.kind(args) else {
        return None;
    };
    let params = cx.list(list);
    if params.len() != 1 {
        return None;
    }
    if let NodeKind::Arg(s) = *cx.kind(params[0]) {
        Some(cx.symbol_str(s).to_owned())
    } else {
        None
    }
}

fn is_lvar_named(cx: &Cx<'_>, id: NodeId, name: &str) -> bool {
    if let NodeKind::Lvar(s) = *cx.kind(id) {
        cx.symbol_str(s) == name
    } else {
        false
    }
}

fn is_bare_it(cx: &Cx<'_>, id: NodeId) -> bool {
    matches!(*cx.kind(id), NodeKind::Send { .. })
        && cx.method_name(id) == Some("it")
        && cx.call_receiver(id).get().is_none()
        && cx.call_arguments(id).is_empty()
}

fn is_it_receiver(cx: &Cx<'_>, id: NodeId) -> bool {
    // Murphy translates bare `it` to `(send nil :it)` with no args.
    if let NodeKind::Lvar(s) = *cx.kind(id) {
        return cx.symbol_str(s) == "it";
    }
    matches!(*cx.kind(id), NodeKind::Send { .. })
        && cx.method_name(id) == Some("it")
        && cx.call_receiver(id).get().is_none()
        && cx.call_arguments(id).is_empty()
}

fn block_arg_pipes_range(
    cx: &Cx<'_>,
    _block: NodeId,
    call: NodeId,
    body: NodeId,
) -> Option<Range> {
    // NOTE: Murphy's `Block.call` (Send) range covers the whole
    // `with_options ... do |x| ... end` (verified vs translate), so the
    // header gap must be `call.start..body.start`, not `call.end..`.
    let src = cx.source().as_bytes();
    let call_start = cx.range(call).start as usize;
    let body_start = cx.range(body).start as usize;
    if body_start <= call_start || call_start >= src.len() {
        return None;
    }
    let header_end = body_start.min(src.len());
    let header = &src[call_start..header_end];
    let header_str = std::str::from_utf8(header).ok()?;
    // Last `|...|` in the header is the block-arg list (`do |assoc|`,
    // `{ |a, b|`, `do|assoc|`). Body cannot contain `|` before its start.
    let pipe_start_rel = header_str.rfind('|')?;
    // Find matching opening `|` before it.
    let before = &header_str[..pipe_start_rel];
    let pipe_open_rel = before.rfind('|')?;
    let abs_start_pipes = call_start + pipe_open_rel;
    let abs_end_pipes = call_start + pipe_start_rel + 1;
    // Include preceding spaces/tabs (upstream space-search).
    let mut start = abs_start_pipes;
    while start > 0 && (src[start - 1] == b' ' || src[start - 1] == b'\t') {
        start -= 1;
    }
    Some(Range {
        start: start as u32,
        end: abs_end_pipes as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::RedundantReceiverInWithOptions;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_explicit_receiver() {
        test::<RedundantReceiverInWithOptions>().expect_offense(indoc! {r#"
            class Account < ApplicationRecord
              with_options dependent: :destroy do |assoc|
                assoc.has_many :customers
                ^^^^^ Redundant receiver in `with_options`.
                assoc.has_many :products
                ^^^^^ Redundant receiver in `with_options`.
                assoc.has_many :invoices
                ^^^^^ Redundant receiver in `with_options`.
                assoc.has_many :expenses
                ^^^^^ Redundant receiver in `with_options`.
              end
            end
        "#});
    }

    #[test]
    fn corrects_explicit_receiver() {
        test::<RedundantReceiverInWithOptions>().expect_correction(
            indoc! {r#"
                class Account < ApplicationRecord
                  with_options dependent: :destroy do |assoc|
                    assoc.has_many :customers
                    ^^^^^ Redundant receiver in `with_options`.
                    assoc.has_many :products
                    ^^^^^ Redundant receiver in `with_options`.
                    assoc.has_many :invoices
                    ^^^^^ Redundant receiver in `with_options`.
                    assoc.has_many :expenses
                    ^^^^^ Redundant receiver in `with_options`.
                  end
                end
            "#},
            "class Account < ApplicationRecord\n  with_options dependent: :destroy do\n    has_many :customers\n    has_many :products\n    has_many :invoices\n    has_many :expenses\n  end\nend\n",
        );
    }

    #[test]
    fn corrects_numblock() {
        test::<RedundantReceiverInWithOptions>().expect_correction(
            indoc! {r#"
                class Account < ApplicationRecord
                  with_options dependent: :destroy do
                    _1.has_many :customers
                    ^^ Redundant receiver in `with_options`.
                    _1.has_many :products
                    ^^ Redundant receiver in `with_options`.
                    _1.has_many :invoices
                    ^^ Redundant receiver in `with_options`.
                    _1.has_many :expenses
                    ^^ Redundant receiver in `with_options`.
                  end
                end
            "#},
            "class Account < ApplicationRecord\n  with_options dependent: :destroy do\n    has_many :customers\n    has_many :products\n    has_many :invoices\n    has_many :expenses\n  end\nend\n",
        );
    }

    #[test]
    fn corrects_itblock() {
        test::<RedundantReceiverInWithOptions>().expect_correction(
            indoc! {r#"
                class Account < ApplicationRecord
                  with_options dependent: :destroy do
                    it.has_many :customers
                    ^^ Redundant receiver in `with_options`.
                    it.has_many :products
                    ^^ Redundant receiver in `with_options`.
                    it.has_many :invoices
                    ^^ Redundant receiver in `with_options`.
                    it.has_many :expenses
                    ^^ Redundant receiver in `with_options`.
                  end
                end
            "#},
            "class Account < ApplicationRecord\n  with_options dependent: :destroy do\n    has_many :customers\n    has_many :products\n    has_many :invoices\n    has_many :expenses\n  end\nend\n",
        );
    }

    #[test]
    fn allows_implicit_receiver() {
        test::<RedundantReceiverInWithOptions>().expect_no_offenses(indoc! {r#"
            class Account < ApplicationRecord
              with_options dependent: :destroy do
                has_many :customers
                has_many :products
                has_many :invoices
                has_many :expenses
              end
            end
        "#});
    }

    #[test]
    fn corrects_nested_sends_single_line() {
        test::<RedundantReceiverInWithOptions>().expect_correction(
            indoc! {r#"
                with_options options: false do |merger|
                  merger.invoke(merger.something)
                  ^^^^^^ Redundant receiver in `with_options`.
                                ^^^^^^ Redundant receiver in `with_options`.
                end
            "#},
            "with_options options: false do\n  invoke(something)\nend\n",
        );
    }

    #[test]
    fn allows_different_receivers() {
        test::<RedundantReceiverInWithOptions>().expect_no_offenses(indoc! {r#"
            client = ApplicationClient.new
            with_options options: false do |merger|
              client.invoke(merger.something, something)
            end
        "#});
    }

    #[test]
    fn allows_nested_block() {
        test::<RedundantReceiverInWithOptions>().expect_no_offenses(indoc! {r#"
            with_options options: false do |merger|
              merger.invoke
              with_another_method do |another_receiver|
                merger.invoke(another_receiver)
              end
            end
        "#});
    }

    #[test]
    fn allows_no_block_arg() {
        test::<RedundantReceiverInWithOptions>().expect_no_offenses(indoc! {r#"
            with_options do
              obj.do_something
            end
        "#});
    }

    #[test]
    fn allows_empty() {
        test::<RedundantReceiverInWithOptions>().expect_no_offenses(indoc! {r#"
            with_options options: false do |merger|
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(RedundantReceiverInWithOptions);
