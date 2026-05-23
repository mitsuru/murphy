# ADR 0041 — Plugin loading schema (`[[plugins]]` table-array + Name|Detailed)

- Date: 2026-05-24
- Status: Accepted
- Issue: `murphy-9cr.10.1`
- Parent: `murphy-9cr.10` (Plugin distribution / build UX、murphy-9cr 配下)
- Supersedes: pre-public `[[cop_packs]]` schema (production user 0、migration shim なし)
- Related: ADR 0038 (single-surface plugin ABI)、ADR 0042 (planned: plugin name resolution、murphy-9cr.10.2)

## 決定

ユーザー設定 `murphy.toml` における動的 plugin pack の宣言キーを **`[[plugins]]`** (TOML table-array) または **`plugins = [...]`** (bare array) とする。配列要素は **heterogeneous** で:

```toml
# RuboCop の `.rubocop.yml` を s/rubocop-/murphy-/ で書き換えただけで動く
# (name resolution は murphy-9cr.10.2 で実装)
plugins = ["murphy-rails", "murphy-rspec"]

# 詳細形 (MVP で動作する form): 明示パス指定
[[plugins]]
name = "murphy-example-pack"
path = "target/debug/libmurphy_example_pack.so"

# 混在も可能 (heterogeneous):
# plugins = [
#   "murphy-rails",
#   { name = "local-pack", path = "./libfoo.so" }
# ]
```

Rust 型は serde untagged enum:

```rust
#[derive(Deserialize)]
#[serde(untagged)]
pub enum PluginConfig {
    /// "murphy-rails" — name-only shorthand。search path 解決は ADR 0042。
    Name(String),
    /// { name = "...", path = "..." } — explicit path、MVP で動作する form。
    Detailed { name: String, path: PathBuf },
}
```

フィールドは `name` と `path` の 2 つのみ。`version` は drop。

## 動機

**RuboCop 互換 UX が load-bearing**: `.rubocop.yml` の `plugins:` directive と直接互換のキー名 / シンタックスを取る。これにより RuboCop ユーザーは

```yaml
plugins:
  - rubocop-rails
```

を

```yaml
plugins:
  - murphy-rails
```

へ機械的に置換するだけ (`s/rubocop-/murphy-/`) で Murphy への migration が完了する。YAML config 移行 (murphy-ii8 で別途検討中) と組み合わせると配布チェーンの最終形になる。

`[[cop_packs]]` という以前の (pre-public) 名前ではこの一行 migration が成立せず、ユーザーは必ず手動でキー名を書き換える必要があった。

## 補足: なぜ heterogeneous array か

TOML と YAML の双方で同じ schema-as-data の形にマップできるため。Name(String) shorthand と Detailed { name, path } を 1 つの `plugins` キーで受けるには、serde untagged enum + 文字列要素 / mapping 要素の混在配列が最も自然な表現になる:

| Form         | TOML                                      | YAML                                |
|--------------|-------------------------------------------|-------------------------------------|
| Name only    | `plugins = ["X"]`                         | `plugins:\n  - X`                   |
| Detailed     | `[[plugins]]\nname = "X"\npath = "Y"`     | `plugins:\n  - name: X\n    path: Y`|
| Heterogeneous| `plugins = ["A", { name = "B", path = "C" }]` | `plugins:\n  - A\n  - name: B\n    path: C` |

YAML 移行時 (murphy-ii8) に schema をそのまま使える。

## `version` を drop した理由

- RuboCop の `plugins:` directive には version 概念がない (gem version は `Gemfile` が管理)。互換目的に不要。
- 旧 `CopPackConfig.version` は parser に存在したが、`load_plugin_pack` には渡らず registry にも保存されない **完全な未使用フィールド**だった。
- 将来 ABI version pin / compat hint として必要になれば、`Detailed` variant の optional field として untagged enum を非破壊で拡張できる。

## MVP での `Name(String)` の扱い

murphy-9cr.10.1 (本 ADR を merge するタスク) では `PluginConfig::Name(String)` 形は parse には成功するが、`CopRegistry::discover_with_config` での load 時に明示エラーで reject される:

```text
Plugin `murphy-rails`: name resolution is not yet implemented
(murphy-9cr.10.2). Use the detailed form:
`[[plugins]] name = "murphy-rails" path = "..."`.
```

search path 解決 (`MURPHY_PLUGIN_PATH` env、project-local `.murphy/plugins/`、user-local `$XDG_DATA_HOME/murphy/plugins/` の優先順) は ADR 0042 (murphy-9cr.10.2) で別途規定する。schema を本 ADR で先に確定することで、ユーザー設定の forward compatibility を 10.1 時点で保証する。

## `#[serde(deny_unknown_fields)]` のドキュメント化された制限

serde の untagged enum は struct variant 内側の `deny_unknown_fields` を完全には honor しない。したがって:

```toml
[[plugins]]
name = "x"
path = "y"
version = "0.1"  # 未知 field、silently accepted
```

は (Phase 1 時点では) silently accept される。**ユーザーが間違って `name` だけ書いて `path` を抜かすと "data did not match any variant of untagged enum" という cryptic error** になる UX 退化も同根の問題。

これは MVP 時点の **documented limitation** とし、解消は **murphy-9cr.10.3** (PluginDetailed struct extract) で行う:

```rust
#[derive(Deserialize)] #[serde(deny_unknown_fields)]
pub struct PluginDetailed { pub name: String, pub path: PathBuf }

#[derive(Deserialize)] #[serde(untagged)]
pub enum PluginConfig { Name(String), Detailed(PluginDetailed) }
```

これで `deny_unknown_fields` も missing-field エラー文言も復活する。Phase 1 の `plugins_unknown_field_silently_accepted_for_now` test がこの limitation の sentinel を兼ねる。

## 代替案の検討

| 代替案 | 採否 | 理由 |
|---|---|---|
| `[[cop_packs]]` 維持 (pack 用語一貫性) | rejected | RuboCop 互換 UX が失われる。本 ADR の load-bearing 動機に反する |
| `plugins = [{name, path}]` のみ (heterogeneous なし) | rejected | RuboCop の `plugins:` の name-only shorthand が表現できない |
| `[[plugins]] version = "..."` 維持 | rejected | RuboCop に該当概念なし、Murphy で未使用、`Detailed` variant に後付け可能 |
| `[[plugins]] kind = "..." path = "..."` (RuboCop 非互換の独自 schema) | rejected | RuboCop ユーザーの 1 行 migration を不可能にする |

## マイグレーション

murphy-9cr.10.1 で `[[cop_packs]]` → `[[plugins]]` 置換と同時に `CopPackConfig` → `PluginConfig` enum 化を行う。pre-public schema のため shim なし、breaking change。`murphy migrate <.rubocop.yml>` は `plugins:` directive を翻訳するようになり、`rubocop-X` → `murphy-X` の auto-rename は **行わない** (ユーザー責任、明示性優先)。

## Consequences

### Positive

- RuboCop ユーザーの migration が `s/rubocop-/murphy-/` 1 行で完結する世界を予約 (murphy-9cr.10.2 で resolution が完成すれば実現)。
- TOML / YAML 双方で同じ schema を提示できる。murphy-ii8 の YAML 移行が直接乗る。
- `Detailed` form は murphy-9cr.10.1 の MVP e2e (`crates/murphy-cli/tests/plugin_pack_e2e.rs`) で動作確認済み。

### Negative

- `version` を将来再導入する際は schema breaking change (production user が増えてからは互換に注意)。
- Untagged enum + struct variant の `deny_unknown_fields` 不完備は documented limitation として一旦受容、murphy-9cr.10.3 で解消予定。
- `Name(String)` shorthand を書いたユーザーは murphy-9cr.10.2 完了まで明示エラーで弾かれる (`Detailed` form への誘導文言は registry の error message に含まれる)。

## 関連

- ADR 0038 — single-surface plugin ABI (ADR 0037 と並ぶ reboot 基盤)
- ADR 0042 (planned) — plugin name resolution / search path、murphy-9cr.10.2
- murphy-9cr.10.1 — 本 ADR を merge するタスク
- murphy-9cr.10.2 — name resolution 実装
- murphy-9cr.10.3 — `PluginDetailed` struct extract (deny_unknown_fields + missing-field error 復活)
- murphy-ii8 — YAML config 移行 (本 schema は YAML へ非破壊で写像)
