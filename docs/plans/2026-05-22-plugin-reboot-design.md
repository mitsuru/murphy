# Murphy プラグイン機構 reboot — 設計

**Status**: 設計合意 2026-05-22。`2026-05-22-ast-representation-strategy.md`
を実装可能な設計へ具体化したもの。murphy-9cr epic を全面再構成する。
§9 step 1 の形式 ADR は ADR 0037(arena parser-shaped typed AST)・
ADR 0038(単一表面プラグイン ABI)として起票済み(murphy-9cr.13)。

この設計は AST 戦略メモの方向(arena・parser-shaped・typed AST、Route B)を
引き継ぎつつ、次の点を確定・修正する:

- AST 表面を **単一表面** に統一(標準 cop もプラグインも同一 API)。
- 戦略メモの「node-pattern DSL は first-class な必須コンポーネント」を
  **「1 文法・2 バックエンド(コンパイル時 lowering + ランタイム matcher)」**
  として具体化。独立したランタイム DSL エンジンは作らない。
- arena のバイナリキャッシュを設計目標に加える。

## 前提: spike を捨てて greenfield で作り直す

現行のプラグイン実装(`MurphyNodeContext` / `MurphyPluginCopV1` / 現 `Cop`
トレイト等)は **ABI フリーズ前の spike レベル**。後方互換・移行シムは
**一切考えない**。綺麗さを最優先に再設計する。

ABI フリーズ規律(additive-only、予約パディング等)は reboot の制約ではなく、
**v1 出荷時に初めて適用する将来規律**として後ろに置く。

## 1. 全体アーキテクチャとクレート構成

以下は **reboot 完了後の最終アーキテクチャ**(本 reboot epic `murphy-9cr`
で全部到達するわけではない。`murphy-rails` の `.so` 化と 131 cop 移植は
follow-up epic `murphy-au8` の責任 ── §5/§6 で詳述)。

```text
source
 → prism parse (1回)
 → murphy-translate : prism AST → arena AST          ← 新規
 → murphy-ast       : arena/parser-shaped/typed AST  ← 単一の共有AST
 → dispatch (arena を1パス走査)
     ├─ 標準cop (Murphy/Lint/Style/Layout) ─ murphy-plugin-api 経由 (静的リンク)
     ├─ .so プラグイン (murphy-rails 等)   ─ murphy-plugin-api 経由 (同一表面、au8 で到達)
     └─ .rb ユーザーcop                   ─ embedded mruby + ランタイムmatcher(C)
 → offense 集約 → 出力 / autocorrect
```

| クレート | 区分 | 役割 |
|---|---|---|
| `murphy-ast` | 新規 | arena AST 本体。`Ast` arena・`NodeKind` enum・`NodeId`・走査API・interner・comment list・source buffer |
| `murphy-translate` | 新規 | prism AST → murphy-ast 変換層。`Prism::Translation::Parser` の Rust 版。1ファイル1パス |
| `murphy-pattern` | 新規 | S式パターンの文法・パーサ。B/C 両バックエンドが共有。ランタイム用 `Pattern` IR も |
| `murphy-plugin-api` | 改修 | cop が AST を読む唯一の表面。arena を直読み。`Cop`/`NodeCop` を arena 向けに再定義 |
| `murphy-plugin-macros` | 改修 | `node_pattern!`(B)を追加。`register_cops!`・`#[derive(CopOptions)]` は存続 |
| `murphy-core` | 改修 | 「共有不変AST」が `ruby_prism::Node` → `murphy-ast::Ast` に置換。標準 cop は `murphy-std` に分離し、本 crate はエンジン専業 |
| `murphy-std` | 新規 | 標準 cop pack (Murphy/Lint/Style/Layout、ADR 0018) を分離した crate。Murphy 依存は `murphy-plugin-api` 1 本のみ ── 単一表面 ABI をコンパイラ境界で強制する。`murphy-cli` に静的リンクされる |
| `murphy-rails` | 改修 (**au8 epic で実施、reboot epic ではスコープ外**) | 動的 `.so` プラグインパック化 ── murphy-plugin-api を消費し、`[[plugins]] path = "..."` 経由でロードされる(全ユーザー必須ではない opt-in pack)。reboot 本体完了時点では依然 text-matching 経路で動作。 |

> `murphy-pattern` を他クレートで消費する見込みが薄ければ、独立させず
> `murphy-plugin-api` に畳む案もある(その場合は新規 2 クレート)。

## 2. arena AST (`murphy-ast`)

```rust
// 1ファイル分を所有するアリーナ
pub struct Ast {
    nodes:      Vec<AstNode>,   // 全ノード。NodeId = この添字
    node_lists: Vec<NodeId>,    // 可変長の子(args 等)の側テーブル
    interner:   Interner,       // Symbol / StringId
    comments:   Vec<Comment>,
    source:     SourceBuffer,
    file:       PathBuf,
}

#[repr(C)]
pub struct AstNode {
    kind:   NodeKind,           // payload 付き enum
    parent: NodeId,             // 根は番兵
    range:  Range,              // (u32 start, u32 end)
}

#[repr(C, u8)]                  // 判別子 u8 固定 → レイアウト安定
pub enum NodeKind {
    Send  { receiver: OptNodeId, method: Symbol, args: NodeList },
    Const { scope: OptNodeId, name: Symbol },
    If    { cond: NodeId, then_: OptNodeId, else_: OptNodeId },
    Int(i64), Str(StringId), /* … ~100 variants … */
}
```

- **固定長ノード。** `AstNode` は最大 variant サイズの固定長。`Vec<AstNode>`
  はフラットなバイト列 ── これが単一表面・`.so` 直読み・バイナリキャッシュ
  すべての土台。
- **可変長の子は `NodeList = (u32 start, u32 len)`** で `node_lists` を参照。
  ノード内に `Box`/`Vec`/ポインタを持たない。
- **`NodeId = u32`**、`OptNodeId` は番兵値 `u32::MAX` を `None` とする
  (`Option` のニッチに頼らず ABI で明示)。
- **走査API**: `parent()` / `children()` / `ancestors()` / `descendants()`
  イテレータ。`parent` フィールドが両方向リンクを保証 ── 戦略メモの
  ~40 traversal cop(`each_ancestor` 等)を満たす。
- **エラーノード**: prism のパースエラーは `NodeKind::Error` に対応付け、
  dispatch は素通しする(構文エラーで cop を落とさない)。

**シリアライズ可能性を v1 設計目標に固定**: ノードは POD、側テーブルは
フラット、interner はオフセット配列でブロブ化。3.5 のキャッシュを可能にする。

**ABI 凍結の性質(将来規律)**: payload 付き `#[repr(C, u8)]` enum は
レイアウトこそ定義されるが、variant の追加・並べ替え・フィールド変更は
すべて破壊的 ABI 変更になる。よって `NodeKind` の variant 集合は **v1 で
決め切る** 価値がある。**v1 は parser-gem 準拠**(Route B = 機械的移植が
目的なので命名と子レイアウトを parser に揃える)。出荷後の規律は ADR で
別途定める。

## 3. 変換層 (`murphy-translate`)

prism AST → arena AST を **1ファイル1パス** で構築する。
`Prism::Translation::Parser` の Rust 版。

```text
prism parse
 → prism の Visit で木を1回 DFS
 → 各 prism ノードを arena ノードへ翻訳
     ├─ NodeKind を parser-shaped へマッピング(collapse/split)
     ├─ AstNode を nodes.push、返り値 NodeId
     ├─ 子の NodeId を node_lists へ詰める
     └─ parent を後埋め(再帰の戻りで設定)
 → comments / source / file をそのまま Ast へ移送
```

**collapse/split の吸収点。** prism と parser-gem はノード分割が異なる
(murphy-9cr.1 の差分表)。この差を **変換層の内部だけで吸収** するのが
Route B の肝。cop 側はこの差分を一切見ず、「RuboCop cop が前提とする形」
だけが見える。

**翻訳コストは計測必須**(戦略メモの open question)。想定: prism parse
自体が高速なので追加 1 パスの DFS + `Vec::push` は許容範囲のはず。だが
Murphy の存在意義が速度なので、**プロトタイプで実測** し、ベースライン
(prism parse のみ)に対する増分%をゲートにする。許容不能なら「翻訳を
やめ prism ノードを薄くラップ」へ後退する判断点。

## 3.5 arena のバイナリキャッシュ

arena は POD ノードのフラット配列 + フラットな側テーブルなので、実体が
ほぼそのままシリアライズ形式になる(rust-analyzer / Ruff のキャッシュと
同じ構図)。

```text
cache file = [header] [nodes] [node_lists] [interner blob] [comments] [source]
  header: magic, format-version, murphy-version, content-hash, target-triple
```

- **キャッシュキー**: ファイル内容ハッシュ + Murphy バージョン + 変換層
  バージョン。一致すれば **prism parse と変換層の両方をスキップ** する。
  リンタはエディタ/CI/pre-commit で同じファイルを何度も走るので hit 率が高い。
- **ロード**: 最小実装は各セクションを `Vec` へ memcpy するだけ。さらに
  `AstNode` を 8B アラインで配置すれば **mmap ゼロコピー** まで伸ばせる。
- **マシンローカル前提**: `i64` やパディングが target 依存なのでキャッシュは
  使い捨て。`format-version` / `target-triple` 不一致なら黙って再生成。
- **変換コスト問題への保険**: §3 の実測が渋くてもキャッシュが複数回実行を
  またいで償却する。

**スコープ**: シリアライズ可能性は murphy-ast の v1 設計目標として固定。
キャッシュ機能本体(キー・無効化・CLI 統合)は reboot epic 内の **後段
fast-follow サブタスク**(v1 ブロッキングではない)。

## 4. パターン機構 (`murphy-pattern` + B/C)

文法・パーサは 1 つ、バックエンドが 2 つ。

```text
pattern source  "(send nil? :puts $...)"
 → murphy-pattern: パーサ → PatternAst (パターン自身のAST)
     ├─ B backend  (node_pattern! proc macro / murphy-plugin-macros)
     │    PatternAst → Rust TokenStream (match / if-let / loop)
     │    コンパイル時 lowering、capture は型付きで束縛
     │    → Rust 標準cop + .so プラグイン
     └─ C backend  (murphy-pattern ランタイム)
          PatternAst → Pattern IR (コンパクトなデータ)
          ランタイム interpreter が arena を歩く
          → mruby .rb ユーザーcop
```

両バックエンドとも対象は同じ arena `NodeKind`。**セマンティクスは 1 つ**、
共有のセマンティクステストスイートで一度だけ検証する。

**capture の型付き差。** B は静的に構造が分かるので capture を型付きで返す
(`$_` → `NodeId`、`$...` → `&[NodeId]`、`$(int _)` → リテラル)。C は実行時
まで型不明なので汎用 `Captures`(`NodeId`/値の列)。これが「B が理想・
C は補助」の本質。

**v1 文法スコープ:**

| カテゴリ | v1 採用 | v1 見送り |
|---|---|---|
| 構造 | ノードマッチ・`_`・`...`・`{}` union・`!` 否定・リテラル・`nil?` | `[]` all・`<>` any-order |
| capture | `$`(位置 capture)・`$name`(名前付き位置 capture) | 名前付き capture の back-reference |
| 走査 | `^` 親・`` ` `` 子孫探索 | ― |
| 述語 | `#predicate` | `%param`・regexp |

**名前付き位置 capture(`$name`)** は murphy-9cr.17 で v1 採用(当初の見送り判断を改訂)。
`$name` は body が暗黙の `_` の位置 capture で、`.so`/B バックエンドの型付き capture を
名前付きフィールドとして生成できる。back-reference(同名 = 等価制約)は引き続き見送り。
詳細は murphy-9cr.17 の design 参照。

**「ネストした node 探索」= `` ` `` 子孫探索オペレータ** として文法に入れる。
B では `node.descendants().find(...)` ループへ lowering、C では interpreter が
子孫を歩く ── 両バックエンド対応の文法機能であり別モードにはしない。
`#predicate` は B では Rust 関数呼び出し、C では述語レジストリ(mruby
メソッド名)で解決する。

## 5. プラグイン ABI と単一表面

`murphy-plugin-api` が **cop が AST を読む唯一の表面**。murphy-core
(標準 cop)も `.so` プラグインも同一クレートに依存する。

```rust
// dispatch 時に cop へ渡るコンテキスト
pub struct Cx<'a> {
    arena:    *const AstNode,        // 直読み(FFI 呼び出しゼロ)
    lists:    *const NodeId,
    interner: InternerRef,
    fns:      &'a FnTable,           // murphy-core ロジックが要る操作だけ
    _marker:  PhantomData<&'a Ast>,  // arena より長生きしない保証
}
```

**直読み + 最小関数テーブル。** 木の走査・`NodeKind` マッチは純粋な
メモリ読み。murphy-core のロジックが要る操作だけ `FnTable` 関数ポインタ:

- `intern_to_str(Symbol) -> &str`
- `emit_offense(range, message, …)` / `emit_edit(range, replacement)`
- `comments() -> &[Comment]` / `raw_source(range) -> &str`(audit カテゴリ B/C)

**`.so` 境界。** murphy-core がプラグインへ渡すのは「arena ポインタ + len +
node_lists + interner blob + FnTable + ABI version」。プラグインのエントリは
`murphy_plugin_register` 1 点(`register_cops!` が生成)。

**run_file 撤廃。** `FileCop` トレイトと `MurphyRunFile` 関数ポインタを
削除。raw-source エスケープハッチは持たない。

**トレイト再編(spike を捨てて再設計):**

- `Cop` ── メタデータのみ(const name/description/severity/enabled/options)。
  ADR 0035 の const ベースは存続。
- `NodeCop` ── 実ディスパッチ `fn check(&self, node: NodeId, cx: &Cx)` +
  対象 `NodeKind` 宣言(`#[on_node]`)。
- `FileCop` 削除(run_file 撤廃)。`CallCop` は `NodeCop` on `Send` の特殊例
  にすぎず `NodeCop` へ統合。

**標準 cop = 組込みプラグインパック (静的リンク、`murphy-std`)。** 標準 cop
(Murphy/Lint/Style/Layout、ADR 0018) は専用 crate `murphy-std` に切り出す。
`murphy-std` の Murphy 依存は `murphy-plugin-api` 1 本のみで、`murphy-core`
の内部 API には届かない ── 単一表面 ABI が **コンパイラ境界で強制** され、
`crate::internals::*` で抜け道を作る経路が build error になる。`murphy-cli`
はこの crate を **静的リンク** して同梱するため、`.so` ロードのコストは
払わない。差は発見方法(組込みリスト vs `.so` スキャン)だけ。

**murphy-rails は動的 `.so` プラグイン。** Rails 利用者向けの opt-in pack
なので、`murphy-rails.so` を `[[plugins]] path = "..."` 経由でロードする
経路に乗せる。これによって `.so` 配布パス(template repo / load
diagnostic / plugin pack 形式)も同時に dogfooding される。

**静的リンク時の登録契約。** `murphy-std` と動的 `.so` プラグインは
`register_cops!` proc-macro が生成する **同一の `MurphyPluginV1` テーブル**
(同じ ABI version、同じ cop metadata、同じ関数テーブル形状)を返す。違いは
呼び出し方と emit するシンボル種別だけ:

- **動的 (`.so` プラグイン):** `register_cops!` は `#[no_mangle] extern "C"
  fn murphy_plugin_register` を emit する。`murphy-cli` は `dlopen` した
  `.so` から `dlsym("murphy_plugin_register")` を取得して呼ぶ。
- **静的 (`murphy-std` のような組込みパック):** `register_cops!` は
  C シンボルを emit せず、Rust 公開関数のみ(例
  `murphy_std::__murphy_plugin_register()`)を emit する。`murphy-cli` は
  `Cargo.toml` 依存経由で直接 Rust 関数を呼ぶ。
  → 同一バイナリに複数 static pack を将来リンクしても **`#[no_mangle]` C
  シンボル衝突は発生しない**(C シンボルが存在しないため)。Rust 関数名は
  crate ごとに別 path で衝突しない。

**モード切替は Cargo feature ではなく macro 引数で行う。** Cargo features
は workspace/dependency tree で **unify される**ため、`plugin-dynamic`
のような feature flag で gate すると、別 crate が同 feature を有効化した
瞬間に静的 pack まで C シンボルを emit してしまう。これを避けるため、
モードは `register_cops!` の **明示引数** で指定する:

```rust
register_cops!(mode = static, MyCop1, MyCop2);  // murphy-std はこちら
register_cops!(mode = dynamic, MyCop1, MyCop2); // .so plugin はこちら
```

macro は引数を見て `#[no_mangle]` を emit するかしないかを決定。Cargo
feature を経由しないので unification の影響を受けない。CI で `murphy-std`
の lib.rs が `mode = static` 以外を使っていないことを grep ベースの
check で検証する(将来 procedural な enforcement に置き換え可能)。

`murphy-cli` 側の登録パスは「`MurphyPluginV1` を受け取り plugin-api 経由で
cop を登録する関数 1 本」に集約し、static/dynamic 両方ともそこに合流する
── これによって標準 cop 専用ショートカットが ABI レベルでも存在しないこと
を保証する。

**CI 境界チェック。** `murphy-std` の `Cargo.toml` の **runtime** Murphy
依存が `murphy-plugin-api` 1 本のみであることを CI で検証する(小さな
`cargo metadata` テスト、または `cargo-deny` の `forbidden` ルール)。
`[dev-dependencies]` は対象外 ── test fixtures / snapshot utilities は
任意のクレートを使ってよい(production link 時に含まれないため)。
「後で `murphy-core` の helper をこっそり runtime 依存に足してしまう」
退行を build/CI error として拾うのが目的。

**動的 `.so` のロード境界 (ADR 0004 trusted-code 前提)。** `.so`
プラグインは `murphy.toml` の `[[plugins]] path = "..."` で明示指定された
path のみロードする(暗黙の探索パスは持たない)。プラグインは ADR 0004
に従い **trusted native code** として扱う(サンドボックスなし、host 権限
で動作) ── template repo / config 例 / load diagnostic はこの前提を
ユーザーに明示する文言を含める。

ロード手順とエラー処理:

- **path 正規化:** 相対 path は `murphy.toml` のあるディレクトリを基準に
  解決し、`canonicalize` で symlink / `..` / 重複セパレータを潰す。
- **重複 load の検出:** 正規化後の path(inode 単位での同一視も可)で
  既ロードのものと重複していたら warning を出して skip(エラーにしない
  ── 設定ファイルの commenting/duplication で意図しない break を避ける)。
- **複数 plugin の失敗順序:** `[[plugins]]` 配列は **宣言順** に処理。
  個別 plugin のロード失敗(file not found / symbol missing / ABI version
  mismatch / load 中の panic)は具体的 diagnostic を蓄積し、**全 plugin の
  処理後に exit code 2 で停止**(部分起動はしない ── 設定が壊れている
  状態で lint を続けると false-negative が出るため)。診断は失敗した
  plugin と最初の失敗原因を列挙して STDERR に出す。

**NodeId 有効性。** arena は dispatch 中 immutable・murphy-core 所有で
プラグイン呼び出しより長生き。プラグインは dispatch を超えてポインタを
保持しない(`Cx<'a>` がライフタイムで表現)。

## 6. murphy-9cr epic 再構成

**既存サブタスクの処遇**(spike を捨てる前提):

| サブタスク | 処遇 |
|---|---|
| .1 頻度分析 | 参照資料として保持(NodeKind 設計の入力) |
| .2 ABI option metadata | 再実装 ── 概念は存続、構造体は新 ABI で作り直し |
| .3 plugin-api skeleton | 置換 ── plugin-api を単一表面・arena 向けに全面再設計 |
| .4 no synthesized dispatch (ADR 0034) | 保持 ── arena dispatch にも適用 |
| .5 Tier 1 typed wrappers (prism) | クローズ/superseded ── murphy-ast `NodeKind` が代替 |
| .6 `register_cops!` / .7 `derive(CopOptions)` | 再ターゲット ── マクロ概念は存続、新構造体を生成 |
| .8 `#[on_node]`/`#[murphy::cop]` | 再スコープ ── arena 向けに |
| .9 config 検証 / .10 配布UX / .12 safe_autocorrect | 持ち越し ── 後段へ |

**新 epic のサブタスク DAG(依存順):**

```text
1  ADR: arena AST + 単一表面プラグイン ABI
2  murphy-ast (arena/NodeKind/走査/interner、シリアライズ可能に)
3  murphy-translate (prism→arena)             ← 2
4  プロトタイプで変換コスト実測               ← 3  [ゲート]
5  murphy-pattern (S式文法・パーサ)           ← 2
6  B backend: node_pattern! proc macro        ← 5
7  C backend: ランタイム matcher + IR         ← 5
8  murphy-plugin-api 再設計(Cx/NodeCop)      ← 2
9  register_cops! / derive(CopOptions) 再ターゲット ← 8
10 #[on_node] / #[murphy::cop]                ← 8,6
11 murphy-core dispatch を arena へ差替        ← 2,8
12a `murphy-std` 空 crate + dependency boundary
    (Cargo.toml `murphy-plugin-api` 単一 runtime 依存、CI 境界検証)  ← 8,11
12b 静的登録経路(murphy-cli が murphy-std の register を直接呼ぶ、
    動的 `.so` と同等の `MurphyPluginV1` 契約)                       ← 12a
12c 一時無効化メカニズム + disabled registry
    (未移植 cop は `enabled=false` で trunk green、CLI 表示まで)    ← 12a,12b
12d 代表 cop の arena AST 再実装 (`murphy-std` 内、各 namespace 1 個以上) ← 12b,12c
13 mruby ブリッジ → C backend                                       ← 7,8
14 run_file 撤廃                                                    ← 12d,13
15 arena バイナリキャッシュ(fast-follow)     ← 2,3
16 持ち越し: .9 / .10 / .12
```

**murphy-rails 全書き換えがゴール。** §11 で dispatch を arena へ差し替え、
§14 で run_file を撤廃すると、現行の 131 個の text-matching cop は壊れる。
**移行戦略 = 一時無効化 → 順次移植:**

- §12 を 4 サブタスク(12a–12d)に分割し、それぞれ独立完了条件を持つ:
  - **12a 完了条件:** `cargo build -p murphy-std` 成功、
    `murphy-std/Cargo.toml` の runtime Murphy 依存が `murphy-plugin-api`
    1 本のみ(`[dev-dependencies]` は対象外)、CI が他 Murphy crate への
    runtime 依存追加を build/CI error として拒否。
  - **12b 完了条件:** `murphy-cli` が起動時に `murphy-std` を
    `MurphyPluginV1` として登録し、動的 `.so` と同じ登録パスを通る。
    `register_cops!` が static/dynamic 両モードで生成する `MurphyPluginV1`
    が、(i) ABI version、(ii) cop metadata の内容(name / description /
    default severity / default enabled / options)、(iii) function table
    の shape(関数の本数と signature)、(iv) 登録後の observable behavior
    (cop が同じ順序・条件で発火) において等価であることを fixture で
    検証する。**関数 pointer の literal アドレスは static/dynamic で
    一致しないので bit 等価は要求しない。**
  - **12c 完了条件:** 未移植標準 cop が disabled registry に登録され
    `enabled=false` で skip、`cargo test --workspace` が green。CLI
    は新規 subcommand `murphy cops list` で disabled cop を可視化
    (公開インターフェース、本タスクで安定化):
    - **デフォルト出力:** 表形式 ── `NAME / NAMESPACE / STATUS / SOURCE
      PACK` 4 列。`STATUS` は `enabled` / `disabled: arena migration` /
      `disabled: user config` のいずれか。
    - **machine-readable 出力:** `--format json` でフラットな JSON 配列。
      schema は ADR 0006 と同等の方針(キー名は snake_case、追加フィールド
      で破壊的変更なし)。
    - **exit code:** 一覧は情報出力なので、disabled cop の有無に関わらず
      `0`(設定エラーは別経路で `2`)。
    - ユーザーの既存 `murphy.toml` で disabled 中の cop が明示的に
      `enabled = true` 指定されていた場合は **warning を出して skip**
      (エラーにはしない ── 設定互換性のため)。warning 文には au8 /
      murphy-9cr.23 の参照を含める。
  - **12d 完了条件:** **各 namespace 1 個以上、計 4 個以上**の代表 cop が
    arena 上で再実装され、既存スナップショットテストが green。残りは
    12c で disabled。候補 cop と検証観点:
    - **Murphy/NoReceiverPuts** — `CallNode` dispatch、receiver の有無
      判定、warning severity、autocorrect なし。
    - **Lint/UnreachableCode** — flow 解析(`ReturnNode` / `RaiseNode`
      / `BreakNode` 直後の dead statement)、container 走査、`BeginNode`
      内位置依存。
    - **Style/StringLiterals** — `StringNode` / `InterpolatedStringNode`
      dispatch、config option (`preferred_quote = single|double`)、
      autocorrect (range edit)、option schema validation 経路。
    - **Layout/TrailingWhitespace** — raw source access (`comments()`
      ではなく `raw_source(range)` カテゴリ)、line-based 検出、
      autocorrect (削除)。
    候補は実装段階で 1 個ずつ差し替え可だが、上の **4 観点
    (call dispatch / flow 解析 / literal+option+autocorrect / raw source
    access)** はカバーする。簡単な cop ばかり選ばない。
- murphy-rails の動的 `.so` 化と 131 個全量の arena AST 移植は **専用の
  follow-up epic (murphy-au8)** で追跡する。これは reboot の付録ではなく
  **reboot が存在する目的そのもの** であり、優先度を落とさず一時無効化
  リストを 0 に向けて消化する。

**移行期のユーザー影響:**

- **設定ファイルの後方互換:** cop 名 (`Style/Foo` 等) は変わらない。
  `[cops.rules."Style/Foo"]` セクションは reboot 前後で同じ動作。
  `murphy-std` の `register_cops!` が `cop.name()` を従来名で返すため、
  既存 `murphy.toml` は touch 不要。
- **無効化中 cop の明示有効化:** ユーザーの `murphy.toml` で
  `enabled = true` 明示指定されている cop が disabled registry にある
  場合、**warning を出して skip**(エラーにはしない)。warning には
  「arena migration 進行中であること」と「au8 / murphy-9cr.23 進捗」
  への参照を含める。
- **無効化された標準 cop の可視化:** disabled registry の内容は新規
  CLI subcommand `murphy cops list` で `disabled: arena migration in
  progress` 注記付きで一覧表示される(machine-readable は
  `--format json`)。
- **disabled cop の severity / autocorrect / option:** disabled 状態の
  cop は dispatch されないので runtime には severity / autocorrect /
  option は未参照。ただし `murphy.toml` 内の対応する設定 section は
  **削除を強制しない**(au8 完了で active 化した時に再評価される
  ように)。option key の typo 等は、cop が active 化したタイミングで
  通常の option schema validation 経路で warning 化される。
- **murphy-rails の扱い:** §14 で `run_file` が撤廃される時点で
  text-matching 経路は失われる。murphy-rails の 131 cop はそのタイミングで
  **disabled registry に一括登録** され、au8 epic で個別に `.so` 化 / arena
  AST 再実装が完了するまで「disabled」扱いになる(標準 cop と同じ移行戦略 ──
  壊さない = warning + skip、エラーにはしない)。これによって §14 と
  「murphy-rails を壊さない」が両立する: 「Rails cops は実行されないが
  murphy.toml の `[cops.rules."Rails/..."]` セクションは保持され、cop が
  存在しないことによる error にはならない」状態になる。au8 完了で 131 cop
  が disabled リストから順次抜けて active になる。

> reboot epic は murphy-9cr の ID を再利用し、superseded なサブタスクを
> クローズ・新サブタスクを追加する形で in-place 再構成する。

## 7. テスト方針

- **TDD 必須**(CLAUDE.md)。各クレートに failing test 先行。
- **共有セマンティクステスト**: B と C は同一パターン集合に対し同一結果を
  出すことを 1 スイートで検証。
- **翻訳の等価性**: prism AST と arena AST のスナップショット対応を
  ゴールデンテストで保護。
- **キャッシュ往復**: シリアライズ → ロードで arena が bit 等価。
- **決定性 / JSON 契約**: ADR 0006 の offense JSON・exit code は据え置き、
  スナップショットで保護。
- **実測ゲート(§4)**: 翻訳コストがベースライン比で閾値超なら設計後退の
  判断点。

## 8. 主要リスク

1. **変換コストが速度目標を侵食** → §3 の実測ゲートで早期検知、§3.5 の
   キャッシュで複数回実行をまたいで償却。
2. **`NodeKind` variant 集合の確定** → v1 凍結後は編集が破壊的。parser-gem
   準拠で決め切る。
3. **epic が大きい(16 項目)** → §4 の実測ゲートまでを第一マイルストーンに
   し、設計の致命的破綻を早期に出す。
4. **移行中の一時無効化が放置される** → 無効化リストを follow-up epic で
   明示追跡し、件数を可視化する。

## 9. 次のステップ

1. 形式 ADR を 2 本起こす ── (a) arena parser-shaped typed AST をコア AST
   表現とする、(b) 単一表面プラグイン ABI(`#[repr(C)]` arena 直読み)。
2. murphy-9cr epic を本設計の §6 DAG で in-place 再構成する。
3. murphy-9cr.5(prism Tier 1 wrapper)をクローズ、.8 を arena 向けに
   再スコープ。
4. §2〜§4 のプロトタイプを作り、prism→arena 変換コストを実測してから
   本実装にコミットする。
