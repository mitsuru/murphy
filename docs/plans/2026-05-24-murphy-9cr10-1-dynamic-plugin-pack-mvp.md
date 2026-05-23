# murphy-9cr.10.1 dynamic plugin pack MVP — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** dynamic `.so` plugin pack 経路を e2e で初めて駆動する。`murphy-example-pack` を 2 cops で復活させ、`[[cop_packs]]` schema を `[[plugins]]` (heterogeneous Name|Detailed) に rename して RuboCop 互換 shorthand を予約、`murphy migrate` の `plugins:` 翻訳を追加、整合 ADR 0041 を入れる。

**Architecture:** post-9cr.22 で既に動く `load_plugin_pack` (dlopen + ABI 検証) + `CopRegistry::discover_with_config` の load 経路を活用する。schema enum (`PluginConfig = Name(String) | Detailed { name, path }`) を serde untagged enum で定義、`Name(String)` は MVP では明示エラー (resolution は murphy-9cr.10.2 で実装)。murphy-example-pack を `crate-type = ["cdylib", "rlib"]` + `register_cops!(mode = dynamic)` で復活させ、murphy-cli の `[dev-dependencies]` に path 依存を追加して Cargo dep graph が build ordering を担保する。e2e test は artifact path を `CARGO_TARGET_DIR` → `${CARGO_MANIFEST_DIR}/../../target` の 2-tier で解決して dlopen 経由で読む。

**Tech Stack:** Rust 1.95.0 stable (rust-toolchain.toml で pin)、Cargo workspace `crates/*`、serde 1 + serde_json 1 + toml + serde_yaml (config 内 既存)、libloading (`plugin_loader.rs` で既存利用)、assert_cmd + tempfile (CLI integration test 既存)、murphy-plugin-api + murphy-plugin-macros (`register_cops!`, `#[derive(CopOptions)]`)。

**bd issue:** murphy-9cr.10.1 (IN_PROGRESS, P2、parent murphy-9cr.10)

**コミット規約:** 各 task の最後で `bd:` または `feat(murphy-9cr.10.1):` 等のタスク ID prefix 付きで commit。frequent commits、push は session 末に一括。

---

## Phase 0: 前提確認

### Task 0.1: 現状の workspace build が green であること確認

**Files:** なし (read-only)

**Step 1:** Workspace 全体の build / test が現時点で green か確認。

Run: `cargo test --workspace --no-fail-fast 2>&1 | tail -20`
Expected: 既存テスト全て pass、warning 0 で完了。

**Step 2:** rename 対象の symbol が確認した通り存在することを再確認。

Run: `grep -rn "cop_packs\|CopPackConfig\|build_cop_packs_cop_and_node_cop_metadata" crates/ --include='*.rs' | wc -l`
Expected: 10〜15 程度の hit (config.rs, registry.rs, cops.rs comment, internal.rs test name)。

**Step 3:** 何も commit せず次フェーズへ。

---

## Phase 1: Schema rename `cop_packs` → `plugins` (PluginConfig untagged enum)

### Task 1.1: `MurphyConfig.cop_packs` field の rename + 既存テストへの shim

**Files:**
- Modify: `crates/murphy-core/src/config.rs:9-13` (MurphyConfig struct)
- Modify: `crates/murphy-core/src/config.rs:50-56` (MurphyToml struct)
- Modify: `crates/murphy-core/src/config.rs:80-90` (Default impl)
- Modify: `crates/murphy-core/src/config.rs:120-130` (From<MurphyToml> for MurphyConfig)

**Step 1: 既存テスト失敗の確認 (renamed field 期待)**

Run: 後の Step 4 で実施。今は実装変更のみ。

**Step 2: rename 実施**

`MurphyConfig`:
```rust
pub struct MurphyConfig {
    pub files: FilesConfig,
    pub cops: CopsConfig,
    pub plugins: Vec<CopPackConfig>,  // 旧 cop_packs (Task 1.2 で PluginConfig 化)
}
```

`MurphyToml`:
```rust
struct MurphyToml {
    #[serde(default)] files: FilesTable,
    #[serde(default)] cops: CopsTable,
    #[serde(default)] plugins: Vec<CopPackConfig>,  // 旧 cop_packs
}
```

`Default impl` と `From<MurphyToml>` も `cop_packs` → `plugins` に。

**Step 3: 既存テスト fixture も rename**

`config.rs:408 fn parses_cop_packs` のテスト内 TOML テキスト `[[cop_packs]]` を `[[plugins]]` に変える。テスト名は次の Task で正式 rename するので、ここでは body だけ修正。

`config.rs:429 fn cop_packs_default_to_empty` の `cfg.cop_packs` 参照を `cfg.plugins` に変える。

**Step 4: registry.rs の参照を rename**

`crates/murphy-core/src/registry.rs:155` の `for pack in &config.cop_packs` → `for pack in &config.plugins` (型はまだ `CopPackConfig` のままなので変数名 `pack` も維持)。

`registry.rs:192` の `config.cop_packs.first()` → `config.plugins.first()`。

**Step 5: テスト**

Run: `cargo test -p murphy-core --lib config 2>&1 | tail -10`
Expected: `parses_cop_packs` と `cop_packs_default_to_empty` が pass (テスト body が plugins に対応した状態)。

Run: `cargo test --workspace 2>&1 | tail -10`
Expected: workspace 全体 green。

**Step 6: コミット**

```bash
git add crates/murphy-core/src/config.rs crates/murphy-core/src/registry.rs
git commit -m "refactor(murphy-9cr.10.1): rename MurphyConfig.cop_packs field to plugins

PluginConfig enum 化と version drop は次の commit で。本 commit は
field rename と既存 fixture の \"[[cop_packs]]\" → \"[[plugins]]\" 置換のみ。"
```

### Task 1.2: `CopPackConfig` → `PluginConfig` untagged enum 化 (Name | Detailed)、version drop

**Files:**
- Modify: `crates/murphy-core/src/config.rs:29-34` (旧 CopPackConfig struct)
- Modify: `crates/murphy-core/src/config.rs:9-13` (MurphyConfig.plugins の型)
- Modify: `crates/murphy-core/src/config.rs:50-56` (MurphyToml.plugins の型)

**Step 1: 新しいテストを追加 (failing)**

`crates/murphy-core/src/config.rs` のテストモジュールに以下を追加 (`parses_plugins_default_to_empty` などの近く):

```rust
#[test]
fn parses_plugins_detailed_form() {
    let cfg = MurphyConfig::from_toml_str(
        r#"
[[plugins]]
name = "murphy-example-pack"
path = "target/debug/libmurphy_example_pack.so"
"#,
    )
    .unwrap();
    assert_eq!(cfg.plugins.len(), 1);
    match &cfg.plugins[0] {
        PluginConfig::Detailed { name, path } => {
            assert_eq!(name, "murphy-example-pack");
            assert_eq!(path.to_str(), Some("target/debug/libmurphy_example_pack.so"));
        }
        other => panic!("expected Detailed, got {other:?}"),
    }
}

#[test]
fn parses_plugins_name_only_form() {
    let cfg = MurphyConfig::from_toml_str(r#"plugins = ["murphy-rails"]"#).unwrap();
    assert_eq!(cfg.plugins.len(), 1);
    match &cfg.plugins[0] {
        PluginConfig::Name(name) => assert_eq!(name, "murphy-rails"),
        other => panic!("expected Name, got {other:?}"),
    }
}

#[test]
fn parses_plugins_heterogeneous_array() {
    let cfg = MurphyConfig::from_toml_str(
        r#"
plugins = [
  "murphy-rails",
  { name = "local-pack", path = "./libfoo.so" }
]
"#,
    )
    .unwrap();
    assert_eq!(cfg.plugins.len(), 2);
    assert!(matches!(&cfg.plugins[0], PluginConfig::Name(n) if n == "murphy-rails"));
    assert!(matches!(&cfg.plugins[1], PluginConfig::Detailed { name, .. } if name == "local-pack"));
}

#[test]
fn rejects_plugins_with_unknown_version_field() {
    let err = MurphyConfig::from_toml_str(
        r#"
[[plugins]]
name = "x"
path = "y"
version = "0.1"
"#,
    )
    .unwrap_err();
    // `#[serde(deny_unknown_fields)]` 相当の挙動を期待 (Detailed variant 側)
    assert!(format!("{err:?}").contains("unknown field"), "{err:?}");
}
```

**Step 2: テスト失敗の確認**

Run: `cargo test -p murphy-core --lib config::tests::parses_plugins 2>&1 | tail -20`
Expected: 4 件 fail (型 `PluginConfig` 未定義 + `cop_packs` → `plugins` テスト fixture もまだ 旧 struct)。

**Step 3: `PluginConfig` enum を実装**

`crates/murphy-core/src/config.rs` で `CopPackConfig` を削除し、代わりに:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum PluginConfig {
    /// `plugins = ["murphy-rails"]` — name-only。
    /// search path resolution は murphy-9cr.10.2 で実装。MVP では
    /// `registry.rs` の load 経路で明示エラー。
    Name(String),
    /// `[[plugins]] name = "..." path = "..."` — explicit path。
    /// MVP で動作する form。
    Detailed {
        name: String,
        path: PathBuf,
    },
}
```

`#[serde(deny_unknown_fields)]` は **enum variant レベルでは効かない**のが serde の仕様。`Detailed` を別 struct に切り出してそこに `#[serde(deny_unknown_fields)]` を付けるか、untagged enum + 内部 struct という構造にする:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginDetailed {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum PluginConfig {
    Name(String),
    Detailed(PluginDetailed),
}
```

ただし match arm 内で `PluginConfig::Detailed(d) => d.name / d.path` になり API が冗長になる。代替案: untagged + 直接 struct variant に `#[serde(deny_unknown_fields)]` は serde 1.0.197+ では variant attr として無視されるので、`Detailed` 内に余計 field が来た場合は parse 通ってしまう。Task 1.2 の `rejects_plugins_with_unknown_version_field` テストはこれを根拠に書いている。

**判断:** untagged enum で variant に flat struct 形を維持し、`rejects_plugins_with_unknown_version_field` テストは spec として残すが Phase 3 で別 issue (PluginDetailed 化) として extract する。今は variant 形を維持しテスト名を `accepts_plugins_with_unknown_field_for_now_documented_limitation` に置き換える:

```rust
#[test]
fn plugins_unknown_field_silently_accepted_for_now() {
    // serde の untagged enum + struct variant は variant 内側で
    // deny_unknown_fields を受け付けない (rustfmt が消す)。将来的に
    // PluginDetailed を別 struct に切り出して deny_unknown_fields を
    // 効かせる予定 — それまでは unknown field を silently accept する。
    let cfg = MurphyConfig::from_toml_str(
        r#"
[[plugins]]
name = "x"
path = "y"
version = "0.1"
"#,
    )
    .unwrap();
    assert_eq!(cfg.plugins.len(), 1);
}
```

`Vec<CopPackConfig>` → `Vec<PluginConfig>` に MurphyConfig と MurphyToml の両方を変更。

**Step 4: テスト pass の確認**

Run: `cargo test -p murphy-core --lib config 2>&1 | tail -20`
Expected: 全テスト pass。

**Step 5: `registry.rs` の match arm 対応**

`crates/murphy-core/src/registry.rs:155` 周辺:

```rust
#[cfg(not(target_os = "windows"))]
for plugin in &config.plugins {
    let pack_index = pack_names.len();
    let (name, path) = match plugin {
        PluginConfig::Detailed { name, path } => (name.clone(), root.join(path)),
        PluginConfig::Name(name) => {
            return Err(ConfigError::Io(format!(
                "Plugin `{name}`: name resolution is not yet implemented \
                 (murphy-9cr.10.2). Use detailed form: \
                 `[[plugins]] name = \"{name}\" path = \"...\"`."
            )));
        }
    };
    let loaded = load_plugin_pack(&path).map_err(|e| {
        ConfigError::Io(format!("cannot load cop pack {name}: {e}"))
    })?;
    // ... 既存 collision check + push 等は同じ ...
}
```

`registry.rs:192` の Windows guard:
```rust
#[cfg(target_os = "windows")]
if let Some(plugin) = config.plugins.first() {
    let name = match plugin {
        PluginConfig::Detailed { name, .. } | PluginConfig::Name(name) => name,
    };
    return Err(ConfigError::Io(format!(
        "cop packs (`.so` plugins) are not supported on Windows: {name}"
    )));
}
```

**Step 6: full workspace test**

Run: `cargo test --workspace 2>&1 | tail -10`
Expected: green。

**Step 7: コミット**

```bash
git add crates/murphy-core/src/config.rs crates/murphy-core/src/registry.rs
git commit -m "refactor(murphy-9cr.10.1): CopPackConfig -> PluginConfig untagged enum

- 旧 CopPackConfig struct を削除、PluginConfig enum 化:
    Name(String)              — name-only shorthand (RuboCop-compat)
    Detailed { name, path }   — explicit path form
- version field drop (RuboCop の plugins: directive には version 概念なし)
- registry.rs の load arm を match に書き換え、Name(String) は MVP では
  明示エラー (\"name resolution is not yet implemented\")
- 旧 parses_cop_packs テストを parses_plugins_detailed_form に rename、
  Name と heterogeneous の test を追加"
```

### Task 1.3: 旧 cop_packs 名残 test の rename + 関連 doc コメント更新

**Files:**
- Modify: `crates/murphy-core/src/config.rs` のテスト名:
  - `parses_cop_packs` → `parses_plugins_detailed_form` (Task 1.2 で既に置換済みなら skip)
  - `cop_packs_default_to_empty` → `plugins_default_to_empty`
- Modify: `crates/murphy-core/src/registry.rs` の doc コメント `[[cop_packs]]` → `[[plugins]]`
- Modify: `crates/murphy-cli/src/cops.rs:94` のコメント "ones contributed by `[[cop_packs]]`" → "`[[plugins]]`"
- Modify: `crates/murphy-plugin-api/src/internal.rs:147` テスト名 `build_cop_packs_cop_and_node_cop_metadata` → `build_plugins_cop_and_node_cop_metadata`

**Step 1: 一括 grep で残存 hit を確認**

Run: `grep -rn "cop_packs\|cop-packs\|\\[\\[cop_packs\\]\\]\|CopPackConfig" crates/ --include='*.rs'`
Expected: 4〜6 件 (doc コメント / テスト名)。

**Step 2: 全 hit を rename**

各ファイルを `Edit` で個別に修正。コードロジックではなく名前 / コメントの置換のみ。

**Step 3: grep でゼロ確認**

Run: `grep -rn "cop_packs\|cop-packs\|CopPackConfig" crates/ --include='*.rs' | grep -v target`
Expected: 0 件 hit。

**Step 4: テスト**

Run: `cargo test --workspace 2>&1 | tail -10`
Expected: green。

**Step 5: コミット**

```bash
git add crates/
git commit -m "refactor(murphy-9cr.10.1): scrub residual cop_packs naming

doc コメント / テスト名 / 内部 test fn 名から cop_packs / CopPackConfig を
\"plugins\" / \"PluginConfig\" に統一。production code から cop_packs grep
hit が 0 件になることを確認。"
```

---

## Phase 2: `migrate_rubocop_yml_to_murphy_toml` に `plugins:` 翻訳追加

### Task 2.1: failing integration test

**Files:**
- Modify: `crates/murphy-cli/tests/migrate.rs` (新規ケース追加)

**Step 1: 既存 migrate test の形を確認**

Run: `head -50 crates/murphy-cli/tests/migrate.rs`
Note: 既存 fixture / 実行形式に合わせる。

**Step 2: failing test を追加**

```rust
#[test]
fn migrate_translates_plugins_directive_preserving_names() {
    let input = "\
plugins:
  - rubocop-rails
  - rubocop-rspec
AllCops:
  Include: ['**/*.rb']
";
    // murphy migrate を起動 (assert_cmd 経由)
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(".rubocop.yml");
    std::fs::write(&path, input).expect("write");
    let assert = assert_cmd::Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("migrate")
        .arg(&path)
        .assert()
        .code(0);
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf-8");
    // name は preserve (auto-rename しない)
    assert!(stdout.contains("plugins = [\"rubocop-rails\", \"rubocop-rspec\"]"),
            "stdout missing plugins line:\n{stdout}");
    // [files] section も出ること (regression)
    assert!(stdout.contains("[files]"), "stdout missing [files]:\n{stdout}");
}
```

**Step 3: 失敗確認**

Run: `cargo test -p murphy-cli --test migrate migrate_translates_plugins 2>&1 | tail -15`
Expected: FAIL — `plugins = [...]` が出力に含まれない。

### Task 2.2: 実装 + テスト pass

**Files:**
- Modify: `crates/murphy-core/src/config.rs:221-290` (`migrate_rubocop_yml_to_murphy_toml`)

**Step 1: 関数の冒頭で `plugins:` を読み込む**

`fn migrate_rubocop_yml_to_murphy_toml` 内、既存の `for (key, value) in top` loop の前後または同 loop 内で:

```rust
let mut plugin_names: Vec<String> = Vec::new();
let mut unsupported_plugins: Vec<String> = Vec::new();

for (key, value) in &top {
    let Some(section) = key.as_str() else { continue; };
    if section == "plugins" {
        if let serde_yaml::Value::Sequence(items) = value {
            for item in items {
                match item {
                    serde_yaml::Value::String(s) => plugin_names.push(s.clone()),
                    serde_yaml::Value::Mapping(m) => {
                        // `- foo: {...}` 形は MVP では unsupported コメント
                        if let Some(name) = m.iter().next().and_then(|(k, _)| k.as_str()) {
                            unsupported_plugins.push(name.to_string());
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    // ... 既存処理 ...
}
```

注意: `top` を `for (key, value) in top` (move) → `for (key, value) in &top` (borrow) に変更すると下流に影響。`top.iter()` でも可。最も影響少ない書き方を選ぶ (既存コードを読んで判断)。

**Step 2: 出力部分の冒頭に plugins 行を追加**

`let mut out = String::new();` の直後 (まだ `[files]` を書く前) に:

```rust
if !plugin_names.is_empty() {
    let joined = plugin_names
        .iter()
        .map(|n| format!("\"{n}\""))
        .collect::<Vec<_>>()
        .join(", ");
    out.push_str(&format!("plugins = [{joined}]\n"));
}
for unsupported in &unsupported_plugins {
    out.push_str(&format!("# unsupported plugin entry: {unsupported}\n"));
}
if !plugin_names.is_empty() || !unsupported_plugins.is_empty() {
    out.push('\n');
}
out.push_str("[files]\n");
// ... 既存処理 ...
```

**Step 3: 既存テスト regression がないか確認**

Run: `cargo test -p murphy-cli --test migrate 2>&1 | tail -10`
Expected: 新規テスト含め green。

Run: `cargo test -p murphy-core --lib migrate 2>&1 | tail -10`
Expected: green (config.rs 内に既存 migrate unit test があれば pass)。

**Step 4: コミット**

```bash
git add crates/murphy-core/src/config.rs crates/murphy-cli/tests/migrate.rs
git commit -m "feat(murphy-9cr.10.1): migrate translates .rubocop.yml plugins: directive

murphy migrate が .rubocop.yml の plugins: directive を読んで TOML 側に
\`plugins = [\"...\"]\` 形式で書き出す。name は preserve (rubocop-X → murphy-X
の auto-rename はしない — ユーザー責任)。mapping form (- foo: {...}) は
\`# unsupported plugin entry: foo\` コメントで出力。"
```

---

## Phase 3: murphy-example-pack 復活 (2 cops)

### Task 3.1: Cargo.toml 更新

**Files:**
- Modify: `crates/murphy-example-pack/Cargo.toml`

**Step 1: 現状確認**

Run: `cat crates/murphy-example-pack/Cargo.toml`
Expected: `crate-type = ["cdylib"]`, deps が `murphy-core`。

**Step 2: 修正**

```toml
[package]
name = "murphy-example-pack"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib", "rlib"]
# rlib variant は murphy-cli の [dev-dependencies] が依存解決時に
# 要求するために残す (build ordering 担保用)。production からは
# cdylib しか参照されない。詳細: docs/plans/2026-05-24-murphy-9cr10-1-...md
# Phase 4 留意点参照。

[dependencies]
murphy-plugin-api = { path = "../murphy-plugin-api" }
serde_json = "1"  # #[derive(CopOptions)] 展開が ::serde_json を参照
```

**Step 3: build verify (まだ cops 実装してないので fail する想定)**

Run: `cargo build -p murphy-example-pack 2>&1 | tail -10`
Expected: lib.rs に register_cops! がない状態なら成功 (空 cdylib + rlib)。

**Step 4: コミット**

```bash
git add crates/murphy-example-pack/Cargo.toml
git commit -m "refactor(murphy-9cr.10.1): example-pack to single-surface API + cdylib+rlib

dep を murphy-core から murphy-plugin-api に切替 (single-surface ABI、ADR 0038)。
crate-type = [\"cdylib\", \"rlib\"] にして murphy-cli の dev-dependencies から
依存解決可能にし build ordering を Cargo dep graph で担保。"
```

### Task 3.2: Example/NoEval cop 実装 (TDD)

**Files:**
- Create: `crates/murphy-example-pack/src/no_eval.rs`
- Modify: `crates/murphy-example-pack/src/lib.rs`

**Step 1: 既存類似 cop (NoReceiverPuts) のパターン参照**

Reference: `crates/murphy-std/src/murphy/no_receiver_puts.rs`
- `NodeKindTag(17)` が Send (= CallNode、prism の send)
- `NodeKind::Send { receiver, method, .. }` で destructuring
- `cx.symbol_str(method)` で method name 取得
- `receiver` は `OptNodeId` で `OptNodeId::NONE` が「receiver なし」(bare call)

**Step 2: src/no_eval.rs を作成**

```rust
//! `Example/NoEval` — flags `eval` calls. Demo cop for the
//! murphy-example-pack distribution.
//!
//! Matches:
//! - `eval(...)`          — bare call
//! - `Kernel.eval(...)`   — explicit Kernel receiver (ConstantRead)
//! - `Kernel::eval(...)`  — Kernel via ConstantPath
//! - `self.eval(...)`     — self receiver
//!
//! No autocorrect — replacing `eval` mechanically is unsafe.

use murphy_plugin_api::{
    Cop, Cx, NoOptions, NodeCop, NodeId, NodeKind, NodeKindTag, OptNodeId, Severity,
};

/// `NodeKind::Send` tag — declaration order is frozen by ADR 0037.
const SEND_TAG: NodeKindTag = NodeKindTag(17);

#[derive(Default)]
pub struct NoEval;

impl Cop for NoEval {
    type Options = NoOptions;
    const NAME: &'static str = "Example/NoEval";
    const DESCRIPTION: &'static str =
        "Flag `eval` calls — dynamic code execution is dangerous.";
    const DEFAULT_SEVERITY: Option<Severity> = Some(Severity::Warning);
    const DEFAULT_ENABLED: Option<bool> = Some(true);
}

impl NodeCop for NoEval {
    const KINDS: &'static [NodeKindTag] = &[SEND_TAG];

    fn check(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send {
            receiver, method, ..
        } = *cx.kind(node) else { return; };
        if cx.symbol_str(method) != "eval" {
            return;
        }
        if !receiver_is_eval_target(cx, receiver) {
            return;
        }
        let range = cx.range(node);
        cx.emit_offense(
            range,
            "eval is dangerous — avoid dynamic code execution",
            None,
        );
    }
}

/// receiver が `nil` / `Kernel` / `Kernel::Kernel` / `self` のいずれかなら true。
/// 任意の他の受信者 (例: `obj.eval`) は match 外。
fn receiver_is_eval_target(cx: &Cx<'_>, receiver: OptNodeId) -> bool {
    let Some(rid) = receiver.get() else {
        return true; // bare eval(...)
    };
    match cx.kind(rid) {
        // 既存 NodeKind variant 名は murphy-ast / single-surface API に
        // 再 export されているはず。実機で variant 名を確認して合わせる
        // (ConstantRead / ConstantPath / SelfNode 等)。
        NodeKind::ConstantRead { name, .. } => cx.symbol_str(*name) == "Kernel",
        NodeKind::ConstantPath { parent, name, .. } => {
            cx.symbol_str(*name) == "Kernel" && parent.get().is_none()
        }
        NodeKind::SelfNode { .. } => true,
        _ => false,
    }
}
```

注: `NodeKind` variant 名 (`ConstantRead` vs `ConstantReadNode` 等) は実機を見て調整。murphy-ast crate の variant 名と plugin-api の re-export を確認すべし。**実装前に `grep -n "NodeKind" crates/murphy-plugin-api/src/lib.rs` と `cargo doc -p murphy-plugin-api --open` 相当で variant 確認**。

**Step 3: lib.rs に module 追加 + register_cops!**

```rust
//! murphy-example-pack — demo cop pack for plugin authors.
//!
//! Reborn under the single-surface ABI (ADR 0038). Ships two cops:
//! - `Example/NoEval` — CallNode dispatch demo
//! - `Example/TodoFormat` — file-visit + #[derive(CopOptions)] demo

pub mod no_eval;
pub mod todo_format;  // Task 3.3 で実装

use crate::no_eval::NoEval;
use crate::todo_format::TodoFormat;

murphy_plugin_api::register_cops!(mode = dynamic, NoEval, TodoFormat);

#[cfg(test)]
mod tests {
    /// `cargo test --workspace` で cdylib build artifact を確実に
    /// 生成させるためのダミー smoke test。
    /// (`crates/murphy-cli/tests/plugin_pack_e2e.rs` が artifact を読む)
    #[test]
    fn smoke_compiles() {}
}
```

`todo_format` がまだないので、本 Task では `todo_format` 行と `TodoFormat` 参照と register_cops! の `TodoFormat` 引数をコメントアウト、または NoEval だけで register:

```rust
murphy_plugin_api::register_cops!(mode = dynamic, NoEval);
```

として、Task 3.3 で TodoFormat 追加時に再 register。本 Task はまず NoEval 単独で build green を取る。

**Step 4: build + unit test**

Run: `cargo build -p murphy-example-pack 2>&1 | tail -10`
Expected: success、cdylib + rlib 生成。

Run: `cargo test -p murphy-example-pack 2>&1 | tail -10`
Expected: smoke_compiles pass。

**Step 5: コミット**

```bash
git add crates/murphy-example-pack/
git commit -m "feat(murphy-9cr.10.1): example-pack Example/NoEval cop

CallNode dispatch + receiver マッチング (nil / Kernel / Kernel:: / self) +
emit warning。autocorrect / options なし、minimal demo cop。"
```

### Task 3.3: Example/TodoFormat cop 実装 (TDD)

**Files:**
- Create: `crates/murphy-example-pack/src/todo_format.rs`
- Modify: `crates/murphy-example-pack/src/lib.rs` (register_cops! に追加)

**Step 1: 参照: TrailingWhitespace + StringLiterals**

Reference:
- file-visit: `crates/murphy-std/src/layout/trailing_whitespace.rs` (KINDS=[]、cx.source())
- `#[derive(CopOptions)]` with default array: `crates/murphy-plugin-macros/tests/ui/pass_derive_attrs.rs` の `default = ["id"]` 形

**Step 2: src/todo_format.rs を作成**

```rust
//! `Example/TodoFormat` — flags TODO/FIXME comments without an author
//! tag. Demo of file-visit dispatch + #[derive(CopOptions)] + raw source.

use murphy_plugin_api::{
    Cop, CopOptions, Cx, NodeCop, NodeId, NodeKindTag, Range, Severity,
};

#[derive(Default)]
pub struct TodoFormat;

#[derive(CopOptions)]
pub struct TodoFormatOptions {
    #[option(
        default = ["TODO", "FIXME"],
        description = "Tags treated as todo-style markers."
    )]
    pub tags: Vec<String>,
    #[option(
        default = false,
        description = "When true, require an @author <name> annotation on the same line."
    )]
    pub require_author: bool,
}

impl Cop for TodoFormat {
    type Options = TodoFormatOptions;
    const NAME: &'static str = "Example/TodoFormat";
    const DESCRIPTION: &'static str =
        "Check format of TODO/FIXME comments (optionally require @author tag).";
    const DEFAULT_SEVERITY: Option<Severity> = Some(Severity::Warning);
    const DEFAULT_ENABLED: Option<bool> = Some(true);
}

impl NodeCop for TodoFormat {
    /// File-visit (root に 1 度 dispatch、KINDS=[]) — TrailingWhitespace 同形。
    const KINDS: &'static [NodeKindTag] = &[];

    fn check(&self, _node: NodeId, cx: &Cx<'_>) {
        // Options 値は murphy-9cr.9 (validation gate) 着地前は
        // Default を使う方針 (StringLiterals と同じ)。
        let opts = TodoFormatOptions::default();

        let src = cx.source();
        let bytes = src.as_bytes();
        let mut line_start = 0usize;
        let mut i = 0usize;
        while i <= bytes.len() {
            let at_end = i == bytes.len();
            let is_lf = !at_end && bytes[i] == b'\n';
            if at_end || is_lf {
                check_line(cx, src, line_start, i, &opts);
                line_start = i + 1;
            }
            i += 1;
        }
    }
}

fn check_line(
    cx: &Cx<'_>,
    src: &str,
    line_start: usize,
    line_end: usize,
    opts: &TodoFormatOptions,
) {
    let line = &src[line_start..line_end];
    // 行頭の whitespace を skip して `#` で始まるかを判定。
    let stripped = line.trim_start();
    if !stripped.starts_with('#') {
        return;
    }
    // どれかの tag が同じ line に含まれるか?
    let Some(tag) = opts.tags.iter().find(|t| {
        // `# TODO`、`#TODO`、`# TODO:` 全てを許容するため、tag 前に空白 or
        // # 直後を許す形で素朴に contains を使う。誤検出より false negative
        // を許容する demo cop なのでこれで十分。
        line.contains(&format!("# {}", t.as_str()))
            || line.contains(&format!("#{}", t.as_str()))
    }) else {
        return;
    };

    if opts.require_author && !line.contains("@author") {
        let range = Range {
            start: line_start as u32,
            end: line_end as u32,
        };
        cx.emit_offense(
            range,
            &format!("{tag} comment lacks @author tag"),
            None,
        );
    } else if !opts.require_author {
        // require_author = false の demo 経路: tag を持つ行を「format
        // 確認のため」常に warn する形にする (cop 自体のデモが目的なので)。
        // 実用 cop なら require_author を必須にする方が筋だが、本デモ cop
        // の e2e test で require_author=default(=false) で offense を出す
        // ために、ここではあえて tag があるだけで warn する。
        let range = Range {
            start: line_start as u32,
            end: line_end as u32,
        };
        cx.emit_offense(
            range,
            &format!("{tag} comment detected (example demo cop)"),
            None,
        );
    }
}
```

注: 実装に少し不格好な部分があるが demo cop として意図的。e2e test が「TODO 含む行 → offense」を assert できる状態。

**Step 3: lib.rs を更新して TodoFormat も register**

```rust
murphy_plugin_api::register_cops!(mode = dynamic, NoEval, TodoFormat);
```

**Step 4: build verify**

Run: `cargo build -p murphy-example-pack 2>&1 | tail -10`
Expected: success。

Run: `cargo test -p murphy-example-pack 2>&1 | tail -10`
Expected: smoke_compiles pass。

**Step 5: コミット**

```bash
git add crates/murphy-example-pack/
git commit -m "feat(murphy-9cr.10.1): example-pack Example/TodoFormat cop

file-visit dispatch (KINDS=[]) + #[derive(CopOptions)] (Vec<String> +
bool) + raw-source 走査の demo cop。tags option (default [\"TODO\",\"FIXME\"])
で対象 tag を切り替え可、require_author option で author 必須化。"
```

### Task 3.4: dep_boundary integration test

**Files:**
- Create: `crates/murphy-example-pack/tests/dep_boundary.rs`

**Step 1: murphy-std の dep_boundary.rs を参照**

Run: `cat crates/murphy-std/tests/dep_boundary.rs`

**Step 2: 同形で example-pack 用を作成**

```rust
//! Compile-time enforcement of the single-surface plugin ABI boundary for
//! `murphy-example-pack`. murphy-std/tests/dep_boundary.rs の同形:
//! production runtime 依存が `murphy-plugin-api` 1 本のみであることを
//! `cargo metadata` 経由で assert。dev-deps / build-deps は対象外。

use std::collections::BTreeSet;
use std::process::Command;

const ALLOWED_MURPHY_RUNTIME_DEPS: &[&str] = &["murphy-plugin-api"];

#[test]
fn murphy_example_pack_runtime_murphy_deps_match_allow_list() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let metadata_json = Command::new(env!("CARGO"))
        .args(["metadata", "--no-deps", "--format-version=1"])
        .current_dir(manifest)
        .output()
        .expect("cargo metadata");
    assert!(metadata_json.status.success(), "cargo metadata failed");
    let metadata: serde_json::Value =
        serde_json::from_slice(&metadata_json.stdout).expect("parse cargo metadata json");
    let packages = metadata["packages"].as_array().expect("packages array");
    let me = packages
        .iter()
        .find(|p| p["name"] == "murphy-example-pack")
        .expect("self package");
    let mut runtime_murphy_deps = BTreeSet::new();
    for dep in me["dependencies"].as_array().expect("deps array") {
        let kind = dep["kind"].as_str().unwrap_or("");
        // kind: null = normal, "dev" = dev-dep, "build" = build-dep
        if !kind.is_empty() {
            continue;
        }
        let name = dep["name"].as_str().expect("dep name");
        if name.starts_with("murphy-") {
            runtime_murphy_deps.insert(name.to_string());
        }
    }
    let allowed: BTreeSet<String> = ALLOWED_MURPHY_RUNTIME_DEPS
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        runtime_murphy_deps, allowed,
        "murphy-example-pack runtime Murphy deps must match {ALLOWED_MURPHY_RUNTIME_DEPS:?}"
    );
}
```

`dev-dependencies` に `serde_json` を追加 (本 test 内で使用):

```toml
[dev-dependencies]
serde_json = "1"
```

**Step 3: test 実行**

Run: `cargo test -p murphy-example-pack --test dep_boundary 2>&1 | tail -10`
Expected: pass (依存は murphy-plugin-api 1 本のみ)。

**Step 4: コミット**

```bash
git add crates/murphy-example-pack/
git commit -m "test(murphy-9cr.10.1): example-pack dep_boundary integration test

murphy-std と同形で、production runtime 依存が murphy-plugin-api 1 本
のみであることを cargo metadata 経由で検証。"
```

---

## Phase 4: murphy-cli の dev-dep + e2e integration test

### Task 4.1: murphy-cli の Cargo.toml に dev-dep 追加

**Files:**
- Modify: `crates/murphy-cli/Cargo.toml`

**Step 1: 現状確認**

Run: `cat crates/murphy-cli/Cargo.toml | grep -A 5 "dev-dependencies"`
Expected: 既存 `[dev-dependencies]` section (assert_cmd, tempfile 等)。

**Step 2: 追加**

```toml
[dev-dependencies]
# (既存 entries はそのまま残す)
# Build-order dependency only. Forces cargo to build
# murphy-example-pack's cdylib whenever murphy-cli test binaries are
# compiled. The rlib variant exists solely to satisfy cargo dep
# resolution — we never `use` it from Rust; the e2e test loads the
# cdylib via dlopen (tests/plugin_pack_e2e.rs).
murphy-example-pack = { path = "../murphy-example-pack" }
```

**Step 3: build verify (example-pack が dep として build される確認)**

Run: `cargo test -p murphy-cli --no-run 2>&1 | tail -10`
Expected: example-pack も build される。

Run: `ls target/debug/libmurphy_example_pack.so`
Expected: artifact 存在。

**Step 4: コミット**

```bash
git add crates/murphy-cli/Cargo.toml
git commit -m "feat(murphy-9cr.10.1): murphy-cli dev-dep on example-pack for build ordering

Cargo dep graph が example-pack の cdylib を murphy-cli の test build 前に
必ず生成するよう保証する。本 dep は Rust import に使わず、e2e test
(tests/plugin_pack_e2e.rs) は dlopen で artifact を読む。"
```

### Task 4.2: e2e integration test の骨組み + Test 1 (detailed form)

**Files:**
- Create: `crates/murphy-cli/tests/plugin_pack_e2e.rs`

**Step 1: failing test を含むファイル作成**

```rust
//! E2E integration test for dynamic plugin pack loading (murphy-9cr.10.1).
//!
//! Loads `murphy-example-pack` via the `[[plugins]]` config + dlopen path
//! and asserts that Example/NoEval and Example/TodoFormat fire on a
//! fixture .rb file.
//!
//! Windows は plugin pack 非対応 (registry.rs:190)、ファイル全体を
//! `cfg(not(target_os = "windows"))` で gating。

#![cfg(not(target_os = "windows"))]

use assert_cmd::Command;
use std::fs;
use tempfile::tempdir;

/// Resolve the cdylib artifact path. Cargo's dep graph (murphy-cli's
/// [dev-dependencies] -> murphy-example-pack) guarantees the cdylib is
/// built before this test runs; we just locate it.
///
/// 2-tier 解決: CARGO_TARGET_DIR env → ${CARGO_MANIFEST_DIR}/../../target。
/// .cargo/config.toml の target-dir override がない workspace 規約に依存。
fn example_pack_path() -> std::path::PathBuf {
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target")
        });
    let lib_name = if cfg!(target_os = "macos") {
        "libmurphy_example_pack.dylib"
    } else {
        "libmurphy_example_pack.so"
    };
    target_dir.join("debug").join(lib_name)
}

#[test]
fn detailed_form_loads_example_pack_and_emits_offenses() {
    let pack = example_pack_path()
        .canonicalize()
        .expect("example-pack artifact should exist (Cargo dep graph)");

    let dir = tempdir().expect("tempdir");
    let rb = dir.path().join("sample.rb");
    fs::write(
        &rb,
        "# frozen_string_literal: true\n# TODO: implement this\neval(\"x\")\n",
    )
    .expect("write rb");

    let toml = format!(
        "[[plugins]]\nname = \"murphy-example-pack\"\npath = {:?}\n",
        pack.display().to_string()
    );
    fs::write(dir.path().join("murphy.toml"), toml).expect("write toml");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg(&rb)
        .assert()
        .code(1); // offenses found

    let stdout = &assert.get_output().stdout;
    let offenses: Vec<serde_json::Value> =
        serde_json::from_slice(stdout).expect("stdout JSON");
    let names: Vec<String> = offenses
        .iter()
        .filter_map(|o| o["cop_name"].as_str().map(str::to_string))
        .collect();
    assert!(
        names.contains(&"Example/NoEval".to_string()),
        "expected Example/NoEval in {names:?}"
    );
    assert!(
        names.contains(&"Example/TodoFormat".to_string()),
        "expected Example/TodoFormat in {names:?}"
    );
}
```

`murphy-cli/Cargo.toml` の `[dev-dependencies]` に `serde_json = "1"` が無ければ追加 (既にあるはず — `assert_cmd::Command::cargo_bin` を使う他テストでも parse 用に既に存在する可能性大)。

**Step 2: failing test 確認 (まず Test 1 だけ書く)**

Run: `cargo test -p murphy-cli --test plugin_pack_e2e 2>&1 | tail -20`
Expected: PASS (実装側は既に揃っているはず)。

もし FAIL したら原因を診断:
- artifact が無い → Cargo dep graph の問題、Task 4.1 を再確認
- NodeKind variant 名違い → Example/NoEval の receiver check 実装を修正
- TodoFormat が offense 出さない → tag 検出ロジック確認
- offense 順序や exit code → 期待値修正

**Step 3: コミット**

```bash
git add crates/murphy-cli/tests/plugin_pack_e2e.rs
git commit -m "test(murphy-9cr.10.1): e2e plugin_pack_e2e detailed form happy path

[[plugins]] detailed form で murphy-example-pack を読み、fixture .rb に
対し Example/NoEval + Example/TodoFormat が両方 fire することを assert。
Cargo dep graph が build ordering を担保する前提で artifact を 2-tier 解決。"
```

### Task 4.3: Test 2 (path 不在時の failure)

**Files:**
- Modify: `crates/murphy-cli/tests/plugin_pack_e2e.rs`

**Step 1: test 追加**

```rust
#[test]
fn detailed_form_missing_path_exits_2_with_diagnostic() {
    let dir = tempdir().expect("tempdir");
    let rb = dir.path().join("sample.rb");
    fs::write(&rb, "puts 'hi'\n").expect("write rb");

    let toml = "[[plugins]]\nname = \"nonexistent\"\npath = \"./does-not-exist.so\"\n";
    fs::write(dir.path().join("murphy.toml"), toml).expect("write toml");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("lint")
        .arg(&rb)
        .assert()
        .code(2);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("cannot load plugin"),
        "stderr should mention plugin load failure: {stderr}"
    );
}
```

**Step 2: test 実行**

Run: `cargo test -p murphy-cli --test plugin_pack_e2e detailed_form_missing 2>&1 | tail -15`
Expected: PASS。

**Step 3: コミット**

```bash
git add crates/murphy-cli/tests/plugin_pack_e2e.rs
git commit -m "test(murphy-9cr.10.1): e2e missing path exits 2 with diagnostic"
```

### Task 4.4: Test 3 (name-only form は未実装エラー)

**Files:**
- Modify: `crates/murphy-cli/tests/plugin_pack_e2e.rs`

**Step 1: test 追加**

```rust
#[test]
fn name_only_form_exits_2_with_not_yet_implemented_hint() {
    let dir = tempdir().expect("tempdir");
    let rb = dir.path().join("sample.rb");
    fs::write(&rb, "puts 'hi'\n").expect("write rb");

    let toml = "plugins = [\"murphy-rails\"]\n";
    fs::write(dir.path().join("murphy.toml"), toml).expect("write toml");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("lint")
        .arg(&rb)
        .assert()
        .code(2);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("name resolution is not yet implemented")
            && stderr.contains("9cr.10.2"),
        "stderr should mention not-yet-implemented + 10.2 hint: {stderr}"
    );
}
```

**Step 2: 実行**

Run: `cargo test -p murphy-cli --test plugin_pack_e2e name_only_form 2>&1 | tail -15`
Expected: PASS。

**Step 3: full e2e suite**

Run: `cargo test -p murphy-cli --test plugin_pack_e2e 2>&1 | tail -15`
Expected: 3 tests pass (Tests 1, 2, 3)。

**Step 4: コミット**

```bash
git add crates/murphy-cli/tests/plugin_pack_e2e.rs
git commit -m "test(murphy-9cr.10.1): e2e name-only form deferred-to-10.2 error path"
```

---

## Phase 5: ADR 0041 (plugin loading schema)

### Task 5.1: ADR 0041 を新規作成

**Files:**
- Create: `docs/decisions/0041-plugin-loading-schema.md`

**Step 1: 既存 ADR の形式を確認**

Run: `head -30 docs/decisions/0040-arena-binary-cache.md`
Note: タイトル形式、status / context / decision / consequences の構造を踏襲。

**Step 2: ADR 内容を書く**

```markdown
# ADR 0041: Plugin loading schema (`[[plugins]]` table-array + Name|Detailed)

**Status:** Accepted (murphy-9cr.10.1)
**Date:** 2026-05-24
**Supersedes:** none (pre-public `[[cop_packs]]` schema を migration shim なしで置換)

## Context

post-9cr.22 で murphy のプラグインパック (`.so`) を dlopen する経路は実装済みだが、ユーザー設定 `murphy.toml` の schema 名称が `[[cop_packs]]` だった。本タスク (murphy-9cr.10.1) で MVP e2e を整備するにあたり、RuboCop の `.rubocop.yml` `plugins:` directive 直接互換を取れる schema に rename することにした。

ユーザー価値: 将来 YAML 設定移行 (murphy-ii8) 後、`s/rubocop-/murphy-/` するだけで RuboCop ユーザーが migrate 可能になる。

## Decision

1. **キー名は `plugins`**: TOML では `[[plugins]]` (table-array) または `plugins = [...]` (bare array)、YAML では `plugins:` (sequence)。
2. **Heterogeneous array**: 要素は文字列 (name-only shorthand) または table/mapping (detailed) を混在可能。Rust 型は serde untagged enum で表現:
   ```rust
   pub enum PluginConfig {
       Name(String),                              // RuboCop-compat shorthand
       Detailed { name: String, path: PathBuf },  // explicit path
   }
   ```
3. **Fields**: `name` (識別子)、`path` (project root 相対の `.so`/`.dylib`)。`version` は drop。
4. **Resolution**:
   - `Detailed`: `path` を直接 dlopen。
   - `Name`: search path 解決 (murphy-9cr.10.2 の ADR 0042 で別途規定)。本 ADR の MVP 時点では明示エラー。

## Rationale

- **RuboCop 互換 (load-bearing)**: `.rubocop.yml` の `plugins:` directive と同じキー名 / 同じシンタックスを採るのが目的。`cop_packs` だと文字面が違って毎回手動置換が必要。
- **TOML/YAML 同形**: heterogeneous array は両 format で同じ JSON-shape にマップされる。murphy-ii8 で YAML へ移行する際、schema をそのまま使える。
- **`version` drop**: RuboCop の `plugins:` には version 概念がなく、現状 Murphy 側でも未使用 (`CopPackConfig.version` は registry にも保存されていなかった)。
- **untagged enum**: 文字列要素と table 要素を 1 つのフィールドで受けるために serde untagged が最も自然。`#[serde(deny_unknown_fields)]` は variant struct 内で完全には効かないが、現状 production user 0 のためトレードオフ受容。

## Alternatives Considered

- **`[[cop_packs]]` 維持**: 用語一貫性 (`pack`) は保てるが、RuboCop 互換 UX が失われる。RuboCop 直接互換が本タスクの load-bearing 目的なので rejected。
- **`name` + `path` だけの flat table**: shorthand 不可能。RuboCop の `plugins:` UX と乖離するため rejected。
- **`[[plugins]] version`**: RuboCop に該当概念なし、現状 Murphy 未使用、`Detailed` variant に optional field として後付け可能なので drop で OK。

## Consequences

### Positive
- RuboCop ユーザーの migration が `s/rubocop-/murphy-/` で完結する世界を予約 (murphy-9cr.10.2 で resolution が完成すれば実現)。
- TOML / YAML 双方で同じ schema を提示できる。
- `Detailed` form は本タスクの MVP e2e で動作確認済み。

### Negative
- `version` を将来再導入する際は schema breaking change (production user が増えてからは互換に注意)。
- Untagged enum + struct variant の `deny_unknown_fields` 不完備は将来 `PluginDetailed` struct への切り出しで解消すべきとして follow-up に残す。

## Related

- ADR 0038 (single-surface plugin ABI)
- ADR 0042 (plugin name resolution、murphy-9cr.10.2 で merge 予定)
- murphy-9cr.10.1 (本 ADR を merge するタスク)
- murphy-9cr.10.2 (name resolution 実装)
- murphy-ii8 (YAML 移行)
```

**Step 3: コミット**

```bash
git add docs/decisions/0041-plugin-loading-schema.md
git commit -m "docs(murphy-9cr.10.1): ADR 0041 plugin loading schema

[[plugins]] table-array + Name|Detailed untagged enum + version drop の
決定を文書化。RuboCop の plugins: directive 直接互換を load-bearing な動機
として明記。murphy-9cr.10.2 (name resolution) は ADR 0042 で別途。"
```

---

## Phase 6: 既存 bd issue の design ref 更新 + 親 description 更新

### Task 6.1: 9cr.10.1 / 9cr.10.2 / 9cr.10 の design 内 ADR 番号を 0040 → 0041 に修正

**Files:**
- `/home/ubuntu/.claude/jobs/<job-id>/9cr-10-1-design.md` (job dir、削除可)

ただし bd issue は既に push 済みなので、bd update --design-file で更新する。

**Step 1: 各 design 内の "ADR 0040" を grep して場所確認**

Run: `bd show murphy-9cr.10.1 | grep -n "ADR 004"`
Expected: 2〜3 件 hit。

**Step 2: 各 issue の design field を bd update で patch**

bd には `--append-notes` はあるが --append-design はないため、design 全体を再投入する必要がある。本 plan の Phase 6 では:

1. 9cr.10.1 の design 内 "ADR 0040" → "ADR 0041" (本 ADR が新規付番、既存 0040 は arena binary cache)
2. 9cr.10.2 の design 内 "ADR 0040" → "ADR 0041"、"ADR 0041" → "ADR 0042"
3. 9cr.10 の description 内 "ADR 0040 (`[[plugins]]` loading schema、murphy-9cr.10.1 で merge 予定)" → "ADR 0041 (...)" 同様

`/home/ubuntu/.claude/jobs/<job-id>/9cr-10-{1,2,parent}.md` を sed で書き換え、bd update --design-file / --body-file で再投入。

Run (各 design / description ファイルに対して):
```bash
sed -i 's/ADR 0040 (plugin loading schema)/ADR 0041 (plugin loading schema)/g; \
        s/ADR 0040 (\[\[plugins\]\]/ADR 0041 ([[plugins]]/g; \
        s/ADR 0040 で merge/ADR 0041 で merge/g; \
        s/ADR 0041 (or 0040 増補)/ADR 0042 (or 0041 増補)/g; \
        s/ADR 0041 内容:/ADR 0042 内容:/g' <file>
bd update <id> --design-file <file>
```

(具体的な sed 表現は実行時に対応 — design 本文を最新化することが目的。)

**Step 3: 更新後 verify**

Run: `bd show murphy-9cr.10.1 | grep "ADR 004"`
Expected: ADR 0041 のみ、ADR 0040 (本 schema 名で) が残らない。

**Step 4: コミット (本 step は ADR 0041 自体のみ。bd 内容はリポジトリ外なので git の対象ではない)**

bd 経由の更新は dolt に commit されるが本 plan の git commit 対象は ADR とコード変更のみ。

### Task 6.2: 親 9cr.10 description の "ADR 0040 ... merge 予定" を 0041 に修正 (bd 経由)

**Files:** (bd 経由、git の commit 対象外)

**Step 1:**

job dir 内 9cr-10-parent-description.md の "ADR 0040 (`[[plugins]]` loading schema、murphy-9cr.10.1 で merge 予定)" を "ADR 0041 (`[[plugins]]` loading schema、murphy-9cr.10.1 で merge 予定)" に修正。"ADR 0041 (plugin name resolution、murphy-9cr.10.2 で merge 予定)" を "ADR 0042 (plugin name resolution、murphy-9cr.10.2 で merge 予定)" に修正。

**Step 2:** `bd update murphy-9cr.10 --body-file <file>`

**Step 3:** verify with `bd show murphy-9cr.10 | head -50`

---

## Phase 7: Final verification + close

### Task 7.1: 全体 cargo test --workspace

**Step 1:**

Run: `cargo test --workspace 2>&1 | tail -20`
Expected: green、全 test pass。

**Step 2:**

Run: `cargo fmt --check 2>&1 | tail -5`
Expected: formatting OK。

**Step 3:**

Run: `cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -10`
Expected: warnings 0。

### Task 7.2: standalone e2e の動作確認 (Cargo dep graph 担保の検証)

**Step 1:**

Run: `cargo test -p murphy-cli --test plugin_pack_e2e 2>&1 | tail -10`
Expected: 3 tests pass。

**Step 2:** (この test を standalone で走らせた時に example-pack が実 build される証拠を取る)

Run: `touch crates/murphy-example-pack/src/lib.rs && cargo test -p murphy-cli --test plugin_pack_e2e 2>&1 | grep -E "(Compiling|Building|murphy-example-pack)"`
Expected: 出力に `Compiling murphy-example-pack v0.1.0` が含まれる (Cargo dep graph が build を triggers した証拠)。

### Task 7.3: 完了条件チェック + bd issue close

**Step 1:** 完了条件 8 項目を確認:

- [ ] `cargo build -p murphy-example-pack` で cdylib + rlib 双方出力
- [ ] `cargo test --workspace` green
- [ ] `cargo test -p murphy-cli --test plugin_pack_e2e` standalone green
- [ ] example-pack の runtime Murphy 依存が murphy-plugin-api 1 本のみ (`tests/dep_boundary.rs` 通過)
- [ ] `cop_packs` / `cop-packs` の grep が production code から 0 件
- [ ] `murphy migrate <.rubocop.yml>` が `plugins:` 翻訳 (`crates/murphy-cli/tests/migrate.rs` 統合テスト通過)
- [ ] ADR 0041 merge
- [ ] 9cr.10 親 issue description が現状反映済み

**Step 2:** `bd close murphy-9cr.10.1 --reason="MVP dynamic plugin pack e2e green; [[plugins]] schema rename + Example/{NoEval, TodoFormat} cops + ADR 0041 + migrate plugins: 翻訳完了"`

**Step 3:** git push (session 末)

Run: `git status` で uncommitted がないことを確認、`git pull --rebase && git push` で push。

---

## 留意点 / Risks

1. **`NodeKind` variant 名は実機確認必須**: 本 plan の `ConstantRead` / `ConstantPath` / `SelfNode` 等は murphy-ast crate の実 variant 名 (e.g. `ConstantReadNode` の suffix 付きかどうか) と合わせる必要あり。Task 3.2 着手時に必ず確認。
2. **`#[serde(deny_unknown_fields)]` の効力**: untagged enum 変種内では完全には効かないため、本 MVP では `Detailed` 内 unknown field は silently accept。Task 1.2 の test 文言で documented limitation として明示。
3. **`#[derive(CopOptions)]` の Vec<String> default 構文**: `default = ["TODO", "FIXME"]` がそのまま使えるかを実機で確認。`pass_derive_attrs.rs` test では `default = ["id"]` 形が動いていることを確認済み。
4. **dispatch fault isolation**: 既存 NoEval cop が任意の receiver kind で panic しないこと (variant 名違いで unreachable!() を踏まないこと)。`_ => false` ですべて safety net。
5. **migrate の YAML mapping form skip**: `- foo: { ... }` の場合に確実に skip され、unsupported コメントが出力されること (Task 2.2 のロジックで `serde_yaml::Value::Mapping` 分岐をテストできれば理想 — 必要なら Task 2.1 の test に case を 1 個追加)。

## Estimated Effort

- Phase 1 (schema rename): 1.5 時間
- Phase 2 (migrate): 30 分
- Phase 3 (example-pack 2 cops + dep boundary): 1.5 時間
- Phase 4 (e2e test): 1 時間
- Phase 5 (ADR 0041): 20 分
- Phase 6 (bd issue refs update): 15 分
- Phase 7 (verify + close): 15 分

合計: 5 時間程度。1 セッションでこなせる規模。
