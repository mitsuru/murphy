# murphy-9cr.8 — `#[cop]` + `#[on_node]` attribute macros (design)

**Status**: 設計合意 2026-05-23(brainstorming 完了)。murphy-9cr epic の §6 DAG
で「step 10: `#[on_node]` / `#[murphy::cop]`」に対応するサブタスク。
依存(murphy-9cr.18 = B backend `node_pattern!`、murphy-9cr.20 = plugin-api
再設計、murphy-9cr.6 = `register_cops!`、murphy-9cr.5 / .3 = arena AST
kinds)はすべて closed。

## 1. 目的

現在 cop 作者は次のような boilerplate を手書きしている:

```rust
#[derive(Default)]
struct NoTabs;

impl Cop for NoTabs {
    type Options = NoOptions;
    const NAME: &'static str = "Plugin/NoTabs";
}

impl NodeCop for NoTabs {
    const KINDS: &'static [NodeKindTag] = &[NodeKindTag(17)]; // 17 == send (生マジックナンバー)
    fn check(&self, node: NodeId, cx: &Cx<'_>) {
        // 自前で kind 分岐を書く
    }
}

murphy_plugin_macros::register_cops!(NoTabs);
```

これを `#[cop]` + `#[on_node]` で次のように書けるようにする:

```rust
use murphy_ast::NodeId;
use murphy_plugin_api::{Cx, NoOptions};
use murphy_plugin_macros::{cop, on_node};

#[derive(Default)]
struct NoTabs;

#[cop(
    name = "Plugin/NoTabs",
    description = "flag literal tabs in source",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl NoTabs {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) { /* user body */ }

    #[on_node(kind = "if")]
    #[on_node(kind = "case")]
    fn check_branch(&self, node: NodeId, cx: &Cx<'_>) { /* user body */ }

    fn helper(&self, _x: u32) {}
}

murphy_plugin_macros::register_cops!(NoTabs);
```

得られる効果:

- `KINDS` 配列の生マジックナンバー(`NodeKindTag(17)` 等)を排除し、
  pattern-name 文字列(`"send"`)を介して書く。typo は **compile error**。
- `Cop` / `NodeCop` のトレイト impl を 2 つ手書きする手間を 1 つの
  attribute に集約。
- メソッドごとに対応 kind を宣言できる(`fn check_send` / `fn check_branch`)
  ので、self-dispatch の `match` を cop 側で書く必要がない。

## 2. マクロの形

- `#[cop(...)]` は **inherent impl ブロック専用** の attribute proc-macro。
  - 受理: `impl Type { ... }`(generic 引数なし、`unsafe` でない)
  - 拒否: struct/enum/fn 上、`impl Trait for Type`、`impl<T> Foo<T>`、
    `unsafe impl`(すべて compile error)
- `#[on_node(...)]` は **`#[cop]` 付き impl ブロック内のメソッド専用** の
  attribute proc-macro。
  - cop 作者の視点: メソッドに付けることで「このメソッドはこの kind の
    ノードに対して呼ばれる」と宣言する。
  - 実装の視点: `#[on_node]` が proc-macro として呼ばれるのは
    「`#[cop]` の外で誤用された」ときだけ。`#[cop]` 側が impl の
    TokenStream を受け取った時点で `#[on_node]` 属性をパースして消費
    するため、正しい使い方なら `on_node` proc-macro 自体は呼ばれない。
  - スタブ実装: `syn::Error::new(call_site, "#[on_node] must be used inside a #[cop] impl block")`。

## 3. `#[cop(...)]` 引数

すべて名前付き、順序自由。

| キー | 必須 | 型 | デフォルト |
|---|---|---|---|
| `name` | ✓ | string literal | — |
| `description` | | string literal | `""` |
| `default_severity` | | string literal `"warning"`/`"error"`/`"info"` | `None`(=トレイトデフォルト) |
| `default_enabled` | | bool literal | `None` |
| `options` | | path (型名) | `::murphy_plugin_api::NoOptions` |

エラー診断:

- `name` 欠落 → `error: #[cop]: missing required argument 'name'`(マクロ位置)。
- 重複キー → `error: #[cop]: duplicate argument 'name'`(2 つめのキーの span)。
- 未知のキー → `error: #[cop]: unknown argument 'foo'`(キーの span)。
- `default_severity` の文字列が `"warning"`/`"error"`/`"info"` 以外 →
  `error: #[cop]: default_severity must be one of "warning"/"error"/"info"`
  (リテラルの span)。
- 型不一致(例: `name = 42`)→ syn のエラーをそのまま使用。

## 4. `#[on_node(...)]` 引数

| キー | 必須 | 型 |
|---|---|---|
| `kind` | ✓ | string literal |

検証:

- `kind` 欠落 → `error: #[on_node]: missing required argument 'kind'`。
- `kind` が `murphy_ast::tag_from_pattern_name` で解決できない →
  `error: #[on_node]: unknown node kind "carrot". Valid kinds: nil, true, ...`
  (リテラル span、`KIND_PATTERN_NAMES` 全件列挙)。
- `kind = ""` → 同上(`tag_from_pattern_name("")` は `None`)。

## 5. メソッドに対する形式制約

`#[on_node]` 付きメソッドのシグネチャは厳密に
`fn name(&self, node: NodeId, cx: &Cx<'_>)`。

- `pub`/`pub(crate)` 可。
- 戻り値型は `()`(明示も省略も可)以外不可。
- `async`、generic、`mut self`、`Self` 受け取り、追加引数すべて不可。
- 違反は span 付き compile error(各引数または `fn` キーワードの位置)。

これらの制約は厳密なほうがマクロ展開が読みやすく(余計な adapter
レイヤがいらない)、不適合な書き方は早期に明確なエラーになる。

`#[on_node]` のないメソッド・関連 const・関連型は impl 内に同居可能。
そのまま通す。

## 6. impl ブロック内の整合性チェック

- 同じ kind を 2 つ以上のメソッドで宣言 →
  `error: #[cop]: kind "send" is dispatched to multiple methods (first at line X)`
  (2 つめの `#[on_node]` の span、関連 span として 1 つめを `note:` 表示)。
- `#[on_node]` メソッドが 1 個もない →
  `error: #[cop]: impl block has no #[on_node] methods`(impl の span)。
- 1 つのメソッドに同じ kind の `#[on_node]` が 2 つ → 同じ重複 kind エラーで
  捕捉(同一メソッドかどうかに関わらず重複は不可)。

## 7. マクロ展開の形

入力:

```rust
#[derive(Default)]
struct NoTabs;

#[cop(name = "Plugin/NoTabs", description = "...", default_severity = "warning",
      default_enabled = true, options = NoOptions)]
impl NoTabs {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) { /* body */ }

    #[on_node(kind = "if")]
    #[on_node(kind = "case")]
    fn check_branch(&self, node: NodeId, cx: &Cx<'_>) { /* body */ }

    fn helper(&self, _x: u32) {}
}
```

出力(cargo expand での見え方):

```rust
#[derive(Default)]
struct NoTabs;

impl ::murphy_plugin_api::Cop for NoTabs {
    type Options = NoOptions;
    const NAME: &'static str = "Plugin/NoTabs";
    const DESCRIPTION: &'static str = "...";
    const DEFAULT_SEVERITY: ::core::option::Option<::murphy_plugin_api::Severity> =
        ::core::option::Option::Some(::murphy_plugin_api::Severity::Warning);
    const DEFAULT_ENABLED: ::core::option::Option<bool> =
        ::core::option::Option::Some(true);
}

impl ::murphy_plugin_api::NodeCop for NoTabs {
    const KINDS: &'static [::murphy_plugin_api::NodeKindTag] = &[
        ::murphy_plugin_api::NodeKindTag(17u8),  // send
        ::murphy_plugin_api::NodeKindTag(25u8),  // if
        ::murphy_plugin_api::NodeKindTag(26u8),  // case
    ];

    fn check(&self, node: ::murphy_ast::NodeId, cx: &::murphy_plugin_api::Cx<'_>) {
        match ::murphy_plugin_api::NodeKindTag::of(cx.kind(node)).0 {
            17u8 => Self::check_send(self, node, cx),
            25u8 | 26u8 => Self::check_branch(self, node, cx),
            _ => {}
        }
    }
}

impl NoTabs {
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) { /* body */ }
    fn check_branch(&self, node: NodeId, cx: &Cx<'_>) { /* body */ }
    fn helper(&self, _x: u32) {}
}
```

ポイント:

- `KINDS` の `u8` リテラルは `murphy_ast::tag_from_pattern_name` を
  マクロ展開時に呼んで解決(`node_pattern!` と同じパターン)。
- dispatch は `NodeKindTag::of(cx.kind(node)).0` を `match` する u8 タグ
  ベース。1 メソッドが複数 kind を持つ場合は `|` でアームをまとめる。
- 元の `impl NoTabs { ... }` ブロックは `#[on_node]` 属性だけを剥がして
  再放出。これにより内部の helper / 関連 const / 関連型もそのまま生きる。
- 型参照は `::murphy_plugin_api::...` / `::murphy_ast::...` のフルパスで
  生成し、ユーザの `use` に依存しない。

## 8. クレート構造

`murphy-plugin-macros/src/` のレイアウト:

```text
crates/murphy-plugin-macros/src/
├── lib.rs            # proc-macro エントリ4本 (register_cops, derive_cop_options, node_pattern, cop, on_node)
├── cop_options.rs    # 既存 (derive(CopOptions))
├── node_pattern.rs   # 既存 (node_pattern!)
└── cop_attr.rs       # 新規 (cop attribute と on_node の本体)
```

`cop_attr.rs` 内部:

```rust
pub fn cop(args: TokenStream, item: TokenStream) -> TokenStream { ... }
pub fn on_node(_args: TokenStream, _item: TokenStream) -> TokenStream { ... }
//  └→ 「外で誤用された」ときのスタブ。常に compile_error!

struct CopArgs { name, description, default_severity, default_enabled, options }
fn parse_cop_args(args: TokenStream) -> syn::Result<CopArgs>

struct OnNodeArgs { kind_lit: LitStr, kind_tag: u8 }
fn parse_on_node_args(args: TokenStream) -> syn::Result<OnNodeArgs>
//  └→ 内で murphy_ast::tag_from_pattern_name() を呼んで未知 kind を弾く

struct CopMethod { ident, kinds: Vec<(LitStr, u8)>, sig, block, vis, attrs }
fn collect_cop_methods(item_impl: &mut ItemImpl) -> syn::Result<Vec<CopMethod>>
//  └→ #[on_node] 属性を ItemImpl から剥がしつつ集める (in-place 修正)

fn validate_signature(method: &ImplItemFn) -> syn::Result<()>
fn validate_no_duplicate_kinds(methods: &[CopMethod]) -> syn::Result<()>

fn lower_cop_impl(args: CopArgs, methods: &[CopMethod], stripped: ItemImpl) -> TokenStream
//  └→ impl Cop + impl NodeCop + 元の impl(剥がし後) を3点出力
```

`lib.rs` 追加部:

```rust
mod cop_attr;

#[proc_macro_attribute]
pub fn cop(args: TokenStream, item: TokenStream) -> TokenStream {
    cop_attr::cop(args.into(), item.into()).into()
}

#[proc_macro_attribute]
pub fn on_node(args: TokenStream, item: TokenStream) -> TokenStream {
    cop_attr::on_node(args.into(), item.into()).into()
}
```

依存関係追加なし。`syn`/`quote`/`proc-macro2` は既存、`murphy-pattern`
経由で既に `murphy_ast::tag_from_pattern_name` が呼べる。

新規公開型なし。proc-macro 2 本のみ追加。

## 9. テスト戦略

### trybuild UI テスト(既存 `tests/ui/` に追加)

**pass**:
- `pass_cop_minimum.rs` — `#[cop(name = "...")]` + 1 個の `#[on_node]`
- `pass_cop_all_args.rs` — 全引数指定
- `pass_cop_multi_kind_methods.rs` — 異なる kind のメソッド複数
- `pass_cop_multi_attrs_one_method.rs` — 1 メソッドに `#[on_node]` を複数
- `pass_cop_helper_methods.rs` — `#[on_node]` 無しメソッド・関連 const も通す
- `pass_cop_with_derive_options.rs` — `#[derive(CopOptions)]` 連携

**fail**:
- `fail_cop_missing_name.rs`
- `fail_cop_unknown_kind.rs`
- `fail_cop_duplicate_kind.rs`
- `fail_cop_no_on_node.rs`
- `fail_cop_wrong_signature_no_self.rs`
- `fail_cop_wrong_signature_wrong_node_type.rs`
- `fail_cop_wrong_signature_wrong_cx_type.rs`
- `fail_cop_on_struct.rs`
- `fail_cop_on_trait_impl.rs`
- `fail_cop_generic_impl.rs`
- `fail_cop_unknown_arg.rs`
- `fail_cop_invalid_severity.rs`
- `fail_on_node_outside_cop.rs`

### Behavior テスト(`tests/cop_attr_behavior.rs` 新規)

1. `KINDS` の中身が正しい順に並ぶ。
2. 複数 `#[on_node]` を持つメソッドの kind が全部 `KINDS` に並ぶ。
3. `check` が node の kind tag に応じて正しいメソッドに dispatch する。
4. `KINDS` 外のノード(例: Nil) に対する `check` は no-op。
5. `register_cops!` との統合(`__internal::build_cop` が成功し、
   生成 `PluginCopV1` の `kinds_ptr`/`kinds_len`/`dispatch` が読める)。
6. `default_severity` / `default_enabled` / `options` が Cop trait の const
   に反映される。

## 10. TDD 実装サイクル

1. `lib.rs` に空の `cop` / `on_node` proc-macro 登録 → `pass_cop_minimum`
   失敗を確認。
2. `on_node` を「常に error」スタブで実装 → `fail_on_node_outside_cop` のみ通す。
3. `CopArgs` パースを実装 → `fail_cop_missing_name` / `fail_cop_unknown_arg` 通す。
4. `#[on_node]` 属性収集 + kind 解決 → `fail_cop_unknown_kind` 通す。
5. シグネチャ検証 → `fail_cop_wrong_signature_*` 通す。
6. impl 形式チェック → `fail_cop_on_struct` 等通す。
7. 重複 kind 検出 → `fail_cop_duplicate_kind` 通す。
8. `#[on_node]` 無し検出 → `fail_cop_no_on_node` 通す。
9. lowering 本体(`impl Cop` + `impl NodeCop` + stripped impl 出力)→
   `pass_*` 通す。
10. severity/enabled/options の trait const 反映 → behavior test 通す。
11. `KINDS` 順序 / dispatch / no-op の behavior test 通す。

各段階で `cargo test -p murphy-plugin-macros && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check` を通す。

## 11. 非スコープ

- `#[on_node(pattern = "(send nil? :puts $...)")]` 形式での `node_pattern!`
  との統合(scope creep 回避、将来検討)。
- `cop` でない自由関数や module 上の `#[on_node]` 集約(現状不要)。
- 既存の hand-written `impl Cop` / `impl NodeCop` を `#[cop]` 形式に
  自動移行するスクリプト(`murphy-rails` の cop 移植は murphy-9cr epic の
  別サブタスクで扱う)。
- 上位ファサード `murphy` クレートの新設(別タスク、必要になったら)。

## 12. リスクと緩和

- **`KIND_PATTERN_NAMES` の表が変わる**: 表は `murphy-ast` で frozen
  (ADR 0037、64 variant)。proc-macro 側は `tag_from_pattern_name` 関数
  呼び出しで間接化されているので、配列の並び順や追加は破壊しない。
- **`Cop`/`NodeCop` trait シグネチャ変更**: マクロ展開コードは
  `::murphy_plugin_api::Cop`/`NodeCop` のフルパス参照なので追従が必要。
  workspace 内なので CI で即座に検出できる。
- **trybuild snapshot のブレ**: rustc バージョン差で `.stderr` が
  揺れる可能性。既存の `node_pattern!` でも同じ運用なので踏襲。
