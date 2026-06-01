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
