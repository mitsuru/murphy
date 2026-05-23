# murphy-9cr.22 — murphy-core dispatch を arena へ差替 — 設計

**Status**: 設計ドラフト 2026-05-23。レビュー待ち。承認後に implementation。
**スコープ**: epic murphy-9cr §11(reboot-design.md)を実装可能な設計へ具体化。
**依存**: ✓ murphy-9cr.14(arena AST)・✓ murphy-9cr.20(plugin-api 単一表面)。
**ブロック**: .23(一時無効化メカニズム + rails 組込みパック化)・.24(mruby→C backend)。

## 0. 大方針

murphy-core から「`ruby_prism::Node` を直接歩く dispatch」を全廃し、
murphy-ast の arena を 1 パス走査する dispatch host に置き換える。cop が AST
を読む表面は murphy-plugin-api の `Cx` / `NodeCop` のみ — 標準 cop も `.so`
も同一表面。spike(`MurphyPluginV1` / `MurphyNodeContext` / `Cop` トレイト等)
は **削除**。後方互換は持たない(ADR 0038)。

## 1. .22 / .23 / .24 のスコープ境界(advisor 1.)

reboot epic は .22→.23→.24 と直列。.22 単体で `cargo test --workspace` を
green に保つために必要な「最小限の一時無効化」は .22 の deliverable に含める。
それ以上の置換(rails 全書き換え・mruby C backend)は **後段** に委ねる。

| 項目 | .22(本タスク) | .23 | .24 |
|---|---|---|---|
| arena dispatch host(murphy-core) | **新規実装** | — | — |
| 新 ABI ローダ(`.so` プラグイン) | **置換** | — | — |
| 旧 ABI loader / `MurphyPluginV1` 等 | **削除** | — | — |
| 組込みビルトイン cop(`NoReceiverPuts`) | **新表面へ移植** | — | — |
| murphy-rails(138 cop) | **lib body を空に**(後述 §6) | 一時無効化機構 + 代表数個を新表面へ | 残量を順次移植 |
| murphy-example-pack | **lib body を空に** + テスト無力化 | — | — |
| mruby user cop ディスパッチ | **feature gate でランタイム disabled**(.24 開始時に再導入) | — | C backend matcher へ |
| ADR 0006 offense JSON 契約 | **不変**(byte-identical) | 同 | 同 |
| autocorrect fixpoint 契約 | **不変** | 同 | 同 |

スコープを膨張させない原則: rails や mruby の「本格的置換」は決して .22 に
入れない。CI green を保つだけの最小限のみ。

## 2. クレート依存変更

```
murphy-core
  - 依存追加: murphy-ast, murphy-translate, murphy-plugin-api
  - 依存維持: ruby-prism(parse の prism 呼び出しは murphy-translate 内部に
              移動するため最終的に murphy-core 側の直接依存は **削除可能**。
              .22 で完全除去するか維持するかは §3 末参照)
  - 依存削除: なし(serde_json などはそのまま)

murphy-rails
  - 依存変更: murphy-core 旧 surface への依存をすべて削除。新表面に依存しない
              空 lib にする(後段 .23 で murphy-plugin-api 経由で再実装)

murphy-example-pack
  - 同上(murphy-rails と同様に空 lib 化)
```

## 3. murphy-core 内部レイアウト(再編後)

```
crates/murphy-core/src/
├── lib.rs           — pub use を新表面ベースへ全面入れ替え
├── parse.rs         — prism 呼び出しを撤去、`parse(source, path) -> Ast` は
│                      murphy-translate::translate の薄いラッパ。返り値は
│                      `murphy_ast::Ast`(所有・lifetime なし)
├── dispatch.rs      — 新規。arena 1 パス走査 dispatch host(§4)
├── builtin.rs       — 新規。組込みビルトイン cop 集約。v1 は NoReceiverPuts
│                      1 個。各 cop は static `PluginCopV1` テーブル経由
│                      (§4 末で確定)
├── plugin.rs        — 新規 ABI(`PluginCopV1`/`PluginRegistration`)ローダの
│                      みに削減(2007 行 → 数百行)
├── aggregator.rs    — 不変(content-based sort、§7)
├── autocorrect.rs   — 不変(Offense+Edit を消費するだけ、AST 非依存)
├── config.rs        — 不変
├── discovery.rs     — 不変
├── offense.rs       — 不変
├── registry.rs      — builtin + .so loaded cops を統一 view で返す簡素な
│                      コンテナへ簡略化(500 行 → ~100 行)
├── cop.rs           — **削除**(旧 Cop/CopContext/Visit dispatch は全て撤去)
├── cops.rs/cops/    — 旧 NoReceiverPuts は `builtin.rs` へ移動して削除
├── ast_sexp.rs      — prism ベースの S 式は撤去 or murphy-ast 版に書き直し
│                      (本タスクでは「撤去 + follow-up issue」を推奨。
│                      `murphy lint --ast` の UX は .22 では一時的に縮退可)
└── mruby/           — feature gate `mruby-user-cops` で囲い込み、default は
                       off。run_mruby_cop_* の呼び出し sites を dispatch.rs
                       から外す(.24 でリバース)
```

prism 依存の最終的な所在は murphy-translate のみ(murphy-core からは推移
依存として残るが直接依存はしない)。

## 4. arena dispatch host(`dispatch.rs`)

### 4.1 入口

```rust
pub fn run_cops(
    ast: &murphy_ast::Ast,
    cops: &[&'static PluginCopV1],
    sink: &mut OffenseSink,
);
```

`PluginCopV1`(murphy-plugin-api ABI)を統一インタフェイスとする。組込み
cop も `register_cops!` 同等のテーブル(`builtin.rs` 側で static に組む)。
`.so` から来る cop と区別しない。

### 4.2 NodeKindTag インデックス

dispatch 前に、cops を `NodeKindTag` で逆引き索引化する:

```rust
struct DispatchIndex {
    by_kind: [SmallVec<&'static PluginCopV1>; NODEKINDTAG_COUNT],
}
```

ノード N 個 × cop M 個 を線形にやると O(N·M)。`KINDS` で hit する cop だけ
を kind 別バケットに事前登録する。bucket は 1 要素〜数要素の見込みなので
SmallVec(or Vec、確定はベンチ次第)で十分。

### 4.3 主ループ(arena 1 パス)

```rust
for node_id in 0..ast.node_count() {
    let kind_tag = NodeKindTag::from(ast.kind(NodeId(node_id)));
    for cop in &index.by_kind[kind_tag as usize] {
        invoke(cop, NodeId(node_id), ast, sink);
    }
}
```

走査順は arena の push 順(murphy-translate は post-order DFS で push する
ので post-order に近いが順序は ADR 0006 不変性に効かない、§7 参照)。

### 4.4 CxRaw の組み立てと `cop_name` の扱い(advisor 4.)

`CxRaw` の各 ptr+len は `Ast::raw_parts()` から 1 回ぶん取って使い回せる。
変わるのは **`cop_name` だけ**:

```rust
// dispatch 開始前に一度だけ作る
let base = build_cx_raw_base(ast, &fns, sink_ptr);
for cop in cops {
    // 各 cop turn の冒頭で 1 フィールド書き換え
    let mut cx_raw = base;
    cx_raw.cop_name = cop.name;          // RawSlice (cop の static 文字列)
    // 同一 cop の連続呼び出し中は cx_raw を使い回し
    for &node_id in nodes_for(cop.kinds) {
        unsafe { (cop.dispatch)(node_id, &cx_raw) };
    }
}
```

代替案として「外側ループを node、内側を cop」にすると毎ノードごとに
`cop_name` を切り替える羽目になる。**外 cop / 内 node** で 1 cop あたりの
書き換えを 1 回に抑える(arena は 1 パスのまま、配列の値だけ事前計算)。

### 4.5 OffenseSink + FnTable

```rust
pub struct OffenseSink {
    offenses: Vec<Offense>,
    edits: Vec<Edit>,
    file_path: String,
    source: String,
}

unsafe extern "C" fn host_emit_offense(sink: *mut c_void, o: *const RawOffense) {
    let sink = unsafe { &mut *(sink as *mut OffenseSink) };
    let o = unsafe { &*o };
    // RawSlice → String 変換、cop_name / message を copy
    sink.offenses.push(Offense::new(&sink.file_path, …, o.range, sev, msg));
}
unsafe extern "C" fn host_emit_edit(sink: *mut c_void, e: *const RawEdit) { … }

static FNS: FnTable = FnTable { emit_offense: host_emit_offense, emit_edit: host_emit_edit };
```

`FnTable` の関数ポインタは **dispatch 実行中に変わらない static**。`sink`
が opaque host 状態。.so 側は `Cx::emit_offense` を呼び、host 側で `Offense`
にレンダリングする。Offense/Edit 型は murphy-core::offense のままで不変。

### 4.6 dispatch thunk の panic 捕捉

`DispatchFn` は `unsafe extern "C" fn(NodeId, *const CxRaw) -> i32`。0 が
成功、非 0 が panic 捕捉(register_cops! が `std::panic::catch_unwind` で
ラップする想定 — これは .21 の責務)。host 側は非 0 を受けたら cop 単位
で **当該 cop だけ無効化**(ファイル単位停止はしない)+ stderr に診断 1 行。
ファイル全体は最後まで完走させる(per-cop 障害分離、ADR 0033 に整合)。

### 4.7 組込み cop のテーブル化

```rust
// builtin.rs
pub static BUILTINS: &[&PluginCopV1] = &[
    &cops::no_receiver_puts::COP,
];
```

各組込みは `register_cops!` を使わず手書きで `PluginCopV1` を組む(マクロは
.21 で同一形になるが、murphy-core 内部は依存を増やしたくないので static
を手で書く)。`.21` の macro 出力との整合は ABI レイアウトで担保される。

## 5. ビルトイン `NoReceiverPuts` の移植

現状 `crates/murphy-core/src/cops/no_receiver_puts.rs` で旧 `Cop` トレイト
+ `restrict_on_send` を使い、`puts` への receiver-less 呼び出しを検出。

新表面:

```rust
struct NoReceiverPuts;
impl Cop for NoReceiverPuts {
    const NAME: &'static str = "Murphy/NoReceiverPuts";
    const DESCRIPTION: &'static str = …;
    const DEFAULT_SEVERITY: Severity = Severity::Warning;
    const DEFAULT_ENABLED: Option<bool> = Some(true);
    type Options = NoOptions;
}
impl NodeCop for NoReceiverPuts {
    const KINDS: &'static [NodeKindTag] = &[NodeKindTag::Send];
    fn check(&self, node: NodeId, cx: &Cx) {
        let NodeKind::Send { receiver, method, .. } = *cx.kind(node) else { return };
        if !receiver.is_none() { return }
        if cx.symbol_str(method) != "puts" { return }
        cx.emit_offense(cx.range(node), "Use logger instead of puts", None);
    }
}

pub static COP: PluginCopV1 = PluginCopV1 { … dispatch: dispatch_thunk … };
```

これ自体が `register_cops!` 不在で動くことの試金石になる。

## 6. 一時無効化(.22 で **含める** 最小限)(advisor 1./7.)

### 6.1 murphy-rails / murphy-example-pack

両クレートの `lib.rs` を **空(or 公開 `fn _placeholder() {}` 1 個)** に置換。
Cargo.toml の依存(`murphy-core` 旧型を引いていた箇所)を削除して compile を
通す。テスト本体は削除 or `#[ignore]`。

理由: rails 138 cop / example-pack 3 cop は旧 ABI(`MurphyPluginV1`/
`MurphyCallContext`)に密結合しており、新表面への移植は .23 のスコープ。
.22 で半端な shim を入れると後段で剥がす負債になる。spike は捨てる。

各クレートに follow-up コメント:
```
// SUPERSEDED by murphy-9cr.23 (legacy lib body deleted in 9cr.22).
// New impl will register cops via murphy-plugin-api/macros.
```

### 6.2 mruby user cop ディスパッチ(advisor 7.)

**判断: feature gate を選ぶ**(hard-delete ではない)。

- `murphy-core` に新 feature `mruby-user-cops`(default off)を追加。
- `mod mruby` を `#[cfg(feature = "mruby-user-cops")]` でゲート。
- `lib.rs` の `pub use mruby::…` 群を同じ feature gate に。
- dispatch.rs から `run_mruby_cop_*` 呼び出し sites を撤去(arena dispatch
  路線では呼ばれない。.24 が C backend matcher として再導入する)。
- CLI / discovery が .rb cop を見つけても、`.22` 時点では「mruby user cop
  ディスパッチは .24 待ち」の警告 1 行に降格(エラーにはしない)。

ハードデリートしない理由: 既存実装は ~数千行あり、`mruby/sandbox.rs` /
`primitives.rs` / `package` まで含む。.24 がそれを **C backend matcher と
組み合わせ直す** 設計なので、コードを残して dead-code 化しておく方が
.24 の作業が単純になる。`-D warnings` 下でも feature gate 外なら dead-code
警告は出ない。

### 6.3 CLI / tests のマイグレーション順序(advisor 5.)

`lib.rs` の旧 `pub use plugin::{…}` 群を削除する **前** に、これらを参照する
全 sites を新表面へ移行 or 削除する。順序:

1. murphy-rails / murphy-example-pack を空 lib 化(旧 import 消滅)。
2. murphy-core tests の旧 surface 参照を撤去 or 新表面に置換。
   - `cop_no_receiver_puts.rs` — 新 NodeCop で書き直し。
   - `cop_deadline_isolation.rs` — mruby gate に従い `#[cfg]`。
   - `plugin_abi_exports.rs` — 新 symbol(`murphy_plugin_register`)で書き直し。
3. murphy-cli tests:
   - `native_plugin_pack.rs` — example-pack を空にしたので test ごと
     `#[ignore]` + follow-up リンク(.23 で復活)。
   - `mruby_e2e.rs` / `fix_e2e.rs` — `#[cfg(feature = "mruby-user-cops")]`。
   - `integration_snapshot.rs` / `autocorrect_snapshot.rs` / `cli.rs` /
     `ast_cli.rs` / `parallel_determinism.rs` / `migrate.rs` — fixture 内
     の `.rb` user cop を **builtin NoReceiverPuts のみ** で再現できる範囲
     に絞り込むか、mruby gate 配下に移す(個別に判断)。
4. `lib.rs` の `pub use plugin::{ MurphyPluginV1, … }` を削除。

順序が逆だと中間 commit が compile しない。各 step を独立 commit にする。

### 6.4 拡張カバレッジの追加(advisor 3.)

rails snapshot を gut すると JSON 契約の biteable 表面が薄くなる。
**.22 で追加で入れるべき fixture**:

- `crates/murphy-cli/tests/fixtures/builtin_only_project/` を新設し、
  `puts "x"` / `obj.puts("x")` / 多バイト・ネストされた式に対する
  NoReceiverPuts のみのスナップショット(JSON + autocorrect 後の文字列)を
  保持。これが .22 で「JSON 契約 byte-identical を本当に検証している」
  唯一の固定 anchor になる。
- 既存 sample_project / autocorrect_project は **mruby gate に依存** する
  ので、`mruby-user-cops` 有効テスト + 無効テストの両 path を回す
  matrix を CI に書く(or 単一 path のみで .22 では充分なら片方)。

## 7. ADR 0006 JSON 契約の不変性(advisor 2.)

aggregator.rs:46-58 のソートキーは:

```
(file, range.start, range.end, cop_name, message, DESC severity)
```

**完全に content-based** で dispatch 順非依存。murphy-core/src/aggregator.rs
の doc comment(L7、L24)が ADR 0007 の「determinism: independent of input/
engine/thread order」を明示している。

したがって prism `Visit` の pre-order から arena push-order(post-order)
へ走査順が変わっても、aggregator 通過後の offense 列は同一内容なら同一順。
JSON byte-identical を **設計レベルで保証**。

これを §8 のスナップショットテスト(builtin_only_project)で empirically
にも固定する。

## 8. テスト戦略(TDD 必須・CLAUDE.md)

**新規 unit テスト(failing first)**:

1. `dispatch_index_groups_cops_by_kind` — DispatchIndex の構築。
2. `dispatch_iterates_arena_once_per_node` — N ノード走査で arena indices
   を全踏みする。
3. `dispatch_invokes_only_matching_kinds` — KINDS に含まれないノードに対し
   cop が呼ばれないこと。
4. `dispatch_stamps_cop_name_into_cx_raw_per_cop` — `cop_name` が cop ごと
   に正しく差し変わる(2 cop · 共通ノードシナリオ)。
5. `panicking_cop_is_isolated_and_others_complete` — i32 != 0 リターンを
   返す dispatch でも他 cop が完走する(thunk 側 catch_unwind は本テストの
   外、ここでは host 動作のみ確認)。
6. `host_emit_offense_renders_into_offense_sink` — FnTable callback の
   sink ストアが Offense へ正しく落ちる。
7. `builtin_no_receiver_puts_detects_puts_send` — 新表面の cop 実装。
8. `plugin_loader_validates_abi_version_and_struct_size` — 新 ABI loader。

**JSON 契約スナップショット**:

- `builtin_only_project.json` を新設(builtin NoReceiverPuts のみで生成)。
- `integration_snapshot.rs` が builtin_only_project に対して byte-identical
  を assert。

**autocorrect fixpoint**:

- builtin_only fixture に「`puts` → `# puts` 化(または削除)」の autocorrect
  を仕込んでおき、fixpoint 等冪性を assert。
- 注: NoReceiverPuts の現状は autocorrect なし。`emit_edit` する版を追加
  するか、最小限 fixpoint テストのみ(変更ゼロが収束する)とするかは
  実装時に決定(後者が安全)。

**CI**:
- `cargo build --workspace`
- `cargo test --workspace`(default features)
- `cargo test --workspace --features murphy-core/mruby-user-cops`(matrix
  第 2 row、mruby path 保護)
- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

## 9. 実装ステップ DAG

```
S1 parse.rs を murphy-translate へ薄ラップ(prism 直依存撤去 or 内部隠蔽)
   └─ test: parse() が murphy_ast::Ast を返す
S2 dispatch.rs スケルトン(DispatchIndex + 主ループ + OffenseSink + FnTable)
   └─ test: 4.1〜4.6 の unit テスト
S3 builtin.rs: NoReceiverPuts 移植
   └─ test: §8 (7)
S4 新 ABI loader(plugin.rs 縮減)
   └─ test: §8 (8)
S5 一時無効化: murphy-rails / murphy-example-pack lib gut、mruby feature
   gate、tests の cfg/ignore 設定
   └─ ここで初めて `cargo test --workspace` が green になる
S6 builtin_only_project fixture + JSON 契約スナップショット
   └─ test: §8 スナップショット
S7 lib.rs の旧 pub use 群を削除、registry.rs / cop.rs / cops.rs を整理
   └─ test: 全 workspace ゲート再走
S8 docs/decisions に ADR 補足(必要なら)、bd close 直前 quality gate
```

各 step は独立 commit。S5 までは中間で `cargo build` だけは通すが
`cargo test --workspace` は赤を許容(理由を commit message に明記)。S6 以降
は test も green。

## 10. リスク

1. **`NodeKindTag` の射程不足**: murphy-plugin-api の `NodeKindTag` が
   murphy-ast::NodeKind 37 variant をフルカバーしているか .20 で確認済み
   だが、組込み NoReceiverPuts が `Send` だけで足りるかは .22 実装時に
   テストで再確認。
2. **`Ast::raw_parts()` が想定する形を本当に返すか**: .20 で plugin-api
   側 unit test が通っているはずだが、.22 の host 側で fresh に組み立てる
   際 CxRaw のフィールド整合を assert する unit test を 1 本入れる。
3. **mruby feature gate の matrix 漏れ**: CI で `--features
   mruby-user-cops` を走らせ忘れると dead-code 化に気づけない。CI 設定の
   PR を最小スコープで .22 に同梱。
4. **CLI integration test の縮退**: builtin-only fixture だけでは現行
   sample_project の多様性を完全には代替できない。.23 で rails 代表数個
   が戻った時に snapshot を厚くすればよく、.22 は **薄くてもよい**。
5. **prism 依存の最終削除を欲張らない**: §3 末に書いた通り、.22 で
   murphy-core の直接 prism 依存を完全に切るかは optional。確実に切れる
   なら切る、parse.rs の薄ラップで残るなら follow-up に分離。
6. **rails の gut commit が大きい**: lib 削除だけにとどめる(テストは
   `#[ignore]` か削除)。「移行先 stub を書く」は .23 まで持ち越し。

## 11. 不採用案

- **rails / example-pack を新表面で同時並行移植**: .22 のスコープが膨張、
  プラン提示の意味が消える。spike を捨てる方針(reboot-design §6 §11)に
  整合的に従い、まず一時無効化のみ。
- **mruby user cop の hard-delete**: 数千行を捨てるリスクが高く、.24 の
  C backend matcher 設計に支障。feature gate で「呼ばれない」状態にする
  方が手戻りが少ない(advisor 7.)。
- **cop ループ外側を node、内側を cop**: 4.4 で示した通り `cop_name` の
  差し換えコストが N×M に膨らむ。**外 cop / 内 node** を採用。
- **`Cx` を copy せず参照渡し**: `Cx<'a>` は `Copy` で 16B(`&CxRaw` +
  PhantomData)なので copy が安い。`unsafe extern "C"` 境界では `*const
  CxRaw` のみ渡る。問題なし。
- **prism Visit を arena に再実装**: arena は配列なので Visit パターン
  ではなく単純 index ループで十分。dispatch を Visit に縛らない。
- **dispatch を per-file 並列化**: ADR 0007/0011 の決定性は aggregator が
  入力順非依存で吸収するが、.22 では並列化はスコープ外(per-cop 障害分離
  と panic catch を先に固める)。並列化は別 issue。

## ACCEPTANCE CRITERIA

- `crates/murphy-core/src/dispatch.rs` に arena 1 パス走査の dispatch host
  が存在し、`PluginCopV1` テーブルを統一インタフェイスとして消費する。
- 組込み `NoReceiverPuts` が `impl Cop + NodeCop`(murphy-plugin-api)で
  書き直され、`builtin.rs` の static `PluginCopV1` 経由で登録される。
- `murphy-core` の `lib.rs` から旧 surface
  (`MurphyPluginV1` / `MurphyCallContext` / `MurphyFileContext` /
  `MurphyNodeContext` / `MurphyEmitOffense` / `MurphyRunFile` /
  `MurphyRunCallDispatch` / `MurphyRunNodeDispatch` / `MurphyPluginCopV1` /
  `MurphyCallDispatchV1` / `MurphyNodeDispatchV1` / `MurphyCopOptionV1` /
  `MurphySlice` / `MurphyRange` / `MurphyPluginOffense` / `MurphyPluginEdit` /
  `MurphyPluginAutocorrect` / `MurphyPluginCallArgument` /
  `MURPHY_CALL_*` / `MURPHY_SEVERITY_*` / `MURPHY_TRISTATE_*` / `cop_v1` /
  `cop_v1_dispatch_only` / `validate_plugin_cop_ids` / `PluginFileCop` /
  旧 `Cop` / `CopContext` / `NodeDispatchRestriction` / `prism_node_kind` /
  `rubocop_hook_node_kinds` / `run_cop` / `run_cop_timed` / `run_cops`)が
  削除されている。
- 新 ABI ローダが `MurphyPluginRegister` シンボルを `dlopen` し、`abi_version
  == 1` および `PluginCopV1.size == size_of::<PluginCopV1>()` を検証して
  reject する。
- murphy-rails / murphy-example-pack の `lib.rs` body が空化され、follow-up
  コメント(`SUPERSEDED by murphy-9cr.23`)が残る。
- `mruby` モジュールが feature `mruby-user-cops`(default off)配下に隔離
  され、default feature では関連 sites がすべて compile out される。
- `crates/murphy-cli/tests/fixtures/builtin_only_project/` が新設され、
  JSON 契約スナップショット + (任意で)autocorrect fixpoint が assert
  される。
- aggregator.rs に変更がない(content-based ソートを再利用するだけ)。
- autocorrect.rs に変更がない。
- ADR 0006 offense JSON 契約と ADR 0007 determinism が破られていない
  (snapshot tests が green)。
- `cargo build --workspace` / `cargo test --workspace`(default features
  および `--features murphy-core/mruby-user-cops` の 2 matrix)/
  `cargo fmt --check` / `cargo clippy --workspace --all-targets -- -D
  warnings` がすべて green。
- §8 の TDD ユニットテスト 1〜8 が存在し green。

## 関連 ADR / 設計

- ADR 0006(offense JSON 契約)・ADR 0007(determinism)・ADR 0011(severity
  precedence) — いずれも本タスクで不変。
- ADR 0037(arena parser-shaped typed AST) — .14 で実装済み。
- ADR 0038(単一表面プラグイン ABI) — .20 で実装済み。
- docs/plans/2026-05-22-plugin-reboot-design.md §1, §3, §6, §11 — 本タスク
  の上位設計。
