//! `Layout/SpaceInsideParens` — flags extra spaces immediately inside
//! parentheses. Mirrors RuboCop's same-named cop.

use murphy_plugin_api::{
    Cop, Cx, NoOptions, NodeCop, NodeId, NodeKind, NodeKindTag, Range, Severity,
};

/// `NodeKind` discriminants — declaration order is frozen by ADR 0037.
const SEND_TAG: NodeKindTag = NodeKindTag(17);
const BEGIN_TAG: NodeKindTag = NodeKindTag(28);
const DEF_TAG: NodeKindTag = NodeKindTag(32);
const ARGS_TAG: NodeKindTag = NodeKindTag(35);
const UNKNOWN_TAG: NodeKindTag = NodeKindTag(37);
const SCLASS_TAG: NodeKindTag = NodeKindTag(50);

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SpaceInsideParens;

impl Cop for SpaceInsideParens {
    type Options = NoOptions;
    const NAME: &'static str = "Layout/SpaceInsideParens";
    const DESCRIPTION: &'static str = "Flag extra spaces immediately inside parentheses.";
    const DEFAULT_SEVERITY: Option<Severity> = Some(Severity::Warning);
    const DEFAULT_ENABLED: Option<bool> = Some(true);
}

impl NodeCop for SpaceInsideParens {
    const KINDS: &'static [NodeKindTag] = &[
        SEND_TAG,
        BEGIN_TAG,
        DEF_TAG,
        SCLASS_TAG,
        ARGS_TAG,
        UNKNOWN_TAG,
    ];

    fn check(&self, node: NodeId, cx: &Cx<'_>) {
        let range = cx.range(node);
        let src = cx.raw_source(range);
        match cx.kind(node) {
            NodeKind::Send { .. } => check_outer_parens(cx, range.start, src),
            NodeKind::Begin(_) | NodeKind::Unknown => {
                if is_grouped_expression(src) {
                    check_pair(cx, range.start, src, 0, src.len() - 1);
                }
            }
            NodeKind::Def { .. } => {
                if let Some((open, close)) = def_argument_parens(src) {
                    check_pair(cx, range.start, src, open, close);
                }
            }
            NodeKind::Sclass { .. } | NodeKind::Args(_) => {
                // Hooked for parity with the design note. Their current
                // translated ranges do not include parentheses, so there is
                // nothing to emit here.
            }
            _ => {}
        }
    }
}

fn is_grouped_expression(src: &str) -> bool {
    let bytes = src.as_bytes();
    matches!((bytes.first(), bytes.last()), (Some(b'('), Some(b')')))
}

fn check_outer_parens(cx: &Cx<'_>, base: u32, src: &str) {
    let bytes = src.as_bytes();
    let Some(open) = bytes.iter().position(|b| *b == b'(') else {
        return;
    };
    let Some(close) = bytes.iter().rposition(|b| *b == b')') else {
        return;
    };
    if open < close {
        check_pair(cx, base, src, open, close);
    }
}

fn def_argument_parens(src: &str) -> Option<(usize, usize)> {
    let head_end = src.find('\n').unwrap_or(src.len());
    let head = &src[..head_end];
    let bytes = head.as_bytes();
    let open = bytes.iter().position(|b| *b == b'(')?;
    let close = bytes.iter().rposition(|b| *b == b')')?;
    (open < close).then_some((open, close))
}

fn check_pair(cx: &Cx<'_>, base: u32, src: &str, open: usize, close: usize) {
    let bytes = src.as_bytes();
    let after_open = whitespace_run_forward(bytes, open + 1, close);
    let before_close = whitespace_run_backward(bytes, open, close);

    if let Some((start, end)) = after_open {
        emit_space(cx, base, start, end);
    }
    if let Some((start, end)) = before_close {
        if after_open != Some((start, end)) {
            emit_space(cx, base, start, end);
        }
    }
}

fn whitespace_run_forward(bytes: &[u8], mut start: usize, limit: usize) -> Option<(usize, usize)> {
    let original = start;
    while start < limit && is_inline_space(bytes[start]) {
        start += 1;
    }
    (start > original).then_some((original, start))
}

fn whitespace_run_backward(bytes: &[u8], limit: usize, mut end: usize) -> Option<(usize, usize)> {
    let original = end;
    while end > limit + 1 && is_inline_space(bytes[end - 1]) {
        end -= 1;
    }
    (end < original).then_some((end, original))
}

fn is_inline_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t')
}

fn emit_space(cx: &Cx<'_>, base: u32, start: usize, end: usize) {
    let range = Range {
        start: base + start as u32,
        end: base + end as u32,
    };
    cx.emit_offense(range, "Space inside parentheses detected", None);
    cx.emit_edit(range, "");
}
