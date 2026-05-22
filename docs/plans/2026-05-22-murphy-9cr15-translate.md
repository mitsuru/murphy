# murphy-translate: prism→arena 変換層 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** prism AST を 1 ファイル 1 パス DFS で murphy-ast の arena AST へ翻訳する新規クレート `murphy-translate` を作る。

**Architecture:** 再帰ポストオーダー DFS で各 prism ノードを `AstBuilder` に push し、所有権を持つ `murphy_ast::Ast` を返す。prism と parser-gem のノード分割差（collapse/split）は変換層の内部だけで吸収する（Route B）。対応する `NodeKind` variant が無い prism ノードは `NodeKind::Unknown` へ落とし、panic しない。

**Tech Stack:** Rust 2024 / `ruby-prism = "=1.9.0"`（既存 exact pin）/ `murphy-ast`（path 依存）。TDD（@superpowers:test-driven-development）。

beads issue: murphy-9cr.15。設計は同 issue の design フィールド。

---

## 重要な前提（全タスク共通）

### NodeKind variant 追加時は 3 ファイルを同時更新

`NodeKind` に variant を追加すると、以下 3 箇所すべてが exhaustive match のため
コンパイルエラーになる。新 variant ごとに同一コミット内で全部更新する:

1. `crates/murphy-ast/src/node.rs` — `enum NodeKind` に **末尾追加**（ADR 0037:
   宣言順 = 判別子、追加は末尾のみ）。
2. `crates/murphy-ast/src/ast.rs` — `collect_children` に arm を追加。
   子を **ソース順** で `out` へ push（`descendants` の DFS 正しさが依存）。
3. `crates/murphy-ast/src/serialize.rs` — `write_node_kind` と `read_node_kind`
   の両方に arm を追加。判別子バイトは enum 宣言位置と一致させ、既存の最後の
   番号の次から連番。

`layout_invariants` テスト（`node.rs`）が `size_of::<NodeKind>() <= 32` /
`size_of::<AstNode>() <= 48` を要求する。本計画の variant は最大ペイロード
16 バイトに収まる設計なので維持されるはず。万一超えたら停止して報告する
（境界拡張は ADR 0037 に関わる設計判断）。

### prism API クイックリファレンス

末尾の「Appendix: prism API」に全アクセサ署名を収録。各タスクはそれを参照する。
要点:

- `ruby_prism::parse(source: &[u8]) -> ParseResult`。`ParseResult::node() -> Node`
  はルート（ProgramNode）。`.comments()` はコメントイテレータ。
- `Node::as_xxx_node() -> Option<XxxNode>` でダウンキャスト。`Node::location()
  -> Location`。`Location::start_offset()/end_offset() -> usize`。
- `ConstantId::as_slice() -> &[u8]` で識別子名を解決。
- `Integer` に i64 変換は無い。`TryInto<i32>` と `to_u32_digits() -> (bool,
  &[u32])` のみ（Task 2 で扱う）。
- `NodeList`: `.iter()` / `.len()` / `.is_empty()` / `&NodeList` は `IntoIterator`。

### 共通ヘルパー（Task 1 で `translate.rs` に作る）

```rust
use murphy_ast::{AstBuilder, NodeId, NodeKind, OptNodeId, Range};
use ruby_prism as prism;

struct Translator {
    builder: AstBuilder,
}

impl Translator {
    /// prism Location → murphy Range。
    fn range(loc: &prism::Location<'_>) -> Range {
        Range {
            start: loc.start_offset() as u32,
            end: loc.end_offset() as u32,
        }
    }

    /// Node の Range。
    fn node_range(node: &prism::Node<'_>) -> Range {
        Self::range(&node.location())
    }

    /// ConstantId → interned Symbol。非 UTF-8 は lossy 変換。
    fn sym(&mut self, cid: &prism::ConstantId<'_>) -> murphy_ast::Symbol {
        let text = String::from_utf8_lossy(cid.as_slice());
        self.builder.intern_symbol(&text)
    }
}
```

### テスト記法

計画内のテスト断片に出る `.kind(/*root*/)` は紙幅の都合の略記。実装時は
必ず `.kind(ast.root())` と書く（対象の `Ast` を `let ast = translate(...)` で
束縛してから `ast.kind(ast.root())`）。

### コミット規約

各タスク末尾でコミット。メッセージは `feat(murphy-translate): ...` 形式。
murphy-ast を触るタスクは `feat(murphy-ast): ...` を併記するか、変更が
murphy-translate 起点なら `feat(murphy-translate): ...` 本文で murphy-ast 拡張に
言及する。

---

## Task 1: クレート scaffold + `Unknown` variant + program/statements

新規クレートを作り、`translate` がコンパイル・実行でき、ProgramNode と
StatementsNode だけ翻訳して残りは `Unknown` に落とす状態にする。

**Files:**
- Create: `crates/murphy-translate/Cargo.toml`
- Create: `crates/murphy-translate/src/lib.rs`
- Create: `crates/murphy-translate/src/translate.rs`
- Modify: `crates/murphy-ast/src/node.rs`（`NodeKind::Unknown` 追加）
- Modify: `crates/murphy-ast/src/ast.rs`（`collect_children` に `Unknown`）
- Modify: `crates/murphy-ast/src/serialize.rs`（`Unknown` の read/write）
- Test: `crates/murphy-translate/src/translate.rs`（`#[cfg(test)]`）

**Step 0: prism API の前提確認（コードを書く前に）**

bindings.rs（`target/*/build/ruby-prism-*/out/bindings.rs`、`ruby-prism-sys`
ではない方）を開き、以下を実機確認する:

- `impl<'pr> ProgramNode<'pr>` 直下の `fn statements` の戻り値型 —
  `StatementsNode<'pr>` か `Option<StatementsNode<'pr>>` か。Option なら
  `translate_program` の `Some(prog.statements())` を `prog.statements()` の
  Option をそのまま渡す形に直す。
- `Node` に `as_statements_node()` / `as_program_node()` が存在すること。

**Step 1: `NodeKind::Unknown` を murphy-ast に追加**

`node.rs` の `enum NodeKind` 末尾（`Arg(Symbol)` の後）に追加:

```rust
    // --- fallback ---
    /// A valid prism node with no `NodeKind` mapping yet. Dispatch may
    /// treat it as opaque; `murphy-translate` never panics on unknown
    /// input. Distinct from `Error` (a prism *parse* error).
    Unknown,
```

`ast.rs` の `collect_children` で、`Error | Nil | True_ | ...` の葉ノード列に
`| NodeKind::Unknown` を追加（子なし）。

`serialize.rs` の `write_node_kind` 末尾 arm（既存最後の判別子の次の番号）:

```rust
        NodeKind::Unknown => put_u8(out, /* 既存最後の番号 + 1 */),
```

`read_node_kind` にも対応する番号の arm を追加し `NodeKind::Unknown` を返す。
（既存の番号付けを読み、連番を維持すること。）

**Step 2: `Cargo.toml`**

```toml
[package]
name = "murphy-translate"
version = "0.1.0"
edition = "2024"
description = "prism AST → murphy-ast arena AST translation layer (murphy-9cr.15)."

[dependencies]
# Exact pin: identical to murphy-core. A prism node-schema change must not
# ride in silently on the translation mapping (ADR 0001 discipline).
ruby-prism = "=1.9.0"
murphy-ast = { path = "../murphy-ast" }
```

**Step 3: `lib.rs`**

```rust
//! prism AST → murphy-ast arena AST の変換層（murphy-9cr.15）。
//!
//! prism と parser-gem のノード分割差（collapse/split）を変換層の内部だけで
//! 吸収する（Route B）。対応する `NodeKind` が無い prism ノードは
//! [`murphy_ast::NodeKind::Unknown`] へ落とし、`translate` は決して panic
//! しない。

mod translate;

pub use translate::translate;
```

**Step 4: 失敗するテストを書く（`translate.rs` の `#[cfg(test)]`）**

```rust
#[cfg(test)]
mod tests {
    use super::translate;
    use murphy_ast::NodeKind;

    #[test]
    fn empty_program_root_is_nil() {
        let ast = translate("", "t.rb");
        assert!(matches!(ast.kind(ast.root()), NodeKind::Nil));
    }

    #[test]
    fn single_statement_root_is_that_statement() {
        // `nil` 単文 → ルートはその文（NilNode → NodeKind::Nil）。
        let ast = translate("nil", "t.rb");
        assert!(matches!(ast.kind(ast.root()), NodeKind::Nil));
    }

    #[test]
    fn multi_statement_root_is_begin() {
        // Task 1 時点では各文は未対応 → Unknown だが、ルートは Begin。
        let ast = translate("1\n2\n", "t.rb");
        assert!(matches!(ast.kind(ast.root()), NodeKind::Begin(_)));
        assert_eq!(ast.children(ast.root()).count(), 2);
    }

    #[test]
    fn untranslated_node_falls_to_unknown() {
        // Task 1 時点で IntegerNode は未対応。
        let ast = translate("1", "t.rb");
        assert!(matches!(ast.kind(ast.root()), NodeKind::Unknown));
    }
}
```

**Step 5: テストが失敗することを確認**

Run: `cargo test -p murphy-translate`
Expected: コンパイルエラー（`translate` 未実装）。

**Step 6: `translate.rs` の本体を実装**

```rust
//! 再帰ポストオーダー DFS による prism→arena 変換。

use murphy_ast::{Ast, AstBuilder, NodeId, NodeKind, OptNodeId, Range};
use ruby_prism as prism;
use std::path::PathBuf;

/// Ruby ソースを prism で 1 回 parse し、所有権を持つ arena [`Ast`] へ翻訳する。
///
/// prism は total・panic-free なので本関数も常に成功する。prism の借用ツリーは
/// 本関数内で drop され、ライフタイムは外へ漏れない。
pub fn translate(source: &str, path: impl Into<PathBuf>) -> Ast {
    let result = prism::parse(source.as_bytes());
    let mut t = Translator {
        builder: AstBuilder::new(source, path),
    };
    let root = t.translate_program(&result.node());
    t.builder.finish(root)
}

struct Translator {
    builder: AstBuilder,
}

impl Translator {
    fn range(loc: &prism::Location<'_>) -> Range {
        Range {
            start: loc.start_offset() as u32,
            end: loc.end_offset() as u32,
        }
    }

    fn node_range(node: &prism::Node<'_>) -> Range {
        Self::range(&node.location())
    }

    /// ルート ProgramNode → arena ルート NodeId。
    fn translate_program(&mut self, node: &prism::Node<'_>) -> NodeId {
        let prog = match node.as_program_node() {
            Some(p) => p,
            // prism は常に ProgramNode を返すが、防御的に Unknown ルート。
            None => return self.builder.push(NodeKind::Unknown, Self::node_range(node)),
        };
        let fallback = Self::node_range(node);
        // parser-gem 準拠: 0 文 → nil、1 文 → その文、複数 → begin。
        // `prog.statements()` の Option 性は Step 0 で bindings 確認のうえ確定。
        match self.translate_stmts_opt(Some(prog.statements())).get() {
            Some(id) => id,
            None => self.builder.push(NodeKind::Nil, fallback),
        }
    }

    /// `Option<StatementsNode>` を「ノード 1 個ぶん」の `OptNodeId` に畳む。
    /// 0 文→None、1 文→その文、複数→`Begin`。これは **foundational helper**:
    /// プログラムルート・条件分岐の本体・ループ本体・`begin`/`rescue` 等すべてが
    /// これを使う（Task 8 以降で再利用、新規定義しない）。
    fn translate_stmts_opt(&mut self, stmts: Option<prism::StatementsNode<'_>>) -> OptNodeId {
        let stmts = match stmts {
            Some(s) => s,
            None => return OptNodeId::NONE,
        };
        let ids: Vec<NodeId> = stmts.body().iter().map(|n| self.translate_node(&n)).collect();
        match ids.len() {
            0 => OptNodeId::NONE,
            1 => OptNodeId::some(ids[0]),
            _ => {
                let list = self.builder.push_list(&ids);
                OptNodeId::some(
                    self.builder
                        .push(NodeKind::Begin(list), Self::range(&stmts.location())),
                )
            }
        }
    }

    /// def/class/module/block/sclass の `body`（`Option<Node>`）→ `OptNodeId`。
    /// 中身が `StatementsNode` なら `translate_stmts_opt` で parser-gem 準拠に
    /// 畳む。これも **foundational helper**（Task 6/11 で再利用、新規定義しない）。
    fn translate_body(&mut self, body: Option<prism::Node<'_>>) -> OptNodeId {
        match body {
            None => OptNodeId::NONE,
            Some(n) => match n.as_statements_node() {
                Some(s) => self.translate_stmts_opt(Some(s)),
                None => OptNodeId::some(self.translate_node(&n)),
            },
        }
    }

    /// 任意の prism ノードを翻訳して NodeId を返す。未対応は Unknown。
    fn translate_node(&mut self, node: &prism::Node<'_>) -> NodeId {
        let range = Self::node_range(node);
        // Task 2 以降、ここに各ノード種の arm を足していく。
        self.builder.push(NodeKind::Unknown, range)
    }
}
```

> `OptNodeId` を import に含めること（`use murphy_ast::{... OptNodeId ...}`）。

**Step 7: テストが通ることを確認**

Run: `cargo test -p murphy-translate`
Expected: 4 テスト PASS。

Run: `cargo build --workspace`
Expected: ワークスペース全体ビルド成功。

**Step 8: コミット**

```bash
git add crates/murphy-translate crates/murphy-ast
git commit -m "feat(murphy-translate): scaffold crate + Unknown variant + program/statements"
```

---

## Task 2: アトムとリテラル

NilNode / TrueNode / FalseNode / SelfNode / IntegerNode / FloatNode / StringNode
/ SymbolNode を翻訳する。対応する `NodeKind`（`Nil` `True_` `False_`
`SelfExpr` `Int` `Float` `Str` `Sym`）は既存。murphy-ast 変更なし。

**Files:**
- Modify: `crates/murphy-translate/src/translate.rs`

**Step 1: 失敗するテストを書く**

```rust
    #[test]
    fn translates_atoms() {
        assert!(matches!(translate("nil", "t.rb").kind(/*root*/), NodeKind::Nil));
        // true / false / self も同様にアサート。
    }

    #[test]
    fn translates_integer() {
        let ast = translate("42", "t.rb");
        assert!(matches!(ast.kind(ast.root()), NodeKind::Int(42)));
    }

    #[test]
    fn translates_negative_and_large_integer() {
        assert!(matches!(translate("-7", "t.rb").kind(/*root*/), NodeKind::Int(-7)));
        // i64 を超える巨大整数は Unknown に落ちること（panic しない）。
        let huge = translate("999999999999999999999999999999", "t.rb");
        assert!(matches!(huge.kind(huge.root()), NodeKind::Unknown));
    }

    #[test]
    fn translates_float_string_symbol() {
        // 3.5 → Float、"hi" → Str、:sym → Sym。
        let ast = translate("\"hi\"", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Str(s) => assert_eq!(ast.interner().resolve(s.0), "hi"),
            other => panic!("expected Str, got {other:?}"),
        }
    }
```

**Step 2: 失敗を確認** — `cargo test -p murphy-translate` で新規テストが FAIL。

**Step 3: `translate_node` に arm を追加**

`translate_node` の Unknown フォールバック前に分岐を追加。整数変換ヘルパも書く:

```rust
    /// prism Integer → i64。i64 を超えたら None（呼び出し側で Unknown に落とす）。
    ///
    /// `Integer` は `Copy` ではなく `TryInto<i32>` は `self` を消費するため、
    /// `to_u32_digits()`（`&self`、`(negative, &[u32])` を LSB 先頭で返す）だけで
    /// 全ケースを再構成する。
    fn integer_to_i64(int: &prism::Integer<'_>) -> Option<i64> {
        let (negative, digits) = int.to_u32_digits();
        let mut acc: u128 = 0;
        for &d in digits.iter().rev() {
            acc = acc.checked_mul(1u128 << 32)?.checked_add(d as u128)?;
        }
        if negative {
            // 最小値 i64::MIN の絶対値 = i64::MAX as u128 + 1 まで許容。
            if acc <= i64::MAX as u128 + 1 {
                Some((acc as i128).wrapping_neg() as i64)
            } else {
                None
            }
        } else {
            i64::try_from(acc).ok()
        }
    }
```

> 呼び出し側は `let v = int_node.value(); Self::integer_to_i64(&v)` のように、
> `IntegerNode::value()` が返す `Integer` を一旦束縛してから参照を渡す。

`translate_node` 内（`range` 算出後）:

```rust
        if node.as_nil_node().is_some() {
            return self.builder.push(NodeKind::Nil, range);
        }
        if node.as_true_node().is_some() {
            return self.builder.push(NodeKind::True_, range);
        }
        if node.as_false_node().is_some() {
            return self.builder.push(NodeKind::False_, range);
        }
        if node.as_self_node().is_some() {
            return self.builder.push(NodeKind::SelfExpr, range);
        }
        if let Some(int) = node.as_integer_node() {
            return match Self::integer_to_i64(&int.value()) {
                Some(v) => self.builder.push(NodeKind::Int(v), range),
                None => self.builder.push(NodeKind::Unknown, range),
            };
        }
        if let Some(f) = node.as_float_node() {
            return self.builder.push(NodeKind::Float(f.value()), range);
        }
        if let Some(s) = node.as_string_node() {
            let text = String::from_utf8_lossy(s.unescaped());
            let id = self.builder.intern_string(&text);
            return self.builder.push(NodeKind::Str(id), range);
        }
        if let Some(sym) = node.as_symbol_node() {
            // 単純シンボル :foo。value_loc が無いので unescaped を使う。
            // SymbolNode は補間なしシンボル。content は parts ではなく
            // 別アクセサ — Appendix を確認し、unescaped 相当を intern。
            let text = symbol_text(&sym); // Appendix 参照のうえ実装
            let id = self.builder.intern_symbol(&text);
            return self.builder.push(NodeKind::Sym(id), range);
        }
```

> 注: `SymbolNode` の内容アクセサは Appendix を確認すること。`unescaped()` が
> 無い場合は `value_loc()` の slice、または `opening_loc`/`closing_loc` を除いた
> content range から `Ast::raw_source` 相当でテキストを得る。実装時に
> bindings.rs（Appendix 記載のパス）で確認する。

**Step 4: テスト PASS を確認** — `cargo test -p murphy-translate`。

**Step 5: コミット**

```bash
git add crates/murphy-translate
git commit -m "feat(murphy-translate): translate atoms and literals"
```

---

## Task 3: 変数参照

LocalVariableReadNode → `Lvar`、InstanceVariableReadNode → `Ivar`、
ClassVariableReadNode → `Cvar`、GlobalVariableReadNode → `Gvar`。
ConstantReadNode + ConstantPathNode → `Const { scope, name }`（collapse: prism は
plain/path で別ノード、murphy は `scope` の None/Some で吸収）。murphy-ast 変更なし。

**Files:** Modify `crates/murphy-translate/src/translate.rs`

**Step 1: 失敗するテスト**

```rust
    #[test]
    fn translates_variable_reads() {
        // `@x` → Ivar、`$g` → Gvar、`x` だけだと LocalVariableRead ではなく
        // CallNode（variable_call）になる点に注意 — lvar は代入後にのみ出る。
        let ast = translate("@x", "t.rb");
        assert!(matches!(ast.kind(ast.root()), NodeKind::Ivar(_)));
    }

    #[test]
    fn translates_plain_constant() {
        let ast = translate("FOO", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Const { scope, name } => {
                assert!(scope.is_none());
                assert_eq!(ast.interner().resolve(name.0), "FOO");
            }
            other => panic!("expected Const, got {other:?}"),
        }
    }

    #[test]
    fn translates_constant_path() {
        // `A::B` → Const { scope: Some(Const A), name: B }。
        let ast = translate("A::B", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Const { scope, name } => {
                assert!(scope.get().is_some());
                assert_eq!(ast.interner().resolve(name.0), "B");
            }
            other => panic!("expected Const, got {other:?}"),
        }
    }
```

**Step 2: 失敗を確認。**

**Step 3: `translate_node` に arm 追加**

```rust
        if let Some(v) = node.as_local_variable_read_node() {
            let name = self.sym(&v.name()); // LocalVariableReadNode.name() — Appendix 確認
            return self.builder.push(NodeKind::Lvar(name), range);
        }
        if let Some(v) = node.as_instance_variable_read_node() {
            let name = self.sym(&v.name());
            return self.builder.push(NodeKind::Ivar(name), range);
        }
        // Cvar / Gvar も同様。
        if let Some(c) = node.as_constant_read_node() {
            let name = self.sym(&c.name());
            return self
                .builder
                .push(NodeKind::Const { scope: OptNodeId::NONE, name }, range);
        }
        if let Some(cp) = node.as_constant_path_node() {
            return self.translate_constant_path(&cp, range);
        }
```

ConstantPathNode 用ヘルパ:

```rust
    fn translate_constant_path(
        &mut self,
        cp: &prism::ConstantPathNode<'_>,
        range: Range,
    ) -> NodeId {
        // parent: Some(Node) なら `A::B`、None なら `::B`（トップレベル）。
        let scope = match cp.parent() {
            Some(p) => OptNodeId::some(self.translate_node(&p)),
            None => OptNodeId::NONE,
        };
        // name は Option<ConstantId>。None（壊れた path）なら Unknown。
        let name = match cp.name() {
            Some(cid) => self.sym(&cid),
            None => return self.builder.push(NodeKind::Unknown, range),
        };
        self.builder.push(NodeKind::Const { scope, name }, range)
    }
```

> `OptNodeId` を import に追加すること。

**Step 4: テスト PASS を確認。**

**Step 5: コミット** — `feat(murphy-translate): translate variable and constant reads`

---

## Task 4: 基本代入

`Lvasgn` `Ivasgn`（既存）、`Gvasgn` `Cvasgn`（**新規 variant**）、
`Casgn`（既存、ConstantWriteNode + ConstantPathWriteNode を collapse）。

**Files:**
- Modify: `crates/murphy-ast/src/node.rs`（`Gvasgn` `Cvasgn` 追加）
- Modify: `crates/murphy-ast/src/ast.rs`（`collect_children`）
- Modify: `crates/murphy-ast/src/serialize.rs`（read/write）
- Modify: `crates/murphy-translate/src/translate.rs`

**Step 1: murphy-ast に variant 追加**

`node.rs` の assignments セクション（`Casgn { .. }` の後）に追加:

```rust
    Gvasgn {
        name: Symbol,
        value: OptNodeId,
    },
    Cvasgn {
        name: Symbol,
        value: OptNodeId,
    },
```

`collect_children`: `Lvasgn { value, .. } | Ivasgn { value, .. }` の arm に
`| NodeKind::Gvasgn { value, .. } | NodeKind::Cvasgn { value, .. }` を追加。

`serialize.rs`: `Ivasgn` の write/read arm をコピーし `Gvasgn` `Cvasgn` 用に
連番判別子で追加。

**Step 2: 失敗するテスト（`translate.rs`）**

```rust
    #[test]
    fn translates_local_assignment() {
        let ast = translate("x = 1", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Lvasgn { name, value } => {
                assert_eq!(ast.interner().resolve(name.0), "x");
                assert!(value.get().is_some());
            }
            other => panic!("expected Lvasgn, got {other:?}"),
        }
    }

    #[test]
    fn translates_constant_assignment_plain_and_path() {
        assert!(matches!(translate("FOO = 1", "t.rb").kind(/*root*/), NodeKind::Casgn { .. }));
        // `A::B = 1` も Casgn（scope = Some）。
        let p = translate("A::B = 1", "t.rb");
        match p.kind(p.root()) {
            NodeKind::Casgn { scope, .. } => assert!(scope.get().is_some()),
            other => panic!("expected Casgn, got {other:?}"),
        }
    }
```

**Step 3: 失敗を確認。**

**Step 4: `translate_node` に arm 追加**

```rust
        if let Some(w) = node.as_local_variable_write_node() {
            let name = self.sym(&w.name());
            let value = OptNodeId::some(self.translate_node(&w.value()));
            return self.builder.push(NodeKind::Lvasgn { name, value }, range);
        }
        // Ivasgn / Gvasgn / Cvasgn も同様（InstanceVariableWriteNode 等）。
        if let Some(w) = node.as_constant_write_node() {
            let name = self.sym(&w.name());
            let value = OptNodeId::some(self.translate_node(&w.value()));
            return self
                .builder
                .push(NodeKind::Casgn { scope: OptNodeId::NONE, name, value }, range);
        }
        if let Some(w) = node.as_constant_path_write_node() {
            // target() は ConstantPathNode。その parent を scope、name を name に。
            let target = w.target();
            let scope = match target.parent() {
                Some(p) => OptNodeId::some(self.translate_node(&p)),
                None => OptNodeId::NONE,
            };
            let name = match target.name() {
                Some(cid) => self.sym(&cid),
                None => return self.builder.push(NodeKind::Unknown, range),
            };
            let value = OptNodeId::some(self.translate_node(&w.value()));
            return self.builder.push(NodeKind::Casgn { scope, name, value }, range);
        }
```

**Step 5: テスト PASS を確認。** `cargo test -p murphy-ast -p murphy-translate`。

**Step 6: コミット** — `feat(murphy-translate): translate basic assignments (+ Gvasgn/Cvasgn)`

---

## Task 5: メソッド呼び出し

CallNode → `Send` / `Csend`（`is_safe_navigation` で振り分け）。ArgumentsNode の
引数リスト。`BlockArgumentNode`（`&blk`）は args 末尾に `BlockPass` として付ける。
`SplatNode`（`*arr`）は `Splat`。`NodeKind`（`Send` `Csend` `BlockPass` `Splat`）
は既存。murphy-ast 変更なし。

> 注: CallNode が `{ } / do end` ブロックを持つ場合（`.block()` が `BlockNode`）の
> `Block` ラップは **Task 6** で扱う。本タスクでは `.block()` が
> `BlockArgumentNode` のケースのみ扱い、`BlockNode` のケースは一旦 args 無し
> 相当（または Unknown ラップ）にせず、Task 6 で完成させる前提で「block は
> BlockArgumentNode のみ処理」と明記する。

**Files:** Modify `crates/murphy-translate/src/translate.rs`

**Step 1: 失敗するテスト**

```rust
    #[test]
    fn translates_call_no_receiver() {
        // `puts 1` → Send { receiver: None, method: puts, args: [Int 1] }。
        let ast = translate("puts 1", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Send { receiver, method, args } => {
                assert!(receiver.is_none());
                assert_eq!(ast.interner().resolve(method.0), "puts");
                assert_eq!(args.len, 1);
            }
            other => panic!("expected Send, got {other:?}"),
        }
    }

    #[test]
    fn translates_call_with_receiver() {
        // `a.foo(b)` → Send { receiver: Some, method: foo, args: [..] }。
        let ast = translate("a.foo(b)", "t.rb");
        assert!(matches!(ast.kind(ast.root()), NodeKind::Send { .. }));
    }

    #[test]
    fn translates_safe_navigation_to_csend() {
        let ast = translate("a&.foo", "t.rb");
        assert!(matches!(ast.kind(ast.root()), NodeKind::Csend { .. }));
    }

    #[test]
    fn translates_block_pass_arg() {
        // `foo(&blk)` → Send の args 末尾が BlockPass。
        let ast = translate("foo(&blk)", "t.rb");
        if let NodeKind::Send { args, .. } = *ast.kind(ast.root()) {
            let last = ast.children(ast.root()).last().unwrap();
            assert!(matches!(ast.kind(last), NodeKind::BlockPass(_)));
            assert!(args.len >= 1);
        } else {
            panic!("expected Send");
        }
    }
```

**Step 2: 失敗を確認。**

**Step 3: 呼び出し翻訳ヘルパを実装**

```rust
    /// CallNode を Send/Csend へ。`block` が BlockArgumentNode の場合のみ
    /// args 末尾に BlockPass を付ける（BlockNode は Task 6）。
    fn translate_call(&mut self, call: &prism::CallNode<'_>, range: Range) -> NodeId {
        let method = self.sym(&call.name());
        let receiver = call.receiver();

        // 引数リスト。
        let mut arg_ids: Vec<NodeId> = Vec::new();
        if let Some(args) = call.arguments() {
            for a in args.arguments().iter() {
                arg_ids.push(self.translate_node(&a));
            }
        }
        // &blk → BlockPass を args 末尾へ。
        if let Some(blk) = call.block() {
            if let Some(ba) = blk.as_block_argument_node() {
                let expr = ba
                    .expression()
                    .map(|e| OptNodeId::some(self.translate_node(&e)))
                    .unwrap_or(OptNodeId::NONE);
                let bp = self
                    .builder
                    .push(NodeKind::BlockPass(expr), Self::range(&ba.location()));
                arg_ids.push(bp);
            }
            // blk が BlockNode のケースは Task 6 で translate_call の呼び出し側が処理。
        }
        let args = self.builder.push_list(&arg_ids);

        match (receiver, call.is_safe_navigation()) {
            (Some(r), true) => {
                let recv = self.translate_node(&r);
                self.builder
                    .push(NodeKind::Csend { receiver: recv, method, args }, range)
            }
            (recv_opt, _) => {
                let receiver = recv_opt
                    .map(|r| OptNodeId::some(self.translate_node(&r)))
                    .unwrap_or(OptNodeId::NONE);
                self.builder
                    .push(NodeKind::Send { receiver, method, args }, range)
            }
        }
    }
```

`translate_node` に arm:

```rust
        if let Some(call) = node.as_call_node() {
            return self.translate_call(&call, range);
        }
        if let Some(s) = node.as_splat_node() {
            let inner = s
                .value()
                .map(|v| OptNodeId::some(self.translate_node(&v)))
                .unwrap_or(OptNodeId::NONE);
            return self.builder.push(NodeKind::Splat(inner), range);
        }
```

**Step 4: テスト PASS を確認。**

**Step 5: コミット** — `feat(murphy-translate): translate method calls and splat args`

---

## Task 6: ブロックとパラメータ

BlockNode → `Block { call, args, body }`。block パラメータ
（BlockParametersNode / ParametersNode）→ `Args(NodeList)`。各パラメータ種を
翻訳: RequiredParameterNode → `Arg`（既存）、**新規** `Optarg` `Restarg`
`Kwarg` `Kwoptarg` `Kwrestarg` `Blockarg`。

**Files:**
- Modify: `crates/murphy-ast/src/node.rs`（6 variant 追加）
- Modify: `crates/murphy-ast/src/ast.rs`
- Modify: `crates/murphy-ast/src/serialize.rs`
- Modify: `crates/murphy-translate/src/translate.rs`

**Step 1: murphy-ast に variant 追加**（arguments セクション末尾、`Arg(Symbol)` の後）

```rust
    /// `def f(a = 1)` の `a = 1`。
    Optarg {
        name: Symbol,
        default: NodeId,
    },
    /// `*rest`。匿名 `*` は `name` が空文字 interned。
    Restarg(Symbol),
    /// `def f(k:)` の必須キーワード引数。
    Kwarg(Symbol),
    /// `def f(k: 1)` の省略可能キーワード引数。
    Kwoptarg {
        name: Symbol,
        default: NodeId,
    },
    /// `**opts`。匿名 `**` は `name` が空文字 interned。
    Kwrestarg(Symbol),
    /// `&blk`。匿名 `&` は `name` が空文字 interned。
    Blockarg(Symbol),
```

> 設計判断: 匿名 rest/kwrest/block の名前は `OptSymbol` 型を新設せず、空文字
> `""`（Ruby の有効な識別子になり得ない）を interned した `Symbol` で表す。
> v1 の簡略化として記録。

`collect_children`:
- `Optarg { default, .. }` → `out.push(default)`
- `Kwoptarg { default, .. }` → `out.push(default)`
- `Restarg(_) | Kwarg(_) | Kwrestarg(_) | Blockarg(_)` → 葉ノード列に追加

`serialize.rs`: read/write に 6 arm を連番判別子で追加。

**Step 2: 失敗するテスト**

```rust
    #[test]
    fn translates_block() {
        // `[1].each { |x| x }` → ルートは Send、その block 子が Block。
        let ast = translate("[1].each { |x| x }", "t.rb");
        // ルートは Block（call を包む）。
        assert!(matches!(ast.kind(ast.root()), NodeKind::Block { .. }));
    }

    #[test]
    fn translates_method_parameters() {
        // `def f(a, b = 1, *r, k:, m: 2, **o, &blk); end`
        let ast = translate("def f(a, b = 1, *r, k:, m: 2, **o, &blk); end", "t.rb");
        // Def の args 子（Args）に 7 つのパラメータが順に並ぶ。
        // Def は Task 11 で完成。本タスクでは parameters 翻訳ヘルパ単体を
        // ブロックパラメータ経由でテストする:
        let b = translate("foo { |a, b = 1, *r, &blk| a }", "t.rb");
        assert!(matches!(b.kind(b.root()), NodeKind::Block { .. }));
    }
```

> Task 6 時点で `Def` 未対応。メソッド定義の本格テストは Task 11。本タスクは
> ブロックパラメータでパラメータ翻訳を検証する。

**Step 3: 失敗を確認。**

**Step 4: パラメータ翻訳ヘルパ**

```rust
    /// ParametersNode → Args ノードの NodeId。requireds → optionals → rest
    /// → posts → keywords → keyword_rest → block の順（parser-gem 順）。
    fn translate_parameters(
        &mut self,
        params: Option<prism::ParametersNode<'_>>,
        args_range: Range,
    ) -> NodeId {
        let mut ids: Vec<NodeId> = Vec::new();
        if let Some(p) = &params {
            for n in p.requireds().iter() {
                ids.push(self.translate_param(&n));
            }
            for n in p.optionals().iter() {
                ids.push(self.translate_param(&n));
            }
            if let Some(rest) = p.rest() {
                ids.push(self.translate_param(&rest));
            }
            for n in p.posts().iter() {
                ids.push(self.translate_param(&n));
            }
            for n in p.keywords().iter() {
                ids.push(self.translate_param(&n));
            }
            if let Some(kwrest) = p.keyword_rest() {
                ids.push(self.translate_param(&kwrest));
            }
            if let Some(block) = p.block() {
                ids.push(self.translate_param(&block.as_node()));
            }
        }
        let list = self.builder.push_list(&ids);
        self.builder.push(NodeKind::Args(list), args_range)
    }

    /// 単一パラメータ prism ノード → arg 系 NodeKind。
    fn translate_param(&mut self, node: &prism::Node<'_>) -> NodeId {
        let range = Self::node_range(node);
        if let Some(p) = node.as_required_parameter_node() {
            let name = self.sym(&p.name());
            return self.builder.push(NodeKind::Arg(name), range);
        }
        if let Some(p) = node.as_optional_parameter_node() {
            let name = self.sym(&p.name());
            let default = self.translate_node(&p.value());
            return self.builder.push(NodeKind::Optarg { name, default }, range);
        }
        if let Some(p) = node.as_rest_parameter_node() {
            let name = self.opt_sym(p.name()); // 下記ヘルパ
            return self.builder.push(NodeKind::Restarg(name), range);
        }
        if let Some(p) = node.as_required_keyword_parameter_node() {
            let name = self.sym(&p.name());
            return self.builder.push(NodeKind::Kwarg(name), range);
        }
        if let Some(p) = node.as_optional_keyword_parameter_node() {
            let name = self.sym(&p.name());
            let default = self.translate_node(&p.value());
            return self.builder.push(NodeKind::Kwoptarg { name, default }, range);
        }
        if let Some(p) = node.as_keyword_rest_parameter_node() {
            let name = self.opt_sym(p.name());
            return self.builder.push(NodeKind::Kwrestarg(name), range);
        }
        if let Some(p) = node.as_block_parameter_node() {
            let name = self.opt_sym(p.name());
            return self.builder.push(NodeKind::Blockarg(name), range);
        }
        // MultiTargetNode を分割代入パラメータとして受ける等は Task 16 / Unknown。
        self.builder.push(NodeKind::Unknown, range)
    }

    /// Option<ConstantId> → Symbol。None（匿名）は空文字を interned。
    fn opt_sym(&mut self, cid: Option<prism::ConstantId<'_>>) -> murphy_ast::Symbol {
        match cid {
            Some(c) => self.sym(&c),
            None => self.builder.intern_symbol(""),
        }
    }
```

**Step 5: ブロック翻訳と `translate_call` 連携**

`translate_node` の CallNode arm を更新: `call.block()` が `BlockNode` の場合、
`translate_call` で素の Send/Csend を作ったうえで `Block` で包む。

```rust
        if let Some(call) = node.as_call_node() {
            let send = self.translate_call(&call, range);
            // `{ } / do end` ブロック付き呼び出しは Block で包む。
            if let Some(blk) = call.block() {
                if let Some(block_node) = blk.as_block_node() {
                    return self.translate_block(&block_node, send, range);
                }
            }
            return send;
        }
```

```rust
    /// BlockNode + 既に翻訳済みの call NodeId → Block ノード。
    fn translate_block(
        &mut self,
        block: &prism::BlockNode<'_>,
        call: NodeId,
        range: Range,
    ) -> NodeId {
        // parameters: BlockParametersNode（`|...|`）または ParametersNode 直。
        let params_node = block.parameters().and_then(|p| {
            p.as_block_parameters_node()
                .and_then(|bp| bp.parameters())
                .or_else(|| p.as_parameters_node())
        });
        let block_loc = Self::range(&block.location());
        let args = self.translate_parameters(params_node, block_loc);
        // body は Task 1 の translate_body で（StatementsNode を畳む）。
        let body = self.translate_body(block.body());
        self.builder.push(NodeKind::Block { call, args, body }, range)
    }
```

> `BlockNode.parameters()` は `Option<Node>`。`BlockParametersNode`（`|x|` 構文）
> か、numbered/it パラメータノードのことがある。後者は v1 では params 空 Args
> として扱う（Unknown を避ける）。Appendix を確認して実装する。

**Step 6: テスト PASS を確認。** `cargo test -p murphy-ast -p murphy-translate`。

**Step 7: コミット** — `feat(murphy-translate): translate blocks and parameters (+ 6 param variants)`

---

## Task 7: コレクション

ArrayNode → `Array`、HashNode → `Hash`、AssocNode → `Pair`、AssocSplatNode →
**新規** `Kwsplat`。`Array` `Hash` `Pair` は既存。

**Files:**
- Modify: `crates/murphy-ast/src/node.rs`（`Kwsplat` 追加）
- Modify: `crates/murphy-ast/src/ast.rs` / `serialize.rs`
- Modify: `crates/murphy-translate/src/translate.rs`

**Step 1: murphy-ast に `Kwsplat` 追加**（collections セクション、`Pair` の後）

```rust
    /// `**h` — ハッシュ内のキーワード splat。
    Kwsplat(OptNodeId),
```

`collect_children`: `BlockPass(o) | Splat(o) | Return(o)` の arm に
`| NodeKind::Kwsplat(o)` を追加。`serialize.rs` に read/write arm。

**Step 2: 失敗するテスト**

```rust
    #[test]
    fn translates_array_and_hash() {
        let arr = translate("[1, 2, 3]", "t.rb");
        match arr.kind(arr.root()) {
            NodeKind::Array(l) => assert_eq!(l.len, 3),
            other => panic!("expected Array, got {other:?}"),
        }
        let h = translate("{ a: 1, **rest }", "t.rb");
        match h.kind(h.root()) {
            NodeKind::Hash(l) => assert_eq!(l.len, 2),
            other => panic!("expected Hash, got {other:?}"),
        }
    }

    #[test]
    fn translates_pair_and_kwsplat() {
        let h = translate("{ a: 1, **rest }", "t.rb");
        let kids: Vec<_> = h.children(h.root()).collect();
        assert!(matches!(h.kind(kids[0]), NodeKind::Pair { .. }));
        assert!(matches!(h.kind(kids[1]), NodeKind::Kwsplat(_)));
    }
```

**Step 3: 失敗を確認。**

**Step 4: `translate_node` に arm 追加**

```rust
        if let Some(a) = node.as_array_node() {
            let ids: Vec<NodeId> = a.elements().iter().map(|e| self.translate_node(&e)).collect();
            let list = self.builder.push_list(&ids);
            return self.builder.push(NodeKind::Array(list), range);
        }
        if let Some(h) = node.as_hash_node() {
            let ids: Vec<NodeId> = h.elements().iter().map(|e| self.translate_node(&e)).collect();
            let list = self.builder.push_list(&ids);
            return self.builder.push(NodeKind::Hash(list), range);
        }
        if let Some(assoc) = node.as_assoc_node() {
            let key = self.translate_node(&assoc.key());
            let value = self.translate_node(&assoc.value());
            return self.builder.push(NodeKind::Pair { key, value }, range);
        }
        if let Some(splat) = node.as_assoc_splat_node() {
            let inner = splat
                .value()
                .map(|v| OptNodeId::some(self.translate_node(&v)))
                .unwrap_or(OptNodeId::NONE);
            return self.builder.push(NodeKind::Kwsplat(inner), range);
        }
```

**Step 5: テスト PASS を確認。**

**Step 6: コミット** — `feat(murphy-translate): translate collections (+ Kwsplat)`

---

## Task 8: 条件分岐

IfNode + UnlessNode → `If`（collapse）。CaseNode → `Case`、WhenNode → `When`。
`unless` は parser-gem 準拠で then/else を入れ替える。すべて既存 `NodeKind`。

**Files:** Modify `crates/murphy-translate/src/translate.rs`

**Step 1: 失敗するテスト**

```rust
    #[test]
    fn translates_if() {
        // `if c then a else b end`
        let ast = translate("if c\n  a\nelse\n  b\nend", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::If { then_, else_, .. } => {
                assert!(then_.get().is_some());
                assert!(else_.get().is_some());
            }
            other => panic!("expected If, got {other:?}"),
        }
    }

    #[test]
    fn translates_unless_swaps_branches() {
        // `unless c then a end` → If { cond: c, then_: None, else_: a }
        let ast = translate("unless c\n  a\nend", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::If { then_, else_, .. } => {
                assert!(then_.is_none(), "unless: then_ は None");
                assert!(else_.get().is_some(), "unless: else_ に本体");
            }
            other => panic!("expected If, got {other:?}"),
        }
    }

    #[test]
    fn translates_case_when() {
        let ast = translate("case x\nwhen 1\n  a\nelse\n  b\nend", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Case { subject, whens, else_ } => {
                assert!(subject.get().is_some());
                assert_eq!(whens.len, 1);
                assert!(else_.get().is_some());
            }
            other => panic!("expected Case, got {other:?}"),
        }
    }
```

**Step 2: 失敗を確認。**

**Step 3: arm 追加**

本体の畳み込みには Task 1 で定義済みの `translate_stmts_opt`
（`Option<StatementsNode> → OptNodeId`）をそのまま使う。新規定義しない。

`translate_node` arm:

```rust
        if let Some(iff) = node.as_if_node() {
            let cond = self.translate_node(&iff.predicate());
            let then_ = self.translate_stmts_opt(iff.statements());
            // subsequent: ElseNode か 別の IfNode（elsif）。
            let else_ = match iff.subsequent() {
                Some(sub) => {
                    if let Some(els) = sub.as_else_node() {
                        self.translate_stmts_opt(els.statements())
                    } else {
                        OptNodeId::some(self.translate_node(&sub))
                    }
                }
                None => OptNodeId::NONE,
            };
            return self.builder.push(NodeKind::If { cond, then_, else_ }, range);
        }
        if let Some(unl) = node.as_unless_node() {
            // parser-gem 準拠: unless は then/else を入れ替える。
            let cond = self.translate_node(&unl.predicate());
            let body = self.translate_stmts_opt(unl.statements());
            let else_branch = match unl.else_clause() {
                Some(els) => self.translate_stmts_opt(els.statements()),
                None => OptNodeId::NONE,
            };
            return self.builder.push(
                NodeKind::If { cond, then_: else_branch, else_: body },
                range,
            );
        }
        if let Some(c) = node.as_case_node() {
            let subject = c
                .predicate()
                .map(|p| OptNodeId::some(self.translate_node(&p)))
                .unwrap_or(OptNodeId::NONE);
            let when_ids: Vec<NodeId> =
                c.conditions().iter().map(|w| self.translate_node(&w)).collect();
            let whens = self.builder.push_list(&when_ids);
            let else_ = match c.else_clause() {
                Some(els) => self.translate_stmts_opt(els.statements()),
                None => OptNodeId::NONE,
            };
            return self.builder.push(NodeKind::Case { subject, whens, else_ }, range);
        }
        if let Some(w) = node.as_when_node() {
            let cond_ids: Vec<NodeId> =
                w.conditions().iter().map(|c| self.translate_node(&c)).collect();
            let conds = self.builder.push_list(&cond_ids);
            let body = self.translate_stmts_opt(w.statements());
            return self.builder.push(NodeKind::When { conds, body }, range);
        }
```

**Step 4: テスト PASS を確認。**

**Step 5: コミット** — `feat(murphy-translate): translate conditionals (if/unless/case/when)`

---

## Task 9: ループ

WhileNode / UntilNode → **新規** `While` / `Until`。`is_begin_modifier` を
`post: bool` フラグに畳む（while/while_post の collapse）。

**Files:**
- Modify: `crates/murphy-ast/src/node.rs` / `ast.rs` / `serialize.rs`
- Modify: `crates/murphy-translate/src/translate.rs`

**Step 1: murphy-ast に variant 追加**（control flow セクション、`Or` の後）

```rust
    While {
        cond: NodeId,
        body: OptNodeId,
        /// `true` なら do-while（`begin..end while c`）。
        post: bool,
    },
    Until {
        cond: NodeId,
        body: OptNodeId,
        post: bool,
    },
```

`collect_children`: 新 arm。`cond` を push、`body` を push_opt（ソース順: cond → body）。

```rust
        NodeKind::While { cond, body, .. } | NodeKind::Until { cond, body, .. } => {
            out.push(cond);
            push_opt(out, body);
        }
```

`serialize.rs`: `bool` のシリアライズが既存に無ければ `put_u8(out, post as u8)`
/ 読み出しは `!= 0` で。read/write arm を追加。

**Step 2: 失敗するテスト**

```rust
    #[test]
    fn translates_while_and_until() {
        let w = translate("while c\n  x\nend", "t.rb");
        match w.kind(w.root()) {
            NodeKind::While { post, .. } => assert!(!post),
            other => panic!("expected While, got {other:?}"),
        }
        let u = translate("until c\n  x\nend", "t.rb");
        assert!(matches!(u.kind(u.root()), NodeKind::Until { .. }));
    }

    #[test]
    fn translates_do_while_post_flag() {
        let w = translate("begin\n  x\nend while c", "t.rb");
        match w.kind(w.root()) {
            NodeKind::While { post, .. } => assert!(post, "do-while は post=true"),
            other => panic!("expected While, got {other:?}"),
        }
    }
```

**Step 3: 失敗を確認。**

**Step 4: `translate_node` に arm 追加**

```rust
        if let Some(w) = node.as_while_node() {
            let cond = self.translate_node(&w.predicate());
            let body = self.translate_stmts_opt(w.statements());
            return self.builder.push(
                NodeKind::While { cond, body, post: w.is_begin_modifier() },
                range,
            );
        }
        if let Some(u) = node.as_until_node() {
            let cond = self.translate_node(&u.predicate());
            let body = self.translate_stmts_opt(u.statements());
            return self.builder.push(
                NodeKind::Until { cond, body, post: u.is_begin_modifier() },
                range,
            );
        }
```

**Step 5: テスト PASS を確認。**

**Step 6: コミット** — `feat(murphy-translate): translate while/until loops (+ While/Until variants)`

---

## Task 10: 論理演算と範囲

AndNode → `And`、OrNode → `Or`（既存）。RangeNode → **新規** `Range`
（`exclusive: bool` で irange/erange を collapse、beginless/endless は OptNodeId）。

> 注意: murphy-ast には既に `Range`（ソース範囲を表す `#[repr(C)]` struct）が
> ある。新 variant はそれと名前衝突する。**variant 名は `RangeExpr`** とし、
> `Range` struct と区別する。

**Files:**
- Modify: `crates/murphy-ast/src/node.rs` / `ast.rs` / `serialize.rs`
- Modify: `crates/murphy-translate/src/translate.rs`

**Step 1: murphy-ast に `RangeExpr` 追加**（control flow セクション付近）

```rust
    /// `a..b` / `a...b`。beginless/endless は端が `None`。
    /// 型名 `Range` は既存のソース範囲 struct と衝突するため `RangeExpr`。
    RangeExpr {
        begin_: OptNodeId,
        end_: OptNodeId,
        /// `true` なら `...`（終端排他）。
        exclusive: bool,
    },
```

`collect_children`: `begin_` → `end_` の順で push_opt。`serialize.rs` arm 追加。

**Step 2: 失敗するテスト**

```rust
    #[test]
    fn translates_and_or() {
        assert!(matches!(translate("a && b", "t.rb").kind(/*root*/), NodeKind::And { .. }));
        assert!(matches!(translate("a || b", "t.rb").kind(/*root*/), NodeKind::Or { .. }));
    }

    #[test]
    fn translates_range() {
        let inc = translate("1..5", "t.rb");
        match inc.kind(inc.root()) {
            NodeKind::RangeExpr { exclusive, begin_, end_ } => {
                assert!(!exclusive);
                assert!(begin_.get().is_some() && end_.get().is_some());
            }
            other => panic!("expected RangeExpr, got {other:?}"),
        }
        // endless range `1..`
        let endless = translate("1..", "t.rb");
        match endless.kind(endless.root()) {
            NodeKind::RangeExpr { end_, .. } => assert!(end_.is_none()),
            other => panic!("expected RangeExpr, got {other:?}"),
        }
    }
```

**Step 3: 失敗を確認。**

**Step 4: `translate_node` に arm 追加**

```rust
        if let Some(a) = node.as_and_node() {
            let lhs = self.translate_node(&a.left());
            let rhs = self.translate_node(&a.right());
            return self.builder.push(NodeKind::And { lhs, rhs }, range);
        }
        if let Some(o) = node.as_or_node() {
            let lhs = self.translate_node(&o.left());
            let rhs = self.translate_node(&o.right());
            return self.builder.push(NodeKind::Or { lhs, rhs }, range);
        }
        if let Some(r) = node.as_range_node() {
            let begin_ = r
                .left()
                .map(|n| OptNodeId::some(self.translate_node(&n)))
                .unwrap_or(OptNodeId::NONE);
            let end_ = r
                .right()
                .map(|n| OptNodeId::some(self.translate_node(&n)))
                .unwrap_or(OptNodeId::NONE);
            return self.builder.push(
                NodeKind::RangeExpr { begin_, end_, exclusive: r.is_exclude_end() },
                range,
            );
        }
```

**Step 5: テスト PASS を確認。**

**Step 6: コミット** — `feat(murphy-translate): translate and/or and ranges (+ RangeExpr)`

---

## Task 11: 定義（def / class / module / sclass）

DefNode → `Def`（singleton も collapse: **`Def` に `receiver` フィールドを追加**）。
ClassNode → `Class`、ModuleNode → `Module`（既存）。SingletonClassNode →
**新規** `Sclass`。

**Files:**
- Modify: `crates/murphy-ast/src/node.rs`（`Def` 改修 + `Sclass` 追加）
- Modify: `crates/murphy-ast/src/ast.rs` / `serialize.rs`
- Modify: `crates/murphy-translate/src/translate.rs`

**Step 1: `Def` を改修し `Sclass` を追加**

`node.rs` の `Def` を変更:

```rust
    Def {
        /// singleton method（`def self.foo`）なら receiver が `Some`。
        receiver: OptNodeId,
        name: Symbol,
        args: NodeId,
        body: OptNodeId,
    },
```

definitions セクションに追加:

```rust
    /// `class << expr ... end`。
    Sclass {
        expr: NodeId,
        body: OptNodeId,
    },
```

`collect_children` の `Def` arm をソース順 receiver → args → body に更新:

```rust
        NodeKind::Def { receiver, args, body, .. } => {
            push_opt(out, receiver);
            out.push(args);
            push_opt(out, body);
        }
        NodeKind::Sclass { expr, body } => {
            out.push(expr);
            push_opt(out, body);
        }
```

`serialize.rs`: `Def` の write/read arm に `receiver` フィールドを追加（判別子
番号は不変、ペイロードに 1 フィールド増）。`Sclass` arm を新規追加。

> `builder.rs` / `ast.rs` の既存テストで `NodeKind::Def { .. }` を構築している
> 箇所があればコンパイルエラーになる。`receiver: OptNodeId::NONE` を補って
> 修正する（既存テストの意味は不変）。

**Step 2: 失敗するテスト**

```rust
    #[test]
    fn translates_def() {
        let ast = translate("def foo(a); a; end", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Def { receiver, name, body, .. } => {
                assert!(receiver.is_none());
                assert_eq!(ast.interner().resolve(name.0), "foo");
                assert!(body.get().is_some());
            }
            other => panic!("expected Def, got {other:?}"),
        }
    }

    #[test]
    fn translates_singleton_def() {
        let ast = translate("def self.foo; end", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Def { receiver, .. } => assert!(receiver.get().is_some()),
            other => panic!("expected Def, got {other:?}"),
        }
    }

    #[test]
    fn translates_class_module_sclass() {
        assert!(matches!(translate("class C; end", "t.rb").kind(/*root*/), NodeKind::Class { .. }));
        assert!(matches!(translate("module M; end", "t.rb").kind(/*root*/), NodeKind::Module { .. }));
        assert!(matches!(translate("class << self; end", "t.rb").kind(/*root*/), NodeKind::Sclass { .. }));
    }

    #[test]
    fn translates_class_with_superclass() {
        let ast = translate("class C < D; end", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Class { superclass, .. } => assert!(superclass.get().is_some()),
            other => panic!("expected Class, got {other:?}"),
        }
    }
```

**Step 3: 失敗を確認。**

**Step 4: `translate_node` に arm 追加**

```rust
        if let Some(d) = node.as_def_node() {
            let receiver = d
                .receiver()
                .map(|r| OptNodeId::some(self.translate_node(&r)))
                .unwrap_or(OptNodeId::NONE);
            let name = self.sym(&d.name());
            let args = self.translate_parameters(d.parameters(), range);
            let body = d
                .body()
                .map(|b| OptNodeId::some(self.translate_node(&b)))
                .unwrap_or(OptNodeId::NONE);
            return self.builder.push(NodeKind::Def { receiver, name, args, body }, range);
        }
        if let Some(c) = node.as_class_node() {
            let name = self.translate_node(&c.constant_path());
            let superclass = c
                .superclass()
                .map(|s| OptNodeId::some(self.translate_node(&s)))
                .unwrap_or(OptNodeId::NONE);
            let body = c
                .body()
                .map(|b| OptNodeId::some(self.translate_node(&b)))
                .unwrap_or(OptNodeId::NONE);
            return self.builder.push(NodeKind::Class { name, superclass, body }, range);
        }
        if let Some(m) = node.as_module_node() {
            let name = self.translate_node(&m.constant_path());
            let body = m
                .body()
                .map(|b| OptNodeId::some(self.translate_node(&b)))
                .unwrap_or(OptNodeId::NONE);
            return self.builder.push(NodeKind::Module { name, body }, range);
        }
        if let Some(sc) = node.as_singleton_class_node() {
            let expr = self.translate_node(&sc.expression());
            let body = sc
                .body()
                .map(|b| OptNodeId::some(self.translate_node(&b)))
                .unwrap_or(OptNodeId::NONE);
            return self.builder.push(NodeKind::Sclass { expr, body }, range);
        }
```

> **重要**: 上のコード断片の `body` 算出は説明のための簡略形。`d.body()` /
> `c.body()` / `m.body()` / `sc.body()` はいずれも `Option<Node>` で中身が
> `StatementsNode` のことがあり、素の `translate_node` では Unknown に潰れる。
> **実際には Task 1 で定義済みの `translate_body`（`Option<Node> → OptNodeId`、
> StatementsNode を畳む）を使う**。上の各 arm の
> `.body().map(|b| OptNodeId::some(self.translate_node(&b))).unwrap_or(...)` は
> すべて `self.translate_body(<node>.body())` に置き換えること。Task 6 の
> `translate_block` も既に `translate_body` を使っている前提。

**Step 5: テスト PASS を確認。** `cargo test -p murphy-ast -p murphy-translate`
（murphy-ast の既存テストが `Def` 改修で壊れていないことも確認）。

**Step 6: コミット** — `feat(murphy-translate): translate def/class/module/sclass (+ Def.receiver, Sclass)`

---

## Task 12: ジャンプ（return / break / next / yield / super / defined?）

ReturnNode → `Return`（既存）。**新規** `Break` `Next` `Yield` `Super` `Zsuper`
`Defined`。

**Files:**
- Modify: `crates/murphy-ast/src/node.rs` / `ast.rs` / `serialize.rs`
- Modify: `crates/murphy-translate/src/translate.rs`

**Step 1: murphy-ast に 6 variant 追加**（control flow セクション）

```rust
    /// `break`（引数 0→None、1→その式、複数→Array）。
    Break(OptNodeId),
    /// `next`（同上）。
    Next(OptNodeId),
    /// `yield`（引数リスト）。
    Yield(NodeList),
    /// `super(args)`（明示引数あり）。
    Super(NodeList),
    /// `super`（引数も括弧も無いゼロ引数 super）。
    Zsuper,
    /// `defined?(expr)`。
    Defined(NodeId),
```

`collect_children`:
- `Break(o) | Next(o)` → `BlockPass(o) | Splat(o) | Return(o) | Kwsplat(o)` の
  arm に合流。
- `Yield(l) | Super(l)` → list 系 arm（`Array(l) | Hash(l) | Begin(l) | Args(l)`）
  に合流。
- `Zsuper` → 葉ノード列に追加。
- `Defined(n)` → `out.push(n)` の単独 arm。

`serialize.rs`: 6 arm 追加。

**Step 2: 失敗するテスト**

```rust
    #[test]
    fn translates_jumps() {
        // すべて def 本体に入れて検証（トップレベル break 等は構文エラー回避）。
        let ast = translate("def f; return 1; end", "t.rb");
        // body をたどって Return を確認。
        // 簡便には descendants で種別を数える:
        assert!(ast.descendants(ast.root()).any(|n| matches!(ast.kind(n), NodeKind::Return(_))));
    }

    #[test]
    fn translates_yield_super_defined() {
        let y = translate("def f; yield 1; end", "t.rb");
        assert!(y.descendants(y.root()).any(|n| matches!(y.kind(n), NodeKind::Yield(_))));
        let z = translate("def f; super; end", "t.rb");
        assert!(z.descendants(z.root()).any(|n| matches!(z.kind(n), NodeKind::Zsuper)));
        let d = translate("defined?(x)", "t.rb");
        assert!(matches!(d.kind(d.root()), NodeKind::Defined(_)));
    }
```

**Step 3: 失敗を確認。**

**Step 4: `translate_node` に arm 追加**

`ArgumentsNode` を `break`/`next` の単一 OptNodeId に畳むヘルパ:

```rust
    /// break/next の引数。0→None、1→その式、複数→Array。
    fn translate_jump_arg(&mut self, args: Option<prism::ArgumentsNode<'_>>, range: Range) -> OptNodeId {
        let args = match args {
            Some(a) => a,
            None => return OptNodeId::NONE,
        };
        let ids: Vec<NodeId> = args.arguments().iter().map(|n| self.translate_node(&n)).collect();
        match ids.len() {
            0 => OptNodeId::NONE,
            1 => OptNodeId::some(ids[0]),
            _ => {
                let list = self.builder.push_list(&ids);
                OptNodeId::some(self.builder.push(NodeKind::Array(list), range))
            }
        }
    }
```

arm:

```rust
        if let Some(r) = node.as_return_node() {
            return {
                let v = self.translate_jump_arg(r.arguments(), range);
                self.builder.push(NodeKind::Return(v), range)
            };
        }
        if let Some(b) = node.as_break_node() {
            let v = self.translate_jump_arg(b.arguments(), range);
            return self.builder.push(NodeKind::Break(v), range);
        }
        if let Some(n) = node.as_next_node() {
            let v = self.translate_jump_arg(n.arguments(), range);
            return self.builder.push(NodeKind::Next(v), range);
        }
        if let Some(y) = node.as_yield_node() {
            let ids: Vec<NodeId> = match y.arguments() {
                Some(a) => a.arguments().iter().map(|n| self.translate_node(&n)).collect(),
                None => Vec::new(),
            };
            let list = self.builder.push_list(&ids);
            return self.builder.push(NodeKind::Yield(list), range);
        }
        if let Some(s) = node.as_super_node() {
            let ids: Vec<NodeId> = match s.arguments() {
                Some(a) => a.arguments().iter().map(|n| self.translate_node(&n)).collect(),
                None => Vec::new(),
            };
            let list = self.builder.push_list(&ids);
            return self.builder.push(NodeKind::Super(list), range);
        }
        if node.as_forwarding_super_node().is_some() {
            return self.builder.push(NodeKind::Zsuper, range);
        }
        if let Some(d) = node.as_defined_node() {
            let inner = self.translate_node(&d.value());
            return self.builder.push(NodeKind::Defined(inner), range);
        }
```

> 既存の `Return` arm が Task 1〜11 のどこかで簡易実装されていれば、この
> `translate_jump_arg` 版で置き換える。

**Step 5: テスト PASS を確認。**

**Step 6: コミット** — `feat(murphy-translate): translate jumps (return/break/next/yield/super/defined)`

---

## Task 13: 例外（begin / rescue / ensure）

BeginNode → `Begin`。**新規** `Rescue` `Resbody` `Ensure`。parser-gem の
`kwbegin(ensure(rescue(body, resbody*, else), ensure_body))` ネストを変換層で
組み立てる。

**Files:**
- Modify: `crates/murphy-ast/src/node.rs` / `ast.rs` / `serialize.rs`
- Modify: `crates/murphy-translate/src/translate.rs`

**Step 1: murphy-ast に 3 variant 追加**

```rust
    /// `begin..rescue..else..end` の rescue 構造。
    Rescue {
        /// 保護対象本体。
        body: OptNodeId,
        /// `Resbody` の並び。
        resbodies: NodeList,
        /// `else` 節。
        else_: OptNodeId,
    },
    /// 単一の `rescue Exc => e; ...` 節。
    Resbody {
        /// 捕捉する例外クラスの並び（無指定なら空）。
        exceptions: NodeList,
        /// `=> e` の束縛先（無ければ None）。
        var: OptNodeId,
        body: OptNodeId,
    },
    /// `ensure` 構造。`body` は保護本体（rescue 節 or 素の本体）。
    Ensure {
        body: OptNodeId,
        ensure_: OptNodeId,
    },
```

`collect_children`:
- `Rescue { body, resbodies, else_ }` → body(opt) → resbodies(list) → else_(opt)
- `Resbody { exceptions, var, body }` → exceptions(list) → var(opt) → body(opt)
- `Ensure { body, ensure_ }` → body(opt) → ensure_(opt)

`serialize.rs`: 3 arm 追加。

**Step 2: 失敗するテスト**

```rust
    #[test]
    fn translates_begin_rescue() {
        let ast = translate("begin\n  x\nrescue => e\n  y\nend", "t.rb");
        // ルートは Begin、その子に Rescue。
        assert!(ast.descendants(ast.root()).chain([ast.root()])
            .any(|n| matches!(ast.kind(n), NodeKind::Rescue { .. })));
        assert!(ast.descendants(ast.root())
            .any(|n| matches!(ast.kind(n), NodeKind::Resbody { .. })));
    }

    #[test]
    fn translates_begin_ensure() {
        let ast = translate("begin\n  x\nensure\n  z\nend", "t.rb");
        assert!(ast.descendants(ast.root()).chain([ast.root()])
            .any(|n| matches!(ast.kind(n), NodeKind::Ensure { .. })));
    }

    #[test]
    fn translates_rescue_with_exception_class() {
        let ast = translate("begin\nx\nrescue StandardError => e\ny\nend", "t.rb");
        let resbody = ast.descendants(ast.root())
            .find(|&n| matches!(ast.kind(n), NodeKind::Resbody { .. }))
            .unwrap();
        match ast.kind(resbody) {
            NodeKind::Resbody { exceptions, var, .. } => {
                assert_eq!(exceptions.len, 1);
                assert!(var.get().is_some());
            }
            _ => unreachable!(),
        }
    }
```

**Step 3: 失敗を確認。**

**Step 4: BeginNode 翻訳を実装**

```rust
        if let Some(b) = node.as_begin_node() {
            return self.translate_begin(&b, range);
        }
```

```rust
    /// prism BeginNode → arena ノード。
    /// 構造: `Begin([ Ensure?( Rescue?( body, resbodies, else ) ) ])`。
    /// rescue も ensure も無ければ素の `Begin(statements)`。
    fn translate_begin(&mut self, b: &prism::BeginNode<'_>, range: Range) -> NodeId {
        let body = self.translate_stmts_opt(b.statements());

        // rescue 節（subsequent でリンクした RescueNode 列）。
        let inner = if let Some(first) = b.rescue_clause() {
            let mut resbody_ids: Vec<NodeId> = Vec::new();
            let mut cur = Some(first);
            while let Some(rn) = cur {
                resbody_ids.push(self.translate_resbody(&rn));
                cur = rn.subsequent();
            }
            let resbodies = self.builder.push_list(&resbody_ids);
            let else_ = match b.else_clause() {
                Some(els) => self.translate_stmts_opt(els.statements()),
                None => OptNodeId::NONE,
            };
            OptNodeId::some(self.builder.push(
                NodeKind::Rescue { body, resbodies, else_ },
                range,
            ))
        } else {
            body
        };

        // ensure 節。
        let protected = if let Some(ens) = b.ensure_clause() {
            let ensure_ = self.translate_stmts_opt(ens.statements());
            OptNodeId::some(self.builder.push(
                NodeKind::Ensure { body: inner, ensure_ },
                range,
            ))
        } else {
            inner
        };

        // `begin..end`（kwbegin）は Begin で包む。
        let child = match protected.get() {
            Some(id) => vec![id],
            None => Vec::new(),
        };
        let list = self.builder.push_list(&child);
        self.builder.push(NodeKind::Begin(list), range)
    }

    /// prism RescueNode 1 個 → Resbody。
    fn translate_resbody(&mut self, rn: &prism::RescueNode<'_>) -> NodeId {
        let range = Self::range(&rn.location());
        let exc_ids: Vec<NodeId> =
            rn.exceptions().iter().map(|e| self.translate_node(&e)).collect();
        let exceptions = self.builder.push_list(&exc_ids);
        let var = rn
            .reference()
            .map(|r| OptNodeId::some(self.translate_node(&r)))
            .unwrap_or(OptNodeId::NONE);
        let body = self.translate_stmts_opt(rn.statements());
        self.builder.push(NodeKind::Resbody { exceptions, var, body }, range)
    }
```

> Begin が statements だけ（rescue/ensure 無し）でも `Begin([..])` で包むのは
> parser の kwbegin 準拠。素の複数文ブロック（`translate_stmts_opt` の複数→
> Begin）と二重 Begin になり得るが、`translate_stmts_opt` は単一文を畳むため
> `begin; x; end` は `Begin([x])` で一段。許容範囲。golden テストで形を固定する。

**Step 5: テスト PASS を確認。**

**Step 6: コミット** — `feat(murphy-translate): translate begin/rescue/ensure (+ Rescue/Resbody/Ensure)`

---

## Task 14: op-assign（`+=` / `||=` / `&&=`）

**新規** `OpAsgn` `OrAsgn` `AndAsgn`。lvar/ivar/cvar/gvar/const ターゲット系の
prism operator-write ノードを翻訳。target は値なしの write-shape 子ノード。
call/index ターゲット系は Unknown 許容（設計どおり）。

**Files:**
- Modify: `crates/murphy-ast/src/node.rs` / `ast.rs` / `serialize.rs`
- Modify: `crates/murphy-translate/src/translate.rs`

**Step 1: murphy-ast に 3 variant 追加**

```rust
    /// `target op= value`（`+=` `-=` 等）。target は値なし write ノード。
    OpAsgn {
        target: NodeId,
        op: Symbol,
        value: NodeId,
    },
    /// `target ||= value`。
    OrAsgn {
        target: NodeId,
        value: NodeId,
    },
    /// `target &&= value`。
    AndAsgn {
        target: NodeId,
        value: NodeId,
    },
```

`collect_children`:
- `OpAsgn { target, value, .. }` → target → value
- `OrAsgn { target, value } | AndAsgn { target, value }` → target → value

`serialize.rs`: 3 arm。

**Step 2: 失敗するテスト**

```rust
    #[test]
    fn translates_op_assign() {
        let ast = translate("x = 0; x += 1", "t.rb");
        let op = ast.descendants(ast.root())
            .find(|&n| matches!(ast.kind(n), NodeKind::OpAsgn { .. }));
        assert!(op.is_some(), "expected an OpAsgn node");
        if let Some(n) = op {
            if let NodeKind::OpAsgn { op, .. } = *ast.kind(n) {
                assert_eq!(ast.interner().resolve(op.0), "+");
            }
        }
    }

    #[test]
    fn translates_or_and_assign() {
        let or = translate("@x ||= 1", "t.rb");
        assert!(matches!(or.kind(or.root()), NodeKind::OrAsgn { .. }));
        let and = translate("@x &&= 1", "t.rb");
        assert!(matches!(and.kind(and.root()), NodeKind::AndAsgn { .. }));
    }

    #[test]
    fn call_target_op_assign_is_unknown_not_panic() {
        // `a.b += 1`（CallOperatorWriteNode）は v1 では Unknown 許容。
        let ast = translate("a.b += 1", "t.rb");
        assert!(matches!(ast.kind(ast.root()), NodeKind::Unknown));
    }
```

**Step 3: 失敗を確認。**

**Step 4: 実装**

操作対象を「値なし write ノード」として push するヘルパと、各 operator-write
ノードの arm を書く。lvar/ivar/cvar/gvar/const の 5 ファミリ × op/or/and の
組み合わせ。コードが多いので、prism ノード名から `(target_kind, name_or_path)`
を取り出すヘルパに寄せる:

```rust
    /// 名前ベースのターゲット（lvar/ivar/cvar/gvar）の値なし write ノードを
    /// push する。`kind_ctor` は name から NodeKind を作るクロージャ。
    fn push_named_target(
        &mut self,
        name: murphy_ast::Symbol,
        range: Range,
        ctor: impl FnOnce(murphy_ast::Symbol) -> NodeKind,
    ) -> NodeId {
        self.builder.push(ctor(name), range)
    }
```

`translate_node` の arm（lvar の 3 種を例示。ivar/cvar/gvar も
`as_instance_variable_operator_write_node` 等で同型に書く）:

```rust
        if let Some(w) = node.as_local_variable_operator_write_node() {
            let name = self.sym(&w.name());
            let target = self.builder.push(
                NodeKind::Lvasgn { name, value: OptNodeId::NONE },
                Self::range(&w.name_loc()),
            );
            let op = self.sym(&w.binary_operator());
            let value = self.translate_node(&w.value());
            return self.builder.push(NodeKind::OpAsgn { target, op, value }, range);
        }
        if let Some(w) = node.as_local_variable_or_write_node() {
            let name = self.sym(&w.name());
            let target = self.builder.push(
                NodeKind::Lvasgn { name, value: OptNodeId::NONE },
                Self::range(&w.name_loc()),
            );
            let value = self.translate_node(&w.value());
            return self.builder.push(NodeKind::OrAsgn { target, value }, range);
        }
        if let Some(w) = node.as_local_variable_and_write_node() {
            // 同様に AndAsgn。
        }
```

> ivar/cvar/gvar ターゲットは `Ivasgn`/`Cvasgn`/`Gvasgn` を値なしで使う。
> constant ターゲット（`ConstantOperatorWriteNode` 等）は `Casgn`（値なし、
> scope None）。constant-path ターゲット（`ConstantPathOperatorWriteNode`）は
> v1 では Unknown でよい。各 prism ノードの正確な名前・アクセサは bindings.rs
> （Appendix のパス）で確認すること。murphy-9cr.1 の collapse 表に 21 variant
> の split がある — lvar/ivar/cvar/gvar/const の 5×3=15 を翻訳、残り
> （call/index/constant-path）は Unknown フォールバックに任せる。

**Step 5: テスト PASS を確認。**

**Step 6: コミット** — `feat(murphy-translate): translate op-assign (+ OpAsgn/OrAsgn/AndAsgn)`

---

## Task 15: 文字列補間・正規表現・xstring

**新規** `Dstr` `Dsym` `Xstr` `Regexp`。InterpolatedStringNode → `Dstr`、
InterpolatedSymbolNode → `Dsym`、XStringNode / InterpolatedXStringNode → `Xstr`、
RegularExpressionNode / InterpolatedRegularExpressionNode → `Regexp`。
EmbeddedStatementsNode（`#{...}`）は内側 statements を `Begin` に畳んで部品にする。

> 設計の `Regopt` variant は省略。正規表現オプションは `Regexp { parts, opts }`
> の `opts: Symbol`（`"imx"` 等を interned）に畳む。

**Files:**
- Modify: `crates/murphy-ast/src/node.rs` / `ast.rs` / `serialize.rs`
- Modify: `crates/murphy-translate/src/translate.rs`

**Step 1: murphy-ast に 4 variant 追加**（string 系セクション、`Sym` 付近）

```rust
    /// 補間文字列 `"a#{b}"` / 隣接文字列連結。部品の並び。
    Dstr(NodeList),
    /// 補間シンボル `:"a#{b}"`。
    Dsym(NodeList),
    /// バッククォート文字列 `` `cmd` ``（補間あり/なし両方）。
    Xstr(NodeList),
    /// 正規表現 `/re/imx`（補間あり/なし両方）。`opts` はフラグ文字列
    /// （`"imx"` 等）を interned した Symbol。
    Regexp {
        parts: NodeList,
        opts: Symbol,
    },
```

`collect_children`: `Dstr(l) | Dsym(l) | Xstr(l)` を list 系 arm に合流。
`Regexp { parts, .. }` → `push_list(out, lists, parts)`。

`serialize.rs`: 4 arm。

**Step 2: 失敗するテスト**

```rust
    #[test]
    fn translates_interpolated_string() {
        let ast = translate("\"a#{b}c\"", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Dstr(parts) => assert!(parts.len >= 2),
            other => panic!("expected Dstr, got {other:?}"),
        }
    }

    #[test]
    fn translates_regexp_with_opts() {
        let ast = translate("/ab/im", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Regexp { opts, .. } => {
                let s = ast.interner().resolve(opts.0);
                assert!(s.contains('i') && s.contains('m'));
            }
            other => panic!("expected Regexp, got {other:?}"),
        }
    }

    #[test]
    fn translates_xstring() {
        assert!(matches!(translate("`ls`", "t.rb").kind(/*root*/), NodeKind::Xstr(_)));
    }
```

**Step 3: 失敗を確認。**

**Step 4: 実装**

補間部品を翻訳するヘルパ。部品は StringNode / EmbeddedStatementsNode /
EmbeddedVariableNode のいずれか:

```rust
    /// 補間部品の NodeList を作る。EmbeddedStatementsNode は内側 statements を
    /// Begin に畳む。EmbeddedVariableNode は中の変数ノードを翻訳。
    fn translate_interp_parts(&mut self, parts: prism::NodeList<'_>) -> Vec<NodeId> {
        let mut ids = Vec::new();
        for p in parts.iter() {
            if let Some(emb) = p.as_embedded_statements_node() {
                let range = Self::range(&emb.location());
                let inner: Vec<NodeId> = match emb.statements() {
                    Some(s) => s.body().iter().map(|n| self.translate_node(&n)).collect(),
                    None => Vec::new(),
                };
                let list = self.builder.push_list(&inner);
                ids.push(self.builder.push(NodeKind::Begin(list), range));
            } else if let Some(ev) = p.as_embedded_variable_node() {
                ids.push(self.translate_node(&ev.variable()));
            } else {
                // StringNode 等はそのまま。
                ids.push(self.translate_node(&p));
            }
        }
        ids
    }

    /// 正規表現フラグ文字列を組み立てて intern。
    fn regexp_opts(&mut self, ignore: bool, ext: bool, multi: bool) -> murphy_ast::Symbol {
        let mut s = String::new();
        if ignore { s.push('i'); }
        if multi { s.push('m'); }
        if ext { s.push('x'); }
        self.builder.intern_symbol(&s)
    }
```

`translate_node` arm:

```rust
        if let Some(s) = node.as_interpolated_string_node() {
            let ids = self.translate_interp_parts(s.parts());
            let list = self.builder.push_list(&ids);
            return self.builder.push(NodeKind::Dstr(list), range);
        }
        if let Some(s) = node.as_interpolated_symbol_node() {
            let ids = self.translate_interp_parts(s.parts());
            let list = self.builder.push_list(&ids);
            return self.builder.push(NodeKind::Dsym(list), range);
        }
        if let Some(x) = node.as_x_string_node() {
            // 補間なし xstring。content を Str 1 部品に。
            let text = String::from_utf8_lossy(x.unescaped());
            let sid = self.builder.intern_string(&text);
            let str_id = self.builder.push(NodeKind::Str(sid), range);
            let list = self.builder.push_list(&[str_id]);
            return self.builder.push(NodeKind::Xstr(list), range);
        }
        if let Some(x) = node.as_interpolated_x_string_node() {
            let ids = self.translate_interp_parts(x.parts());
            let list = self.builder.push_list(&ids);
            return self.builder.push(NodeKind::Xstr(list), range);
        }
        if let Some(re) = node.as_regular_expression_node() {
            let text = String::from_utf8_lossy(re.unescaped());
            let sid = self.builder.intern_string(&text);
            let str_id = self.builder.push(NodeKind::Str(sid), range);
            let parts = self.builder.push_list(&[str_id]);
            let opts = self.regexp_opts(re.is_ignore_case(), re.is_extended(), re.is_multi_line());
            return self.builder.push(NodeKind::Regexp { parts, opts }, range);
        }
        if let Some(re) = node.as_interpolated_regular_expression_node() {
            let ids = self.translate_interp_parts(re.parts());
            let parts = self.builder.push_list(&ids);
            let opts = self.regexp_opts(re.is_ignore_case(), re.is_extended(), re.is_multi_line());
            return self.builder.push(NodeKind::Regexp { parts, opts }, range);
        }
```

**Step 5: テスト PASS を確認。**

**Step 6: コミット** — `feat(murphy-translate): translate interpolated strings, regexp, xstring`

---

## Task 16: 多重代入

**新規** `Masgn` `Mlhs`。MultiWriteNode → `Masgn { lhs: Mlhs, rhs }`。
MultiTargetNode / 各 target ノード → `Mlhs` の要素。

**Files:**
- Modify: `crates/murphy-ast/src/node.rs` / `ast.rs` / `serialize.rs`
- Modify: `crates/murphy-translate/src/translate.rs`

**Step 1: murphy-ast に 2 variant 追加**

```rust
    /// 多重代入 `a, b = 1, 2`。`lhs` は `Mlhs`。
    Masgn {
        lhs: NodeId,
        rhs: NodeId,
    },
    /// 多重代入の左辺ターゲット並び。
    Mlhs(NodeList),
```

`collect_children`: `Masgn { lhs, rhs }` → lhs → rhs。`Mlhs(l)` → list 系へ合流。
`serialize.rs`: 2 arm。

**Step 2: 失敗するテスト**

```rust
    #[test]
    fn translates_multiple_assignment() {
        let ast = translate("a, b = 1, 2", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Masgn { lhs, .. } => {
                assert!(matches!(ast.kind(lhs), NodeKind::Mlhs(_)));
            }
            other => panic!("expected Masgn, got {other:?}"),
        }
    }
```

**Step 3: 失敗を確認。**

**Step 4: 実装**

```rust
        if let Some(mw) = node.as_multi_write_node() {
            let lhs = self.translate_mlhs(
                mw.lefts(), mw.rest(), mw.rights(), Self::node_range(node),
            );
            let rhs = self.translate_node(&mw.value());
            return self.builder.push(NodeKind::Masgn { lhs, rhs }, range);
        }
```

```rust
    /// 多重代入左辺（lefts + rest + rights）→ Mlhs ノード。
    fn translate_mlhs(
        &mut self,
        lefts: prism::NodeList<'_>,
        rest: Option<prism::Node<'_>>,
        rights: prism::NodeList<'_>,
        range: Range,
    ) -> NodeId {
        let mut ids: Vec<NodeId> = Vec::new();
        for n in lefts.iter() {
            ids.push(self.translate_target(&n));
        }
        if let Some(r) = rest {
            // `*rest` ターゲットは Splat で包む。
            let rid = self.translate_target(&r);
            let sr = Self::node_range(&r);
            ids.push(self.builder.push(NodeKind::Splat(OptNodeId::some(rid)), sr));
        }
        for n in rights.iter() {
            ids.push(self.translate_target(&n));
        }
        let list = self.builder.push_list(&ids);
        self.builder.push(NodeKind::Mlhs(list), range)
    }

    /// 代入ターゲット（LocalVariableTargetNode 等、または入れ子 MultiTargetNode）
    /// を翻訳。target 系は値なし write ノードへ。
    fn translate_target(&mut self, node: &prism::Node<'_>) -> NodeId {
        let range = Self::node_range(node);
        if let Some(t) = node.as_local_variable_target_node() {
            let name = self.sym(&t.name());
            return self.builder.push(NodeKind::Lvasgn { name, value: OptNodeId::NONE }, range);
        }
        if let Some(t) = node.as_instance_variable_target_node() {
            let name = self.sym(&t.name());
            return self.builder.push(NodeKind::Ivasgn { name, value: OptNodeId::NONE }, range);
        }
        if let Some(mt) = node.as_multi_target_node() {
            return self.translate_mlhs(mt.lefts(), mt.rest(), mt.rights(), range);
        }
        // class/global var target、constant target、call/index target、
        // RestParameterNode の `name` 無し等 → translate_node に委譲（多くは
        // それ自体が write/call ノードとして処理されるか Unknown）。
        self.translate_node(node)
    }
```

> `RestParameterNode` の `*` がターゲット位置の MultiTargetNode 内に出る等の
> 細部は v1 では Unknown 許容。class/global variable target は ivar 同様に
> `Cvasgn`/`Gvasgn` 値なしを使ってよい（実装時に prism のターゲットノード名を
> bindings.rs で確認）。

**Step 5: テスト PASS を確認。**

**Step 6: コミット** — `feat(murphy-translate): translate multiple assignment (+ Masgn/Mlhs)`

---

## Task 17: コメント移送

prism のコメントを `Ast` の comment list へ移送する。`Comment { range, kind }`。

**Files:** Modify `crates/murphy-translate/src/translate.rs`

**Step 1: 失敗するテスト**

```rust
    #[test]
    fn translates_comments() {
        let ast = translate("# a line comment\nx = 1\n", "t.rb");
        assert_eq!(ast.comments().len(), 1);
        assert_eq!(ast.comments()[0].kind, murphy_ast::CommentKind::Inline);
    }

    #[test]
    fn translates_block_comment() {
        let ast = translate("=begin\nblock\n=end\nx = 1\n", "t.rb");
        assert_eq!(ast.comments().len(), 1);
        assert_eq!(ast.comments()[0].kind, murphy_ast::CommentKind::Block);
    }
```

**Step 2: 失敗を確認。**

**Step 3: `translate` 本体にコメント移送を追加**

`translate` 関数内、`finish` の前にコメントループを追加:

```rust
pub fn translate(source: &str, path: impl Into<PathBuf>) -> Ast {
    let result = prism::parse(source.as_bytes());
    let mut t = Translator { builder: AstBuilder::new(source, path) };
    let root = t.translate_program(&result.node());
    for c in result.comments() {
        let loc = c.location();
        let range = Translator::range(&loc);
        // CommentType: InlineComment（`#`）/ EmbDocComment（`=begin/=end`）。
        // ワイルドカード arm: prism に 3 つ目の variant が増えていても
        // コンパイルを壊さず Block 扱いにフォールバックする。
        let kind = match c.type_() {
            prism::CommentType::InlineComment => murphy_ast::CommentKind::Inline,
            _ => murphy_ast::CommentKind::Block,
        };
        t.builder.add_comment(range, kind);
    }
    t.builder.finish(root)
}
```

> **着手前に確認**: `~/.cargo/registry/src/*/ruby-prism-1.9.0/src/lib.rs` の
> `CommentType` 定義（420〜463 行付近）を読み、variant が `InlineComment` /
> `EmbDocComment` の 2 つだけか確認する。上の `match` はワイルドカード arm で
> 3 つ目があっても安全だが、`EmbDocComment` 以外が Block 扱いで妥当かは
> variant 名を見て判断する。

**Step 4: テスト PASS を確認。**

**Step 5: コミット** — `feat(murphy-translate): carry source comments into the arena`

---

## Task 18: S 式ゴールデンテスト

arena AST を S 式テキストへダンプする手書きヘルパを書き、代表プログラムを
commit 済み golden ファイルと照合する（リポジトリ慣例: `insta` 不使用、
commit 済みスナップショット）。

**Files:**
- Create: `crates/murphy-translate/tests/golden.rs`
- Create: `crates/murphy-translate/tests/fixtures/*.rb`（代表プログラム）
- Create: `crates/murphy-translate/tests/snapshots/*.sexp`（golden）

**Step 1: テストハーネスを書く**

`tests/golden.rs` に S 式プリンタを実装する。`murphy_ast::Ast` の公開 API
（`root()` `kind()` `children()` `interner()` `raw_source()`）だけを使う。
各ノードを `(KindName child1 child2 ...)` 形式で、シンボル/文字列は中身を
添えて出力する。例: `(send nil "puts" (int 1))`。

ハーネスは `fixtures/<name>.rb` を読んで `translate` し、S 式化して
`snapshots/<name>.sexp` と比較。環境変数 `BLESS=1` のとき snapshot を上書き。

```rust
//! prism→arena 翻訳の S 式ゴールデンテスト。
//!
//! `BLESS=1 cargo test -p murphy-translate --test golden` で snapshot 再生成。

use murphy_ast::{Ast, NodeId, NodeKind};
use std::path::PathBuf;

fn sexp(ast: &Ast) -> String {
    let mut out = String::new();
    write_node(ast, ast.root(), 0, &mut out);
    out.push('\n');
    out
}

fn write_node(ast: &Ast, id: NodeId, depth: usize, out: &mut String) {
    // KindName を出し、子を再帰。リテラルは中身を添える。
    // 実装はシンプルでよい。NodeKind を網羅的に名前へマップする。
    // （match の網羅性で新 variant の出力漏れを防ぐ。）
    // ... 実装 ...
}

fn check(name: &str) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let src = std::fs::read_to_string(dir.join("fixtures").join(format!("{name}.rb"))).unwrap();
    let ast = murphy_translate::translate(&src, format!("{name}.rb"));
    let got = sexp(&ast);
    let snap_path = dir.join("snapshots").join(format!("{name}.sexp"));
    if std::env::var("BLESS").is_ok() {
        std::fs::write(&snap_path, &got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&snap_path).unwrap_or_default();
    assert_eq!(got, want, "snapshot mismatch for {name}; BLESS=1 to re-bless");
}

#[test]
fn golden_control_flow() { check("control_flow"); }

#[test]
fn golden_method_def() { check("method_def"); }

#[test]
fn golden_mixed() { check("mixed"); }
```

**Step 2: fixture を作る**

`tests/fixtures/control_flow.rb`, `method_def.rb`, `mixed.rb` に、Task 2〜17 で
翻訳した構文を広く含む代表的な Ruby プログラムを書く（if/unless/case/while、
def with all param kinds、begin/rescue/ensure、ブロック、補間文字列、op-assign、
多重代入など）。各 10〜30 行程度。

**Step 3: snapshot を生成して目視確認**

Run: `BLESS=1 cargo test -p murphy-translate --test golden`
生成された `tests/snapshots/*.sexp` を**目視レビュー**し、翻訳結果が意図どおり
（collapse/split が正しく、Unknown が想定外に出ていない）か確認する。

**Step 4: bless 無しでテストが通ることを確認**

Run: `cargo test -p murphy-translate --test golden`
Expected: 3 テスト PASS。

**Step 5: コミット**

```bash
git add crates/murphy-translate/tests
git commit -m "test(murphy-translate): add S-expression golden tests"
```

---

## Task 19: Unknown 比率テスト + 仕上げゲート

代表的な実 Ruby ファイルで `Unknown` 比率を測るテストを追加し、設計の受け入れ
基準（< 5%）を守る。fmt / clippy を通す。

**Files:**
- Create: `crates/murphy-translate/tests/coverage.rs`
- Create: `crates/murphy-translate/tests/fixtures/realistic.rb`

**Step 1: Unknown 比率テストを書く**

`tests/fixtures/realistic.rb` に、現実的な Ruby コード（100〜200 行程度。
クラス・メソッド・ブロック・条件分岐・例外処理・文字列補間などを自然に含む。
murphy リポジトリ内の `.rb` fixture を流用しても可）を用意する。

```rust
//! 代表的な実 Ruby ファイルで Unknown 比率が受け入れ基準内かを検証する。

use murphy_ast::NodeKind;

#[test]
fn unknown_ratio_under_5_percent() {
    let src = include_str!("fixtures/realistic.rb");
    let ast = murphy_translate::translate(src, "realistic.rb");
    let total = ast.len();
    let unknown = (0..total)
        .filter(|&i| matches!(ast.kind(murphy_ast::NodeId(i as u32)), NodeKind::Unknown))
        .count();
    let ratio = unknown as f64 / total.max(1) as f64;
    assert!(
        ratio < 0.05,
        "Unknown ratio {:.1}% exceeds 5% ({unknown}/{total} nodes)",
        ratio * 100.0,
    );
}

#[test]
fn translate_never_panics_on_diverse_input() {
    // パターンマッチ・flip-flop・alias 等の稀構文を含む入力でも panic しない。
    for src in [
        "case x; in [1, *rest]; end",
        "alias foo bar",
        "BEGIN { x }",
        "x = 1 if (a..b)",
        "->(x) { x }",
    ] {
        let _ = murphy_translate::translate(src, "t.rb");
    }
}
```

**Step 2: テストを実行**

Run: `cargo test -p murphy-translate`
Expected: 全テスト PASS。Unknown 比率が 5% を超えるなら、超過の原因ノードを
特定し（`realistic.rb` を調整するか、頻出ノードが漏れていれば該当 Task に
戻って variant を足す）、再実行する。

**Step 3: fmt / clippy ゲート**

Run: `cargo fmt --check`
Expected: 差分なし（あれば `cargo fmt` で修正）。

Run: `cargo clippy -p murphy-translate -p murphy-ast --all-targets -- -D warnings`
Expected: 警告なし。

**Step 4: ワークスペース全体テスト**

Run: `cargo test --workspace`
Expected: murphy-translate・murphy-ast はグリーン。既存の無関係な失敗
（あれば）は murphy-9my 等で別途追跡されているもののみ許容。

**Step 5: コミット**

```bash
git add crates/murphy-translate
git commit -m "test(murphy-translate): add Unknown-ratio gate and panic-free fuzz"
```

---

## 完了条件

- 受け入れ基準（beads issue murphy-9cr.15 の acceptance フィールド）9 項目を満たす。
- `cargo test --workspace` で murphy-translate・murphy-ast グリーン。
- `cargo fmt --check` / `cargo clippy --workspace --all-targets -- -D warnings` 通過。
- S 式 golden が翻訳結果を固定。Unknown 比率 < 5%。

---

## Appendix: prism API（ruby-prism 1.9.0）

生成 bindings: `target/*/build/ruby-prism-*/out/bindings.rs`
（`ruby-prism-sys` ではない方）。手書き部: `~/.cargo/registry/src/*/ruby-prism-1.9.0/src/lib.rs`。
実装時に不明点はこの 2 ファイルで一次確認すること。

### コア型

- `ruby_prism::parse(source: &[u8]) -> ParseResult<'_>`
- `ParseResult`: `node() -> Node`, `source() -> &[u8]`, `comments() -> Comments`
  （`Comment` のイテレータ）, `errors() -> Diagnostics`。
- `Node<'pr>`: `location() -> Location<'pr>`、`as_<snake>_node() -> Option<XxxNode<'pr>>`。
- `Location<'pr>`: `start_offset() -> usize`, `end_offset() -> usize`,
  `as_slice() -> &[u8]`。
- `ConstantId<'pr>`: `as_slice() -> &'pr [u8]`（識別子バイト列）。
- `Integer<'pr>`: `TryInto<i32>`（length==0 のときのみ Ok）、
  `to_u32_digits() -> (bool /* negative */, &[u32] /* LSB first */)`。i64 直変換なし。
- `NodeList<'pr>`: `iter() -> impl Iterator<Item = Node>`, `len() -> usize`,
  `is_empty() -> bool`, `first()/last() -> Option<Node>`。`&NodeList` は `IntoIterator`。
- `Comment<'pr>`: `location() -> Location`, `type_() -> CommentType`,
  `text() -> &[u8]`。`CommentType { InlineComment, EmbDocComment }`。

### 主要ノードアクセサ（戻り値型つき）

```
ProgramNode      statements() -> StatementsNode | Option<StatementsNode>（実装時に確認）
StatementsNode   body() -> NodeList
IntegerNode      value() -> Integer
FloatNode        value() -> f64
StringNode       unescaped() -> &[u8]
SymbolNode       parts() -> NodeList / 内容アクセサは bindings 確認（補間なし symbol）
NilNode/TrueNode/FalseNode/SelfNode  （フィールドなし）
LocalVariableReadNode      name() -> ConstantId（実装時に確認）
InstanceVariableReadNode   name() -> ConstantId
ClassVariableReadNode      name() -> ConstantId
GlobalVariableReadNode     name() -> ConstantId
ConstantReadNode           name() -> ConstantId
ConstantPathNode           parent() -> Option<Node>, name() -> Option<ConstantId>
LocalVariableWriteNode     name() -> ConstantId, value() -> Node
InstanceVariableWriteNode  name() -> ConstantId, value() -> Node
ClassVariableWriteNode     name() -> ConstantId, value() -> Node
GlobalVariableWriteNode    name() -> ConstantId, value() -> Node
ConstantWriteNode          name() -> ConstantId, value() -> Node
ConstantPathWriteNode      target() -> ConstantPathNode, value() -> Node
CallNode    is_safe_navigation() -> bool, receiver() -> Option<Node>,
            name() -> ConstantId, arguments() -> Option<ArgumentsNode>,
            block() -> Option<Node>
ArgumentsNode    arguments() -> NodeList
BlockArgumentNode    expression() -> Option<Node>
BlockNode    parameters() -> Option<Node>, body() -> Option<Node>
BlockParametersNode  parameters() -> Option<ParametersNode>
ParametersNode  requireds()/optionals()/posts()/keywords() -> NodeList,
                rest() -> Option<Node>, keyword_rest() -> Option<Node>,
                block() -> Option<BlockParameterNode>
RequiredParameterNode  name() -> ConstantId
OptionalParameterNode  name() -> ConstantId, value() -> Node
RestParameterNode      name() -> Option<ConstantId>
KeywordRestParameterNode  name() -> Option<ConstantId>
BlockParameterNode     name() -> Option<ConstantId>
RequiredKeywordParameterNode  name() -> ConstantId
OptionalKeywordParameterNode  name() -> ConstantId, value() -> Node
ArrayNode    elements() -> NodeList
HashNode     elements() -> NodeList
AssocNode    key() -> Node, value() -> Node
AssocSplatNode   value() -> Option<Node>
SplatNode    value() -> Option<Node>
IfNode       predicate() -> Node, statements() -> Option<StatementsNode>,
             subsequent() -> Option<Node>
UnlessNode   predicate() -> Node, statements() -> Option<StatementsNode>,
             else_clause() -> Option<ElseNode>
ElseNode     statements() -> Option<StatementsNode>
CaseNode     predicate() -> Option<Node>, conditions() -> NodeList,
             else_clause() -> Option<ElseNode>
WhenNode     conditions() -> NodeList, statements() -> Option<StatementsNode>
WhileNode    is_begin_modifier() -> bool, predicate() -> Node,
             statements() -> Option<StatementsNode>
UntilNode    is_begin_modifier() -> bool, predicate() -> Node,
             statements() -> Option<StatementsNode>
AndNode      left() -> Node, right() -> Node
OrNode       left() -> Node, right() -> Node
RangeNode    is_exclude_end() -> bool, left() -> Option<Node>, right() -> Option<Node>
DefNode      name() -> ConstantId, receiver() -> Option<Node>,
             parameters() -> Option<ParametersNode>, body() -> Option<Node>
ClassNode    constant_path() -> Node, superclass() -> Option<Node>,
             body() -> Option<Node>, name() -> ConstantId
ModuleNode   constant_path() -> Node, body() -> Option<Node>, name() -> ConstantId
SingletonClassNode  expression() -> Node, body() -> Option<Node>
ReturnNode/BreakNode/NextNode  arguments() -> Option<ArgumentsNode>
YieldNode    arguments() -> Option<ArgumentsNode>
SuperNode    arguments() -> Option<ArgumentsNode>, block() -> Option<BlockNode>
ForwardingSuperNode  block() -> Option<BlockNode>（フィールド実質なし）
DefinedNode  value() -> Node
BeginNode    statements() -> Option<StatementsNode>,
             rescue_clause() -> Option<RescueNode>,
             else_clause() -> Option<ElseNode>,
             ensure_clause() -> Option<EnsureNode>
RescueNode   exceptions() -> NodeList, reference() -> Option<Node>,
             statements() -> Option<StatementsNode>, subsequent() -> Option<RescueNode>
EnsureNode   statements() -> Option<StatementsNode>
LocalVariableOperatorWriteNode  name() -> ConstantId,
             binary_operator() -> ConstantId, value() -> Node, name_loc() -> Location
LocalVariableOrWriteNode   name() -> ConstantId, value() -> Node, name_loc() -> Location
LocalVariableAndWriteNode  name() -> ConstantId, value() -> Node, name_loc() -> Location
（ivar/cvar/gvar/constant の Operator/Or/And Write も同型 — bindings で名前確認）
InterpolatedStringNode  parts() -> NodeList
InterpolatedSymbolNode  parts() -> NodeList
XStringNode             unescaped() -> &[u8]
InterpolatedXStringNode parts() -> NodeList
RegularExpressionNode   unescaped() -> &[u8], is_ignore_case()/is_extended()/
                        is_multi_line() -> bool
InterpolatedRegularExpressionNode  parts() -> NodeList, フラグ accessor 同上
EmbeddedStatementsNode  statements() -> Option<StatementsNode>
EmbeddedVariableNode    variable() -> Node
MultiWriteNode   lefts() -> NodeList, rest() -> Option<Node>,
                 rights() -> NodeList, value() -> Node
MultiTargetNode  lefts() -> NodeList, rest() -> Option<Node>, rights() -> NodeList
LocalVariableTargetNode     name() -> ConstantId
InstanceVariableTargetNode  name() -> ConstantId
MissingNode      （フィールドなし — NodeKind::Error か Unknown へ）
```

> 一部のアクセサ名（特に `*ReadNode::name()`、`SymbolNode` の内容、
> ivar/cvar/gvar operator-write のノード名）は実装時に bindings.rs で一次確認
> すること。本 Appendix は実装の出発点であり、コンパイラと bindings.rs が
> 最終的な真実。
