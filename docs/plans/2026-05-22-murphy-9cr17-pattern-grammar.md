# murphy-pattern: S式パターン文法・パーサ Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 新規クレート `murphy-pattern` で S 式パターンの文法・パーサ(`PatternAst`)・ランタイム IR(`PatternIr`)・lowering を実装する(murphy-9cr.17)。

**Architecture:** パターン文字列 → `lexer`(トークン列)→ `parser`(`PatternAst` spanned tree、ノード種別名を `NodeKindTag` へ早期解決)→ `lower`(後順走査 flatten で `PatternIr` フラットノード配列へ)。`PatternAst` が正典で B バックエンド(.18)が直接消費、`PatternIr` は C バックエンド(.19)の interpreter 用。文法は RuboCop `node_pattern` の v1 サブセット。

**Tech Stack:** Rust(edition 2024)、Cargo workspace、依存は `murphy-ast` のみ。テストは手書きスナップショット(`insta` 不使用、`assert_eq!` + `{:#?}` 比較)。

設計の出典: beads issue murphy-9cr.17 の design フィールド、および `docs/plans/2026-05-22-plugin-reboot-design.md` §4。

---

## グラウンドルール

- **TDD 必須**: 各タスクは failing test → 実行して fail 確認 → 最小実装 → pass 確認 → commit。
- **コミット粒度**: タスクごとに 1 コミット以上。コミットメッセージは `feat(murphy-pattern): ...` / `feat(murphy-ast): ...`。
- **品質ゲート**: 各タスク完了時に該当クレートの `cargo test` を通す。最終タスクで workspace 全体の `cargo test` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo fmt --check`。
- **シェル安全**: `rm -f` / `cp -f` など非対話形式を使う。

---

## Task 1: murphy-ast に NodeKindTag とパターン名解決を追加

ノード種別名(`send` 等)を `NodeKindTag(u8)` へ解決する API を murphy-ast に足す。`murphy-pattern` のパーサがこれを使う。

**Files:**
- Create: `crates/murphy-ast/src/kinds.rs`
- Modify: `crates/murphy-ast/src/lib.rs`(モジュール宣言と re-export)
- Modify: `crates/murphy-ast/src/node.rs`(`NodeKind::tag` メソッド追加、末尾の `tests` 隣)

**Step 1: failing test を書く**

`crates/murphy-ast/src/kinds.rs` を新規作成し、まずテストだけ:

```rust
//! Pattern-name ↔ `NodeKindTag` resolution for `murphy-pattern`.
//!
//! `NodeKindTag` is the `u8` discriminant of a [`NodeKind`] variant
//! (declaration order, frozen — see ADR 0037). The `KIND_PATTERN_NAMES`
//! table maps the snake_case node-type name a pattern author writes
//! (`send`, `lvasgn`, …) to that tag.

use crate::NodeKind;

/// The `u8` discriminant of a [`NodeKind`] variant.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeKindTag(pub u8);

/// Pattern-name → tag. Declaration order of `NodeKind` minus `Error`
/// (you cannot match an error node in a pattern). Keep in sync with the
/// `NodeKind` enum in `node.rs`; the `table_matches_tag` test guards this.
pub const KIND_PATTERN_NAMES: &[(&str, u8)] = &[
    ("nil", 1), ("true", 2), ("false", 3), ("self", 4),
    ("int", 5), ("float", 6), ("str", 7), ("sym", 8),
    ("lvar", 9), ("ivar", 10), ("cvar", 11), ("gvar", 12), ("const", 13),
    ("lvasgn", 14), ("ivasgn", 15), ("casgn", 16),
    ("send", 17), ("csend", 18), ("block", 19), ("block_pass", 20),
    ("splat", 21), ("array", 22), ("hash", 23), ("pair", 24),
    ("if", 25), ("case", 26), ("when", 27), ("begin", 28), ("return", 29),
    ("and", 30), ("or", 31),
    ("def", 32), ("class", 33), ("module", 34), ("args", 35), ("arg", 36),
];

/// Resolve a pattern node-type name to its tag. `None` for unknown names.
pub fn tag_from_pattern_name(name: &str) -> Option<NodeKindTag> {
    KIND_PATTERN_NAMES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, t)| NodeKindTag(*t))
}

/// The pattern node-type name for a tag (diagnostics / reverse lookup).
pub fn pattern_name(tag: NodeKindTag) -> Option<&'static str> {
    KIND_PATTERN_NAMES
        .iter()
        .find(|(_, t)| *t == tag.0)
        .map(|(n, _)| *n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NodeKind;

    /// One constructed instance of EVERY `NodeKind` variant, in declaration
    /// order. Adding a variant to `NodeKind` forces an update here — paired
    /// with the exhaustive `match` in `NodeKind::tag`, this is the staleness
    /// guard for `KIND_PATTERN_NAMES`.
    fn all_variants() -> Vec<NodeKind> {
        use crate::{NodeId, NodeList, OptNodeId, StringId, Symbol};
        let n = NodeId(0);
        let s = Symbol(0);
        vec![
            NodeKind::Error,
            NodeKind::Nil,
            NodeKind::True_,
            NodeKind::False_,
            NodeKind::SelfExpr,
            NodeKind::Int(0),
            NodeKind::Float(0.0),
            NodeKind::Str(StringId(0)),
            NodeKind::Sym(s),
            NodeKind::Lvar(s),
            NodeKind::Ivar(s),
            NodeKind::Cvar(s),
            NodeKind::Gvar(s),
            NodeKind::Const { scope: OptNodeId::NONE, name: s },
            NodeKind::Lvasgn { name: s, value: OptNodeId::NONE },
            NodeKind::Ivasgn { name: s, value: OptNodeId::NONE },
            NodeKind::Casgn { scope: OptNodeId::NONE, name: s, value: OptNodeId::NONE },
            NodeKind::Send { receiver: OptNodeId::NONE, method: s, args: NodeList::EMPTY },
            NodeKind::Csend { receiver: n, method: s, args: NodeList::EMPTY },
            NodeKind::Block { call: n, args: n, body: OptNodeId::NONE },
            NodeKind::BlockPass(OptNodeId::NONE),
            NodeKind::Splat(OptNodeId::NONE),
            NodeKind::Array(NodeList::EMPTY),
            NodeKind::Hash(NodeList::EMPTY),
            NodeKind::Pair { key: n, value: n },
            NodeKind::If { cond: n, then_: OptNodeId::NONE, else_: OptNodeId::NONE },
            NodeKind::Case { subject: OptNodeId::NONE, whens: NodeList::EMPTY, else_: OptNodeId::NONE },
            NodeKind::When { conds: NodeList::EMPTY, body: OptNodeId::NONE },
            NodeKind::Begin(NodeList::EMPTY),
            NodeKind::Return(OptNodeId::NONE),
            NodeKind::And { lhs: n, rhs: n },
            NodeKind::Or { lhs: n, rhs: n },
            NodeKind::Def { name: s, args: n, body: OptNodeId::NONE },
            NodeKind::Class { name: n, superclass: OptNodeId::NONE, body: OptNodeId::NONE },
            NodeKind::Module { name: n, body: OptNodeId::NONE },
            NodeKind::Args(NodeList::EMPTY),
            NodeKind::Arg(s),
        ]
    }

    #[test]
    fn tag_is_declaration_order() {
        for (i, k) in all_variants().iter().enumerate() {
            assert_eq!(k.tag().0 as usize, i, "tag mismatch for {k:?}");
        }
    }

    #[test]
    fn table_matches_tag() {
        // Every table entry resolves to a real variant with that tag, and
        // every variant except Error (tag 0) has exactly one table entry.
        let variants = all_variants();
        for (name, tag) in KIND_PATTERN_NAMES {
            assert_eq!(variants[*tag as usize].tag().0, *tag, "table entry {name}");
        }
        for k in &variants {
            let t = k.tag();
            if t.0 == 0 {
                assert!(pattern_name(t).is_none(), "Error must have no pattern name");
            } else {
                assert!(pattern_name(t).is_some(), "missing table entry for {k:?}");
            }
        }
    }

    #[test]
    fn round_trip_and_unknown() {
        assert_eq!(tag_from_pattern_name("send"), Some(NodeKindTag(17)));
        assert_eq!(pattern_name(NodeKindTag(17)), Some("send"));
        assert_eq!(tag_from_pattern_name("sned"), None);
        assert_eq!(tag_from_pattern_name("error"), None);
    }

    #[test]
    fn tag_matches_serialize_discriminant() {
        // `tag()` and `serialize::write_node_kind` both assign discriminants;
        // this cross-checks them directly rather than via a round-trip.
        for k in all_variants() {
            let mut buf = vec![];
            crate::serialize::write_node_kind(&k, &mut buf);
            assert_eq!(buf[0], k.tag().0, "discriminant mismatch for {k:?}");
        }
    }
}
```

**Step 2: test を実行して fail を確認**

Run: `cargo test -p murphy-ast kinds`
Expected: コンパイルエラー(`NodeKind::tag` 未定義、`kinds` モジュール未宣言)。

**Step 3: 最小実装**

`crates/murphy-ast/src/node.rs` の `NodeKind` impl(ファイル末尾 `tests` モジュールの直前)に追加:

```rust
impl NodeKind {
    /// This variant's `u8` discriminant (declaration order, frozen — ADR
    /// 0037). Exhaustive `match`: a new variant breaks compilation here.
    pub fn tag(&self) -> crate::NodeKindTag {
        let t: u8 = match self {
            NodeKind::Error => 0,
            NodeKind::Nil => 1,
            NodeKind::True_ => 2,
            NodeKind::False_ => 3,
            NodeKind::SelfExpr => 4,
            NodeKind::Int(_) => 5,
            NodeKind::Float(_) => 6,
            NodeKind::Str(_) => 7,
            NodeKind::Sym(_) => 8,
            NodeKind::Lvar(_) => 9,
            NodeKind::Ivar(_) => 10,
            NodeKind::Cvar(_) => 11,
            NodeKind::Gvar(_) => 12,
            NodeKind::Const { .. } => 13,
            NodeKind::Lvasgn { .. } => 14,
            NodeKind::Ivasgn { .. } => 15,
            NodeKind::Casgn { .. } => 16,
            NodeKind::Send { .. } => 17,
            NodeKind::Csend { .. } => 18,
            NodeKind::Block { .. } => 19,
            NodeKind::BlockPass(_) => 20,
            NodeKind::Splat(_) => 21,
            NodeKind::Array(_) => 22,
            NodeKind::Hash(_) => 23,
            NodeKind::Pair { .. } => 24,
            NodeKind::If { .. } => 25,
            NodeKind::Case { .. } => 26,
            NodeKind::When { .. } => 27,
            NodeKind::Begin(_) => 28,
            NodeKind::Return(_) => 29,
            NodeKind::And { .. } => 30,
            NodeKind::Or { .. } => 31,
            NodeKind::Def { .. } => 32,
            NodeKind::Class { .. } => 33,
            NodeKind::Module { .. } => 34,
            NodeKind::Args(_) => 35,
            NodeKind::Arg(_) => 36,
        };
        crate::NodeKindTag(t)
    }
}
```

> 注: `serialize.rs` の `write_node_kind` が同じ discriminant 割当を持つ。`tag()` の値は `serialize.rs` の数値と完全一致させること。`tag_matches_serialize_discriminant` テストがこれを直接検証するため、`serialize.rs` の `fn write_node_kind` の可視性を `pub(crate) fn write_node_kind` に変更する(`crates/murphy-ast/src/serialize.rs:69` 付近)。理想的には `tag()` に寄せて一本化できるが本タスクのスコープ外。

`crates/murphy-ast/src/lib.rs` を編集:

```rust
mod kinds;
```
を `mod node;` の隣に追加し、re-export に `NodeKindTag` 等を足す:

```rust
pub use kinds::{KIND_PATTERN_NAMES, NodeKindTag, pattern_name, tag_from_pattern_name};
```

**Step 4: test を実行して pass を確認**

Run: `cargo test -p murphy-ast`
Expected: PASS(既存テスト含め全通過)。

**Step 5: commit**

```bash
git add crates/murphy-ast/src/kinds.rs crates/murphy-ast/src/lib.rs \
        crates/murphy-ast/src/node.rs crates/murphy-ast/src/serialize.rs
git commit -m "feat(murphy-ast): add NodeKindTag and pattern-name resolution"
```

---

## Task 2: murphy-pattern クレートの骨組みと error モジュール

ワークスペースに空クレートを追加し、`ParseError` / `PatSpan` を定義する。

**Files:**
- Create: `crates/murphy-pattern/Cargo.toml`
- Create: `crates/murphy-pattern/src/lib.rs`
- Create: `crates/murphy-pattern/src/error.rs`

ワークスペース `Cargo.toml` は `members = ["crates/*"]` なので自動で拾われる(編集不要)。

**Step 1: failing test を書く**

`crates/murphy-pattern/src/error.rs`:

```rust
//! Parse errors with a byte-offset span into the pattern source string.

/// A half-open byte range into the pattern source string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PatSpan {
    pub start: u32,
    pub end: u32,
}

impl PatSpan {
    pub fn new(start: usize, end: usize) -> PatSpan {
        PatSpan { start: start as u32, end: end as u32 }
    }
}

/// A pattern parse error: a human-readable message plus the offending span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub span: PatSpan,
}

impl ParseError {
    pub fn new(message: impl Into<String>, span: PatSpan) -> ParseError {
        ParseError { message: message.into(), span }
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (at {}..{})", self.message, self.span.start, self.span.end)
    }
}

impl std::error::Error for ParseError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_carries_message_and_span() {
        let e = ParseError::new("unknown node type `sned`", PatSpan::new(1, 5));
        assert_eq!(e.span, PatSpan { start: 1, end: 5 });
        assert!(e.to_string().contains("sned"));
        assert!(e.to_string().contains("1..5"));
    }
}
```

`crates/murphy-pattern/src/lib.rs`:

```rust
//! S-expression pattern grammar, parser, and runtime IR for Murphy.
//!
//! See beads issue murphy-9cr.17 and `docs/plans/2026-05-22-plugin-reboot-design.md` §4.

mod error;

pub use error::{ParseError, PatSpan};
```

`crates/murphy-pattern/Cargo.toml`:

```toml
[package]
name = "murphy-pattern"
version = "0.1.0"
edition = "2024"
description = "S-expression pattern grammar, parser, and runtime IR for Murphy cops (murphy-9cr.17)."

[dependencies]
murphy-ast = { path = "../murphy-ast" }
```

**Step 2: test を実行して fail を確認**

Run: `cargo test -p murphy-pattern`
Expected: 最初は新クレートが認識され、テストが PASS する(error.rs は完結しているため)。fail させるべき対象がないので、このタスクは「クレートが workspace に乗り `cargo test -p murphy-pattern` が通る」ことの確認で良い。

**Step 3 / 4: 実装 = 上記ファイルそのもの。`cargo test -p murphy-pattern` PASS を確認。**

**Step 5: commit**

```bash
git add crates/murphy-pattern/
git commit -m "feat(murphy-pattern): scaffold crate with ParseError/PatSpan"
```

---

## Task 3: PatternAst 型定義

`PatternAst` の spanned tree 型を定義する(まだパーサは無い)。

**Files:**
- Create: `crates/murphy-pattern/src/ast.rs`
- Modify: `crates/murphy-pattern/src/lib.rs`

**Step 1 & 3: 型を書く**

`crates/murphy-pattern/src/ast.rs`:

```rust
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
    Capture { slot: u16, name: Option<String>, body: Box<Pat> },
    /// `^x` — match `x` against the parent of the current node.
    Parent(Box<Pat>),
    /// `` `x `` — descendant search: match `x` against some descendant.
    Descend(Box<Pat>),
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
    /// `$...` — binds zero or more nodes.
    Seq,
}
```

`crates/murphy-pattern/src/lib.rs` に追加:

```rust
mod ast;

pub use ast::{CaptureKind, Head, Lit, Pat, PatKind, PatternAst};
```

**Step 2: コンパイル確認のテスト**

`ast.rs` 末尾に:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::PatSpan;

    #[test]
    fn pattern_ast_construction_smoke() {
        let p = PatternAst {
            root: Pat { kind: PatKind::Wildcard, span: PatSpan::new(0, 1) },
            captures: vec![],
        };
        assert_eq!(p.n_captures(), 0);
        assert!(p.capture_kinds().is_empty());
    }
}
```

**Step 4: `cargo test -p murphy-pattern` PASS を確認。**

**Step 5: commit**

```bash
git add crates/murphy-pattern/src/ast.rs crates/murphy-pattern/src/lib.rs
git commit -m "feat(murphy-pattern): add PatternAst type definitions"
```

---

## Task 4: Lexer

パターン文字列をトークン列(span 付き)へ分解する。

**Files:**
- Create: `crates/murphy-pattern/src/lexer.rs`
- Modify: `crates/murphy-pattern/src/lib.rs`(`mod lexer;` — `pub` 不要、crate 内部)

**トークン仕様:**

| 字句 | Token |
|---|---|
| `(` `)` `{` `}` | `LParen` `RParen` `LBrace` `RBrace` |
| `_` (単独) | `Underscore` |
| `...` | `Ellipsis` |
| `!` `$` `^` `` ` `` | `Bang` `Dollar` `Caret` `Backtick` |
| `nil?` | `NilQuestion` |
| `#name` | `Predicate(String)`(`name` は `[a-z_][a-z0-9_]*[?!]?`) |
| `[a-z_][a-z0-9_]*` | `Ident(String)`(`true`/`false`/`nil` もこれ、parser が分類) |
| `123` `-1` | `Int(i64)` |
| `1.5` `-0.5` | `Float(f64)` |
| `"..."` | `Str(String)`(エスケープは `\"` `\\` のみ v1 対応) |
| `:name` | `Sym(String)` |

- トークンは `Spanned { tok: Token, span: PatSpan }` で返す。
- 空白(` ` `\t` `\n` `\r`)は区切りで、トークンにしない。
- `_` 単独は `Underscore`。識別子内の `_`(`block_pass`)は `Ident` の一部。識別子読み取り後その綴りが `"_"` なら `Underscore` を出す。
- 数値: `-` の直後が数字なら数値リテラル。`-` 単独や他用途は v1 では字句エラー。
- `nil?`: 識別子を読み、末尾に `?` が付くケースを処理。綴りが `"nil?"` なら `NilQuestion`。それ以外で `?`/`!` で終わる裸識別子は字句エラー(`expected '#' before a predicate name`)。
- 未知文字(`%` `[` `]` `<` `>` `@` 等)は字句エラー。`%`/`[`/`<` は「v1 では未対応」と分かるメッセージにする。
- 字句エラーは `Result<Vec<Spanned>, ParseError>` の `Err`。

**Step 1: failing test**

`lexer.rs` 末尾:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn toks(src: &str) -> Vec<Token> {
        tokenize(src).expect("lex ok").into_iter().map(|s| s.tok).collect()
    }

    #[test]
    fn lexes_node_match() {
        assert_eq!(
            toks("(send nil? :puts $...)"),
            vec![
                Token::LParen,
                Token::Ident("send".into()),
                Token::NilQuestion,
                Token::Sym("puts".into()),
                Token::Dollar,
                Token::Ellipsis,
                Token::RParen,
            ]
        );
    }

    #[test]
    fn lexes_literals_and_sigils() {
        assert_eq!(
            toks("{ !_ ^x `y #pred 42 -1 1.5 \"s\" true }"),
            vec![
                Token::LBrace, Token::Bang, Token::Underscore,
                Token::Caret, Token::Ident("x".into()),
                Token::Backtick, Token::Ident("y".into()),
                Token::Predicate("pred".into()),
                Token::Int(42), Token::Int(-1), Token::Float(1.5),
                Token::Str("s".into()), Token::Ident("true".into()),
                Token::RBrace,
            ]
        );
    }

    #[test]
    fn span_points_at_token() {
        let t = tokenize("(send)").expect("ok");
        // `send` occupies bytes 1..5.
        assert_eq!(t[1].tok, Token::Ident("send".into()));
        assert_eq!((t[1].span.start, t[1].span.end), (1, 5));
    }

    #[test]
    fn lex_error_on_unsupported_sigil() {
        let e = tokenize("(send %1)").expect_err("must reject %");
        assert!(e.message.contains('%'));
        // span points at the `%`
        assert_eq!(e.span.start, 6);
    }

    #[test]
    fn lex_error_on_bare_predicate_name() {
        assert!(tokenize("even?").is_err());
    }
}
```

**Step 2:** `cargo test -p murphy-pattern lexer` → コンパイルエラー(`Token`/`tokenize`/`Spanned` 未定義)。

**Step 3: 実装**

`Token` enum、`Spanned` struct、`tokenize(src: &str) -> Result<Vec<Spanned>, ParseError>` を実装。バイトオフセットで span を作る。`Token` は `#[derive(Debug, Clone, PartialEq)]`。

**Step 4:** `cargo test -p murphy-pattern lexer` PASS。

**Step 5: commit**

```bash
git add crates/murphy-pattern/src/lexer.rs crates/murphy-pattern/src/lib.rs
git commit -m "feat(murphy-pattern): add pattern lexer"
```

---

## 文法(Task 5–8 共通リファレンス)

```text
pattern  := prefixed
prefixed := '!' prefixed
          | '^' prefixed
          | '`' prefixed
          | '$' capture-tail
          | primary
capture-tail := IDENT                       -- 名前付き capture(body は暗黙の `_`)
              | prefixed                    -- 無名 capture(任意のパターン)
primary  := '_'                             -- Wildcard
          | '...'                           -- Rest(Node 子リスト内のみ合法)
          | 'nil?'                          -- NilTest
          | literal                         -- Lit
          | '#' name                        -- Predicate
          | IDENT                           -- Kind(裸の種別名)/ true/false/nil リテラル
          | '(' head pattern* ')'           -- Node
          | '{' pattern+ '}'                -- Union
head     := IDENT | '_' | '{' IDENT+ '}'
literal  := INT | FLOAT | STR | SYM         -- true/false/nil は IDENT 経由
```

**capture の曖昧性解消ルール(重要):** `$` の次が **識別子トークン**なら名前付き capture(`name = ident`、body は暗黙の `Wildcard`)。それ以外(`_` `...` `(` リテラル `#` `{` `^` `` ` `` `!`)なら無名 capture でその後のパターンを body とする。つまり裸の種別を capture したいときは `$(send)` と書く(`$send` は「send という名前の capture」)。名前付き seq capture(`$rest...` 形)は v1 非対応 — 無名 `$...` を使う。

**capture slot 採番:** パーサは `$` 出現ごとに左→右で 0,1,2,… を振り、`PatternAst.captures` に `CaptureKind` を push する(`$...` は `Seq`、それ以外は `Node`)。

---

## Task 5: Parser — atoms と prefix

recursive-descent パーサの骨組み。`_` / リテラル / `nil?` / 裸種別名 / `#predicate` / `!` `^` `` ` `` prefix をパースする(`()` `{}` `$` は Task 6–8)。

**Files:**
- Create: `crates/murphy-pattern/src/parser.rs`
- Modify: `crates/murphy-pattern/src/lib.rs`(`mod parser;` と `pub use` で `parse` を公開)

**`parse` の契約:** `pub fn parse(src: &str) -> Result<PatternAst, ParseError>`。トークン列を消費しきること(余分なトークンはエラー)。トップレベルで `...` が来たらエラー(`Rest` は Node 子のみ)。

**Step 1: failing test**

`parser.rs` 末尾(テストヘルパは `format!("{:#?}", parse(src).unwrap().root)` で `Pat` の Debug を比較):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Lit, PatKind};

    fn k(src: &str) -> PatKind {
        parse(src).expect("parse ok").root.kind
    }

    #[test]
    fn parses_wildcard() {
        assert_eq!(k("_"), PatKind::Wildcard);
    }

    #[test]
    fn parses_literals() {
        assert_eq!(k("42"), PatKind::Lit(Lit::Int(42)));
        assert_eq!(k("-1"), PatKind::Lit(Lit::Int(-1)));
        assert_eq!(k("1.5"), PatKind::Lit(Lit::Float(1.5)));
        assert_eq!(k("\"s\""), PatKind::Lit(Lit::Str("s".into())));
        assert_eq!(k(":puts"), PatKind::Lit(Lit::Sym("puts".into())));
        assert_eq!(k("true"), PatKind::Lit(Lit::True));
        assert_eq!(k("false"), PatKind::Lit(Lit::False));
        assert_eq!(k("nil"), PatKind::Lit(Lit::Nil));
    }

    #[test]
    fn parses_nil_test_distinct_from_nil_literal() {
        assert_eq!(k("nil?"), PatKind::NilTest);
        assert_eq!(k("nil"), PatKind::Lit(Lit::Nil));
    }

    #[test]
    fn parses_bare_kind_name() {
        assert_eq!(k("send"), PatKind::Kind(murphy_ast::NodeKindTag(17)));
    }

    #[test]
    fn parses_predicate() {
        assert_eq!(k("#odd?"), PatKind::Predicate("odd?".into()));
    }

    #[test]
    fn parses_prefixes() {
        assert!(matches!(k("!_"), PatKind::Not(_)));
        assert!(matches!(k("^_"), PatKind::Parent(_)));
        assert!(matches!(k("`_"), PatKind::Descend(_)));
    }

    #[test]
    fn unknown_kind_name_is_span_error() {
        let e = parse("sned").expect_err("unknown kind");
        assert!(e.message.contains("sned"));
        assert_eq!((e.span.start, e.span.end), (0, 4));
    }

    #[test]
    fn rest_at_top_level_is_error() {
        assert!(parse("...").is_err());
    }
}
```

**Step 2:** `cargo test -p murphy-pattern parser` → fail(`parse` 未定義)。

**Step 3: 実装**

トークンカーソルを持つ `Parser` struct を作り、`prefixed` / `primary` を実装。`IDENT` の分類: `true`/`false`/`nil` → `Lit`、`nil?` トークン → `NilTest`、それ以外の識別子 → `murphy_ast::tag_from_pattern_name` で解決、`None` なら span 付きエラー `unknown node type \`{name}\``。`...` を `primary` で受けたらこの段階ではエラー(Task 6 で Node 子リスト内のみ許可)。各 `Pat` に span(構成要素の最初〜最後のバイト)を付ける。

**Step 4:** `cargo test -p murphy-pattern parser` PASS。

**Step 5: commit**

```bash
git add crates/murphy-pattern/src/parser.rs crates/murphy-pattern/src/lib.rs
git commit -m "feat(murphy-pattern): parse atoms, literals, predicates, prefixes"
```

---

## Task 6: Parser — Node マッチ `(...)` と Head

`(head child...)` をパースする。`head` は `Exact` / `Any` / `OneOf`。`...` を子リスト内で受け付ける。

**Files:**
- Modify: `crates/murphy-pattern/src/parser.rs`

**Step 1: failing test**(`parser.rs` の `tests` に追加)

```rust
#[test]
fn parses_node_with_children() {
    let p = parse("(send nil :puts)").expect("ok");
    match p.root.kind {
        PatKind::Node { head, children } => {
            assert_eq!(head, crate::Head::Exact(murphy_ast::NodeKindTag(17)));
            assert_eq!(children.len(), 2);
        }
        other => panic!("expected Node, got {other:?}"),
    }
}

#[test]
fn parses_any_head() {
    let p = parse("(_ _)").expect("ok");
    assert!(matches!(p.root.kind, PatKind::Node { head: crate::Head::Any, .. }));
}

#[test]
fn parses_oneof_head() {
    let p = parse("({send csend} _)").expect("ok");
    match p.root.kind {
        PatKind::Node { head: crate::Head::OneOf(tags), .. } => {
            assert_eq!(tags, vec![murphy_ast::NodeKindTag(17), murphy_ast::NodeKindTag(18)]);
        }
        other => panic!("expected OneOf head, got {other:?}"),
    }
}

#[test]
fn parses_rest_in_child_list() {
    let p = parse("(array ... _)").expect("ok");
    match p.root.kind {
        PatKind::Node { children, .. } => {
            assert_eq!(children[0].kind, PatKind::Rest);
            assert_eq!(children[1].kind, PatKind::Wildcard);
        }
        other => panic!("expected Node, got {other:?}"),
    }
}

#[test]
fn rejects_multiple_rest() {
    let e = parse("(array ... ...)").expect_err("two rests");
    assert!(e.message.to_lowercase().contains("..."));
}

#[test]
fn rejects_unbalanced_paren() {
    assert!(parse("(send").is_err());
}

#[test]
fn rejects_empty_node() {
    // `()` has no head.
    assert!(parse("()").is_err());
}
```

**Step 2:** 実行して fail を確認。

**Step 3: 実装**

`primary` に `LParen` 分岐を追加。`(` の直後を head としてパース: `IDENT` → `Head::Exact`、`Underscore` → `Head::Any`、`LBrace` → `IDENT` を 1 個以上読んで `}` まで(各識別子を `tag_from_pattern_name` で解決、未知ならエラー)→ `Head::OneOf`。head 以外のトークンはエラー。続けて `)` まで子パターンを読む。子の中で `...` は `PatKind::Rest`(span 付き)として許可、ただし子リスト内で 2 個目を見たらエラー。`)` が来ずトークン終端ならエラー(unbalanced)。

**Step 4 / 5:** PASS 確認 → commit `feat(murphy-pattern): parse node match with Exact/Any/OneOf head`。

---

## Task 7: Parser — Union `{}`

`{a b ...}` の union をパースする(head 位置でない `{}`)。

**Files:**
- Modify: `crates/murphy-pattern/src/parser.rs`

**Step 1: failing test**

```rust
#[test]
fn parses_union() {
    let p = parse("{send csend}").expect("ok");
    match p.root.kind {
        PatKind::Union(alts) => assert_eq!(alts.len(), 2),
        other => panic!("expected Union, got {other:?}"),
    }
}

#[test]
fn parses_union_of_subpatterns() {
    let p = parse("{(send _ :a) (send _ :b)}").expect("ok");
    assert!(matches!(p.root.kind, PatKind::Union(alts) if alts.len() == 2));
}

#[test]
fn rejects_empty_union() {
    let e = parse("{}").expect_err("empty union");
    assert!(e.message.to_lowercase().contains("union") || e.message.contains("{}"));
}
```

**Step 2:** fail 確認。

**Step 3: 実装**

`primary` の `LBrace` 分岐: `}` まで 1 個以上のパターンを読む。0 個なら span 付きエラー(`empty union`)。`PatKind::Union(Vec<Pat>)` を返す。head 位置の `{}`(Task 6)と本体位置の `{}` の違いに注意 — head の `{}` は識別子のみ、本体の `{}` は任意パターン。

**Step 4 / 5:** PASS → commit `feat(murphy-pattern): parse union patterns`。

---

## Task 8: Parser — captures `$` / `$name`

`$` 無名 capture と `$ident` 名前付き capture をパースし、slot を採番する。

**Files:**
- Modify: `crates/murphy-pattern/src/parser.rs`

**Step 1: failing test**

```rust
use crate::CaptureKind;

#[test]
fn parses_anonymous_capture() {
    let p = parse("(send $_ :puts)").expect("ok");
    assert_eq!(p.n_captures(), 1);
    assert_eq!(p.capture_kinds(), &[CaptureKind::Node]);
}

#[test]
fn parses_seq_capture() {
    let p = parse("(send nil :puts $...)").expect("ok");
    assert_eq!(p.capture_kinds(), &[CaptureKind::Seq]);
}

#[test]
fn parses_named_capture_body_is_wildcard() {
    let p = parse("(send $receiver :puts)").expect("ok");
    assert_eq!(p.n_captures(), 1);
    match &p.root.kind {
        PatKind::Node { children, .. } => match &children[0].kind {
            PatKind::Capture { slot, name, body } => {
                assert_eq!(*slot, 0);
                assert_eq!(name.as_deref(), Some("receiver"));
                assert_eq!(body.kind, PatKind::Wildcard);
            }
            other => panic!("expected Capture, got {other:?}"),
        },
        _ => unreachable!(),
    }
}

#[test]
fn capture_of_subpattern_uses_parens() {
    let p = parse("$(const _ :Foo)").expect("ok");
    match p.root.kind {
        PatKind::Capture { slot, name, body } => {
            assert_eq!(slot, 0);
            assert!(name.is_none());
            assert!(matches!(body.kind, PatKind::Node { .. }));
        }
        other => panic!("expected Capture, got {other:?}"),
    }
}

#[test]
fn capture_slots_are_left_to_right() {
    let p = parse("(send $_ $...)").expect("ok");
    assert_eq!(p.capture_kinds(), &[CaptureKind::Node, CaptureKind::Seq]);
}

#[test]
fn nested_captures_are_source_order() {
    // outer `$(...)` = slot 0, inner `$inner` = slot 1 — source order,
    // NOT post-order. Guards the nested-capture slot-numbering bug.
    let p = parse("$(send $inner _)").expect("ok");
    assert_eq!(p.n_captures(), 2);
    assert_eq!(p.capture_kinds(), &[CaptureKind::Node, CaptureKind::Node]);
    match &p.root.kind {
        PatKind::Capture { slot, body, .. } => {
            assert_eq!(*slot, 0, "outer capture is slot 0");
            match &body.kind {
                PatKind::Node { children, .. } => match &children[0].kind {
                    PatKind::Capture { slot, name, .. } => {
                        assert_eq!(*slot, 1, "inner capture is slot 1");
                        assert_eq!(name.as_deref(), Some("inner"));
                    }
                    other => panic!("expected inner Capture, got {other:?}"),
                },
                _ => unreachable!(),
            }
        }
        other => panic!("expected outer Capture, got {other:?}"),
    }
}

#[test]
fn rejects_duplicate_capture_name() {
    let e = parse("(send $x $x)").expect_err("dup name");
    assert!(e.message.contains('x'));
}
```

**Step 2:** fail 確認。

**Step 3: 実装**

`prefixed` に `Dollar` 分岐。**slot は `$` トークンを見た瞬間に予約する**(pre-order)— これがネスト時のソース順を保証する(advisor 指摘の修正)。手順:

1. `$` を consume した時点で `slot = self.captures.len() as u16` を確保し、`self.captures` にプレースホルダ `CaptureKind::Node` を push(`captures` は slot 添字の `Vec<CaptureKind>`)。
2. `$` の次トークンを peek:
   - `Ident(name)` → 名前付き capture。`name` を consume。body は暗黙の `Wildcard`(span は `$` の位置)。重複名チェック(パーサに `Vec<String>` を持たせ既出ならエラー)。
   - `Ellipsis` → 無名 seq capture。body = `PatKind::Rest`。`self.captures[slot] = CaptureKind::Seq` に更新。
   - それ以外 → `prefixed` を再帰呼び出しして body を得る無名 capture。body が `Rest`(理論上ここには来ないが)以外なら `Node` のまま。
3. `PatKind::Capture { slot, name, body }` を構築して返す。

> 重要: slot 予約を body パースより**前**に行うこと。後順(body を先にパースしてから push)だと `$(send $inner _)` で inner が slot 0、outer が slot 1 になりソース順とずれる。`nested_captures_are_source_order` テストがこれを検出する。
>
> 注: `$...` は `$` + `Ellipsis`。`Ellipsis` は通常 `primary` でエラーになるため、`$` 分岐内で `Ellipsis` を `PatKind::Rest` として受ける分岐を明示的に書く。`captures` の slot 添字と最終的な `PatternAst.captures` は同一の `Vec` で良い。

**Step 4 / 5:** PASS → commit `feat(murphy-pattern): parse anonymous and named captures`。

---

## Task 9: Parser スナップショットテストと文法カバレッジ

v1 文法全機能を網羅する parse スナップショットテストを追加する。

**Files:**
- Create: `crates/murphy-pattern/tests/parse_snapshots.rs`

**Step 1: テストを書く**

各 v1 機能を 1 つ以上、`format!("{:#?}", parse(src).unwrap())` の文字列を期待値リテラルと `assert_eq!` で比較する手書きスナップショット。最低限カバーする入力:

```text
_                          Wildcard
nil?                       NilTest
:puts                      Lit::Sym
(send nil? :puts $...)     Node + NilTest + Sym + Seq capture
({send csend} _ ...)       OneOf head + Rest
{int float}                Union
!(send _ :x)               Not
^(def _ _ _)               Parent
`(send nil? :raise)        Descend
(send $receiver #pred?)    named capture + Predicate
```

各ケースで `n_captures()` / `capture_kinds()` も assert する。エラー系(未知種別名・空 union・`...` 重複・capture 名重複・括弧不整合)は別テストで `span` の `(start, end)` を明示的に assert する。

> スナップショット文字列は実装後の実出力から一度だけ「bless」する(`cargo test` の出力を貼る)。手で推測しない。

**Step 2 / 3 / 4:** 実装は不要(Task 5–8 で完了済み)。テストを走らせ、期待値を実出力で確定 → PASS。

**Step 5: commit**

```bash
git add crates/murphy-pattern/tests/parse_snapshots.rs
git commit -m "test(murphy-pattern): parser snapshots covering v1 grammar"
```

---

## Task 10: Pattern IR 型定義

`PatternIr` フラットノード配列の型を定義する(lowering は Task 11)。

**Files:**
- Create: `crates/murphy-pattern/src/ir.rs`
- Modify: `crates/murphy-pattern/src/lib.rs`

**Step 1 & 3: 型を書く**

`crates/murphy-pattern/src/ir.rs`:

```rust
//! `PatternIr` — a flat, pointer-free node array derived from `PatternAst`.
//! Consumed by the C backend interpreter (murphy-9cr.19). Mirrors the
//! `murphy-ast` arena design: nodes in a `Vec`, variable-length children in
//! a side table.

use crate::CaptureKind;
use murphy_ast::NodeKindTag;

/// Index into [`PatternIr::nodes`].
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IrNodeId(pub u32);

/// A reference to a contiguous slice of a side table (`children` or `tags`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IrSlice {
    pub start: u32,
    pub len: u32,
}

/// A reference to a `[start, start+len)` byte range of `str_pool`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StrRef {
    pub start: u32,
    pub len: u32,
}

/// A compiled pattern: a flat node array plus side tables. No pointers.
#[derive(Debug, Clone, PartialEq)]
pub struct PatternIr {
    pub nodes: Vec<IrNode>,
    /// Side table for `IrNode::Node` children and `IrNode::Union` arms.
    pub children: Vec<IrNodeId>,
    /// Side table for `IrHead::OneOf` alternatives.
    pub tags: Vec<NodeKindTag>,
    /// Predicate names, string/symbol literals, capture names.
    pub str_pool: String,
    /// One entry per `$` capture, in slot order.
    pub captures: Vec<CaptureMeta>,
    /// The root node.
    pub root: IrNodeId,
}

/// Per-capture metadata, indexed by slot.
#[derive(Debug, Clone, PartialEq)]
pub struct CaptureMeta {
    pub kind: CaptureKind,
    /// `Some` for `$ident` named captures.
    pub name: Option<StrRef>,
}

/// A flat IR node. `PatKind` resolved and flattened; children by index.
#[derive(Debug, Clone, PartialEq)]
pub enum IrNode {
    Wildcard,
    Rest,
    NilTest,
    LitInt(i64),
    LitFloat(f64),
    LitStr(StrRef),
    LitSym(StrRef),
    LitTrue,
    LitFalse,
    LitNil,
    Predicate(StrRef),
    Kind(NodeKindTag),
    Node { head: IrHead, children: IrSlice },
    Union(IrSlice),
    Not(IrNodeId),
    Capture { slot: u16, body: IrNodeId },
    Parent(IrNodeId),
    Descend(IrNodeId),
}

/// The head of an `IrNode::Node`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrHead {
    Exact(NodeKindTag),
    Any,
    OneOf(IrSlice),
}
```

`lib.rs` に追加:

```rust
mod ir;

pub use ir::{CaptureMeta, IrHead, IrNode, IrNodeId, IrSlice, PatternIr, StrRef};
```

**Step 2: コンパイル確認のスモークテスト**(`ir.rs` 末尾)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ir_construction_smoke() {
        let ir = PatternIr {
            nodes: vec![IrNode::Wildcard],
            children: vec![],
            tags: vec![],
            str_pool: String::new(),
            captures: vec![],
            root: IrNodeId(0),
        };
        assert_eq!(ir.nodes.len(), 1);
        assert_eq!(ir.root, IrNodeId(0));
    }
}
```

**Step 4:** `cargo test -p murphy-pattern` PASS。

**Step 5: commit**

```bash
git add crates/murphy-pattern/src/ir.rs crates/murphy-pattern/src/lib.rs
git commit -m "feat(murphy-pattern): add PatternIr flat-node-array types"
```

---

## Task 11: Lowering(PatternAst → PatternIr)

後順走査で `PatternAst` を `PatternIr` へ flatten する。infallible。

**Files:**
- Modify: `crates/murphy-pattern/src/ir.rs`(`lower` 関数)
- Modify: `crates/murphy-pattern/src/lib.rs`

**`lower` の契約:** `pub fn lower(ast: &PatternAst) -> PatternIr`。検証はパーサ済みなので `Result` ではない。

**Step 1: failing test**(`ir.rs` の `tests`)

```rust
use crate::parse;

#[test]
fn lowers_wildcard() {
    let ir = lower(&parse("_").unwrap());
    assert_eq!(ir.nodes, vec![IrNode::Wildcard]);
    assert_eq!(ir.root, IrNodeId(0));
}

#[test]
fn lowers_node_children_into_side_table() {
    let ir = lower(&parse("(send nil :puts)").unwrap());
    // root is a Node; its children live in the `children` side table.
    let root = &ir.nodes[ir.root.0 as usize];
    match root {
        IrNode::Node { head, children } => {
            assert_eq!(*head, IrHead::Exact(murphy_ast::NodeKindTag(17)));
            assert_eq!(children.len, 2);
        }
        other => panic!("expected Node, got {other:?}"),
    }
}

#[test]
fn lowers_strings_into_pool() {
    let ir = lower(&parse(":puts").unwrap());
    match ir.nodes[ir.root.0 as usize] {
        IrNode::LitSym(r) => {
            let s = &ir.str_pool[r.start as usize..(r.start + r.len) as usize];
            assert_eq!(s, "puts");
        }
        ref other => panic!("expected LitSym, got {other:?}"),
    }
}

#[test]
fn lowers_capture_slots_and_meta() {
    let ir = lower(&parse("(send $receiver $...)").unwrap());
    assert_eq!(ir.captures.len(), 2);
    assert_eq!(ir.captures[0].kind, CaptureKind::Node);
    assert_eq!(ir.captures[1].kind, CaptureKind::Seq);
    // named capture's name is in the pool
    let r = ir.captures[0].name.expect("named");
    assert_eq!(&ir.str_pool[r.start as usize..(r.start + r.len) as usize], "receiver");
    assert!(ir.captures[1].name.is_none());
}

#[test]
fn lowers_oneof_head_tags() {
    let ir = lower(&parse("({send csend} _)").unwrap());
    match ir.nodes[ir.root.0 as usize] {
        IrNode::Node { head: IrHead::OneOf(s), .. } => {
            let tags = &ir.tags[s.start as usize..(s.start + s.len) as usize];
            assert_eq!(tags, &[murphy_ast::NodeKindTag(17), murphy_ast::NodeKindTag(18)]);
        }
        ref other => panic!("expected OneOf, got {other:?}"),
    }
}

#[test]
fn lower_capture_slots_match_pattern_ast() {
    // Nested captures: outer $(...) = 0, $named = 1, $tail = 2. IR capture
    // metadata must be slot-indexed and agree with the PatternAst.
    let p = parse("$(send $named (send $tail))").expect("ok");
    let ir = lower(&p);
    assert_eq!(ir.captures.len(), p.n_captures());
    let ir_kinds: Vec<_> = ir.captures.iter().map(|c| c.kind).collect();
    assert_eq!(ir_kinds.as_slice(), p.capture_kinds());
}
```

**Step 2:** fail 確認。

**Step 3: 実装**

`lower` はまず `ir.captures` を `ast.n_captures()` 個のプレースホルダ `CaptureMeta` で確保する(slot 添字でアクセスするため)。次に再帰ヘルパ `fn lower_pat(pat: &Pat, ir: &mut PatternIr) -> IrNodeId` を後順で:
1. 子を先に lower して `IrNodeId` を得る。
2. `Node`/`Union` の子 ID 列を `ir.children` へ push し `IrSlice` を作る。
3. 文字列(述語名・`Str`/`Sym` リテラル・capture 名)は `intern` ヘルパで `ir.str_pool` に追記し `StrRef` を返す(v1 は重複排除なしの単純追記で良い)。
4. `OneOf` の tag 列は `ir.tags` へ push。
5. 自分の `IrNode` を `ir.nodes` へ push して `IrNodeId` を返す。
6. `Capture { slot, name, body }` は **`slot` をそのまま使う**(走査順に依存しない)。body を lower して `IrNode::Capture { slot, body }` を作り、`ir.captures[slot as usize] = CaptureMeta { kind: ast.captures[slot as usize], name: name を intern }` で該当 slot を埋める。
最後に `ir.root` をルートの `IrNodeId` に設定。

> slot は `PatKind::Capture.slot`(パーサが pre-order で採番済み)を信頼する。lowering は走査順に slot を振り直してはならない — `lower_capture_slots_match_pattern_ast` テストがネスト時の整合性を検出する。

**Step 4:** `cargo test -p murphy-pattern` PASS。

**Step 5: commit**

```bash
git add crates/murphy-pattern/src/ir.rs crates/murphy-pattern/src/lib.rs
git commit -m "feat(murphy-pattern): add PatternAst-to-PatternIr lowering"
```

---

## Task 12: 公開 API `compile` と lowering スナップショット

`compile`(parse + lower)を公開し、lowering スナップショットテストを追加する。

**Files:**
- Modify: `crates/murphy-pattern/src/lib.rs`
- Create: `crates/murphy-pattern/tests/lower_snapshots.rs`

**Step 1: failing test**

`lib.rs` の `tests`(または `tests/lower_snapshots.rs`)に:

```rust
#[test]
fn compile_runs_parse_then_lower() {
    let ir = murphy_pattern::compile("(send nil? :puts $...)").expect("ok");
    assert_eq!(ir.captures.len(), 1);
}

#[test]
fn compile_propagates_parse_error() {
    assert!(murphy_pattern::compile("(sned _)").is_err());
}
```

`tests/lower_snapshots.rs` は Task 9 と同じ v1 機能網羅入力に対し `format!("{:#?}", compile(src).unwrap())` を期待値と比較(実出力から bless)。

**Step 2:** fail 確認(`compile` 未定義)。

**Step 3: 実装**

`lib.rs`:

```rust
/// Parse a pattern source string into a `PatternAst`. For the B backend.
pub fn parse(src: &str) -> Result<PatternAst, ParseError> {
    parser::parse(src)
}

/// Lower a parsed pattern to `PatternIr`. Infallible — all validation
/// happens during `parse`.
pub fn lower(ast: &PatternAst) -> PatternIr {
    ir::lower(ast)
}

/// Parse and lower in one step. For the C backend.
pub fn compile(src: &str) -> Result<PatternIr, ParseError> {
    Ok(lower(&parse(src)?))
}
```

`parser::parse` / `ir::lower` の可視性を crate 内に整える。

**Step 4:** `cargo test -p murphy-pattern` PASS。

**Step 5: commit**

```bash
git add crates/murphy-pattern/src/lib.rs crates/murphy-pattern/tests/lower_snapshots.rs
git commit -m "feat(murphy-pattern): public compile API and lowering snapshots"
```

---

## Task 13: 設計ドキュメント §4 更新と最終品質ゲート

設計 §4 の「名前付き capture = 見送り」を改訂し、workspace 全体の品質ゲートを通す。

**Files:**
- Modify: `docs/plans/2026-05-22-plugin-reboot-design.md`(§4 の v1 文法スコープ表)

**Step 1: §4 を更新**

§4 の v1 文法スコープ表の `capture` 行を、名前付き位置 capture の v1 採用を反映するよう書き換える。現状:

```text
| capture | `$`(位置 capture) | 名前付き capture |
```

を次へ:

```text
| capture | `$`(位置 capture)・`$name`(名前付き位置 capture) | 名前付き capture の back-reference |
```

§4 本文に 1〜2 文の注記を足す: 「名前付き位置 capture(`$name`、body は暗黙の `_`)は murphy-9cr.17 で v1 採用。back-reference(同名=等価制約)は引き続き見送り。詳細は murphy-9cr.17 の design 参照。」

**Step 2: 最終品質ゲート**

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

すべて PASS させる。clippy 警告は実コードを直して解消する(`#[allow]` で握りつぶさない)。

**Step 3: commit**

```bash
git add docs/plans/2026-05-22-plugin-reboot-design.md
git commit -m "docs(plugin-reboot): adopt named positional captures in v1 grammar scope"
```

---

## 完了の定義

beads issue murphy-9cr.17 の acceptance(design フィールド参照)を全て満たすこと:

- `crates/murphy-pattern` がワークスペースに追加され `cargo build` が通る。
- `parse` が v1 文法全機能をパースし、不正パターンは span 付き `ParseError` を返す。
- `lower` / `compile` が `PatternAst` を `PatternIr` フラットノード配列へ変換する。
- `n_captures()` / `capture_kinds()` / 名前付き capture メタデータが正しい。
- murphy-ast に `NodeKindTag` 系 API があり、未知種別名がパースエラーになる。
- parse / lower スナップショットと `ParseError` span テストが揃う。
- `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo fmt --check` が通る。
```
