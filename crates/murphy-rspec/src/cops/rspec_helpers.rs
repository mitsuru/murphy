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

/// `true` when `call` is a bare `let` / `let!` send (any args).
///
/// Mirrors the block arm of upstream `Language#let?` (`(block (send nil?
/// #Helpers.all ...) ...)`); `Helpers.all` is exactly `let` / `let!`.
/// The bare-send `block_pass` arm (`let(:a, &blk)`) can never be a
/// single-line brace block, so the align cops only need this predicate.
pub(crate) fn is_bare_let_call(cx: &Cx<'_>, call: NodeId) -> bool {
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(call)
    else {
        return false;
    };
    if receiver != OptNodeId::NONE {
        return false;
    }
    matches!(cx.symbol_str(method), "let" | "let!")
}

/// Every single-line bare `let` / `let!` block in the file, in document
/// order.
///
/// Mirrors upstream `AlignLetBrace#single_line_lets`
/// (`root.each_node(:block).select { let? && single_line? }`): only
/// `Block` nodes (never `Numblock` / `Itblock`, same as upstream's
/// `:block` search), bare `let` / `let!` calls, spanning exactly one
/// line. Sorted by (first line, start offset) so chunking below sees
/// document order.
pub(crate) fn single_line_let_blocks(cx: &Cx<'_>) -> Vec<NodeId> {
    let root = cx.root();
    let mut out: Vec<NodeId> = core::iter::once(root)
        .chain(cx.descendants(root))
        .filter(|&id| {
            let NodeKind::Block { call, .. } = *cx.kind(id) else {
                return false;
            };
            if !is_bare_let_call(cx, call) {
                return false;
            }
            cx.is_single_line(id)
        })
        .collect();
    let src = cx.source();
    out.sort_by_key(|&id| {
        let range = cx.range(id);
        (line_index_of_offset(src, range.start), range.start)
    });
    out
}

/// Maximal runs of `lets` on consecutive first-lines.
///
/// Mirrors upstream `AlignLetBrace#adjacent_let_chunks` (chunk by
/// `last_line + 1 == line` over the document-ordered lets). A blank
/// line, a comment line, or any other node on an in-between line all
/// break the run, since only the lets' own first lines are compared.
pub(crate) fn adjacent_let_chunks(cx: &Cx<'_>, lets: &[NodeId]) -> Vec<Vec<NodeId>> {
    let src = cx.source();
    let mut chunks: Vec<Vec<NodeId>> = Vec::new();
    for &id in lets {
        let line = line_index_of_offset(src, cx.range(id).start);
        let extend = chunks.last().is_some_and(|last: &Vec<NodeId>| {
            let prev_line =
                line_index_of_offset(src, cx.range(*last.last().expect("non-empty chunk")).start);
            prev_line + 1 == line
        });
        if extend {
            chunks.last_mut().expect("checked above").push(id);
        } else {
            chunks.push(vec![id]);
        }
    }
    chunks
}

/// 0-based line index containing byte `offset`.
pub(crate) fn line_index_of_offset(src: &str, offset: u32) -> usize {
    let offset = (offset as usize).min(src.len());
    src.as_bytes()[..offset]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
}

/// Display column (in characters, not bytes) of byte `offset` within
/// its line.
///
/// Upstream compares `loc.column` (character columns); byte offsets
/// would drift after multibyte text (e.g. a non-ASCII `let` body
/// before the closing brace on the same line).
pub(crate) fn char_column_of_offset(src: &str, offset: u32) -> usize {
    let offset = (offset as usize).min(src.len());
    let line_start = src.as_bytes()[..offset]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |pos| pos + 1);
    src[line_start..offset].chars().count()
}

/// The block's opening token range: the `{` that opens the brace body,
/// or the `do` keyword for single-line `do...end` lets.
///
/// The opener is the last `{` ending at or before the body start, so
/// hash braces inside the args (`let(:a, {x: 1}) { b }`) or at the
/// body start (`let(:a) { {x: 1} }`) never shadow it. With no body
/// (`let(:a) {}`) every `{` in range qualifies and the last one — the
/// opener — wins. Returns `None` when no brace precedes the body (a
/// `do...end` block falls through to the `do` search; anything else
/// is not a brace block).
pub(crate) fn block_open_token(cx: &Cx<'_>, block: NodeId) -> Option<Range> {
    let NodeKind::Block { body, .. } = *cx.kind(block) else {
        return None;
    };
    let block_range = cx.range(block);
    let body_start = body.get().map_or(block_range.end, |id| cx.range(id).start);
    let toks = cx.tokens_in(block_range);
    if let Some(tok) = toks
        .iter()
        .rev()
        .find(|t| t.kind == SourceTokenKind::LeftBrace && t.range.end <= body_start)
    {
        return Some(tok.range);
    }
    // Single-line `do...end` fallback (`let(:a) do b end`): last bare
    // `do` before the body start, mirroring upstream `loc.begin`.
    toks.iter()
        .rev()
        .find(|t| {
            t.kind == SourceTokenKind::Other
                && t.range.end <= body_start
                && cx.token_text(**t) == "do"
        })
        .map(|t| t.range)
}

/// The block's closing token range: the `}` (or `end` keyword) ending
/// exactly at the block expression end.
///
/// Mirrors upstream `loc.end`. Returns `None` when no such token
/// exists (never happens for well-formed blocks; the align cops skip
/// the node then).
pub(crate) fn block_close_token(cx: &Cx<'_>, block: NodeId) -> Option<Range> {
    let NodeKind::Block { .. } = *cx.kind(block) else {
        return None;
    };
    let block_range = cx.range(block);
    cx.tokens_in(block_range)
        .iter()
        .rev()
        .find(|t| {
            t.range.end == block_range.end
                && (t.kind == SourceTokenKind::RightBrace
                    || (t.kind == SourceTokenKind::Other && cx.token_text(**t) == "end"))
        })
        .map(|t| t.range)
}

/// Example selectors (`Examples.all` in rubocop-rspec's default config):
/// regular (`it`, `specify`, `example`, `scenario`, `its`), focused
/// (`fit`, `fspecify`, `fexample`, `fscenario`, `focus`), skipped
/// (`xit`, `xspecify`, `xexample`, `xscenario`, `skip`) and pending
/// (`pending`).
pub(crate) fn is_example_name(name: &str) -> bool {
    matches!(
        name,
        "it" | "specify"
            | "example"
            | "scenario"
            | "its"
            | "fit"
            | "fspecify"
            | "fexample"
            | "fscenario"
            | "focus"
            | "xit"
            | "xspecify"
            | "xexample"
            | "xscenario"
            | "skip"
            | "pending"
    )
}

/// Shared-group selectors (`SharedGroups.all`).
pub(crate) fn is_shared_group_name(name: &str) -> bool {
    matches!(
        name,
        "shared_examples" | "shared_examples_for" | "shared_context"
    )
}

/// Include selectors (`Includes.all`): example includes plus
/// `include_context`.
pub(crate) fn is_include_name(name: &str) -> bool {
    matches!(
        name,
        "it_behaves_like" | "it_should_behave_like" | "include_examples" | "include_context"
    )
}

/// `true` when `call` is a spec-group entrypoint (example groups or
/// shared groups) with a bare or `RSpec` receiver.
///
/// Mirrors upstream `spec_group?` (`{(shared, example) groups}` with
/// `#rspec?` = explicit-`RSpec` or bare).
pub(crate) fn is_spec_group_call(cx: &Cx<'_>, call: NodeId) -> bool {
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(call)
    else {
        return false;
    };
    if !is_rspec_or_bare_receiver(cx, receiver) {
        return false;
    }
    let name = cx.symbol_str(method);
    is_example_group_name(name) || is_shared_group_name(name)
}

/// `true` when `id` is a `Block` that changes helper scope: a nested
/// example/shared group (bare or `RSpec` receiver) or a bare include.
///
/// Mirrors `ExampleGroup#scope_change?` (`(block { (send #rspec?
/// {#SharedGroups.all #ExampleGroups.all} ...) (send nil?
/// #Includes.all ...) } ...)`).
pub(crate) fn is_scope_change_block(cx: &Cx<'_>, id: NodeId) -> bool {
    let NodeKind::Block { call, .. } = *cx.kind(id) else {
        return false;
    };
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(call)
    else {
        return false;
    };
    let name = cx.symbol_str(method);
    if is_include_name(name) {
        return receiver == OptNodeId::NONE;
    }
    is_rspec_or_bare_receiver(cx, receiver)
        && (is_example_group_name(name) || is_shared_group_name(name))
}

/// `true` when `id` is a bare-example `Block`.
///
/// Mirrors `Language#example?` (`(block (send nil? #Examples.all ...)
/// ...)`).
pub(crate) fn is_bare_example_block(cx: &Cx<'_>, id: NodeId) -> bool {
    let NodeKind::Block { call, .. } = *cx.kind(id) else {
        return false;
    };
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(call)
    else {
        return false;
    };
    receiver == OptNodeId::NONE && is_example_name(cx.symbol_str(method))
}

/// `true` when `id` is a `let?` node: a bare `let` / `let!` block, or
/// the bare-send form with exactly one value argument plus a
/// `BlockPass` (`let(:foo, &blk)`).
///
/// Mirrors `Language#let?` (`{(block (send nil? #Helpers.all ...) ...)
/// (send nil? #Helpers.all _ block_pass)}`).
pub(crate) fn is_let_node(cx: &Cx<'_>, id: NodeId) -> bool {
    match *cx.kind(id) {
        NodeKind::Block { call, .. } => is_bare_let_call(cx, call),
        NodeKind::Send {
            receiver,
            method,
            args,
        } => {
            if receiver != OptNodeId::NONE {
                return false;
            }
            if !matches!(cx.symbol_str(method), "let" | "let!") {
                return false;
            }
            let args = cx.list(args);
            args.len() == 2 && matches!(*cx.kind(args[1]), NodeKind::BlockPass(_))
        }
        _ => false,
    }
}

/// `true` when `id` is a `subject?` node: a bare `subject` /
/// `subject!` block.
///
/// Mirrors `Language#subject?` (`(block (send nil? #Subjects.all ...)
/// ...)`).
pub(crate) fn is_subject_block(cx: &Cx<'_>, id: NodeId) -> bool {
    let NodeKind::Block { call, .. } = *cx.kind(id) else {
        return false;
    };
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(call)
    else {
        return false;
    };
    receiver == OptNodeId::NONE && matches!(cx.symbol_str(method), "subject" | "subject!")
}
