//! `PatternAst` — the parser's output. A spanned tree; the canonical
//! representation. The B backend (proc macro, murphy-9cr.18) consumes this
//! directly; the C backend consumes the derived `PatternIr`.

use crate::PatSpan;
use murphy_ast::NodeKindTag;

/// A parsed pattern: the root node plus capture metadata computed at parse
/// time (positional order, left-to-right).
#[derive(Debug, Clone, PartialEq)]
pub struct PatternAst {
    pub root: Pat,
    /// One entry per `$` capture, in source order. Index = capture slot.
    pub captures: Vec<CaptureKind>,
}

impl PatternAst {
    /// Number of `$` captures in the pattern.
    pub fn n_captures(&self) -> usize {
        self.captures.len()
    }

    /// The capture kinds, in positional (slot) order.
    pub fn capture_kinds(&self) -> &[CaptureKind] {
        &self.captures
    }
}

/// A pattern tree node: a [`PatKind`] plus its span in the source string.
#[derive(Debug, Clone, PartialEq)]
pub struct Pat {
    pub kind: PatKind,
    pub span: PatSpan,
}

/// The kind of a pattern node. v1 grammar (RuboCop node_pattern subset).
#[derive(Debug, Clone, PartialEq)]
pub enum PatKind {
    /// `_` — matches any single node.
    Wildcard,
    /// `...` — matches zero or more nodes. Only valid in a `Node` child list.
    Rest,
    /// `nil?` — built-in: matches a `nil` node or an absent slot.
    NilTest,
    /// A literal: matches the corresponding atom node.
    Lit(Lit),
    /// `#name` — predicate call. Resolved by each backend, not here.
    Predicate(String),
    /// A bare node-type name (`send`) — matches kind only, children free.
    Kind(NodeKindTag),
    /// `(head child...)` — node match with an ordered child sequence.
    Node { head: Head, children: Vec<Pat> },
    /// `{a b ...}` — union; matches if any alternative matches.
    Union(Vec<Pat>),
    /// `!x` — negation.
    Not(Box<Pat>),
    /// `$x` capture. `slot` is the positional capture index, assigned in
    /// source order (left-to-right, outer-before-inner) when the parser
    /// sees the `$` token — see `parser.rs`. `name` is `Some` for `$ident`
    /// named captures, whose `body` is an implicit `Wildcard`; to capture a
    /// sub-pattern use anonymous `$(...)` (so `$send` is a capture *named*
    /// `send`, while `$(send)` captures a node of *kind* `send`).
    Capture {
        slot: u16,
        name: Option<String>,
        body: Box<Pat>,
    },
    /// `^x` — match `x` against the parent of the current node.
    Parent(Box<Pat>),
    /// `` `x `` — descendant search: match `x` against some descendant.
    Descend(Box<Pat>),
    /// `pat*` / `pat+` / `pat?` — postfix quantifier on a node-child element.
    /// `min..=max`: `*` → `0..=u8::MAX`, `+` → `1..=u8::MAX`, `?` → `0..=1`.
    /// Only valid as a direct child of a [`PatKind::Node`]; captures may
    /// appear *around* the quantifier but not *inside* its `body`.
    Quantifier { body: Box<Pat>, min: u8, max: u8 },
    /// `<child*>` — any-order sequence match. All non-rest children must each
    /// match exactly one input element; the overall set of input elements may
    /// appear in any permutation. An optional trailing `...` absorbs leftover
    /// elements. Only valid as a direct child of a [`PatKind::Node`] (not at
    /// the top level, not inside Union/Not/Descend/Quantifier body).
    /// v1 limit: at most 10 non-rest children.
    AnyOrder { children: Vec<Pat> },
}

/// The head of a `Node` match: what the node's kind must satisfy.
#[derive(Debug, Clone, PartialEq)]
pub enum Head {
    /// `(send ...)` — exactly this kind.
    Exact(NodeKindTag),
    /// `(_ ...)` — any kind.
    Any,
    /// `({send csend} ...)` — any of these kinds.
    OneOf(Vec<NodeKindTag>),
}

/// A literal pattern. Matches the corresponding `murphy-ast` atom node.
#[derive(Debug, Clone, PartialEq)]
pub enum Lit {
    Int(i64),
    Float(f64),
    Str(String),
    Sym(String),
    True,
    False,
    Nil,
}

/// Whether a capture binds a single node or a slice of nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureKind {
    /// `$_`, `$(...)`, `$ident`, `$:sym`, … — binds one node.
    Node,
    /// `$...`, `$pat+`, `$pat*` — binds zero or more nodes.
    Seq,
    /// `$pat?` — binds an optional single node (slot present in either arm).
    OptNode,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PatSpan;

    #[test]
    fn pattern_ast_construction_smoke() {
        let p = PatternAst {
            root: Pat {
                kind: PatKind::Wildcard,
                span: PatSpan::new(0, 1),
            },
            captures: vec![],
        };
        assert_eq!(p.n_captures(), 0);
        assert!(p.capture_kinds().is_empty());
    }
}
