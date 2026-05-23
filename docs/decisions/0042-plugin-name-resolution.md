# ADR 0042 — Plugin name resolution / search path

- Date: 2026-05-24
- Status: Accepted
- Issue: `murphy-9cr.10.2`
- Parent: `murphy-9cr.10` (Plugin distribution / build UX)
- Depends on: ADR 0041 (`[[plugins]]` schema — defines the
  `Name(String)` enum variant whose load path this ADR specifies)
- Related: ADR 0038 (single-surface plugin ABI), `murphy-9cr.10.3` (planned
  `PluginDetailed` struct extract)

## 決定

`murphy.toml` の `[[plugins]]` における **`Name(String)` shorthand**
(`plugins = ["murphy-rails"]`) の name → cdylib path 解決手順を以下に確定する:

### 1. 解決優先順位 (高 → 低)

1. **同一 array 内の `Detailed { name, path }`**: 配列の順序に関係なく、
   同名の `Detailed` が存在すればその `path` (project root 相対) を採用。
2. **`MURPHY_PLUGIN_PATH` env**: `std::env::split_paths` 準拠の区切り文字
   (Unix `:`、Windows `;`) で列挙された各ディレクトリを順に探索。
3. **project-local**: `<project_root>/.murphy/plugins/`
4. **user-local**: `dirs::data_dir()/murphy/plugins/`
   - Linux: `$XDG_DATA_HOME/murphy/plugins/`
     (default `$HOME/.local/share/murphy/plugins/`)
   - macOS: `$HOME/Library/Application Support/murphy/plugins/`

ヒットしなかった場合は `ConfigError::Io` を返し、エラー文に **検索した
全ディレクトリ** と **`[[plugins]]` detailed form への誘導** を含める。

### 2. ファイル名規約

各 search dir 内で探すファイル名は `lib<sanitized>.{so,dylib}`:

- `<sanitized>` = name の `-` を `_` に置換した文字列
- 拡張子: macOS = `dylib`、それ以外 = `so`

これは **Cargo cdylib の命名規則**と一致する。`murphy-rails` という
crate を `crate-type = ["cdylib"]` でビルドすると `libmurphy_rails.{so,dylib}`
が生成される。fallback (`lib<name>.so` をそのまま探す) は採らない —
プラグインは Cargo cdylib として配布する想定で、二重命名は build/test
の分岐を増やすだけで RuboCop 互換 UX には寄与しない。

### 3. Name validation

`Name(String)` は path 構築前に以下の検証を通す:

- 1..=64 文字
- ASCII 文字種 `[A-Za-z0-9_.-]` のみ
- `..` substring を含まない

これは `name = "../../../etc/passwd"` のような path traversal を
`find_in_dir(dir, name)` で組み立てる経路を塞ぐ。検証失敗時は
"invalid character" / "must not contain `..`" を含むエラーで reject。

### 4. Dedup pre-pass

同一 name の `Name` と `Detailed` が同 array に共存しても **load は 1 回のみ**
発生する。具体的に:

```toml
plugins = [
  "foo",                                  # Name
  { name = "foo", path = "./vendor.so" }, # Detailed (順序問わず)
]
```

→ 1 load。pre-pass で `Detailed` から `BTreeMap<name, path>` を組み、
`Name` 走査時にそこに hit すれば search path 解決をスキップして
`Detailed` の `path` を直接採用する。`seen` set で **配列出現順** に dedup
するため、既に処理済みの name の重複 entry (Name か Detailed か問わず) は
silently drop される。

これにより、ADR 0041 当時 `registry.rs` が同 name plugin の 2 重 load 時に
出していた `attempts to register 'X' but already registered` という
意味不明なエラーを回避できる。

## 動機

ADR 0041 で `[[plugins]]` schema を確定した時点では、`Name(String)` shorthand
は parse のみ成功し、load 時に明示エラーで弾かれていた。これでは
RuboCop 互換 UX の核心である「`s/rubocop-/murphy-/` 一行で migration 完了」
が成立しない (ユーザーは結局 `Detailed` form で path を書く必要がある)。

本 ADR で resolution を実装することで、RuboCop の以下の設定:

```yaml
plugins:
  - rubocop-rails
  - rubocop-rspec
```

を `s/rubocop-/murphy-/` した上で:

```toml
plugins = ["murphy-rails", "murphy-rspec"]
```

と書くだけで動作する。`MURPHY_PLUGIN_PATH` を `cargo install --path` の
出力先 (典型的には `~/.cargo/bin` の隣) に通すか、project-local に
`.murphy/plugins/` を切るか、user-local に置くかは運用者の選択肢。

## 検討した代替案

| 代替案 | 採否 | 理由 |
|---|---|---|
| `dirs` crate を使わず `std::env::var_os("HOME")` 直書き | rejected | XDG / macOS Application Support の semantics を自前実装すると cross-platform で罠が多い。`dirs` の ~30KB overhead は妥当 |
| 命名規約に `lib<name>.so` (hyphen そのまま) も fallback | rejected | Cargo cdylib 一択に絞れば test/build path 分岐が減る。`murphy migrate` 直後のユーザーは Cargo build を経るので underscore form のみで十分 |
| Detailed override を array 順序依存 (後勝ち) | rejected | 「順序を意識せず Detailed が優先」のほうが migration UX として自然 (ユーザーが配列を再編集する手間が減る) |
| Name resolution 失敗時に warn して silently skip | rejected | 「plugin が一個も load されない理由」がユーザーに見えなくなる。exit 2 (setup error) で即時失敗するほうが debug 可能 |
| systemwide `/usr/local/lib/murphy/plugins/` を加える | deferred | パッケージマネージャ統合 (homebrew / apt) の議論と一緒に再検討。MVP には不要 |
| バージョン制約 (`murphy-rails == 0.1.0`) | deferred | ADR 0041 で `version` を drop 済み。再導入は別 ADR |

## 実装

- `crates/murphy-core/src/plugin_resolver.rs` 新規:
  - `validate_plugin_name(name: &str) -> Result<(), ConfigError>`
  - `lib_filename(name: &str) -> String`
  - `resolve_plugin_name_with_search_dirs(name, overrides, search_dirs)`
    (pure、テスト用 entry point)
  - `resolve_plugin_name(name, project_root, overrides)`
    (production wrapper: env + project + user の dir を組み立てる)
  - `plan_plugin_loads(project_root, plugins) -> Vec<(name, path)>`
    (Detailed/Name dedup + Name resolution の一括処理)
- `crates/murphy-core/src/registry.rs`:
  - 旧 `Name(String) → "not yet implemented" エラー` を削除
  - `plan_plugin_loads` を `discover_with_config` の plugin 列挙の前段に挿入
- `crates/murphy-core/Cargo.toml`: `dirs = "5"` 追加
- `crates/murphy-cli/tests/plugin_pack_e2e.rs`:
  - 旧 `name_only_form_exits_2_with_not_yet_implemented_hint` を削除
  - 新 4 test 追加 (env / project-local / not-found / Name+Detailed dedup)
- `crates/murphy-core/src/config.rs` `migrate_rubocop_yml_to_murphy_toml`:
  - plugin entries 出力時に `# Note: ...` の hint コメントを 1 行追加

## Consequences

### Positive

- RuboCop ユーザーの `.rubocop.yml` を `s/rubocop-/murphy-/` するだけで
  動く migration UX が完成。
- 同名 `Name` + `Detailed` の dedup により、override 用途で配列の任意位置に
  `Detailed` を差し込んでも collision エラーにならない。
- Search path の優先順位が明示的で、ユーザーは自分の plugin がどこから
  load されているか (env / project / user) を error message と manual
  記述から把握できる。

### Negative

- `dirs` crate 依存追加 (依存ツリーは ~30KB、cross-platform XDG/macOS
  分岐を自前で書かないトレードオフ)。
- Cargo cdylib 命名規約に lock-in。手書きで `libfoo.so` を直接配置する
  運用 (例: C で書いた plugin) は `Detailed` form 必須になる。
- Windows は引き続き plugin pack 非対応 (`registry.rs` Windows guard 経路)。
  本 ADR は Unix-only。

## 関連

- ADR 0038 — single-surface plugin ABI
- ADR 0041 — `[[plugins]]` schema (本 ADR の前提)
- `murphy-9cr.10.1` — schema 確定タスク
- `murphy-9cr.10.2` — 本 ADR の実装タスク
- `murphy-9cr.10.3` — `PluginDetailed` struct extract (deny_unknown_fields 復活)
- `murphy-ii8` — YAML config 移行 (本 resolution は YAML 経路でも非破壊で動く)
