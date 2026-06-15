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
    /// `#name` / `#name(arg1 arg2 ...)` — predicate call. Resolved by each
    /// backend, not here. `args` is empty for the no-arg form.
    Predicate { name: String, args: Vec<PredArg> },
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
    /// sees the `$` token — see `parser.rs`.
    ///
    /// `$<known-kind>` (e.g. `$send`, `$array`, `$str`, `$int`) is a **typed
    /// capture**: `name` is `None` and `body` is `Kind(tag)`, so it captures
    /// the node *and* requires it to be of that kind — matching RuboCop's
    /// `$type` node-pattern semantics (murphy-m4dc). `$<non-kind-ident>`
    /// (e.g. `$lhs`) is a **named capture**: `name` is `Some(ident)` and
    /// `body` is an implicit `Wildcard` (matches anything); the name lets a
    /// later predicate arg back-reference it. `$(...)` captures an explicit
    /// sub-pattern, and `$_` captures any single node anonymously.
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
    /// `[a b c]` — intersection AND-pattern: matches subject if **all**
    /// `children` patterns match the same subject. Equivalent to RuboCop's
    /// `node_pattern_no_union: '[' node_pattern_list ']'`. At least one child
    /// is required (the grammar enforces `Pat+` inside `[...]`).
    Intersection { children: Vec<Pat> },
    /// `%name` — named runtime parameter (tPARAM_NAMED, Phase E — murphy-aow).
    ///
    /// At match time the matcher resolves `name` via
    /// [`crate::ParamHost::named`] and compares the subject's literal shape
    /// against the returned [`crate::Param`] using
    /// [`crate::match_lit_against_param`]. The B-backend macro (`def_node_matcher!`)
    /// supplies the same `Param` value from the cop's `CopOptions` field via
    /// [`crate::IntoParam`].
    ///
    /// Supported `Param` shapes: `String`, `Vec<String>`, `i64`, `Vec<i64>`,
    /// `bool`, `Option<T>` (None → always miss), `CopOptionEnum`. Type
    /// mismatch between the subject literal and the resolved `Param` is a
    /// runtime miss, not a panic. Unknown name → miss.
    ParamNamed { name: String },
    /// `%1`, `%2`, … — positional runtime parameter (tPARAM_NUMBER, Phase E — murphy-aow).
    ///
    /// `index` is 1-based (the lexer rejects `%0`). At match time the matcher
    /// resolves `positional[index - 1]` via [`crate::ParamHost::positional`]
    /// and compares the subject's literal shape against the returned
    /// [`crate::Param`] using [`crate::match_lit_against_param`]. The
    /// B-backend macro supplies these from the `positional: &[Param<'_>]`
    /// argument the caller passes on each invocation.
    ///
    /// Out-of-bounds index → miss (no panic).
    ParamNumber { index: u16 },
    /// `_name` — named unification atom (tUNIFY, D4 — murphy-nnr8).
    ///
    /// The first occurrence of `_name` in the pattern binds the current
    /// subject's [`NodeId`]; subsequent occurrences require the subject's
    /// `NodeId` to be **equal** to the bound value. This implements
    /// structural same-node constraints, e.g. `(send _x _ _x)` matches only
    /// when the receiver and the sole argument are the **same AST node**.
    ///
    /// **Semantic difference from RuboCop**: RuboCop uses structural equality
    /// (`==` on `RuboCop::AST::Node`), not object identity. Murphy uses
    /// [`NodeId`] equality (same arena slot) as a pragmatic simplification;
    /// in practice, receiver/argument aliasing is detected by identity anyway.
    ///
    /// [`NodeId`]: murphy_ast::NodeId
    Unify { name: String },
    /// `/.../[imxo]*` — regex match on a Symbol or String slot (tREGEXP,
    /// D5 — murphy-t8km).
    ///
    /// Matches when the subject node is a [`Lit::Sym`] or [`Lit::Str`] atom
    /// whose string value matches the regex. Any other literal kind (Int,
    /// Float, Bool, Nil) is a runtime slot-type mismatch and returns `false`.
    ///
    /// **Regex engine**: Rust's [`regex`](https://docs.rs/regex) crate (RE2
    /// semantics). Behaviour differences from Ruby's Onigmo are documented in
    /// the crate itself (look-around and backreferences are not supported).
    ///
    /// **Flags** (`[imxo]*`):
    /// - `i` — case-insensitive match.
    /// - `m` — multi-line mode (`^`/`$` match line boundaries; `.` does
    ///   NOT match `\n` in the `regex` crate's multi-line mode; use `(?s)` for
    ///   dot-matches-all).
    /// - `x` — extended/verbose mode (whitespace and `#` comments ignored).
    /// - `o` — once-compile (Ruby-only; has no meaning in Rust — silently
    ///   ignored).
    Regex { pattern: String, flags: String },
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

/// An argument to a predicate call (v1: literal or back-reference to a
/// previously-declared `$capture` slot).
///
/// Pattern-arg form (`#pred?({:A :B})`) is v1 scope-out and produces a parse
/// error; only `Lit` and `Capture` are valid at runtime.
#[derive(Debug, Clone, PartialEq)]
pub enum PredArg {
    /// A literal value (int / float / string / symbol / bool / nil).
    Lit(Lit),
    /// A back-reference to an already-declared `$capture` slot (by slot index).
    Capture(u16),
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
