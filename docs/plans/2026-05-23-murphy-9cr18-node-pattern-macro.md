# B backend: `node_pattern!` proc macro — 設計

murphy-9cr.18 の成果物。設計 §4(`docs/plans/2026-05-22-plugin-reboot-design.md`)を
基に詳細化。murphy-9cr.17(`murphy-pattern` クレート)の `PatternAst` を消費する。

## スコープ

`murphy-plugin-macros` に function-like proc macro `node_pattern!` を追加。
パターン文字列をコンパイル時に `murphy_pattern::parse` でパースし、`PatternAst`
を Rust `TokenStream`(`match` / `if-let` / `let-else` / `loop`)へ lowering する。

- B バックエンド = コンパイル時 lowering。Rust 標準 cop + `.so` プラグイン向け。
- C バックエンド(ランタイム interpreter)は murphy-9cr.19。本タスク非スコープ。
- B/C のセマンティクス等価性を検証する共有テストスイートは .19 着手時に積む
  (C 未完のため .18 では B 単独テストに留める)。

## 主要決定(ブレスト 2026-05-23 で合意)

1. **API 表面 = 名前付き matcher 生成**。`node_pattern!(name, "pattern")` が
   module レベルの `fn name` を生成する(RuboCop `def_node_matcher` 流)。
2. **NodeKind 構造スキーマ = 厳選サブセット ~25 個**(複合ノードのみ)。未対応
   kind のノードマッチは `compile_error!`。
3. **`$...`(seq capture)= `&[NodeId]`**。`murphy-plugin-api` の `Cx` に
   `list(NodeList) -> &[NodeId]` を additive 追加して zero-alloc を達成。
4. **`#predicate` = 自由関数・識別子制限**。`#even` → 呼出側スコープの自由関数
   `even(node, cx) -> bool`。`?` / `!` を含む述語名は `compile_error!`。

## マクロの形と展開

```rust
node_pattern!(is_puts, "(send $_ :puts $...)");
```

が module 位置で次の `fn` を生成する:

```rust
fn is_puts<'a>(
    node: ::murphy_ast::NodeId,
    cx: &::murphy_plugin_api::Cx<'a>,
) -> Option<(::murphy_ast::NodeId, &'a [::murphy_ast::NodeId])> {
    /* match / if-let / let-else 列 */
}
```

- マクロ名は issue の deliverable 名 `node_pattern!` を採用。書き味は
  `def_node_matcher` スタイル(item 位置で `fn` を定義)。
- 生成コードは import 非依存にするため `::murphy_ast::` / `::murphy_plugin_api::`
  の完全修飾で出力する。プラグインは両 crate に依存済み。
- 生成 `fn` は private(`node_pattern!(pub name, ...)` 対応は follow-up)。

### 戻り型ルール

| capture 数 | 戻り型 |
|---|---|
| 0 個 | `bool` |
| 1 個以上 | `Option<(C0, …, Cn)>`(slot 順タプル) |

各 `Ci` の型は `PatternAst::capture_kinds()` の `CaptureKind` で決まる:

- `CaptureKind::Node`(`$_` / `$(...)` / `$ident` / `$:sym`)→ `NodeId`
- `CaptureKind::Seq`(`$...`)→ `&'a [NodeId]`

### 名前付き capture

`$ident` は構文上受け付けるが、v1 では戻り型を変えず positional タプルのまま。
名前は可読性のために予約。「全 capture が名前付きのときに struct を返す」モードは
follow-up。

## NodeKind 構造スキーマ

`murphy-plugin-macros` 内に、サポート対象 NodeKind ごとの「パターン子スロット」
テーブルをハードコードする。各スロットは型を持つ。

| スロット型 | arena フィールド型 | パターン子の照合 |
|---|---|---|
| `Node` | `NodeId` | 子パターンを再帰照合(常に存在) |
| `OptNode` | `OptNodeId` | 存在時は再帰照合。`nil?` は不在にもマッチ |
| `Sym` | `Symbol` | `:name` リテラル(文字列比較)か `_` のみ可。`$` capture は `compile_error` |
| `NodeList` | `NodeList` | 残りのパターン子(`...` / `$...` 対応)。`cx.list()` で `&[NodeId]` 解決 |

### 対象 NodeKind(~25、複合ノードのみ)

`Send Csend Block If While Until Case When Def Class Module Const Array Hash
Pair And Or Return Begin Lvasgn Ivasgn Cvasgn Gvasgn Casgn`。

各行のスロット列は parser-gem の子順に合わせて実装計画で確定する。`def`/`defs`
collapse(`receiver: OptNodeId`)等の個別事情は行コメントで明記する。

### atom 系

`Nil True_ False_ SelfExpr Int Float Str Sym Lvar Ivar Cvar Gvar` はスキーマ表
エントリを持たない。**リテラルパターン**(`5` `:foo` `"s"` `true` …)と**裸 kind
名**(`int` `sym` …)でのみ照合する。atom の `(int 5)` ノードマッチ形式は v1 非対応
→ `compile_error!`(リテラル `5` か裸 `int` を促す)。

### 未対応 kind

スキーマ表に無い kind 名(`rescue` 等)をノードマッチ `(...)` 形式で書いたら
`compile_error!`(「v1 未対応、follow-up issue 参照」)。裸 kind 名としては
`tag_from_pattern_name` が解決する全名を許可する。

### スキーマの所在

v1 では `murphy-plugin-macros` 内にハードコード。C バックエンド(.19)と意味論を
共有すべき表だが、共有テーブル化(`murphy-pattern` 等への移設)は .19 着手時に
判断する。

## lowering アルゴリズム

proc macro 内で `PatternAst` を再帰下降で Rust トークンへ落とす。

### capture 変数

slot ごとに `fn` 冒頭で `let __cap{slot}: {ty};`(未初期化)を宣言する。`ty` は
`CaptureKind` から決定。マッチ成功パスでは全 slot が代入されるので Rust の
definite-assignment 解析が通る(失敗パスは `return` で抜ける)。

### ガード列方式

各サブパターンを「失敗したら `return None`(capture 0 個なら `return false`)」
するガード文へ落とす。設計 §4 の "match / if-let / loop" の通り:

```rust
fn is_puts<'a>(node: NodeId, cx: &Cx<'a>) -> Option<(NodeId, &'a [NodeId])> {
    let __cap0: NodeId;
    let __cap1: &[NodeId];
    let NodeKind::Send { receiver, method, args } = *cx.kind(node)
        else { return None; };
    let Some(__r) = receiver.get() else { return None; };  // $_ は OptNode
    __cap0 = __r;
    if cx.symbol_str(method) != "puts" { return None; }     // :puts は Sym
    __cap1 = cx.list(args);                                  // $... は NodeList
    Some((__cap0, __cap1))
}
```

### 各 PatKind の落とし方

| PatKind | lowering |
|---|---|
| `Wildcard` `_` | ガード無し(常に成功) |
| `Lit` | atom 値比較(`Int`→`i64`、`Sym`/`Str`→`symbol_str`/`string_str` 比較、`Float`→`f64`、`Nil`/`True`/`False`→`matches!`) |
| `Kind` 裸種別 | `NodeKindTag` 一致チェック |
| `Node` | `let NodeKind::X { … } = *cx.kind(n) else {…}` + スキーマで子スロットへ分配 |
| `Union` `{a b}` | `if`/`match` の OR |
| `Not(x)` | x のマッチを反転 |
| `NilTest` `nil?` | `OptNode` 位置は不在許容、`Node` 位置は `Nil` ノード判定 |
| `Predicate` `#p` | `if !p(n, cx) { return None; }` |
| `Parent` `^x` | `cx.parent(n)` を取り x を照合 |
| `Descend` `` `x `` | `for d in cx.descendants(n) { … }` ループ。最初の一致を採用 |
| `Rest` / `$...` | `NodeList` スロットの残り。`...` は中間位置可(前後固定長＋中間 rest) |

### capture を許さない位置

`Union` `{}` の各枝・`Not` `!` の内側・`Descend` `` ` `` の内側の `$` capture は
v1 で `compile_error!` とする。理由:

- `Union` 内: 各枝が同じ capture を全代入する保証が無い(definite-assignment が
  通らない)。
- `Not` 内: マッチ反転のため capture は意味を持たない。
- `Descend` 内: 探索ループ内の capture はループ脱出後の有効性が複雑。

いずれも follow-up で再検討する。

## `Cx::list` 追加(`murphy-plugin-api`)

`Cx` に借用スライス accessor を additive 追加する。

```rust
impl<'a> Cx<'a> {
    /// Resolve a `NodeList` to its backing slice of child ids (zero-copy).
    pub fn list(&self, l: NodeList) -> &'a [NodeId] {
        &self.lists()[l.start as usize..(l.start + l.len) as usize]
    }
}
```

`CxRaw` には既に `lists`/`lists_len` があり private な `lists()` を使うだけなので
**ABI 変更は無い**(safe メソッド追加のみ)。これで `$...` と固定長 args 照合の
両方が zero-alloc になる。`murphy-ast` 側 `Ast` に対称な `list` を足すかは実装計画
で判断(生成コードは `Cx` 経由なので必須ではない)。

## エラー処理

### パターンのパースエラー

マクロは `murphy_pattern::parse` をコンパイル時に呼ぶ。`ParseError` は `PatSpan`
(パターン文字列内 byte offset)を持つ。文字列リテラル内の部分 span を安定 API で
ピンポイント指定するのは難しいため、v1 は**リテラル全体の span で `compile_error!`**
を出し、メッセージに byte 範囲を含める:

```
error: pattern parse error: unknown node type `sned` (at 1..5)
```

サブ span の精密化は follow-up。

### マクロ入力エラー

第1引数が ident でない / 第2引数が文字列リテラルでない → `syn::Error` 経由で
`compile_error!`。

### スキーマ由来のエラー

v1 未対応 kind のノードマッチ、atom のノードマッチ形式、`Sym` スロットの `$`
capture、`{}`/`!`/`` ` `` 内 capture、`?`/`!` 付き述語名 → いずれも
`compile_error!`。可能な限り該当パターン断片を引用する。

## クレート構成

- `crates/murphy-plugin-macros/src/node_pattern.rs` — 新規。`node_pattern!` 実装
  (入力パース・スキーマ表・lowering)。
- `crates/murphy-plugin-macros/src/lib.rs` — `#[proc_macro] pub fn node_pattern`
  追加。
- `crates/murphy-plugin-macros/Cargo.toml` — `[dependencies]` に `murphy-pattern`
  と `murphy-ast` を追加(現 dev-dependency の `murphy-ast` を通常依存へ昇格)。
- `crates/murphy-plugin-api/src/cx.rs` — `Cx::list` 追加。

## テスト方針(TDD 必須)

- **挙動テスト**(`tests/node_pattern_behavior.rs`、新規): `murphy-ast` の
  `AstBuilder` で小さな `Ast` を組み、`Cx` を構築(`cx.rs` のテストヘルパ同様に
  `CxRaw` を手で組む)、生成 matcher を呼んで結果・capture を `assert_eq`。v1 文法
  の各機能を最低 1 ケース: 裸 kind / リテラル / `(send …)` ネスト / `$_` /
  `$...`(中間 rest 含む)/ `{}` union / `!` / `nil?` / `#predicate` / `^` /
  `` ` `` / capture 0 個 → `bool`。
- **trybuild UI テスト**(`tests/ui/`、既存 `tests/trybuild.rs` ハーネスに追加):
  `fail_*` = v1 未対応 kind・atom のノードマッチ・`Sym` スロットの `$`・
  `{}`/`!` 内 capture・パースエラー・述語名に `?`。`pass_*` = 代表パターン数件。
- **`Cx::list` 単体テスト**(`murphy-plugin-api`)。
- **品質ゲート**: `cargo test --workspace` /
  `cargo clippy --workspace --all-targets -- -D warnings` / `cargo fmt --check`。

## 非スコープ / follow-up

- B/C 共有セマンティクステスト(設計 §4 §7)→ murphy-9cr.19 で実施。
- スキーマ表の共有テーブル化 → .19 着手時に判断。
- 名前付き capture の struct 戻り値モード。
- `Union`/`Not`/`Descend` 内の `$` capture。
- `Sym` スロットおよびリテラル値の capture(非ノード capture)。
- atom の `(int 5)` ノードマッチ形式。
- v1 未対応 NodeKind(`rescue` 等 ~25 個以外)のスキーマ追加。
- パースエラーの文字列リテラル内サブ span 精密化。
- `node_pattern!(pub name, ...)` での可視性指定。
- murphy-9cr.19 のタイトル/説明調整(IR は murphy-pattern 提供、の乖離修正)。
