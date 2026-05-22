# ADR 0039 — Arena translation cost gate (prism→arena)

- Date: 2026-05-23
- Status: Accepted — **GATE PASSED**
- Issue: `murphy-9cr.16`
- Parent: `murphy-9cr` (Plugin 機構 reboot — arena AST + 単一表面 ABI)
- Gated by: ADR 0037 (arena parser-shaped typed AST), ADR 0038 (single-surface plugin ABI)
- Design: `docs/plans/2026-05-22-plugin-reboot-design.md` §3 / §8

## Verdict

**PASS.** 実パイプライン同士で比較したネットの変換オーバーヘッドは
steady-state（8K 行）で **+29.7%**（後退案 thin-wrap prism の 2.322 ms に対し
arena 案 3.012 ms）。設計 §3 の「追加 1 パスの DFS は許容範囲のはず」という
想定の範囲内であり、arena 方式を続行する。後退案（「prism ノードを薄く
ラップ」）は **採用しない**。

## §3 が問うていたこと

設計 §3 は、prism→arena 変換が Murphy の速度目標を侵食しないか「プロトタイプ
で実測」し、ベースライン（prism parse のみ）に対する増分%をゲートにすると
定めた。許容不能なら arena 方式をやめ、prism ノードを薄くラップする方式へ
後退する判断点（§8 リスク 1、本 epic の第一マイルストーン）。

## 計測方法

素の `translate / parse` 比だけでは過大評価になる。`translate` のコストには
「prism 木を 1 回 DFS する分」が含まれるが、**後退案でも dispatch のため
prism 木の走査は不可避**であり、その走査分は arena 固有の純増ではない。
そこで corpus ごとに 4 ベンチを取り、実パイプライン同士で比較する。

- ハーネス: `crates/murphy-translate/benches/translate_cost.rs`（criterion 0.8、
  `cargo bench -p murphy-translate`）。
- `parse`: `prism::parse(source)` のみ（ベースライン）。
- `prism_walk`: `parse` + prism 木の素の DFS（ruby-prism `Visit` トレイト）。
  **＝ 後退案のパイプライン**（parse + dispatch 走査）。
- `translate`: `murphy_translate::translate` = parse + 1 パス DFS 変換 + finish。
- `arena_walk`: `translate` + arena ノード配列のリニアスキャン（各ノードの
  `kind` を読む）。**＝ arena 案のパイプライン**（parse + 変換 + dispatch 走査）。
- 全ベンチとも返り値（`ParseResult` / `Ast`）を毎反復 drop し、解放コストを
  公平に計上。
- corpus: 既存変換テスト fixture 4 本 + `realistic.rb` を ×10 / ×50 連結した
  steady-state 用合成 2 本。
- 環境: AMD Ryzen AI 9 HX 370、8 CPU、19 GiB RAM、Linux 6.8、
  rustc 1.95.0、`bench` プロファイル（optimized）。VM 上のため criterion が
  外れ値を一定数報告。中央値ベースで判定。

## 計測結果

中央値（criterion `estimates.json`）:

| corpus | 規模 | `parse` | `prism_walk` | `translate` | `arena_walk` |
|---|---|---|---|---|---|
| control_flow | 361 B | 5.32 µs | 5.98 µs | 9.81 µs | 9.65 µs |
| method_def | 455 B | 4.93 µs | 4.89 µs | 9.58 µs | 9.47 µs |
| mixed | 648 B | 10.09 µs | 11.24 µs | 19.15 µs | 19.05 µs |
| realistic | 3.49 KB | 44.08 µs | 49.53 µs | 85.65 µs | 84.85 µs |
| realistic_x10 | 34.9 KB | 436.8 µs | 493.1 µs | 666.9 µs | 669.3 µs |
| **realistic_x50** | **174.5 KB** | **2.069 ms** | **2.322 ms** | **3.011 ms** | **3.012 ms** |

派生指標:

| corpus | 粗い増分<br>`(translate−parse)/parse` | arena 構築の純増<br>`(translate−prism_walk)/parse` | **ネット**<br>`(arena_walk−prism_walk)/prism_walk` |
|---|---|---|---|
| control_flow | +84.5% | +72.1% | +61.5% |
| method_def | +94.5% | +95.2% | +93.5% |
| mixed | +89.8% | +78.4% | +69.5% |
| realistic | +94.3% | +81.9% | +71.3% |
| realistic_x10 | +52.7% | +39.8% | +35.7% |
| **realistic_x50** | **+45.6%** | **+33.3%** | **+29.7%** |

## 分析

steady-state（`realistic_x50`、8K 行、固定費が償却された値）で読む。

1. **粗い増分 +45.6% のうち、arena 固有でない分が大きい。** `prism_walk −
   parse` = 0.253 ms は prism 木の素の DFS コストで、後退案でも dispatch の
   ため必ず払う。これを除いた arena 構築の純増（`translate − prism_walk`）は
   parse 比 **+33.3%**。

2. **arena の dispatch 走査はほぼ無コスト。** `arena_walk − translate` は 8K
   行で 0.33 µs（計測ノイズ下限近傍）。フラット POD 配列のリニアスキャンは
   prism 木の DFS（253 µs）と比べ事実上ゼロ。これが「arena 化で実践において
   返ってくる分」。

3. **実パイプライン同士のネット比較 +29.7%。** 後退案（`prism_walk`
   2.322 ms）に対し arena 案（`arena_walk` 3.012 ms）。差 +0.690 ms。
   ゲートが本来見るべき数字はこれ。粗い +45.6% から ~16 ポイント縮む。

4. **コストは入力サイズに O(n)。** `realistic_x10`→`x50` のスケール比は
   `parse` 4.74 倍 / `translate` 4.51 倍（理想 5.0 倍）。超線形な破綻はない。

5. **小ファイル（28〜38 行）の高い増分はファイル単位の固定費＋ノイズ。**
   `translate` には `AstBuilder::new` / `PathBuf` 確保 / `finish` セットアップ
   など O(1) の固定費がある。parse が 5〜10 µs しかない小ファイルではこれが
   増分%を押し上げ、`method_def` では `prism_walk < parse` のような順序逆転
   （ノイズ）も出る。判定は steady-state（`realistic_x50`）で行う。

6. **絶対コストは lint 全体に対し無視できる。** 8K 行ファイルでネット
   +0.690 ms。linter の総時間は cop 実行が支配し、parse/translate はその一部。
   §3.5 のバイナリキャッシュ（murphy-9cr.26）は複数回実行（エディタ / CI /
   pre-commit）で parse+translate を丸ごとスキップでき、繰り返し実行では
   このコストを 0 へ償却する（本 ADR 時点で未実装のため計測対象外）。

## 判定根拠

ネット +29.7%（steady-state、+0.690 ms / 8K 行）は設計 §3 が明示的に予期した
範囲内。さらに:

- arena の dispatch 走査がほぼ無コスト（分析 2）のため、AST 全体の追加走査を
  要する処理（例: パターンマッチャの `` ` `` 子孫探索）が混じる場合、走査
  回数 N に対し arena 案のコストはほぼ定数、後退案は N に比例する。dispatch
  1 パスのみでも純増は +0.690 ms にとどまる。
- arena 方式は単一表面 ABI・`.so` 直読み・バイナリキャッシュ（ADR 0037 /
  0038、設計 §2 / §3.5）の土台であり、後退案ではこれらが成立しない。

以上より arena 方式を続行する。**後退判断は発動しない。**

## 非ブロッキングな観察（follow-up 候補）

- ファイル単位の固定費（分析 5）は、小さいファイルを多数 lint するプロジェクト
  で僅かに効く。`translate` の per-call セットアップ削減は将来の最適化余地だが
  v1 ブロッカーではない。§3.5 キャッシュ（murphy-9cr.26）でも緩和される。
- バイナリキャッシュ実装後（murphy-9cr.26）、cache-hit パス（load + scan）を
  本ハーネスの 5 つ目のベンチとして追加すると、繰り返し実行の実コストを
  数字で裏付けられる。

## 検証実行

ゲートレビューの一環として実行（worktree のため `mise exec` 経由で
ruby/rake を供給、mruby ネイティブビルドを成立させた — murphy-pwh 参照）:

- `cargo bench -p murphy-translate --bench translate_cost --no-run`（ビルド）
- `cargo bench -p murphy-translate`（本計測、上表）
- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`（全グリーン）

## 再現方法

```bash
cargo bench -p murphy-translate
```

`benches/translate_cost.rs` は PASS/FAIL によらず再現可能なゲートとして
リポジトリに残す。prism / 変換層の将来の変更でコスト特性が変わった場合、
同じハーネスで再評価できる。
