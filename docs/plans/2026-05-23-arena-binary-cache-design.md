# arena バイナリキャッシュ設計(murphy-9cr.26)

**Status**: design  
**Date**: 2026-05-23  
**Beads issue**: murphy-9cr.26(EPIC murphy-9cr: Plugin 機構 reboot)  
**設計参照**: `docs/plans/2026-05-22-plugin-reboot-design.md` §3.5

## 1. 目的

prism parse + prism→arena 変換層をスキップするためのバイナリ
キャッシュを導入する。lint がエディタ/CI/pre-commit で同じファイルを
何度も走らせる前提で hit 率は高い。

設計 §3.5 の "後段 fast-follow" を一括で実装する範囲を本ドキュメントで
固定する:

- バイナリヘッダ(magic / format-version / murphy-version /
  target-triple / content-hash)
- `Ast::from_bytes` の意味的検証(申し送り (1))
- `NodeKind` 判別子 freeze テスト(申し送り (2))
- 新クレート `murphy-cache`(キャッシュキー・I/O・無効化)
- `murphy-core::parse_with_cache` 統合
- CLI 統合(`--no-cache` / `MURPHY_NO_CACHE`)
- ADR 化

**非目標**: `murphy cache clean` 等の補助サブコマンド、eviction、
mmap ゼロコピー(設計ノートには残すが本 issue ではやらない)。

## 2. クレート構造

```text
crates/
  murphy-ast/          ← header + 検証 + freeze テストを追加
    src/serialize.rs   ← from_bytes に header/bounds/utf8 検証を追加
    tests/discriminant_freeze.rs  (新規)
  murphy-cache/        ← 新規クレート(murphy-ast に依存)
    src/lib.rs         ← Cache 構造体
    src/key.rs         ← content-hash + version key
    src/io.rs          ← atomic write (tmpfile + rename), read
  murphy-translate/    ← LAYER_VERSION 定数を公開
  murphy-core/
    src/parse.rs       ← parse_with_cache 追加(parse は維持)
  murphy-cli/          ← --no-cache / MURPHY_NO_CACHE 解釈
```

**依存方向**: `murphy-cache → murphy-ast`、`murphy-core → murphy-cache`、
`murphy-cli → murphy-core`。murphy-cache は murphy-translate に
直接依存しない(LAYER_VERSION は murphy-core 経由で組み立てる)。

## 3. ファイル形式

### 3.1 バイナリレイアウト

```text
file = [header] [body]

header (固定 88 バイト, little-endian):
  +00 magic           = b"MURPHYAS"            (8B)
  +08 format_version  : u32                    (4B)   ← 1 開始
  +0C reserved        : u32                    (4B)   ← 現状 0、将来用
  +10 murphy_version  : [u8; 16]               (16B)  ← CARGO_PKG_VERSION 先頭 16B 0 詰め
  +20 target_triple   : [u8; 24]               (24B)  ← env!("TARGET") 先頭 24B 0 詰め
  +38 content_hash    : [u8; 32]               (32B)  ← source bytes の SHA-256

body = 既存 to_bytes() の出力
       nodes / node_lists / interner blob / interner offsets /
       comments / source text / source path / root
```

サイズ固定の理由: ヘッダだけ部分読み出しが容易で、format-version
mismatch を最小コストで検出できる。

### 3.2 キャッシュキーとファイルパス

```text
$XDG_CACHE_HOME/murphy/v1/<aa>/<aabbcc... 64hex>.ast
                       ^^      ^^^^^^^^^^^^^^^^^^^^^
                       format  cache key の hex(sha256)
                       バージョン
```

- ディレクトリの `v1` は `format_version` の表現(将来 v2 と
  ディスク上で共存可能にする)。
- ファイル名の hash は `sha256(content_hash || version_key)`。
- `version_key = sha256(murphy_version || target_triple ||
  LAYER_VERSION.to_le_bytes())`。

**hash の二重化** は意図的: ファイル名衝突は content の同一性だけでなく
バージョン整合も担保する。これにより `murphy_version` を上げると
**新しいファイル名**になり、旧キャッシュは無視されて自然に共存する。

### 3.3 バージョン管理規律

| 変えるもの | 何を上げる | 既存キャッシュへの影響 |
|---|---|---|
| バイナリ形式(magic/header 構造、フィールド順) | `FORMAT_VERSION` を bump | 全旧 cache が `BadMagic`/`FormatVersionMismatch` で黙却 |
| `NodeKind` の variant 追加・並べ替え | `LAYER_VERSION` を bump | 同 format でもキー違いで cache miss 扱い |
| Murphy crate version (Cargo.toml) | 自動反映 | 同上、キー違いで miss |
| Rust ターゲット | 自動反映 | 同上 |

`LAYER_VERSION` は `murphy-translate` の `pub const LAYER_VERSION: u32;`
として公開し、prism→arena 変換が変わるたび手動 bump する。これは
ADR(murphy-9cr.26-f)に明文化する。

## 4. `from_bytes` の意味的検証(申し送り (1))

### 4.1 新規 SerError variant

```rust
pub enum SerError {
    UnexpectedEof,
    BadDiscriminant,
    InvalidUtf8,
    BadMagic,
    FormatVersionMismatch { found: u32, expected: u32 },
    MurphyVersionMismatch,
    TargetMismatch,
    ContentHashMismatch,
    NodeIdOutOfRange    { id: u32, count: u32 },
    ListIndexOutOfRange { idx: u32, count: u32 },
    SymbolOutOfRange    { id: u32, count: u32 },
    BadOptNodeId        { raw: u32 },
    BadRoot             { id: u32, count: u32 },
    BadNodeListRange    { start: u32, len: u32 },
}
```

### 4.2 検証フェーズ

`from_bytes` は deserialize 後に **1 パス** 走査して、戻す前に下記を
検証する:

1. すべての `AstNode` の `NodeKind` payload について:
   - `NodeId.0 < node_count`(`OptNodeId::None` の sentinel 値は除外)
   - `Symbol.0 / StringId.0 < sym_count`(sym_count = interner.offsets.len())
   - `NodeList { start, len }` が `start + len <= node_lists.len()`
2. `node_lists` 内の各 `NodeId.0 < node_count`
3. `root.0 < node_count`
4. ヘッダの `content_hash` を body 末尾の `source.text` に対して
   再計算して照合(防御層)

検証で得た失敗はすべて `Result::Err(SerError::*)`。Cache 層では
`Option::None` に潰す(= キャッシュミス相当、再生成へ)。

## 5. `NodeKind` 判別子 freeze テスト(申し送り (2))

`crates/murphy-ast/tests/discriminant_freeze.rs` を新規追加。
`NodeKind` の全 variant について 1 ノード AST を構築し、`to_bytes()`
の判別子バイトを hex でアサート:

```rust
#[test]
fn node_kind_discriminants_are_frozen() {
    assert_discriminant(NodeKind::Error, 0);
    assert_discriminant(NodeKind::Nil,   1);
    // ... (0..38 の全 variant)
}
```

判別子がずれた瞬間に落ちる。並べ替えに対する silent 非互換を防ぐ。

代替案として `NodeKind` 自体に `#[repr(u8)]` + 明示判別子を付ける案を
検討したが、payload を持つ variant(例 `Const { scope, name }`)が
あるため `#[repr(u8)]` は単独では機能しない。手書きマッチ + freeze
テストで守る現方針を維持する。

## 6. `murphy-cache` クレート

### 6.1 公開 API

```rust
pub struct Cache {
    dir: PathBuf,            // $XDG_CACHE_HOME/murphy/v1
    version_key: [u8; 32],   // sha256(murphy_version || target || layer_version)
}

impl Cache {
    /// XDG ベースを解決して mkdir まで行う。失敗時は None。
    /// `MURPHY_NO_CACHE=1` でも None を返す。
    pub fn open() -> Option<Cache>;

    /// content-hash に対応する Ast を読み込む。
    /// ヘッダ不一致・検証失敗・I/O エラーはすべて None。
    pub fn lookup(&self, content_hash: &[u8; 32]) -> Option<Ast>;

    /// content-hash をキーに ast を保存する。失敗は黙殺。
    /// 同時書き込みに耐えるよう tmpfile + rename で原子的に置換。
    pub fn put(&self, content_hash: &[u8; 32], ast: &Ast);
}
```

`Cache` は `&self` で I/O を行うので、`Arc<Cache>` または `&Cache`
として複数スレッドから安全に共有できる。

### 6.2 XDG 解決

優先順位:
1. `$XDG_CACHE_HOME/murphy` が定義されていればそれ
2. それ以外で `$HOME/.cache/murphy`
3. `$HOME` 未定義なら `Cache::open` は `None`

`Cache::open` 内で `dir/v1` を mkdir(`create_dir_all`)。失敗時は `None`。

### 6.3 原子的書き込み

```rust
// dir/<aa>/.tmp-<rand>  →  dir/<aa>/<full>.ast
```

`tmpfile` は同一ディレクトリに作って `rename` で置換。これにより
並列実行の競合は最後勝ちで一貫した状態に収束する。

## 7. `murphy-core::parse_with_cache`

```rust
pub fn parse_with_cache(
    source: &str,
    path: impl Into<PathBuf>,
    cache: Option<&Cache>,
) -> Result<Ast, ParseError>;
```

挙動:
1. cache が None なら従来の `parse(source, path)` 相当
2. cache が Some なら content_hash = sha256(source.as_bytes())
3. `cache.lookup(&hash)` で hit → 返す
4. miss → `translate(source, path)` 実行 → `cache.put(&hash, &ast)` → 返す

**重要**: `parse_with_cache` はキャッシュ I/O 失敗を **必ず** miss として
扱い、エラーをユーザーに上げない。lint の正当性が cache に依存しない
ことを型レベルで明示する。

既存の `parse` は維持(stdin など `path` が無いケースで使う)。

## 8. CLI 統合

`murphy-cli` の lint コマンド:

```rust
// 起動時に Cache::open を一度実行
let cache = if cli.no_cache || std::env::var("MURPHY_NO_CACHE").is_ok() {
    None
} else {
    Cache::open()
};

// 各ファイルは Option<&Cache> を共有
files.par_iter().for_each(|f| {
    let ast = parse_with_cache(&source, f, cache.as_ref());
    // ...
});
```

CLI フラグ:

- `--no-cache`: 単発無効化
- `MURPHY_NO_CACHE` 環境変数: セッション無効化(値は何でも可、定義されていれば無効化)

stdin 入力(`--stdin`)はキャッシュ対象外(`path` が無いため、明示的に
`None` を渡す)。

## 9. テスト戦略(TDD)

| レイヤ | テスト | クレート |
|---|---|---|
| Header | magic / format / version / triple / content-hash の各 mismatch が対応する SerError | murphy-ast |
| Validation | NodeId / Symbol / NodeList 範囲外 fixture を手書きし Err 化 | murphy-ast |
| Discriminant freeze | 全 variant の判別子バイトを hex でアサート | murphy-ast |
| Round-trip | 既存テストを header 付きフォーマットに更新 | murphy-ast |
| Cache I/O | `open` → `put` → `lookup` で同一 AST が戻る、別 content-hash は miss | murphy-cache |
| Cache 黙殺 | 壊れた cache ファイル / 読めないディレクトリ → miss を返し panic しない | murphy-cache |
| Version key 不一致 | murphy_version / triple / layer_version を変えると miss | murphy-cache |
| 統合 | `parse_with_cache` 二度目呼び出しが parse をスキップ(spy 経由) | murphy-core |
| CLI | `--no-cache` / `MURPHY_NO_CACHE=1` で cache が書かれない | murphy-cli |

テストは原則 tempdir をキャッシュディレクトリに使う。
`Cache::open` を環境変数経由で差し替えるための test helper
(`Cache::open_in(dir)`)を `#[cfg(test)]` で公開する。

## 10. サブタスク分解

順序が重要(各タスクが前タスクに依存):

| サブタスク | 内容 |
|---|---|
| **9cr.26-a** | murphy-ast: SerError variant 拡張・header 付き to_bytes/from_bytes・bounds 検証・freeze テスト |
| **9cr.26-b** | murphy-translate: `LAYER_VERSION` 定数公開 |
| **9cr.26-c** | murphy-cache: 新規クレート、`Cache::{open,lookup,put}` + version_key + atomic write |
| **9cr.26-d** | murphy-core: `parse_with_cache` 追加 |
| **9cr.26-e** | murphy-cli: `--no-cache` / `MURPHY_NO_CACHE` 解釈、lint コマンド統合 |
| **9cr.26-f** | ADR(`docs/decisions/00NN-arena-binary-cache.md`): フォーマット不変条件・LAYER_VERSION bump 規律 |

サブタスクは独立 PR にせず、本 issue 1 つで 1 PR に束ねる(キャッシュ
機能としての一体性を保ち、レビュー時のコンテキストを失わせない)。

## 11. リスクと回避

| リスク | 回避策 |
|---|---|
| キャッシュ汚染で lint が誤る | 多層防御: format-version, murphy_version, target_triple, content-hash 再計算, bounds 検証。**いずれかが失敗したら必ず miss 扱い**で再生成 |
| `NodeKind` 並べ替えによる silent 非互換 | freeze テスト(§5)+ LAYER_VERSION 規律(§3.3) |
| 並列実行での書き込み競合 | tmpfile + rename の原子置換(§6.3) |
| キャッシュ肥大化 | 本 issue では対処しない。将来 `murphy cache clean` サブコマンドを別 issue 化 |
| Windows での `XDG_CACHE_HOME` 未定義 | v1 は POSIX 想定。Windows 対応は要件外、`HOME` 未定義時 `None` で透過無効化される |
