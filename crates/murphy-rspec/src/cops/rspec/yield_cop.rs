//! `RSpec/Yield` — call the block with `.and_yield` instead of `block.call`.
//!
//! File is `yield_cop.rs` (`yield` is a Rust keyword, so `yield.rs` cannot
//! be a module under `automod::dir!`).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/Yield
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_block` (numblocks deliberately ignored per the
//!   `InternalAffairs/NumblockHandler` disable): `method_on_stub?`
//!   (`(send nil? :receive ...)` anywhere under the block's `send_node`)
//!   gates, then `block_arg` (`(args (blockarg $_))` — exactly one
//!   `Blockarg`, so plain `|block|` never matches) captures the block
//!   name, then `calling_block?` requires the body to be only
//!   `block.call(...)` calls (`(send (lvar %) :call ...)`): a single
//!   `Send` body, or a `Begin` whose every child is one. The offense is
//!   `block_range` (`loc.begin` to `loc.end`: `{ ... }` or `do ... end`)
//!   rebuilt here from tokens (opener `{` / `do` to closer `}` / `end`)
//!   per `add_offense(range)` with `Use `.and_yield`.`. Detection is at
//!   parity (verified vs 3.7.0, including chained `.with`, multi-call
//!   bodies, args to `call`, and the non-only-statement clean case);
//!   autocorrect (replace with `.and_yield(args)`) is not ported in this
//!   batch — same convention as `RSpec/BeNil` (status: partial,
//!   autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block`:
//!
//! - `allow(foo).to receive(:bar) { |&block| block.call }` — flagged
//!   (the `{ ... }` range).
//! - `allow(foo).to receive(:bar) { |&block| block.call(1, 2) }` —
//!   flagged (args still match).
//! - `allow(foo).to receive(:bar).with(anything) { |&block| block.call }`
//!   — chained, flagged.
//! - `allow(foo).to receive(:bar) { |block| block.call }` — plain arg,
//!   clean.
//! - `allow(foo).to receive(:bar) do |&block|; x = block.call; y(x); end`
//!   — non-only statement, clean.
//!
//! ## No autocorrect
//!
//! Upstream replaces with `.and_yield(args)`. This batch reports only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, Range, SourceTokenKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct Yield;

#[cop(
    name = "RSpec/Yield",
    description = "Checks for calling a block within a stub.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl Yield {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, args, body } = *cx.kind(node) else {
            return;
        };
        if !method_on_stub(cx, call) {
            return;
        }
        let Some(block_name) = single_blockarg_name(cx, args) else {
            return;
        };
        let Some(body_id) = body.get() else {
            return;
        };
        if !is_only_block_calls(cx, body_id, &block_name) {
            return;
        }
        let Some(range) = block_braces_range(cx, node, call, body_id) else {
            return;
        };
        cx.emit_offense(range, "Use `.and_yield`.", None);
    }
}

/// `method_on_stub?`: `(send nil? :receive ...)` anywhere under
/// `send_node` (the node itself or any descendant).
fn method_on_stub(cx: &Cx<'_>, send_node: NodeId) -> bool {
    if is_bare_receive(cx, send_node) {
        return true;
    }
    cx.descendants(send_node)
        .iter()
        .any(|&id| is_bare_receive(cx, id))
}

fn is_bare_receive(cx: &Cx<'_>, id: NodeId) -> bool {
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(id)
    else {
        return false;
    };
    if receiver != OptNodeId::NONE {
        return false;
    }
    cx.symbol_str(method) == "receive"
}

/// `block_arg`: `(args (blockarg $_))` — exactly one `Blockarg`; returns
/// its name. Plain `|block|` (`Arg`) never matches.
fn single_blockarg_name(cx: &Cx<'_>, args_id: NodeId) -> Option<String> {
    let NodeKind::Args(list) = *cx.kind(args_id) else {
        return None;
    };
    let ids = cx.list(list);
    if ids.len() != 1 {
        return None;
    }
    let NodeKind::Blockarg(sym) = *cx.kind(ids[0]) else {
        return None;
    };
    Some(cx.symbol_str(sym).to_owned())
}

/// `calling_block?`: the body is a single `block.call` or a `Begin`
/// whose every child is a `block.call`.
fn is_only_block_calls(cx: &Cx<'_>, body: NodeId, block_name: &str) -> bool {
    match *cx.kind(body) {
        NodeKind::Begin(list) => {
            let members = cx.list(list);
            !members.is_empty()
                && members.iter().all(|&id| is_block_call(cx, id, block_name))
        }
        _ => is_block_call(cx, body, block_name),
    }
}

/// `block_call?`: `(send (lvar %) :call ...)` — receiver is an `Lvar`
/// with the blockarg name, method is `call`, any args.
fn is_block_call(cx: &Cx<'_>, id: NodeId, block_name: &str) -> bool {
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(id)
    else {
        return false;
    };
    if cx.symbol_str(method) != "call" {
        return false;
    }
    let Some(recv) = receiver.get() else {
        return false;
    };
    let NodeKind::Lvar(sym) = *cx.kind(recv) else {
        return false;
    };
    cx.symbol_str(sym) == block_name
}

/// `block_range`: opener (`{` / `do`) start to closer (`}` / `end`) end.
///
/// Rebuilt from tokens: the opener is the last `{` / `do` between the
/// call selector end and the body start (mirroring `example_call_range`'s
/// hash-`{` skipping); the closer is the first `}` / `end` between the
/// body end and the block end.
fn block_braces_range(
    cx: &Cx<'_>,
    block_id: NodeId,
    call_id: NodeId,
    body_id: NodeId,
) -> Option<Range> {
    let block_range = cx.range(block_id);
    let body_range = cx.range(body_id);
    let name_end = cx.node(call_id).loc.name.end;
    let source = cx.source().as_bytes();

    let opener = cx
        .tokens_in(Range {
            start: name_end,
            end: body_range.start,
        })
        .iter()
        .rev()
        .find(|t| {
            t.kind == SourceTokenKind::LeftBrace
                || (t.kind == SourceTokenKind::Other
                    && &source[t.range.start as usize..t.range.end as usize] == b"do")
        })?;
    let closer = cx
        .tokens_in(Range {
            start: body_range.end,
            end: block_range.end,
        })
        .iter()
        .find(|t| {
            t.kind == SourceTokenKind::RightBrace
                || (t.kind == SourceTokenKind::Other
                    && &source[t.range.start as usize..t.range.end as usize] == b"end")
        })?;
    Some(Range {
        start: opener.range.start,
        end: closer.range.end,
    })
}

#[cfg(test)]
mod tests {
    use super::Yield;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_block_call_braces() {
        test::<Yield>().expect_offense(indoc! {r#"
                allow(foo).to receive(:bar) { |&block| block.call }
                                            ^^^^^^^^^^^^^^^^^^^^^^^ Use `.and_yield`.
            "#});
    }

    #[test]
    fn flags_block_call_with_args() {
        test::<Yield>().expect_offense(indoc! {r#"
                allow(foo).to receive(:bar) { |&block| block.call(1, 2) }
                                            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `.and_yield`.
            "#});
    }

    #[test]
    fn flags_chained_with() {
        test::<Yield>().expect_offense(indoc! {r#"
                allow(foo).to receive(:bar).with(anything) { |&block| block.call }
                                                           ^^^^^^^^^^^^^^^^^^^^^^^ Use `.and_yield`.
            "#});
    }

    #[test]
    fn ignores_plain_arg() {
        test::<Yield>().expect_no_offenses(indoc! {r#"
                allow(foo).to receive(:bar) { |block| block.call }
            "#});
    }

    #[test]
    fn ignores_non_only_statement() {
        test::<Yield>().expect_no_offenses(indoc! {r#"
                allow(foo).to receive(:bar) do |&block|
                  result = block.call
                  transform(result)
                end
            "#});
    }

    #[test]
    fn ignores_no_receive() {
        test::<Yield>().expect_no_offenses(indoc! {r#"
                allow(foo).to be_bar { |&block| block.call }
            "#});
    }

    #[test]
    fn ignores_numblock() {
        test::<Yield>().expect_no_offenses(indoc! {r#"
                allow(foo).to receive(:bar) { _1.call }
            "#});
    }
}

murphy_plugin_api::submit_cop!(Yield);
