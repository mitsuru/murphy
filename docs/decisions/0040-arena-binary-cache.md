# ADR 0040 — Arena binary cache (format v1)

- Date: 2026-05-23
- Status: Accepted
- Issue: `murphy-9cr.26`
- Parent: `murphy-9cr` (Plugin 機構 reboot — arena AST + 単一表面 ABI)
- Gated by: ADR 0037 (arena parser-shaped typed AST), ADR 0039 (arena translation cost gate)
- Design: `docs/plans/2026-05-22-plugin-reboot-design.md` §3.5,
  `docs/plans/2026-05-23-arena-binary-cache-design.md`

## 決定

[`murphy_ast::Ast`] のオンディスク二値表現に **96 バイト固定ヘッダ** を被せ、
`$XDG_CACHE_HOME/murphy/v1/<aa>/<aabbcc... 64hex>.ast`
にシャード保存する。lint は同じファイルを何度も走るので prism parse +
prism→arena 変換層をスキップして hit する。バイナリ形式は
[`murphy_ast::FORMAT_VERSION`] = 1。

## ヘッダ不変条件

```text
+00 magic           = b"MURPHYAS"            (8B)
+08 format_version  : u32                    (4B)
+0C reserved        : u32                    (4B)  ← 必ず 0、現状読み捨て
+10 murphy_version  : [u8; 16]               (16B)  ← CARGO_PKG_VERSION
+20 target_triple   : [u8; 32]               (32B)  ← env!("TARGET")
+40 content_hash    : [u8; 32]               (32B)  ← sha256(source)
                                             計 96B
```

- ヘッダはサイズ固定。本体先頭オフセットは `HEADER_LEN` 定数で公開。
- magic / format_version / murphy_version / target_triple / content_hash は
  `Ast::from_bytes` で **検証必須**。一致しなければ対応する
  `SerError::*` を返す。
- ヘッダの content_hash は body の `source.text` に対して **必ず再計算
  して照合** する。誤ったキーで開かれた cache を守る最後の砦。

## バージョン管理規律

| 変えるもの | 何を上げる | 既存キャッシュへの影響 |
|---|---|---|
| バイナリ形式そのもの(magic / header 構造 / フィールド順) | [`murphy_ast::FORMAT_VERSION`] | 全旧 cache が `BadMagic` / `FormatVersionMismatch` で黙却 |
| `NodeKind` の variant 追加・削除・並べ替え | [`murphy_translate::LAYER_VERSION`] | 同 format でもキー違いで cache miss 扱い |
| Murphy crate version (`Cargo.toml`) | 自動反映 | 同上、キー違いで miss |
| Rust ターゲット(クロスコンパイル時) | 自動反映 | 同上 |

**`LAYER_VERSION` の bump は手動**。`crates/murphy-translate/src/lib.rs` の
`pub const LAYER_VERSION: u32` を、prism→arena マッピングが変わるたびに
PR の一部として上げる。これを忘れると stale cache が静かにヒットして
誤った AST を返す可能性がある — `discriminant_freeze` テストが番号ずれを
止め、`tests/discriminant_freeze.rs` の存在が変更時の TODO チェック
リマインダになる。

## バリデーション層

`Ast::from_bytes` は deserialize 後に **1 パスの bounds 検証** を走らせ、
すべての `NodeId` / `Symbol` / `NodeList` / `OptNodeId` (非 sentinel) が
バックエンド配列の範囲内であることを保証する。これにより malformed buffer
は traversal 時 panic に陥らず、`Result::Err` で返る (申し送り 1)。

cache 層 (`murphy-cache`) はこの `Err` を含むすべての失敗を `Option::None`
に潰す: ヘッダ不一致・bounds 違反・I/O エラーいずれもキャッシュミス
扱い、リンタは再生成して走り続ける。

## キャッシュヒット時の path 上書き

`content_hash` は `source.text` のみで計算するので、同一内容で path だけ
違う 2 ファイルは同じキャッシュキーに収束する(意図的: 重複ファイルの parse
を 1 回にする)。一方で `Ast::path()` は cop からのソースロケーション
レポートで使われるため、ヒット時は **呼び出し側の path で上書き** する。
具体的には `parse_with_cache` がヒット後に `ast.set_source_path(path)` を
呼び、stale な path がリーキングしない。

これにより:

- `lint a.rb` → cache 書き込み (path = "a.rb")
- 続けて `lint b.rb` (内容同じ) → ヒット、戻る `Ast::path()` は "b.rb"

## ヒット時に prism parse をスキップする(設計 §3.5 の本質)

ヒットが成立する条件は `sha256(source) == 書き込み時の sha256(source)` で
あり、書き込み時に成功 parse が確認されているため、ヒット時の source は
構文的に valid であることが構造的に保証される。`parse_with_cache` は
ヒットパスで prism parse を呼ばない (設計 §3.5「prism parse と変換層の
両方をスキップ」)。ミスパスでは `Murphy/Syntax` 契約 (ADR 0006) を維持する
ため、prism::parse を 1 回走らせて先頭エラーを harvest した上で
translate + put する。

## CLI コントラクト

- デフォルト: `$XDG_CACHE_HOME/murphy/v1` (未定義なら `$HOME/.cache/murphy/v1`)
  に書き込み、起動時に既存エントリを読む。
- `--no-cache` フラグ: CLI セッション無効化。
- `MURPHY_NO_CACHE` 環境変数: プロセス境界での無効化。値は任意 (定義
  されていれば無効化)。
- `Cache::open` が失敗 (`HOME` 未定義など) しても `None` を返して透過に
  無効化される — リンタは cache に依存しない。

## マシンローカル前提

`i64` や padding が target 依存なので、cache はマシン間で共有しない。
ヘッダの `target_triple` が一致しなければ silent regenerate。
ネットワークファイルシステム経由の共有や CI ステージ越え再利用は
**v1 範囲外**。将来必要になれば format_version bump で別形式に進む。

## 性能ゲート

ADR 0039 の +29.7% 翻訳オーバーヘッドは cache hit 経路では消える(parse +
translate そのものをスキップするため)。実測ゲートは `murphy-9cr.26` 完了後の
fast-follow 計測タスクで取り直す予定。

## 範囲外 (将来 issue)

- `murphy cache clean` サブコマンド (TTL / サイズキャップ)。
- mmap ゼロコピーロード (8B アラインを満たせばゲートを開ける、設計 §3.5)。
- Windows サポート (`XDG_CACHE_HOME` 未定義時 `None` 透過無効化で v1 は
  動くが、Windows 標準のキャッシュ位置にローカライズはしない)。
