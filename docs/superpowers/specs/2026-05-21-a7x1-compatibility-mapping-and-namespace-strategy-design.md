# 2026-05-21 A7x.1: Cop ID・設定・ネームスペース戦略（RuboCop 互換）

## 1. 目的

`murphy-4n9.1`（A7x.1）で、`murphy-4n9` の次の実装方針を固定する。

- コア/既存パックの cop 名を **RuboCop 互換のID**で扱う
- cop 設定キー（`[cops.rules]`）を Ruby 既存ユーザー期待に合わせた形で統一する
- `rubocop-rails` / `rubocop-rspec` 系に備え、将来のパック namespace 戦略を先に決める

最終方針は、次の実装チケット（`murphy-4n9.2`〜`murphy-4n9.4`）で「パック化」と「挙動移植」に反映する前提をつくる。

## 2. 設計方針（概要）

まずは **互換の明示**を優先し、`Murphy` 独自拡張と区別して管理する。

- 既存ネイティブ16件は、原則 RuboCop 名を**正規 ID**として扱う。
- 既存の `Murphy/<CopName>` は原則として「Murphy 独自互換」として残す（同名を新規採用しない）。
- `cop` 設定は、`[cops.rules]` 配下のキーを `Cop/Name`（例: `Layout/TrailingWhitespace`）で扱う。
- パック境界では名前空間を固定し、将来 `Rails/*`、`RSpec/*` を衝突なく同居可能にする。

## 3. 互換マッピング

### 3.1 コアルール（v1 の最初の公開マッピング）

現時点（`native_cops_list`）の16件を以下の2分類にする。

- **直接互換（same-id）**: ID と表示名を RuboCop 名と同じ文字列で維持。
- **独自互換（custom）**: `Murphy/...` のまま残し、RuboCop 本体の同名がないもの。

#### 直接互換

- `Lint/Debugger`
- `Lint/DeprecatedClassMethods`
- `Lint/EmptyWhen`
- `Lint/UnreachableCode`
- `Style/AndOr`
- `Style/FrozenStringLiteralComment`
- `Style/IfUnlessModifier`
- `Style/NilComparison`
- `Style/RedundantReturn`
- `Style/StringLiterals`
- `Style/SymbolArray`
- `Style/WordArray`
- `Layout/EmptyLines`
- `Layout/SpaceInsideParens`
- `Layout/TrailingWhitespace`

#### 独自互換

- `Murphy/NoReceiverPuts`

### 3.2 実装へ反映する必須ルール

- `native_cop_names()` の ID を上記 ID の正規表記として扱う。
- `cops.rules` のキーは将来変更しない（`[cops.rules."Lint/Debugger"]` のように完全ID）。
- ドキュメント上、将来の `rubocop` 互換移行では `Murphy/NoReceiverPuts` を「非互換ではなく独自拡張名」と明記する。
- `Murphy/NoReceiverPuts` は **コアインフラ cop**として維持し、`A7x` の今回の plugin 化対象（`A7x.2`〜`A7x.4`）から除外する。
- 既存の `NoReceiverPuts` については、可能なら RuboCop 相当の `Style/`/`Lint/` 置換を別チケットで検討し、いきなりリネームは避ける（壊れやすいため）。

## 4. 名前空間戦略（rails / rspec パック前提）

### 4.1 推奨 namespace の原則

- `namespace` は cop ID の先頭部分を固定:
  - Core/Builtin: `Murphy`, `Lint`, `Style`, `Layout`（現行互換）
  - Rails系: `Rails`（例: `Rails/HasAndBelongsToMany`）
  - RSpec系: `RSpec`（例: `RSpec/Capybara`）
- これらは **同名衝突回避**が容易なため、後方互換的。

### 4.2 同名衝突ポリシー

- 同一ランタイム内で同一 Cop ID は許可しない。
- `murphy-core` の既存 duplicate check（`validate_plugin_cop_ids`）を拡張対象にしない（実装は既存検証に追従）。
- ユーザーは `cop_packs` の順序でロード順を決めるため、同一ID衝突は早期失敗させる。

### 4.3 後方互換ガイド

- パック移行では「旧ID廃止」ではなく「新ID追加＋将来エイリアス対応」を前提にする。
- v1 はエイリアス機構を追加しない（複雑化防止）。ただし、将来版では A7x.2 のレビュー結果を踏まえて追加可否を再検討する。

## 5. 設定互換の実装方針

### 5.1 対応範囲

`Murphy` の既存 TOML スキーマを温存し、以下のみを明文化する。

- 既存キー: `enabled` / `severity`
- キー名: コップ ID（RuboCop ID）

### 5.2 既知の違い（明記）

- `.rubocop.yml` からの `migrate` は引き続き **one-way（片方向）**。
- RuboCop 側の拡張キー（`Max`, `Exclude`, 例外除外/include など）はこの段階では一律受けない。
- 既存ルールが未対応でも、**ID/設定キーの書式**は rubocop 互換を優先して維持する。

## 6. テスト/検証観点

- マッピング表の正しさをドキュメント化（この仕様書）と、簡易の static test 追加を想定。
- 設定キーが完全IDで参照される回帰テスト（`cops.rules."Lint/Debugger"` など）を追加。
- パック namespace の衝突エラー（`RuboCop` 系と `murphy-` 独自の同名ID競合）を、既存 duplicate 検証で担保。

## 7. 選定した実装アプローチ

提案は次を採用する。

1. **Cop ID/設定はまず既存 ID を正規化せずに凍結**（16件の現行IDを真っ先に準拠）
2. **namespace を `Murphy` / `Lint` / `Style` / `Layout` を維持し、将来 `Rails` / `RSpec` を追加**
3. **`NoReceiverPuts` は現行 `Murphy/NoReceiverPuts` を継続**（同等の RuboCop 標準ID移行は別チケット）

この方針で、`murphy-4n9.2`（標準パック抽出）と `murphy-4n9.3`/`4.4`（Rails/Spec パック）に進める。

## 8. A7x.1 スコープ外（次項目への引き継ぎ）

- `NoReceiverPuts` の RuboCop 置換を標準 cop として採択するかどうか
- rails/rspec cop の実際の挙動差分検証（`RuboCop` との差分計測）
- エイリアス（旧ID→新ID）と `NewCops`/`EnabledByDefault` 相当の実装
