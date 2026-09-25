//! Shared internal helpers for the murphy-rspec pack.
//!
//! Anything reusable across cops belongs here so each cop file stays
//! focused on its single rule. Visibility is `pub(crate)`: helpers are
//! an implementation detail of the pack, not part of its public surface.

use murphy_plugin_api::{Cx, NodeId, NodeKind, OptNodeId, Range, SourceTokenKind};

/// `true` when `call` is a bare RSpec example call. Matches the receiver
/// shape `RSpec/ExampleLength` and `RSpec/MultipleExpectations` both
/// police: explicit-receiver forms like `Other.it "x"` are some other
/// DSL's `it` and are skipped.
/// Compute the offense range for an RSpec example call.
///
/// The range covers the call node (method name + args), trimmed to end just
/// before the block opener token (`do` or `{`), skipping any trailing comment
/// or newline tokens between the last real arg and the opener.
///
/// Shared by `ExampleLength` and `MultipleExpectations`.
pub(crate) fn example_call_range(cx: &Cx<'_>, call: NodeId, body: NodeId) -> Range {
    let call_range = cx.range(call);
    let body_range = cx.range(body);
    if body_range.start <= call_range.start {
        return call_range;
    }

    let source = cx.source().as_bytes();

    // Lower bound: just after the method name selector (e.g. after `it`).
    // Upper bound: start of the body node.
    // Pick the *last* `do`/`{` in this window to skip hash-literal `{` inside args.
    let name_end = cx.node(call).loc.name.end;
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
        });

    let offense_end = match opener {
        Some(opener_tok) => {
            // Walk backward from the opener to find the last real token,
            // skipping Comment/Newline/IgnoredNewline.
            let last_real = cx
                .tokens_in(Range {
                    start: call_range.start,
                    end: opener_tok.range.start,
                })
                .iter()
                .rev()
                .find(|t| {
                    !matches!(
                        t.kind,
                        SourceTokenKind::Comment
                            | SourceTokenKind::Newline
                            | SourceTokenKind::IgnoredNewline
                    )
                });
            last_real.map(|t| t.range.end).unwrap_or(call_range.start)
        }
        None => name_end,
    };

    Range {
        start: call_range.start,
        end: offense_end,
    }
}

pub(crate) fn is_example_call(cx: &Cx<'_>, call: NodeId) -> bool {
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(call)
    else {
        return false;
    };
    if receiver != OptNodeId::NONE {
        return false;
    }
    matches!(
        cx.symbol_str(method),
        "it" | "specify"
            | "example"
            | "fit"
            | "fspecify"
            | "fexample"
            | "xit"
            | "xspecify"
            | "xexample"
            | "skip"
            | "pending"
    )
}

/// `true` when `receiver` is bare (`describe`) or the top-level `RSpec`
/// constant (`RSpec.describe`, `::RSpec.describe`).
///
/// Mirrors RuboCop-RSpec's `#rspec?` (`{ #explicit_rspec? nil? }`).
/// The translator collapses bare `RSpec` and cbase `::RSpec` to the same
/// `Const { scope: None }`, so both spellings match here.
pub(crate) fn is_rspec_or_bare_receiver(cx: &Cx<'_>, receiver: OptNodeId) -> bool {
    let Some(rid) = receiver.get() else {
        return true;
    };
    matches!(
        *cx.kind(rid),
        NodeKind::Const { scope, name }
            if scope == OptNodeId::NONE && cx.symbol_str(name) == "RSpec"
    )
}

/// `true` when `block_id` is a top-level group: every ancestor up to the
/// root is one of `Begin`/`Class`/`Module`.
///
/// Mirrors upstream `TopLevelGroup#top_level_nodes`, which walks through
/// `begin`/`class`/`module` only and does not walk through `sclass`
/// (`class << self`), `Block`, `If`, `Def`, etc.
pub(crate) fn is_top_level_block(cx: &Cx<'_>, block_id: NodeId) -> bool {
    let mut cur = block_id;
    while let Some(p) = cx.parent(cur).get() {
        match *cx.kind(p) {
            NodeKind::Begin(_) | NodeKind::Class { .. } | NodeKind::Module { .. } => {
                cur = p;
            }
            _ => return false,
        }
    }
    true
}

/// `true` when `name` is an RSpec example-group selector
/// (`ExampleGroups.all` in `Language`): regular + skipped + focused.
pub(crate) fn is_example_group_name(name: &str) -> bool {
    matches!(
        name,
        "describe"
            | "context"
            | "feature"
            | "example_group"
            | "xdescribe"
            | "xcontext"
            | "xfeature"
            | "fdescribe"
            | "fcontext"
            | "ffeature"
    )
}

/// `true` when `call` is an RSpec example-group entrypoint with an
/// RSpec-or-bare receiver. Used by `MultipleDescribes` and shared
/// group detection.
pub(crate) fn is_example_group_call(cx: &Cx<'_>, call: NodeId) -> bool {
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(call)
    else {
        return false;
    };
    if !is_rspec_or_bare_receiver(cx, receiver) {
        return false;
    }
    is_example_group_name(cx.symbol_str(method))
}

/// `true` when `name` is an RSpec hook selector (`Hooks.all`).
pub(crate) fn is_hook_name(name: &str) -> bool {
    matches!(
        name,
        "before"
            | "after"
            | "around"
            | "prepend_before"
            | "append_before"
            | "prepend_after"
            | "append_after"
    )
}

/// Range of a `Send` without its block wrapper (`fdescribe 'x'` instead of
/// `fdescribe 'x' do ... end`).
///
/// Murphy's translator assigns the `Send` and its wrapping `Block` the same
/// `expression` range (both cover `before {}`), while RuboCop's `Send`
/// range excludes the block. This trims to the pre-opener end so offense
/// ranges match upstream's `add_offense(send)`.
///
/// - No wrapping `Block` → `cx.range(send)` unchanged.
/// - Wrapping `Block` with a body → delegates to [`example_call_range`]
///   (the non-empty trimming already proven by `ExampleLength`).
/// - Wrapping `Block` without a body (empty `do; end` / `{}`) → finds the
///   `do`/`{` opener inside the `Block` range and returns
///   `start..last_real_token_end_before_opener`, mirroring
///   `example_call_range`'s trailing-comment/newline skipping.
pub(crate) fn send_without_block_range(cx: &Cx<'_>, send: NodeId) -> Range {
    let call_range = cx.range(send);
    let Some(block_id) = cx.block_node(send).get() else {
        return call_range;
    };
    let NodeKind::Block { body, .. } = *cx.kind(block_id) else {
        return call_range;
    };
    if let Some(body_id) = body.get() {
        return example_call_range(cx, send, body_id);
    }
    // Empty body: locate the opener inside the Block range.
    let block_range = cx.range(block_id);
    let name_end = cx.node(send).loc.name.end;
    let source = cx.source().as_bytes();
    let opener = cx
        .tokens_in(Range {
            start: name_end,
            end: block_range.end,
        })
        .iter()
        .rev()
        .find(|t| {
            t.kind == SourceTokenKind::LeftBrace
                || (t.kind == SourceTokenKind::Other
                    && &source[t.range.start as usize..t.range.end as usize] == b"do")
        });
    let Some(opener_tok) = opener else {
        return call_range;
    };
    let last_real = cx
        .tokens_in(Range {
            start: call_range.start,
            end: opener_tok.range.start,
        })
        .iter()
        .rev()
        .find(|t| {
            !matches!(
                t.kind,
                SourceTokenKind::Comment
                    | SourceTokenKind::Newline
                    | SourceTokenKind::IgnoredNewline
            )
        });
    let end = last_real
        .map(|tok| tok.range.end)
        .unwrap_or(call_range.start);
    Range {
        start: call_range.start,
        end,
    }
}
