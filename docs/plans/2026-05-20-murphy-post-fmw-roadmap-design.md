# Murphy Post-`murphy-fmw` Roadmap Design

Date: 2026-05-20
Status: brainstorm-result (pre-ADR)
Scope: 機能追加検討 — Phase 8 以降

---

## 1. Context & Scope

`murphy-fmw` エピック (Phase 1–7) 完了時点を起点とした roadmap 検討。
Phase 7 完了時の Murphy は次の状態にある想定:

- `murphy lint`/`--fix`/`migrate` + `murphy.toml` 設定
- v1 標準 cop suite + hyperfine perf-regression CI
- embedded mruby user cop + per-cop 隔離 + deadline
- LSP 統合、サードパーティ cop サンドボックス、代替フロントエンド (Rune/Roto)

そこから先の「機能追加」を 3 軸で検討した結果を本ドキュメントに残す。
各機能は ADR 化前のブレインストーム成果であり、Phase 化したものの実装着手前に
個別 ADR (`docs/decisions/`) で改めて意思決定すること。

明示的に**スコープ外**:

- Ruby 以外への言語拡張 (Crystal, Sorbet 等)
- ERB/HAML/RSpec 専用 cop の Murphy 本体組込み
  (※ 後述 A6a プラガブル cop パックとしてサードパーティ実装はあり得る)
- 決定的フォーマッタ (A1 却下、§2.1 参照)

---

## 2. Axis Decisions

ブレインストームの結果、機能追加の軸を 3 つに整理した。
判定凡例: ✅ やる / 📌 積む (バックログ) / ❌ 却下。

### 2.1 A. 拡張プラットフォーム化 (旧称: コア機能の深化)

Murphy 本体を「単体 linter」から「**拡張可能なプラットフォーム**」に進化させる軸。
最優先。下記が確定:

| ID | 判定 | 内容 |
|----|------|------|
| A1 | ❌ | 決定的フォーマッタ — Rails プロダクトは寿命が長く歴史的にフォーマットが一意に決まらない。`rfmt`/`rubyfmt` が定着しない理由でもある。RuboCop の opt-in/out モデルの方が Ruby らしい |
| A2 | 📌 | cross-file 解析エンジン (定数解決、参照グラフ、未使用検出) |
| A3 | 📌 | RBS/Sorbet sig 取り込み (型ヒント由来の cop) |
| A4 | 📌 | 賢い autocorrect (multi-cop conflict 解消) |
| A5 | ✅ | 永続キャッシュ + インクリメンタル (`murphy-fvh` を繰り込み) |
| A6a | ✅ | プラガブル cop パック配布 (`murphy-rails`, `murphy-rspec` 風) |
| A6b | ✅ | native cop の plugin 機構 (Rust cop の動的拡張) |
| A6c | ✅ | mruby cop テスト基盤 (RuboCop scaffold 相当) |

積みタスク (A2/A3/A4) は bd に backlog 化し、本 roadmap の Phase には載せない。
A6a と組み合わさることで、A2/A3/A4 はサードパーティパックとしての実装も可能になる。

### 2.2 B. 開発者体験 (DX)

全採用。AI 連携を見据えた structured 出力が新しい設計観点。

| ID | 判定 | 内容 |
|----|------|------|
| B1 | ✅ | watch / daemon モード (A5 永続キャッシュ前提) |
| B2 | ✅ | IDE / LSP 高度化 (code action / quick fix / 範囲 lint) |
| B3 | ✅ | baseline / suppress ファイル (形式は **TOML**、`murphy.toml` と整合) |
| B4 | ✅ | オフェンス説明 (**AI 向け** rationale / fix example / docs URL を structured 出力) |
| B5 | ✅ | CI / PR 統合 (`--since=<ref>`, GitHub Actions reusable workflow、SARIF アップロード等) |
| B6 | ✅ | プロファイラ (`--profile` で cop 単位の所要時間、ホットファイル検出) |
| B7 | ✅ | 出力フォーマッタ拡張 (`--format checkstyle\|sarif\|junit\|github\|gnu\|tap`、ADR 0006 デフォルト JSON は不変) |
| B8 | ✅ | pre-commit / git hook 統合 (`murphy install --git-hook`、lefthook / pre-commit / overcommit テンプレート) |
| B9 | ✅ | レポート出力 (`--format html`, `--format markdown` — 人間 / PR レビュー用、rubycritic 相当) |

### 2.3 C. エコシステム

全採用、優先度低。3 軸の中では最後。

| ID | 判定 | 内容 |
|----|------|------|
| C1 | ✅ | cop registry / 公式ディレクトリ |
| C2 | ✅ | Gem 配布 + Bundler 統合 (multi-platform native gem) |
| C3 | ✅ | プリセット / 設定プロファイル (`extends = "murphy:rails-strict"`) |
| C4 | ✅ | cop バージョニング / 互換性ポリシー |
| C5 | ✅ | `murphy init` / セットアップ UX |
| C6 | ✅ | ベンチマーク公開 (Phase 6 perf-CI の継続出力) |

---

## 3. Dependency Map

```text
                     [Phase 6 完了]
                          │
                          ▼
                     [perf-CI 数値] ──────────────── C6 ベンチ公開
                          │
                          ▼
                     [Phase 7 完了]
                          │
                          ├──→ LSP ───────────────── B2 LSP 高度化
                          │
                          ▼
                     ┌─ Phase 8 (拡張基盤) ─┐
                     │                       │
                     │  ┌── A6b native ──┐   │
                     │  │     plugin     │   │
                     │  └────────┬───────┘   │
                     │           ▼           │
                     │       A6a cop パック ─┼──→ Phase 6 標準 cop の再パッケージ化
                     │           │           │
                     │           ▼           │
                     │       C4 バージョニング│
                     │                       │
                     │       A5 永続キャッシュ│
                     │           │           │
                     │       A6c mruby DSL   │
                     └───────────┼───────────┘
                                 │
            ┌────────────────────┼─────────────────────┐
            ▼                    ▼                     ▼
       B1 watch/daemon    B3 baseline             C2 Gem 配布
       (A5 前提)          B4 explain AI                │
                          B5 CI/PR/SARIF              ▼
                          B6 profiler             C1 registry
                                                  C3 プリセット (A6a)
                                                  C5 murphy init
```

クリティカルパス: `A6b → A6a → C4 → 標準 cop 再パッケージ化`。
A5 と A6c は A6 系と並走可能。

---

## 4. Phase Breakdown

### 4.1 Phase 8: 拡張プラットフォーム基盤

Murphy を「拡張可能な土台」に作り変える。一体でリリース。

**含む**:

- A5 永続キャッシュ (`source_digest` 永続化、AST/解析結果のディスクキャッシュ)
- A6a プラガブル cop パック配布 (cop ID namespace、依存定義、ロード機構)
- A6b native cop plugin 機構 (Rust cop の組込み vs 動的ロードの方針確定)
- A6c mruby cop テスト基盤 (`murphy new-cop`, `describe_cop` 的 DSL)
- C4 cop バージョニングポリシー (A6a の配布契約)

**Phase 8 ゲート (= 成功基準)**:

1. Phase 6 標準 cop が「内蔵 cop パック」として再パッケージ化されている
2. 外部 cop パック (PoC として `murphy-example-pack`) が 1 つロード&実行できる
3. 永続キャッシュにより `murphy lint` の連続実行で 2 回目以降が 5× 以上高速
4. `murphy new-cop` で雛形生成→テスト→pack 化のサイクルが回る
5. A6a 配布契約 (cop ID 安定性、severity デフォルト変更ルール、設定 additive 原則)
   が ADR 化されている

**設計上の論点 (ADR 化対象)**:

- A6b: native cop の動的ロードを許すか (security / 配布形態に影響)
  - 案 1: コンパイル時 feature flag による静的リンクのみ (安全、配布パック ≒ Murphy ビルド)
  - 案 2: cdylib による動的ロード (柔軟、ABI 安定化が必要)
  - 案 3: WASM / 限定 IDL 越し (中庸、性能オーバヘッド要評価)
- A5 キャッシュ key: `source_digest` だけで足りるか、cop パックバージョン + 設定も混ぜるか
- 標準 cop 再パッケージ化の移行: cop ID は維持し、内部実装の所属だけ変える方針

### 4.2 Phase 9: DX 一周

Phase 8 と一部並走可。B1 のみ A5 完了が前提、それ以外は独立。

**含む**:

- B1 watch / daemon (A5 完了後)
- B3 baseline TOML (`.murphy-baseline.toml` 仮称)
- B4 AI 向け explain (offense JSON に rationale/fix_example/docs_url、`--explain ID` で人間可読)
- B5 CI/PR 統合 (`--since=<ref>`, GitHub Actions reusable workflow、SARIF アップロード等)
- B6 profiler (`--profile`, cop × file マトリクス)
- B7 出力フォーマッタ拡張 (Checkstyle XML / SARIF / JUnit XML / GitHub annotation / gnu / tap)
- B8 pre-commit / git hook 統合 (`murphy install --git-hook` + lefthook/pre-commit/overcommit テンプレート)
- B9 レポート出力 (HTML / Markdown — PR レビュー / ダッシュボード用)
- B2 LSP 高度化 (code action, quick fix, 範囲 lint — Phase 7 LSP 完了が前提)

**Phase 9 ゲート**:

1. `murphy watch` で常駐起動、ファイル保存→差分 lint が体感即時
2. `.murphy-baseline.toml` を使ったレガシー導入が公式 docs の手順で完結
3. offense JSON に `documentation_url` + `rationale` フィールドが入り、
   `--explain <cop_id>` が docs URL と例示を返す (AI 連携前提)
4. GitHub Actions reusable workflow が公開され、SARIF が GitHub Code Scanning に上がる
5. `--profile` が cop 単位の wall time + p95 を JSON で出す
6. `--format <name>` で Checkstyle XML / SARIF / JUnit XML / GitHub annotation / gnu / tap の 6 種が選択でき、デフォルト JSON (ADR 0006) は不変
7. `murphy install --git-hook` が pre-commit/lefthook/overcommit いずれの構成にも雛形を出せる
8. `--format html` / `--format markdown` で人間レビュー用レポートが生成できる

**設計上の論点**:

- B3 baseline の granularity: ファイル + cop ID 単位か、行レベルか
- B4 AI 向け出力: 既存 offense JSON の拡張か、`--explain` 専用フォーマットか
  → 後者 (既存契約を壊さない、ADR 0006 の凍結契約を尊重) を推奨
- B2 LSP は Phase 7 終わり方による。先にコミュニティ需要を見てから着手

### 4.3 Phase 10: エコシステム整地

Phase 8 完了後。優先度低、ただし採用拡大のためには必要。

**含む**:

- C2 Gem 配布 + Bundler 統合 (multi-platform precompiled gem)
- C1 cop registry (Gem の上に Murphy 専用メタデータ層を被せる)
- C3 プリセット (`extends = "murphy:rails-strict"`, `"minimal"`, `"recommended"`)
- C5 `murphy init` (既存リポへの導入 UX)
- C6 ベンチマーク公開 (Phase 6 perf-CI の数値を README/サイトに継続出力)

**Phase 10 ゲート**:

1. `gem install murphy` で公式 gem が入る (x86_64-linux, aarch64-linux, x86_64-darwin, arm64-darwin)
2. `bundle exec murphy lint` が gem からの cop パック発見を含めて動く
3. `murphy add murphy-rails` 相当のコマンドで registry からパック追加が完結
4. `murphy init` 後の `murphy lint` が 5 秒以内に妥当な出力を返す (既存 Rails アプリ前提)
5. ベンチマーク結果が公式 docs から閲覧可能

**設計上の論点**:

- C1 registry が必要か、Gem 配布 (C2) + `Gemfile` 経由の発見で済ませるか
  → まず C2 を先行し、registry が要るかは導入実績を見て判断
- C3 プリセットの配布形態: 公式 cop パックの一部か、独立した「設定パック」か

---

## 5. Parallelization & Risks

### 並走

- Phase 8 中: B3/B4/B5/B6 は Phase 9 から前倒し可能 (独立性が高い)
- Phase 8 中: B2 LSP 高度化は Phase 7 LSP の完成度次第で前倒し可能
- Phase 10 内: C2 ↔ C5 ↔ C6 は並走可、C1/C3 は C2/A6a 後

### リスク

1. **標準 cop 再パッケージ化のリグレッション**: Phase 6 で詰めた cop の実装が
   内蔵パックへ移動する際、ID/severity/出力が変わらないことを snapshot で守る必要。
2. **A6b native plugin の ABI 凍結**: Rust の ABI 不安定さで動的ロード方針は
   メンテ負担が大きい。**静的リンク (案 1) を強く推奨**、動的は将来課題。
3. **A5 キャッシュ key の取り違え**: cop パックバージョンを混ぜ忘れると、
   パック更新後も古い結果が返る "stale-cache" が起きる。設計時に明示。
4. **B4 AI 向け出力の prompt-injection 面**: offense rationale が外部入力
   (ソースコード) に依存しないこと。固定文 + テンプレートに限る。

---

## 6. Open Questions

- A6b の静的 vs 動的 plugin 方針 (上記推奨は静的、ADR で確定)
- C1 registry の必要性 (Gem だけで済む可能性が高い)
- Phase 8 から Phase 6 標準 cop 再パッケージ化への移行に ADR 0018 の改訂が要るか
- B3 baseline と `.murphyignore` の責務分担 (ファイル単位の無視 vs cop 単位の凍結)

---

## 7. 別 brainstorm 候補 / MVP 補完 (本 roadmap 外で扱う)

本 Phase 8/9/10 とは別ライフサイクルで扱う案件。それぞれ後日独立の brainstorm
セッションを立てるか、`murphy-fmw` 内に補完 issue として吸収する。

- **MVP-Z1: Inline disable / enable / todo comment**
  (`# murphy:disable Cop/Name`, `# murphy:enable`, `# murphy:todo`)
  現状 Murphy に未実装・未 issue 化。**MVP 必須機能**として `murphy-fmw` 完了前に
  別 issue 化し、Phase 6 と並走で入れる。post-fmw roadmap には載せない。
- **D 軸候補: Brakeman 互換 SAST パック**
  cross-file 解析 + taint tracking + Rails-aware project model を要する重量級拡張。
  A6a プラガブル cop パック機構の上に「重量級パック」として乗せる前提なら自然。
  詳細は別 brainstorm。
- **D 軸候補: メトリクス系 cop (Reek / rubycritic 相当)**
  循環的複雑度、メソッド長、ABC、code smell 等。Phase 6 ADR 0018 v1 cop suite に
  metrics 系が含まれるかを先に確認し、なければ別 brainstorm で軸 D に統合 or 独立。

明示的に**検討除外**とした候補 (本 brainstorm 時点での判断):

- C7 NewCops 取り扱いポリシー (`pending`/`enable`/`disable` デフォルト) — C4 で
  必要なら扱うが現時点で独立項目化しない
- Markdown 内 Ruby コードブロック lint (rubocop-md 相当) — サードパーティ
  パックに委ねる
- `inherit_gem` / 設定継承 — C3 プリセットに包含済み
- 共通 AST helper / DSL — A6c mruby DSL + A6b native plugin に包含
- safe vs unsafe autocorrect 区別 — A4 (積み) の範囲
- diff-driven runner (Pronto / Danger) — B5 `--since=<ref>` で代替

---

## 8. Next Actions

1. 本ドキュメントをコミットし、議論の出発点とする
2. `bd` で次の issue 階層を作る:
   - `Phase 8: 拡張プラットフォーム基盤` (epic)
     - A5 / A6a / A6b / A6c / C4 をサブタスク化
   - `Phase 9: DX 一周` (epic)
     - B1〜B9 をサブタスク化、依存 (B1→A5) を張る
   - `Phase 10: エコシステム整地` (epic)
     - C1 / C2 / C3 / C5 / C6 をサブタスク化、依存 (Phase 10→Phase 8) を張る
   - 既存 `murphy-fvh` (Post-Phase-2 永続キャッシュ) を A5 に統合 / リンク
   - **MVP-Z1 inline disable comment** を `murphy-fmw` 配下の MVP 補完 issue として
     別途起こす (Phase 6 並走想定)
   - **Brakeman 互換 SAST パック** brainstorm placeholder issue
   - **D-met メトリクス cop** brainstorm placeholder issue
3. Phase 8 着手前に下記 ADR を起こす (Phase 6 ADR 0018 の流儀に倣う):
   - native plugin 方針 (A6b)
   - cop パック契約 (A6a + C4)
   - 永続キャッシュ key 設計 (A5)
4. Phase 6 完了タイミングで本 roadmap を再評価
   (perf-CI 数値と diff-quality watch の結果を踏まえる)
