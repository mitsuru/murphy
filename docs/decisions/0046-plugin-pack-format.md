# ADR 0046 — Plugin pack format (`murphy-plugin.toml` manifest + directory convention)

- Date: 2026-09-25
- Status: Accepted
- Issue: `murphy-s3s`
- Parent: `murphy-uk7` (Plugin ecosystem post-MVP)
- Depends on: ADR 0041 (`[[plugins]]` schema), ADR 0042 (name resolution),
  ADR 0038 (single-surface plugin ABI)
- Related: ADR 0004 (trust model)

## Context

murphy-9cr.10 (MVP) で動的 plugin pack の load 経路 (10.1) と search path
解決 (10.2) が完成したが、pack は「`.so` 単体」のままだった。pack 自体の
メタデータ (名前、version、提供 cops 一覧、要求 ABI version、author /
license) は cdylib symbol 経由でしか取れないため:

- `murphy cops list` が pack 全体の概要を出せない (catalogue UX が貧弱 —
  概要のために `dlopen` が必要)
- pack 配布形式が `.so` 単体のままなので、複数アーキ対応 / バージョン共存 /
  dependency 宣言ができない
- `cargo install` のような統一 install UX が組めない

本 ADR で pack 配布形式 (manifest + ディレクトリ規約) を確定する。

## 決定

### 1. Manifest: `murphy-plugin.toml`

pack root 直下の `murphy-plugin.toml` (TOML) が pack のメタデータを宣言する:

```toml
[plugin]
name = "murphy-rails"
version = "0.3.1"
murphy-api-version = 4   # 要求ホスト ABI (= MURPHY_PLUGIN_ABI_VERSION)
authors = ["..."]
license = "MIT"
description = "Rails cops for Murphy"  # optional
homepage = "https://..."               # optional

[cops]
# 静的カタログ。省略時は load 時に cdylib から auto-derive (従来動作)。
provided = ["Rails/HttpStatus", "Rails/SaveBang"]
```

Schema (parser: `murphy-core::plugin_manifest`):

| key | 必須 | 備考 |
|---|---|---|
| `plugin.name` | yes | `validate_plugin_name` と同一 alphabet (ASCII letters/digits/`_-.`、1..=64 文字、`..` 禁止) |
| `plugin.version` | yes | 非空必須。semver 推奨だが、比較・共存は install UX 範囲のため host は検証しない |
| `plugin.murphy-api-version` | yes | integer 標準 (`= 4`)、string (`= "4"`) も許容。`murphy_api_version` (underscore) は alias。host ABI 不一致は `dlopen` 前に reject |
| `plugin.authors` | no | default `[]` |
| `plugin.license` / `description` / `homepage` | no | default `None` |
| `cops.provided` | no | default `[]` (= cdylib から derive) |

未知 field は reject (`deny_unknown_fields`) — typo の沈黙を防ぐ。
将来の `[dependencies]` 等の拡張時は parser 更新とともに本 ADR を supersede する。

### 2. ディレクトリ規約

```text
murphy-rails/                 ← pack root
├── murphy-plugin.toml        ← manifest (§1)
├── lib/
│   ├── linux-x86_64/libmurphy_rails.so
│   └── darwin-arm64/libmurphy_rails.dylib
└── README.md
```

- `lib/<arch>/` 配下の `*.so` / `*.dylib` が loadable binary。
- `<arch>` 名は distribution の慣習 (`linux-x86_64` 等)。本 ADR では
  **命名の予約に留め、自動選択ロジックは規定しない** (非スコープ §5):
  現行 host は `lib/` 配下を再帰走査し、current platform の拡張子に合う
  最初のもの (sorted 先頭)、なければ sorted 先頭を 1 本選ぶ。
- manifest の `plugin.name` と pack root の dirname 一致は **要求しない**
  (install 先の dirname は運用者の選択)。正本は manifest 内 `name`。

### 3. 後方互換性

既存の直接 `.so` 参照はすべて動き続ける:

| 経路 | 旧 (存続) | 新 (追加) |
|---|---|---|
| `Detailed { path }` | `.so` ファイルへの直接 path | pack ディレクトリへの path (manifest + `lib/` から cdylib 解決)。`lib/` 空・manifest 破損時は `ConfigError::Io` で明示失敗 |
| search path (`Name` 解決) | `<dir>/lib<sanitized>.{so,dylib}` | `<dir>/<name>/` pack dir を **先に** probe、成功すれば採用; 破損・不在時は legacy file に fallback (半端な install が working copy を shadow しない)。次に `<dir>` full scan ではなく同一 dir の legacy のみ確認し、だめなら次の dir へ |

すなわち `Detailed { path }` は **`.so` 直接 OR pack ディレクトリの両方** を
受け付け、search path は **pack dir 優先・legacy fallback**。MVP 時代に
`.murphy/plugins/` へ直置きした `.so` はそのまま load される。

`plugin_sync` (`murphy plugins sync`) は staged `.so` の鮮度管理のまま
(pack-dir staging の sync 対応は install UX と併せて後続で検討)。

### 4. 将来の install UX への余地

- manifest が `name` + `version` を持つことで、`murphy plugin install
  murphy-foo` 相当が「pack root を search path に配置する」操作として
  定義可能になった (実装本体は後続 follow-up、本 ADR は規約のみ)。
- `cargo install murphy-foo` で入る binary 隣の `lib/` 配置、`MURPHY_PLUGIN_PATH`
  への追加のいずれとも直交する。search path 優先順 (ADR 0042) は不変。
- catalogue (`murphy cops list` の pack 概要) は `[cops].provided` を
  `dlopen` なしに読む形で後続改善できる (本タスクの完了条件外)。

## 検討した代替案

| 代替案 | 採否 | 理由 |
|---|---|---|
| manifest を `Cargo.toml` の `[package.metadata.murphy]` に埋め込む | rejected | Cargo 外で built した pack (C 等) を表現できない。pack 形式は build system 非依存であるべき |
| manifest を JSON (`murphy-plugin.json`) | rejected | Pack 作者は Rust/Cargo 文化圏で TOML が自然。`.murphy.yml` (YAML) とも役割が違い、TOML parser (`toml` crate) は既存 dep |
| `murphy-api-version` を string のみ / integer のみ | rejected | 両方受容が最も手書き耐性が高い。parser 側の分岐は小さい |
| search path で pack dir のみ (legacy `.so` 直置き廃止) | rejected | MVP 期の staged `.so` を壊す。ecosystem 移行期に breaking は不要 |
| pack root dirname と `plugin.name` の一致を強制 | rejected | install 先の rename 運用を縛るだけで安全性の利得がない (traversal 対策は name alphabet 検証で担保) |
| 本 ADR で multi-arch 自動選択まで規定 | deferred | host arch 判定 + `<arch>` パース規約の設計が必要。当面は platform-ext + sorted 先頭の narrow rule (§2) で固定 |

## 実装

- `crates/murphy-core/src/plugin_manifest.rs` 新規:
  `PluginManifest::from_str / from_file / from_pack_dir`,
  `check_api_compat`, `is_pack_dir`, `resolve_pack_cdylib`
  (`MURPHY_PLUGIN_ABI_VERSION` の bump なし — 値は現行 `4` のまま)
- `crates/murphy-core/src/plugin_resolver.rs`:
  `find_in_dir` (pack-dir probe → legacy fallback)、
  `plan_plugin_loads` の `normalize_pack_path` (Detailed の pack-dir 解決)
- `crates/murphy-example-pack/murphy-plugin.toml` 新規 (リファレンス実装;
  `provided` は cdylib 登録 cops と一致 — test で担保)
- `crates/murphy-cli/tests/plugin_pack_e2e.rs`: Detailed pack-dir 形 +
  project-local pack-dir の Name 解決 e2e を追加

## Consequences

### Positive

- pack メタデータが `dlopen` なしに読める (`cops list` catalogue 等の土台)。
- 配布単位が「ディレクトリ」になり、multi-arch / version 共存 / 将来の
  dependency 宣言の置き場ができた。
- 既存ユーザーの `path = "./libfoo.so"` と staged `.so` は無変更で動作。

### Negative

- manifest と cdylib の二重管理 (`provided` と登録 cops の乖離は test でしか
  検出できない — example-pack の test が sentinel)。
- multi-arch 自動選択は未規定 (`lib/` に複数 binary がある場合の選択は
  narrow rule のまま)。matrix release 運用は別途 ADR。

## 関連

- ADR 0038 — single-surface plugin ABI (`murphy-api-version` の参照先)
- ADR 0041 — `[[plugins]]` schema (`Detailed { path }` の定義)
- ADR 0042 — name resolution / search path (本 ADR の probe が載る土台)
- ADR 0004 — trust model (署名・検証は将来の supply-chain hardening 範囲)
- `murphy-uk7` — 後続: template (`murphy-1f7`)、install UX、marketplace
