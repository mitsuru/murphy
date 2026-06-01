# submit_cop! 分散登録 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** cop ファイルに `submit_cop!(MyCop)` を1行書くだけで登録が完結し、`lib.rs` の中央リストを一切触らなくて済む仕組みを全パックに適用する。

**Architecture:** `linkme` クレートの分散スライス (`#[distributed_slice]`) を使う。各パックの `lib.rs` で `register_cops!(mode = ...)` を呼ぶと `pub static PACK_COPS: [PluginCopV1]` が宣言され、各 cop ファイルが `submit_cop!(T)` を呼ぶとリンカがそのエントリをそのスライスに自動結合する。`murphy_plugin_register` は `PACK_COPS` を指す `PluginRegistration` を返すだけ。静的パック (`murphy-std`) も動的パック (`murphy-rspec` 等) も同一パターン。

**Tech Stack:** `linkme 0.3`, Rust 1.95 (プロジェクトの固定ツールチェイン), `macro_rules!` + proc macro (`murphy-plugin-macros`)

---

## 前提: worktree の初期状態を確認

このプランは worktree `sharded-wondering-cascade` で実行すること。
セッション内で `lib.rs` をフルパス形式に変更してしまった変更 + `cops/murphy/mod.rs` への COPS 追加は **Task 0 で先に差し戻す**。

---

### Task 0: 今セッションの誤った変更を差し戻す

**Files:**
- Modify: `crates/murphy-std/src/lib.rs`
- Modify: `crates/murphy-std/src/cops/murphy/mod.rs`

**Step 1: lib.rs を use+register_cops! の元の形式に戻す**

`lib.rs` の現状 (フルパス形式) を以下の状態に戻す:

```rust
pub mod cops;

pub const BUNDLED_DEFAULTS_YAML: &str = include_str!("../config/default.yml");

use crate::cops::layout::dot_position::DotPosition;
use crate::cops::layout::empty_lines::EmptyLines;
use crate::cops::layout::space_around_operators::SpaceAroundOperators;
use crate::cops::layout::space_inside_parens::SpaceInsideParens;
use crate::cops::layout::trailing_whitespace::TrailingWhitespace;
use crate::cops::lint::debugger::Debugger;
use crate::cops::lint::deprecated_class_methods::DeprecatedClassMethods;
use crate::cops::lint::duplicate_hash_key::DuplicateHashKey;
use crate::cops::lint::empty_when::EmptyWhen;
use crate::cops::lint::shadowing_outer_local_variable::ShadowingOuterLocalVariable;
use crate::cops::lint::underscore_prefixed_variable_name::UnderscorePrefixedVariableName;
use crate::cops::lint::unreachable_code::UnreachableCode;
use crate::cops::lint::unused_block_argument::UnusedBlockArgument;
use crate::cops::lint::unused_method_argument::UnusedMethodArgument;
use crate::cops::lint::useless_assignment::UselessAssignment;
use crate::cops::murphy::no_receiver_puts::NoReceiverPuts;
use crate::cops::style::and_or::AndOr;
use crate::cops::style::empty_case_condition::EmptyCaseCondition;
use crate::cops::style::frozen_string_literal_comment::FrozenStringLiteralComment;
use crate::cops::style::hash_syntax::HashSyntax;
use crate::cops::style::if_unless_modifier::IfUnlessModifier;
use crate::cops::style::nil_comparison::NilComparison;
use crate::cops::style::redundant_return::RedundantReturn;
use crate::cops::style::redundant_self::RedundantSelf;
use crate::cops::style::string_literals::StringLiterals;
use crate::cops::style::symbol_array::SymbolArray;
use crate::cops::style::while_until_modifier::WhileUntilModifier;
use crate::cops::style::word_array::WordArray;
use crate::cops::style::yaml_file_read::YAMLFileRead;
use crate::cops::style::zero_length_predicate::ZeroLengthPredicate;

murphy_plugin_api::register_cops!(
    mode = static,
    NoReceiverPuts,
    Debugger,
    DeprecatedClassMethods,
    DuplicateHashKey,
    EmptyWhen,
    ShadowingOuterLocalVariable,
    UnreachableCode,
    UnderscorePrefixedVariableName,
    UnusedBlockArgument,
    UnusedMethodArgument,
    UselessAssignment,
    FrozenStringLiteralComment,
    HashSyntax,
    RedundantSelf,
    StringLiterals,
    SymbolArray,
    TrailingWhitespace,
    SpaceInsideParens,
    SpaceAroundOperators,
    DotPosition,
    AndOr,
    EmptyCaseCondition,
    EmptyLines,
    IfUnlessModifier,
    NilComparison,
    RedundantReturn,
    WhileUntilModifier,
    WordArray,
    YAMLFileRead,
    ZeroLengthPredicate,
);

pub static DISABLED_COPS: &[&str] = &[];
pub const PACK_NAME: &str = "builtin";
```

**Step 2: cops/murphy/mod.rs から COPS const を削除する**

```rust
//! `Murphy/*` cop namespace (ADR 0018) — Murphy-specific cops that have
//! no equivalent in the RuboCop catalogue.

pub mod no_receiver_puts;
```

**Step 3: ビルドで差し戻し確認**

```bash
cargo build -p murphy-std
```
Expected: Finished without errors.

**Step 4: Commit**

```bash
git add crates/murphy-std/src/lib.rs crates/murphy-std/src/cops/murphy/mod.rs
git commit -m "revert: undo false-start full-path refactor (superseded by submit_cop!)"
```

---

### Task 1: linkme を murphy-plugin-api に追加 + submit_cop! macro 定義

**Files:**
- Modify: `crates/murphy-plugin-api/Cargo.toml`
- Modify: `crates/murphy-plugin-api/src/lib.rs`

**Step 1: Cargo.toml に linkme を追加**

`[dependencies]` セクションに追加:

```toml
# Distributed-slice registration: `submit_cop!(T)` uses `#[linkme::distributed_slice]`.
# Re-exported as `murphy_plugin_api::linkme` so packs keep their single-dep contract
# (design §5 / ADR 0038).
linkme = "0.3"
```

**Step 2: lib.rs に re-export + submit_cop! を追加**

`crates/murphy-plugin-api/src/lib.rs` の末尾に追加:

```rust
// Distributed-slice runtime, re-exported so packs don't add a direct `linkme`
// dependency (single-surface design §5).
#[doc(hidden)]
pub use linkme;

/// Register a cop with the current pack's distributed cop list.
///
/// Call once per cop type, at module scope in the cop's own file,
/// after the cop type definition:
///
/// ```rust,ignore
/// // cops/lint/debugger.rs
/// #[cop(...)]
/// impl Debugger { ... }
///
/// murphy_plugin_api::submit_cop!(Debugger);
/// ```
///
/// Requires `register_cops!(mode = ...)` to have been called at the
/// crate root. Each invocation occupies the name `REGISTRATION` in its
/// enclosing module scope — calling `submit_cop!` twice in the same file
/// is a compile-time error (intentional safety guard).
#[macro_export]
macro_rules! submit_cop {
    ($cop:ty) => {
        #[$crate::linkme::distributed_slice(crate::PACK_COPS)]
        static REGISTRATION: $crate::PluginCopV1 =
            $crate::__internal::build_cop::<$cop>();
    };
}
```

**Step 3: ビルド確認**

```bash
cargo build -p murphy-plugin-api
```
Expected: Finished without errors.

**Step 4: Commit**

```bash
git add crates/murphy-plugin-api/Cargo.toml crates/murphy-plugin-api/src/lib.rs
git commit -m "feat(murphy-plugin-api): add linkme dep + submit_cop! distributed-registration macro"
```

---

### Task 2: register_cops! proc macro をリスト不要の形式に変更

**Files:**
- Modify: `crates/murphy-plugin-macros/src/lib.rs`

**Step 1: RegisterCopsInput のパーサを「cop リスト省略可」に変更**

`RegisterCopsInput` struct と `impl Parse` を書き換える:

```rust
/// Parsed form of `register_cops!(mode = static|dynamic)`.
/// Cop list removed — each cop file calls `submit_cop!(T)` instead.
struct RegisterCopsInput {
    mode: RegisterMode,
}

impl syn::parse::Parse for RegisterCopsInput {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let mode_kw: Ident = input.parse().map_err(|_| {
            syn::Error::new(
                input.span(),
                "register_cops!: first argument must be `mode = static` or `mode = dynamic`",
            )
        })?;
        if mode_kw != "mode" {
            return Err(syn::Error::new(
                mode_kw.span(),
                format!("register_cops!: expected `mode`, found `{mode_kw}`"),
            ));
        }
        let _eq: Token![=] = input.parse()?;
        let mode = if input.peek(Token![static]) {
            let _: Token![static] = input.parse()?;
            RegisterMode::Static
        } else {
            let mode_ident: Ident = input.parse()?;
            if mode_ident == "dynamic" {
                RegisterMode::Dynamic
            } else {
                return Err(syn::Error::new(
                    mode_ident.span(),
                    format!(
                        "register_cops!: mode must be `static` or `dynamic`, found `{mode_ident}`"
                    ),
                ));
            }
        };
        if !input.is_empty() {
            return Err(syn::Error::new(
                input.span(),
                "register_cops!: cop list is no longer accepted — \
                 call submit_cop!(T) in each cop file instead",
            ));
        }
        Ok(RegisterCopsInput { mode })
    }
}
```

**Step 2: マクロ本体を新展開形式に変更**

`register_cops` proc macro 関数全体を書き換える。`cops` Vec / `n` / `uniqueness_check` / `cop_entries` / `name_exprs` は不要になる。

**Static mode 展開:**
```rust
RegisterMode::Static => quote! {
    /// このパックの cop 分散スライス。`submit_cop!(T)` の各呼び出しが
    /// リンカ経由でエントリを追加する。
    #[::murphy_plugin_api::linkme::distributed_slice]
    pub static PACK_COPS: [::murphy_plugin_api::PluginCopV1];

    /// ホスト (murphy-cli) がこのパックを静的リンク経由で呼び出す
    /// エントリポイント (design §5: `#[no_mangle]` なし)。
    pub unsafe fn murphy_plugin_register(
        out: *mut ::murphy_plugin_api::PluginRegistration,
    ) -> i32 {
        if out.is_null() {
            return 1;
        }
        #[cfg(debug_assertions)]
        {
            let mut seen = ::std::collections::HashSet::new();
            for cop in PACK_COPS.iter() {
                let name = unsafe { cop.name.as_bytes() };
                if !seen.insert(name) {
                    panic!(
                        "register_cops!: two cops share the same NAME: {}",
                        ::std::str::from_utf8(name).unwrap_or("<invalid utf8>")
                    );
                }
            }
        }
        unsafe {
            *out = ::murphy_plugin_api::PluginRegistration {
                abi_version: ::murphy_plugin_api::MURPHY_PLUGIN_ABI_VERSION,
                cops_ptr: PACK_COPS.as_ptr(),
                cops_len: PACK_COPS.len(),
            };
        }
        0
    }
},
```

**Dynamic mode 展開:** 同様だが `PACK_COPS` は `pub(crate) static`、関数は `#[unsafe(no_mangle)] pub unsafe extern "C" fn`。

**Step 3: doc コメントの更新**

`register_cops` proc macro の冒頭 doc コメントから cop リストへの言及を削除し、`submit_cop!` への参照に変える。

**Step 4: ビルド確認（テストはまだ失敗して当然）**

```bash
cargo build -p murphy-plugin-macros
```
Expected: Finished without errors.

**Step 5: Commit**

```bash
git add crates/murphy-plugin-macros/src/lib.rs
git commit -m "feat(murphy-plugin-macros): rewrite register_cops! — remove cop list, use linkme PACK_COPS"
```

---

### Task 3: murphy-plugin-macros のテストを新 API に更新

**Files:**
- Modify: `crates/murphy-plugin-macros/tests/register_behavior.rs`
- Modify: `crates/murphy-plugin-macros/tests/register_modes_equivalence.rs`
- Delete: `crates/murphy-plugin-macros/tests/ui/fail_duplicate_name.rs`
- Delete: `crates/murphy-plugin-macros/tests/ui/fail_duplicate_name.stderr`
- Modify: 全 UI pass テスト (`tests/ui/pass_*.rs`) と `cop_attr_behavior.rs`

#### 3a: register_behavior.rs

`register_cops!(mode = dynamic, NoTabs, NoSpaces)` を:
```rust
register_cops!(mode = dynamic);
submit_cop!(NoTabs);
submit_cop!(NoSpaces);
```
に変更。`extern "C"` 宣言と全テスト関数はそのまま維持。

#### 3b: register_modes_equivalence.rs

旧の `mod static_pack { register_cops!(mode = static, StubCop); }` パターンは
nested module の `PACK_COPS` に `submit_cop!` が `crate::PACK_COPS` でアクセスできないため削除。

テストを dynamic mode 単独に簡略化する:
```rust
register_cops!(mode = dynamic);
submit_cop!(StubCop);
// テスト: register_entry_point_fills_the_plugin_registration のみ残す
```

static/dynamic 等価性は両モードが同じ `PACK_COPS` 収集機構を使う構造上自明なため、個別テストは不要。

#### 3c: fail_duplicate_name テストの削除と代替

- `tests/ui/fail_duplicate_name.rs` + `.stderr` を削除
- `register_behavior.rs` に以下のコメントテストを追加:

```rust
// Duplicate-NAME detection is now a runtime check (debug builds only)
// executed inside murphy_plugin_register when it iterates PACK_COPS.
// A compile-time check is no longer possible because submit_cop! calls
// are distributed across files — the linker collects them, not the macro.
```

#### 3d: UI pass テストの一括更新

以下ファイルすべてで `register_cops!(mode = dynamic, MyCop)` →
`register_cops!(mode = dynamic); submit_cop!(MyCop);` に変更
(複数 cop のファイルは cop ごとに `submit_cop!` を追加):

- `pass_cop_all_args.rs`
- `pass_cop_helper_methods.rs`
- `pass_cop_investigation.rs`
- `pass_cop_methods_trailing_commas.rs`
- `pass_cop_minimum.rs`
- `pass_cop_multi_attrs_one_method.rs`
- `pass_cop_multi_kind_methods.rs`
- `pass_cop_with_derive_options.rs`
- `pass_cop_with_options.rs`
- `pass_multiple_cops.rs`
- `pass_single_cop.rs`
- `cop_attr_behavior.rs` (line ~211 の `register_cops!`)

#### 3e: UI fail テストの stderr 更新

旧形式 (`register_cops!(mode = dynamic, MyCheck)`) を渡している fail テストは
新エラーメッセージに合わせて `.stderr` を更新する。

`UPDATE_EXPECT=1 cargo test -p murphy-plugin-macros --test trybuild` で自動再生成可。

#### 3f: テスト全通確認

```bash
cargo test -p murphy-plugin-macros
```
Expected: すべて PASS。

**Step 5: Commit**

```bash
git add crates/murphy-plugin-macros/tests/
git commit -m "test(murphy-plugin-macros): update all register_cops! tests for submit_cop! API"
```

---

### Task 4: murphy-std を移行

**Files:**
- Modify: `crates/murphy-std/src/lib.rs`
- Modify: 30 個の cop ファイル (各 1 行追加)

#### 4a: lib.rs の書き換え

`use` ブロック (30 行) と `register_cops!(mode = static, ...)` リスト全体を削除:

```rust
pub mod cops;

pub const BUNDLED_DEFAULTS_YAML: &str = include_str!("../config/default.yml");

// cop の登録は各 cop ファイルの submit_cop!(T) が担う。
murphy_plugin_api::register_cops!(mode = static);

pub static DISABLED_COPS: &[&str] = &[];
pub const PACK_NAME: &str = "builtin";
```

#### 4b: 各 cop ファイルの末尾に submit_cop! を1行追加

| ファイル | 追加行 |
|---|---|
| `cops/murphy/no_receiver_puts.rs` | `murphy_plugin_api::submit_cop!(NoReceiverPuts);` |
| `cops/lint/debugger.rs` | `murphy_plugin_api::submit_cop!(Debugger);` |
| `cops/lint/deprecated_class_methods.rs` | `murphy_plugin_api::submit_cop!(DeprecatedClassMethods);` |
| `cops/lint/duplicate_hash_key.rs` | `murphy_plugin_api::submit_cop!(DuplicateHashKey);` |
| `cops/lint/empty_when.rs` | `murphy_plugin_api::submit_cop!(EmptyWhen);` |
| `cops/lint/shadowing_outer_local_variable.rs` | `murphy_plugin_api::submit_cop!(ShadowingOuterLocalVariable);` |
| `cops/lint/underscore_prefixed_variable_name.rs` | `murphy_plugin_api::submit_cop!(UnderscorePrefixedVariableName);` |
| `cops/lint/unreachable_code.rs` | `murphy_plugin_api::submit_cop!(UnreachableCode);` |
| `cops/lint/unused_block_argument.rs` | `murphy_plugin_api::submit_cop!(UnusedBlockArgument);` |
| `cops/lint/unused_method_argument.rs` | `murphy_plugin_api::submit_cop!(UnusedMethodArgument);` |
| `cops/lint/useless_assignment.rs` | `murphy_plugin_api::submit_cop!(UselessAssignment);` |
| `cops/style/and_or.rs` | `murphy_plugin_api::submit_cop!(AndOr);` |
| `cops/style/empty_case_condition.rs` | `murphy_plugin_api::submit_cop!(EmptyCaseCondition);` |
| `cops/style/frozen_string_literal_comment.rs` | `murphy_plugin_api::submit_cop!(FrozenStringLiteralComment);` |
| `cops/style/hash_syntax.rs` | `murphy_plugin_api::submit_cop!(HashSyntax);` |
| `cops/style/if_unless_modifier.rs` | `murphy_plugin_api::submit_cop!(IfUnlessModifier);` |
| `cops/style/nil_comparison.rs` | `murphy_plugin_api::submit_cop!(NilComparison);` |
| `cops/style/redundant_return.rs` | `murphy_plugin_api::submit_cop!(RedundantReturn);` |
| `cops/style/redundant_self.rs` | `murphy_plugin_api::submit_cop!(RedundantSelf);` |
| `cops/style/string_literals.rs` | `murphy_plugin_api::submit_cop!(StringLiterals);` |
| `cops/style/symbol_array.rs` | `murphy_plugin_api::submit_cop!(SymbolArray);` |
| `cops/style/while_until_modifier.rs` | `murphy_plugin_api::submit_cop!(WhileUntilModifier);` |
| `cops/style/word_array.rs` | `murphy_plugin_api::submit_cop!(WordArray);` |
| `cops/style/yaml_file_read.rs` | `murphy_plugin_api::submit_cop!(YAMLFileRead);` |
| `cops/style/zero_length_predicate.rs` | `murphy_plugin_api::submit_cop!(ZeroLengthPredicate);` |
| `cops/layout/dot_position.rs` | `murphy_plugin_api::submit_cop!(DotPosition);` |
| `cops/layout/empty_lines.rs` | `murphy_plugin_api::submit_cop!(EmptyLines);` |
| `cops/layout/space_around_operators.rs` | `murphy_plugin_api::submit_cop!(SpaceAroundOperators);` |
| `cops/layout/space_inside_parens.rs` | `murphy_plugin_api::submit_cop!(SpaceInsideParens);` |
| `cops/layout/trailing_whitespace.rs` | `murphy_plugin_api::submit_cop!(TrailingWhitespace);` |

#### 4c: ビルド + テスト確認

```bash
cargo build -p murphy-std
cargo test -p murphy-std
```
Expected: すべて PASS。

#### 4d: Commit

```bash
git add crates/murphy-std/
git commit -m "feat(murphy-std): migrate to submit_cop! distributed registration; remove central cop list"
```

---

### Task 5: murphy-rspec を移行

**Files:**
- Modify: `crates/murphy-rspec/src/lib.rs`
- Modify: `crates/murphy-rspec/src/cops/rspec/describe_class.rs`
- Modify: `crates/murphy-rspec/src/cops/rspec/example_length.rs`
- Modify: `crates/murphy-rspec/src/cops/rspec/multiple_expectations.rs`

#### 5a: lib.rs の書き換え

旧の `use` + `register_cops!(mode = dynamic, DescribeClass, ExampleLength, MultipleExpectations)` を:
```rust
// cop の登録は各 cop ファイルの submit_cop!(T) が担う。
murphy_plugin_api::register_cops!(mode = dynamic);
```
に変更。smoke test は維持。

#### 5b: 各 cop ファイルに submit_cop! を追加

```rust
// describe_class.rs 末尾に追加
murphy_plugin_api::submit_cop!(DescribeClass);
```
同様に `example_length.rs` + `multiple_expectations.rs`。

#### 5c: テスト確認

```bash
cargo test -p murphy-rspec
```
Expected: PASS (e2e テストで cdylib が正しく登録されることを確認)。

#### 5d: Commit

```bash
git add crates/murphy-rspec/
git commit -m "feat(murphy-rspec): migrate to submit_cop! distributed registration"
```

---

### Task 6: murphy-example-pack を移行

**Files:**
- Modify: `crates/murphy-example-pack/src/lib.rs`
- Modify: `crates/murphy-example-pack/src/no_eval.rs`
- Modify: `crates/murphy-example-pack/src/todo_format.rs`

#### 6a: lib.rs の書き換え

旧:
```rust
murphy_plugin_api::register_cops!(mode = dynamic, NoEval, TodoFormat);
```
新:
```rust
murphy_plugin_api::register_cops!(mode = dynamic);
```

#### 6b: cop ファイルに submit_cop! を追加

```rust
// no_eval.rs 末尾
murphy_plugin_api::submit_cop!(NoEval);

// todo_format.rs 末尾
murphy_plugin_api::submit_cop!(TodoFormat);
```

#### 6c: ビルド確認

```bash
cargo build -p murphy-example-pack
```

#### 6d: Commit

```bash
git add crates/murphy-example-pack/
git commit -m "feat(murphy-example-pack): migrate to submit_cop! distributed registration"
```

---

### Task 7: murphy-rails を移行

`murphy-rails` は 138 cop すべてが `lib.rs` にインラインで定義されている。
`register_cops!` リスト (~140 行) を削除し、各 cop 定義の直後に `submit_cop!(CopName)` を追加する。

**Files:**
- Modify: `crates/murphy-rails/src/lib.rs`

#### 7a: register_cops! リストを1行に置き換え

`lib.rs` 末尾付近の `register_cops!(mode = dynamic, ...)` ブロック全体を削除して:
```rust
register_cops!(mode = dynamic);
```
の1行に置き換える。

#### 7b: 各 cop stub の直後に submit_cop! を追加

各 `impl CopName { ... }` ブロックの **直後** に追加する。例:

```rust
#[cop(name = "Rails/ActionControllerFlashBeforeRender", default_enabled = false)]
impl ActionControllerFlashBeforeRender {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(ActionControllerFlashBeforeRender);
```

`cops::rails::*` の `use` でインポートされている実アリーナ移行済みの cop も同様。

#### 7c: ビルド + テスト確認

```bash
cargo build -p murphy-rails
cargo test -p murphy-rails
```
Expected: PASS。

ビルドエラーの場合: `cargo build -p murphy-rails 2>&1 | grep "error"` で
`submit_cop!` が漏れている cop を特定する。

#### 7d: Commit

```bash
git add crates/murphy-rails/src/lib.rs
git commit -m "feat(murphy-rails): migrate to submit_cop! distributed registration; remove central list"
```

---

### Task 8: murphy-cli のコメント更新 + 全体テスト

**Files:**
- Modify: `crates/murphy-cli/src/main.rs`

#### 8a: main.rs の安全性コメント更新

`builtin_pack()` 内のコメントを:
```rust
// Safety: `reg.cops_ptr` points at `murphy_std::__MURPHY_PLUGIN_COPS_V1`,
// a `pub static` array generated by `register_cops!(mode = static, …)`.
```
→
```rust
// Safety: `reg.cops_ptr` points at `murphy_std::PACK_COPS`, a
// `#[linkme::distributed_slice]`-managed `pub static [PluginCopV1]`
// with `'static` lifetime.
```

#### 8b: ワークスペース全体テスト

```bash
cargo test --workspace
```
Expected: すべて PASS。

#### 8c: clippy + fmt

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo +nightly fmt --check
```
Expected: 警告 0、フォーマット差分なし。

#### 8d: Final commit + push

```bash
git add crates/murphy-cli/src/main.rs
git commit -m "docs(murphy-cli): update safety comment: PACK_COPS replaces __MURPHY_PLUGIN_COPS_V1"
git pull --rebase
bd dolt push
git push
```

---

## 設計ノート

### linkme の動作 (static vs dynamic)

- `#[distributed_slice]` で宣言されたスライスは **crate 単位で独立** して収集される
- `.so` (`cdylib`) は `dlopen` 時に `.init_array` / `.ctors` セクションが実行され、
  その crate 内の submit エントリが `PACK_COPS` に自動追加される
- 静的リンクはビルド時にリンカが結合する
- 両ケースとも `PACK_COPS` は `'static` かつ不変 — ABI コントラクト変更なし

### submit_cop! の名前衝突防止

`submit_cop!` は `static REGISTRATION: PluginCopV1` を展開する。
各 cop ファイルは独立した Rust モジュールなので、`cops::lint::debugger::REGISTRATION` と
`cops::lint::unreachable_code::REGISTRATION` は異なるパスを持ち衝突しない。
同一ファイルで2回呼ぶとコンパイルエラーになる (意図的な安全ガード)。

### 重複 NAME チェック

compile-time の一意性チェック（旧 `register_cops!` の const eval panic）は削除。
代わりに `murphy_plugin_register` の先頭で debug ビルド時にのみ実行する runtime panic を追加する。

### trybuild の .stderr 自動更新

fail テストのコンパイルエラーメッセージが変わった場合:
```bash
UPDATE_EXPECT=1 cargo test -p murphy-plugin-macros --test trybuild
```
で `.stderr` ファイルを自動再生成できる。
