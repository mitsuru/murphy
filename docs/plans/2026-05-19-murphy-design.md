# Murphy — 設計書

- 日付: 2026-05-19
- ステータス: 設計確定（ブレスト合意済み）
- 種別: スクラッチ新規プロジェクト（rfmt とは無関係）

## 名前の由来

RuboCop = RoboCop（映画）+ Ruby + Cop。その RoboCop の人間名 **Alex Murphy** から。
「RuboCop 一個師団から軽量・高速な相棒だけを取り出す」意。加えて **マーフィーの法則**
（問題が起こりうる所で起こる＝linter が捕まえる対象）とのダブルミーニング。

## 1. 目的とスコープ

RuboCop の遅さ（Ruby VM 起動 + 数百 cop の全 Ruby 実行 + autocorrect 多段再パース +
GVL で並列頭打ち）を、Rust ネイティブコアで解消する高速 Ruby linter/formatter。

- **非目標**: RuboCop の忠実移植・既存 RuboCop 自作 cop の無改変互換。
- **目標**: Ruff for Ruby 路線。標準 cop はネイティブ再実装、自作 cop は Ruby のまま
  プラグインでき、エンドユーザ/作者にビルドツールチェーンを強制しない。

## 2. 確定した主要決定（ブレスト結果）

| 論点 | 決定 |
|---|---|
| プロジェクト形態 | 完全スクラッチ（rfmt 非依存・コード流用なし） |
| コア言語 | Rust。標準 cop はネイティブ実装・全コア並列 |
| 自作 cop 互換ターゲット | 新軽量 cop API（RuboCop ライクだが非互換）。prism AST を直接訪問 |
| 自作 cop 実行機構 | **in-process 埋込み mruby**（デーモン/IPC/Spinel 不採用） |
| 速度対策 | "fast core, scripted glue"：重い AST 操作は Rust 製 native primitive、mruby は薄いグルーのみ |
| AST 受け渡し | コアで prism 単一パース。共有 in-memory 木を native ハンドル経由で mruby に提示（シリアライズ往復なし） |
| 設定 | 独自フォーマット + `.rubocop.yml` 一方向マイグレーション（`murphy migrate`） |
| cop API 境界 | 言語中立面（native primitive 群＋ビジター）。将来 Rune/Roto 等フロントエンド差替の余地のみ確保（v1 は mruby 単一） |
| サンドボックス | v1 なし（自分で置いた信頼 `.rb` 前提）。第三者配布 cop の sandbox は将来課題 |

### 不採用とその理由
- **Spinel(AOT)**: 速度は最良だが cop 作者に C コンパイラ＋クロスビルドを強制 → 採用障壁。
- **CRuby 埋込み(magnus/rb-sys)**: 完全互換だが GVL・起動コストで高速化動機を殺す。
- **Rune**: ネイティブ非コンパイル（バイトコード VM）。mruby に対する優位が薄く脱落。
- **Mun/Roto**: ネイティブで速いが Ruby 構文を捨てる＋基盤としての成熟度リスク。
- **常駐デーモン + prism シリアライズ転送**: in-process 化で不要となり全廃。

## 3. アーキテクチャ

```
source ─▶ prism parse(1回) ─▶ 共有 AST木(コア内, 不変)
                                  │
                ┌─────────────────┼───────────────────┐
                ▼                                      ▼
        Native cop engine                   Embedded mruby runtime
        (標準cop, Rust, 全コア並列)          (自作cop, .rb をそのまま)
                │                                      │
                │         ┌── Rust製 native primitives ─┘
                │         │   (ノード走査/パターン照合/位置計算を公開)
                ▼         ▼
              Offense Aggregator ─▶ 出力 / autocorrect
```

主要コンポーネント:
1. **Core**: prism 単一パース、ファイル発見（独自設定の include/exclude）、オーケストレーション。
2. **Native cop engine**: 標準 cop の Rust 実装。共有 AST に対しマルチコア実行。
3. **Embedded mruby runtime**: `cops/*.rb` をそのまま解釈。cop 単位で独立 mruby state。
4. **Native primitives**: AST 走査・ノードパターン照合・位置/レンジ計算を mruby へ公開。
5. **Offense Aggregator**: native/自作の offense をマージ・重複排除・優先度解決・出力・autocorrect。

## 4. 自作 cop の書き味（例）

```ruby
# cops/no_puts.rb
class NoPutsCop < Murphy::Cop
  MSG = "Use a logger instead of puts"

  def on_call_node(node)
    return unless node.name == :puts && node.receiver.nil?
    add_offense(node.message_loc, message: MSG) do |fix|
      fix.replace(node.message_loc, "logger.info")
    end
  end
end
```

- 基底 `Murphy::Cop` と `on_<prism_node_type>` ビジターを SDK が提供。
- `add_offense(range, message:)` ＋ `fix.replace/insert/remove(range, str)` のみ。
- **read-only 走査 ＋ テキスト編集提案**（AST 書換不可）。
- ノードパターン DSL は v1 では出さない（YAGNI）。素の述語で書く。
- 作者は `.rb` を `cops/` に置くだけ。ビルド・ツールチェーン不要。

## 5. データフロー

1. 対象ファイル決定（独自設定の include/exclude、`.murphyignore`）。
2. prism で1回パース → 共有 AST 木（不変）。`source_digest` をキャッシュ鍵に。
3. 分岐: ネイティブ経路（標準 cop, 並列）／ mruby 経路（自作 cop, native primitive 経由で同じ木を走査）。
4. offense は構造化で返す:
   `{file, cop_name, range:{start_offset,end_offset}, severity, message, autocorrect?:{edits:[{range, replacement}]}}`
5. Aggregator が収集 → 重複排除・優先度・severity 解決 → 出力。
6. autocorrect: 編集をオフセット降順で衝突検出しつつ適用。衝突は不適用＋競合ログ。
   変化あれば再パース→再実行（最大反復で打切り、振動時は最終状態＋警告）。

## 6. エラーハンドリングと隔離

| 障害 | 方針 |
|---|---|
| 自作 cop が Ruby 例外 | mruby が捕捉、その cop×そのファイルだけ `error offense` 化し継続 |
| 自作 cop 暴走 | ファイル単位の実行ステップ/時間デッドライン（mruby 命令フック）。超過で中断・警告・スキップ |
| mruby state 破損 | cop 単位の独立 mruby state。壊れたら state 破棄・再生成。コア/native cop は別メモリで無影響 |
| 構文エラーの対象 Ruby | 1 offense 報告、そのファイルは cop 実行スキップ、他継続 |
| autocorrect 衝突 | 降順適用＋重なり検出、衝突は不適用＋競合ログ、収束は最大反復で打切り |
| 終了コード | `0`=違反なし / `1`=違反 / `2`=設定・cop構成エラー / `3`=内部障害 |

観測性: `--debug` で cop 毎の処理時間・デッドライン超過・例外。

## 7. テスト戦略

| 層 | 対象 | 方法 |
|---|---|---|
| Native cop | 検出/autocorrect | テーブル駆動（入力, 期待offense, 期待修正後ソース） |
| mruby cop API | native↔mruby 境界 | 最小 `.rb` cop で走査/add_offense/fix を検証 |
| エンジン統合 | 単一パース→並走→集約 | プロジェクト fixture の snapshot 比較 |
| autocorrect 収束 | 競合・反復・振動 | 衝突ケース＋**冪等性必須**（修正後再投入で無変化） |
| 隔離/堅牢性 | 例外/無限ループ | 故意に落ちる cop で error 化・全体継続・終了コード検証 |
| 設定 | 独自＋`.rubocop.yml` 移行 | `migrate` のラウンドトリップ検証 |
| 性能回帰 | スケール特性 | 第三者コーパスで hyperfine を CI 化。N=1/20/100 と RuboCop 比を記録 |
| 差分品質 | 現代 Ruby 標準への近さ | RuboCop(-a) との出力差分を定点観測 |

TDD: cop は必ず「失敗する fixture テスト → 実装」。autocorrect は冪等性テストを先に固定。

## 8. 未決・将来課題

- 標準 cop の v1 スコープ（採用率上位の Layout/Style/Lint をどこまで）。
- cop API 境界の正式 IDL（native primitive シグネチャ一覧）。
- prism Rust バインディングの選定と AST ハンドルの mruby 公開方式の PoC。
- 第三者配布 cop のサンドボックス（seccomp 等）。
- LSP 連携。
