# node_pattern! B backend Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** `murphy-plugin-macros` に function-like proc macro `node_pattern!` を追加し、S 式パターンをコンパイル時に Rust の matcher `fn` へ lowering する。

**Architecture:** `node_pattern!(name, "pattern")` が module レベルの `fn name` を生成する。マクロはコンパイル時に `murphy_pattern::parse` でパターンをパースし、`PatternAst` を `match`/`if-let`/`let-else`/`loop` 列へ再帰下降で lowering する。NodeKind ごとの「パターン子スロット」スキーマをマクロ内にハードコードする。`$...` の zero-alloc 借用のため `murphy-plugin-api` の `Cx` に `list()` accessor を additive 追加する。

**Tech Stack:** Rust edition 2024、proc-macro(`syn` / `quote` / `proc-macro2`)、`murphy-pattern`(パターンパーサ)、`murphy-ast`(arena AST / `NodeKindTag`)、`murphy-plugin-api`(`Cx`)、`trybuild`(UI テスト)。

設計の出典: `docs/plans/2026-05-23-murphy-9cr18-node-pattern-macro.md`、`docs/plans/2026-05-22-plugin-reboot-design.md` §4、beads issue murphy-9cr.18。

---

## グラウンドルール

- **TDD 必須**: 各タスクは failing test → 実行して fail 確認 → 最小実装 → pass 確認 → commit。
- **コミット粒度**: タスクごとに 1 コミット以上。メッセージは `feat(murphy-plugin-macros): ...` / `feat(murphy-plugin-api): ...`。
- **品質ゲート**: 各タスク完了時に該当クレートの `cargo test` を通す。最終タスクで workspace 全体の `cargo test` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo fmt --check`。
- **シェル安全**: `rm -f` / `cp -f` など非対話形式を使う。
- **proc macro の TDD 制約**: マクロが未対応の `PatKind` に当たったら `panic!` ではなく `compile_error!` を出すこと。これにより未実装機能を使うテストだけがコンパイルエラーになり、実装済み機能のテストは独立して通る。

---

## Task 1: `Cx::list` accessor を `murphy-plugin-api` に追加

`NodeList`(`Send.args` 等の可変長子)を zero-copy の借用スライスで取り出す accessor を `Cx` に足す。生成コードが `$...` と固定長 args 照合の両方で使う。

**Files:**
- Modify: `crates/murphy-plugin-api/src/cx.rs`(`children` メソッドの隣、および `tests` モジュール)

**Step 1: failing test を書く**

`crates/murphy-plugin-api/src/cx.rs` の `tests` モジュールに追加(`accessors_match_the_underlying_ast` テストの後ろ):

```rust
#[test]
fn list_resolves_node_list_to_a_borrowed_slice() {
    use murphy_ast::{AstBuilder, NodeKind, NodeList, OptNodeId, Range, Symbol};

    // `foo(1, 2)` — a Send whose `args` NodeList holds two Int nodes.
    let mut b = AstBuilder::new("foo(1, 2)", "t.rb".to_string());
    let one = b.push(NodeKind::Int(1), Range { start: 4, end: 5 });
    let two = b.push(NodeKind::Int(2), Range { start: 7, end: 8 });
    let args = b.push_list(&[one, two]);
    let method = b.intern_symbol("foo");
    let root = b.push(
        NodeKind::Send {
            receiver: OptNodeId::NONE,
            method,
            args,
        },
        Range { start: 0, end: 9 },
    );
    let ast = b.finish(root);

    let fns = FnTable {
        emit_offense: noop_offense,
        emit_edit: noop_edit,
    };
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    // Pull the `args` NodeList back out of the Send and resolve it.
    let NodeKind::Send { args, .. } = *cx.kind(root) else {
        panic!("expected Send");
    };
    assert_eq!(cx.list(args), &[one, two]);
    // An empty NodeList resolves to an empty slice.
    assert_eq!(cx.list(NodeList::EMPTY), &[] as &[murphy_ast::NodeId]);
}
```

**Step 2: test を実行して fail を確認**

Run: `cargo test -p murphy-plugin-api list_resolves`
Expected: コンパイルエラー(`Cx::list` 未定義)。

**Step 3: 最小実装**

`crates/murphy-plugin-api/src/cx.rs` の `impl<'a> Cx<'a>` 内、`children` メソッドの直前に追加。`NodeList` の import を `use murphy_ast::{...}` 行に足す:

```rust
/// Resolve a [`NodeList`] to its backing slice of child ids.
///
/// Zero-copy: returns a borrow directly into the arena's `node_lists`
/// side table. This is the allocation-free counterpart to
/// [`Self::children`] for the variable-length child field of a single
/// `NodeKind` variant (e.g. `Send.args`, `Array`'s elements). The
/// generated code of `node_pattern!` (murphy-9cr.18) uses it to bind
/// `$...` seq captures and to match fixed-length argument lists.
pub fn list(&self, l: murphy_ast::NodeList) -> &'a [NodeId] {
    let start = l.start as usize;
    &self.lists()[start..start + l.len as usize]
}
```

`NodeList` は `murphy_ast::NodeList::EMPTY`(`start: 0, len: 0`)に対しても `lists()[0..0]` で空スライスを返す(`lists()` が空でも `&[][0..0]` は valid)。

**Step 4: test を実行して pass を確認**

Run: `cargo test -p murphy-plugin-api`
Expected: PASS(既存テスト含め全通過)。

**Step 5: commit**

```bash
git add crates/murphy-plugin-api/src/cx.rs
git commit -m "feat(murphy-plugin-api): add Cx::list zero-copy NodeList accessor"
```

---

## Task 2: `node_pattern!` マクロ骨格 — `_` のみ・パースエラー処理

クレート依存を整え、`node_pattern!` proc macro を追加する。この段階では `Wildcard`(`_`)だけを lowering し、未対応 `PatKind` と未対応 head は `compile_error!` を出す。パターンのパースエラーを `compile_error!` に写す。

**Files:**
- Modify: `crates/murphy-plugin-macros/Cargo.toml`
- Create: `crates/murphy-plugin-macros/src/node_pattern.rs`
- Modify: `crates/murphy-plugin-macros/src/lib.rs`
- Create: `crates/murphy-plugin-macros/tests/node_pattern_behavior.rs`

**Step 1: failing test を書く**

`crates/murphy-plugin-macros/tests/node_pattern_behavior.rs` を新規作成。先頭にテストヘルパ(`Ast` + `Cx` 構築。`murphy-plugin-api` の `cx.rs` テストと同型)を置く:

```rust
//! Behaviour tests for `node_pattern!`: define matchers, build a small
//! arena, run them, assert the result and captures.

use murphy_ast::{Ast, AstBuilder, NodeId, NodeKind, NodeList, OptNodeId, Range, Symbol};
use murphy_plugin_api::{Cx, CxRaw, FnTable, RawSlice};

unsafe extern "C" fn noop_offense(_: *mut std::ffi::c_void, _: *const murphy_plugin_api::RawOffense) {
}
unsafe extern "C" fn noop_edit(_: *mut std::ffi::c_void, _: *const murphy_plugin_api::RawEdit) {}

/// A `FnTable` whose callbacks are never invoked by matcher code.
fn fns() -> FnTable {
    FnTable {
        emit_offense: noop_offense,
        emit_edit: noop_edit,
    }
}

/// Build a `CxRaw` borrowing `ast` and `fns` for `'a`.
fn cx_raw_for<'a>(ast: &'a Ast, fns: &'a FnTable) -> CxRaw {
    let p = ast.raw_parts();
    CxRaw {
        nodes: p.nodes.as_ptr(),
        nodes_len: p.nodes.len(),
        lists: p.node_lists.as_ptr(),
        lists_len: p.node_lists.len(),
        interner_blob: p.interner_blob.as_ptr(),
        interner_blob_len: p.interner_blob.len(),
        interner_offsets: p.interner_offsets.as_ptr(),
        interner_offsets_len: p.interner_offsets.len(),
        comments: p.comments.as_ptr(),
        comments_len: p.comments.len(),
        source: p.source.as_ptr(),
        source_len: p.source.len(),
        root: p.root,
        cop_name: RawSlice::EMPTY,
        fns: fns as *const FnTable,
        sink: std::ptr::null_mut(),
    }
}

fn r() -> Range {
    Range { start: 0, end: 1 }
}

use murphy_plugin_macros::node_pattern;

node_pattern!(any_node, "_");

#[test]
fn wildcard_matches_any_node() {
    let mut b = AstBuilder::new("nil", "t.rb");
    let root = b.push(NodeKind::Nil, r());
    let ast = b.finish(root);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    // Zero captures -> the matcher returns `bool`.
    assert!(any_node(root, &cx));
}
```

**Step 2: test を実行して fail を確認**

Run: `cargo test -p murphy-plugin-macros --test node_pattern_behavior`
Expected: コンパイルエラー(`node_pattern` マクロ未定義)。

**Step 3: 実装**

**3a. `Cargo.toml`** — `[dependencies]` を編集。proc macro はコンパイル時に `murphy_pattern::parse` を呼び、`murphy_ast::NodeKindTag` 等を名指しするため、両クレートを通常依存にする:

```toml
[dependencies]
proc-macro2 = "1"
quote = "1"
syn = { version = "2", default-features = false, features = ["parsing", "proc-macro", "printing", "clone-impls", "derive", "full"] }
serde_json = "1"
# node_pattern! parses pattern strings at macro-expansion time.
murphy-pattern = { path = "../murphy-pattern" }
# NodeKindTag / pattern-name resolution for the schema table.
murphy-ast = { path = "../murphy-ast" }
```

`[dev-dependencies]` から `murphy-ast` の行を削除する(`[dependencies]` へ昇格したため重複不可)。`murphy-plugin-api` は dev-dependency のまま残す。

**3b. `src/node_pattern.rs`** を新規作成。モジュール骨格:

```rust
//! `node_pattern!` — the B backend of Murphy's pattern mechanism
//! (murphy-9cr.18). Lowers an S-expression pattern to a Rust matcher
//! `fn` at compile time. See
//! `docs/plans/2026-05-23-murphy-9cr18-node-pattern-macro.md`.

use proc_macro2::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Ident, LitStr, Token};

use murphy_pattern::{CaptureKind, PatternAst};

/// Parsed `node_pattern!(name, "pattern")` invocation.
struct NodePatternInput {
    name: Ident,
    pattern: LitStr,
}

impl Parse for NodePatternInput {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let name = input.parse()?;
        input.parse::<Token![,]>()?;
        let pattern = input.parse()?;
        Ok(NodePatternInput { name, pattern })
    }
}

/// Entry point for the `#[proc_macro] node_pattern`.
pub fn node_pattern(input: TokenStream) -> TokenStream {
    let input: NodePatternInput = match syn::parse2(input) {
        Ok(v) => v,
        Err(e) => return e.to_compile_error(),
    };
    let ast = match murphy_pattern::parse(&input.pattern.value()) {
        Ok(a) => a,
        Err(e) => {
            return syn::Error::new(
                input.pattern.span(),
                format!("node_pattern!: pattern parse error: {e}"),
            )
            .to_compile_error();
        }
    };
    match lower_matcher(&input.name, &ast) {
        Ok(ts) => ts,
        Err(e) => e.to_compile_error(),
    }
}

/// Build the whole matcher `fn` from a parsed pattern.
fn lower_matcher(name: &Ident, ast: &PatternAst) -> syn::Result<TokenStream> {
    let n_caps = ast.n_captures();
    // Return type: `bool` for zero captures, `Option<(..)>` otherwise.
    let cap_tys: Vec<TokenStream> = ast
        .capture_kinds()
        .iter()
        .map(|k| match k {
            CaptureKind::Node => quote!(::murphy_ast::NodeId),
            CaptureKind::Seq => quote!(&'a [::murphy_ast::NodeId]),
        })
        .collect();
    let cap_decls: Vec<TokenStream> = (0..n_caps)
        .map(|i| {
            let id = cap_ident(i);
            let ty = &cap_tys[i];
            quote!(let #id: #ty;)
        })
        .collect();
    let cap_idents: Vec<Ident> = (0..n_caps).map(cap_ident).collect();

    let (ret_ty, fail, success) = if n_caps == 0 {
        (quote!(bool), quote!(false), quote!(true))
    } else {
        (
            quote!(::core::option::Option<(#(#cap_tys,)*)>),
            quote!(::core::option::Option::None),
            quote!(::core::option::Option::Some((#(#cap_idents,)*))),
        )
    };

    let mut ctx = Lower {
        fail: fail.clone(),
        capture_allowed: true,
    };
    let body = lower_pat(&ast.root, &quote!(node), &mut ctx)?;

    Ok(quote! {
        fn #name<'a>(
            node: ::murphy_ast::NodeId,
            cx: &::murphy_plugin_api::Cx<'a>,
        ) -> #ret_ty {
            #(#cap_decls)*
            #body
            #success
        }
    })
}

/// The capture binding identifier for slot `i`.
fn cap_ident(i: usize) -> Ident {
    Ident::new(&format!("__cap{i}"), proc_macro2::Span::call_site())
}

/// Mutable state threaded through the recursive lowering.
struct Lower {
    /// The expression a failed guard returns (`false` or `None`).
    fail: TokenStream,
    /// Whether a `$` capture is legal at the current position. Set false
    /// inside `{}` union, `!` negation and `` ` `` descend.
    capture_allowed: bool,
}

/// Lower one `Pat` against `subject` (a `NodeId`-typed expression) into a
/// block of guard statements that `return ctx.fail` on mismatch.
fn lower_pat(
    pat: &murphy_pattern::Pat,
    subject: &TokenStream,
    ctx: &mut Lower,
) -> syn::Result<TokenStream> {
    use murphy_pattern::PatKind;
    match &pat.kind {
        PatKind::Wildcard => Ok(quote!()),
        other => Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            format!("node_pattern!: pattern feature not yet supported: {other:?}"),
        )),
    }
}
```

> 注: この骨格では `lower_pat` が `Wildcard` 以外を `compile_error` 化する。以降のタスクで `lower_pat` の `match` 腕を埋めていく。`syn::Error` の span は v1 ではパターン文字列全体相当(`Span::call_site()`)で良い — サブ span 精密化は follow-up。

**3c. `src/lib.rs`** — モジュール宣言とエントリを追加。ファイル冒頭の `mod cop_options;` の隣に:

```rust
mod node_pattern;
```

を足し、`derive_cop_options` 関数の後ろに proc macro エントリを追加:

```rust
/// Define a compile-time AST pattern matcher (B backend, murphy-9cr.18).
///
/// `node_pattern!(name, "pattern")` expands to a module-level
/// `fn name(node, cx)` that tests whether `node` matches the
/// S-expression `pattern`. With zero `$` captures the matcher returns
/// `bool`; with one or more it returns `Option<(captures…)>` in slot
/// order (`$_` → `NodeId`, `$...` → `&[NodeId]`).
///
/// # Example
///
/// ```ignore
/// use murphy_plugin_macros::node_pattern;
///
/// node_pattern!(is_puts_call, "(send nil? :puts $...)");
/// ```
#[proc_macro]
pub fn node_pattern(input: TokenStream) -> TokenStream {
    node_pattern::node_pattern(input.into()).into()
}
```

**Step 4: test を実行して pass を確認**

Run: `cargo test -p murphy-plugin-macros --test node_pattern_behavior`
Expected: PASS。`cargo test -p murphy-plugin-macros` も既存テスト含め通ること。

**Step 5: commit**

```bash
git add crates/murphy-plugin-macros/Cargo.toml crates/murphy-plugin-macros/src/node_pattern.rs \
        crates/murphy-plugin-macros/src/lib.rs \
        crates/murphy-plugin-macros/tests/node_pattern_behavior.rs
git commit -m "feat(murphy-plugin-macros): node_pattern! skeleton with wildcard lowering"
```

---

## Task 3: リテラル・裸 kind 名・`nil?`(ノード位置)

atom 系の照合を実装する。`PatKind::Lit` / `PatKind::Kind` / `PatKind::NilTest`(ノード位置)を `lower_pat` に追加する。

**Files:**
- Modify: `crates/murphy-plugin-macros/src/node_pattern.rs`
- Modify: `crates/murphy-plugin-macros/tests/node_pattern_behavior.rs`

**Step 1: failing test を書く**

`node_pattern_behavior.rs` の末尾に追加。マクロ呼び出しはファイルの module スコープに、テスト関数の外に置く:

```rust
node_pattern!(is_int_42, "42");
node_pattern!(is_any_int, "int");
node_pattern!(is_sym_foo, ":foo");
node_pattern!(is_true_lit, "true");
node_pattern!(is_nil_node, "nil");
node_pattern!(is_nil_test, "nil?");

#[test]
fn literal_and_kind_matching() {
    let mut b = AstBuilder::new("src", "t.rb");
    let i42 = b.push(NodeKind::Int(42), r());
    let i7 = b.push(NodeKind::Int(7), r());
    let sym_foo = {
        let s = b.intern_symbol("foo");
        b.push(NodeKind::Sym(s), r())
    };
    let tru = b.push(NodeKind::True_, r());
    let niln = b.push(NodeKind::Nil, r());
    // Root just needs to own the others; a Begin list keeps them reachable.
    let list = b.push_list(&[i42, i7, sym_foo, tru, niln]);
    let root = b.push(NodeKind::Begin(list), r());
    let ast = b.finish(root);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    assert!(is_int_42(i42, &cx));
    assert!(!is_int_42(i7, &cx));
    assert!(is_any_int(i42, &cx) && is_any_int(i7, &cx));
    assert!(!is_any_int(tru, &cx));
    assert!(is_sym_foo(sym_foo, &cx));
    assert!(is_true_lit(tru, &cx));
    assert!(!is_true_lit(niln, &cx));
    assert!(is_nil_node(niln, &cx));
    assert!(is_nil_test(niln, &cx));
    assert!(!is_nil_test(i42, &cx));
}
```

**Step 2: test を実行して fail を確認**

Run: `cargo test -p murphy-plugin-macros --test node_pattern_behavior literal_and_kind`
Expected: コンパイルエラー(`Lit` / `Kind` / `NilTest` が `compile_error`)。

**Step 3: 実装**

`lower_pat` の `match` に腕を追加する。

- `PatKind::Lit(lit)`: `subject` ノードの `NodeKind` を値比較する。`fail` を `ctx.fail` として:
  - `Lit::Int(v)` → `if !::core::matches!(*cx.kind(#subject), ::murphy_ast::NodeKind::Int(__v) if __v == #v) { return #fail; }`
  - `Lit::Float(v)` → `Float(__v) if __v == #v`(`v` は `f64` リテラルとして `quote`)
  - `Lit::Str(s)` → `Str(__id) if cx.string_str(__id) == #s`
  - `Lit::Sym(s)` → `Sym(__sym) if cx.symbol_str(__sym) == #s`
  - `Lit::True` → `::murphy_ast::NodeKind::True_`
  - `Lit::False` → `::murphy_ast::NodeKind::False_`
  - `Lit::Nil` → `::murphy_ast::NodeKind::Nil`
- `PatKind::Kind(tag)`: 裸種別名。`tag.0`(`u8`)を取り出し `if cx.kind(#subject).tag() != ::murphy_ast::NodeKindTag(#tag_u8) { return #fail; }`。
- `PatKind::NilTest`: ノード位置では `Nil` ノード判定。`if !::core::matches!(*cx.kind(#subject), ::murphy_ast::NodeKind::Nil) { return #fail; }`。
  > `nil?` が `OptNode` スロットで「不在」にもマッチする挙動は Task 4 のスロット dispatch 側で特別扱いする。ここはノード位置(常に存在)専用。

ヘルパ `fn fail_stmt(ctx: &Lower) -> TokenStream { let f = &ctx.fail; quote!(return #f;) }` を作ると読みやすい。`murphy_ast::NodeKind` のフィールド付き variant 名は arena AST(`crates/murphy-ast/src/node.rs`)の通り。

**Step 4: test を実行して pass を確認**

Run: `cargo test -p murphy-plugin-macros --test node_pattern_behavior`
Expected: PASS。

**Step 5: commit**

```bash
git add crates/murphy-plugin-macros/src/node_pattern.rs \
        crates/murphy-plugin-macros/tests/node_pattern_behavior.rs
git commit -m "feat(murphy-plugin-macros): lower literals, bare kinds and nil?"
```

---

## Task 4: Node マッチ `(head child...)` — スキーマ基盤と `Head::Exact`

NodeKind 構造スキーマの型を定義し、`Send` / `Csend` / `Const` / `If` の 4 種で `Head::Exact` のノードマッチを実装する。スロット dispatch(`Node` / `OptNode` / `Sym`)と再帰、固定長 `List` スロット(明示要素のみ、rest は Task 7)を含む。

**Files:**
- Modify: `crates/murphy-plugin-macros/src/node_pattern.rs`
- Modify: `crates/murphy-plugin-macros/tests/node_pattern_behavior.rs`

**スキーマ型(`node_pattern.rs` に追加):**

```rust
/// One pattern-child slot of a NodeKind: how a pattern child maps onto an
/// arena field.
#[derive(Clone, Copy)]
enum SlotTy {
    /// `NodeId` field — recurse into the child node (always present).
    Node,
    /// `OptNodeId` field — `nil?` matches absence, else the child must be
    /// present and recurse.
    OptNode,
    /// `Symbol` field — only a `:sym` literal or `_` pattern child.
    Sym,
    /// `NodeList` field — the remaining pattern children, `cx.list()`-resolved.
    List,
}

/// A pattern-child slot: the arena field to bind plus its type.
struct Slot {
    /// Field reference for the destructuring pattern. `Named` for struct
    /// variants, `Pos(arity, index)` for tuple variants.
    field: FieldRef,
    ty: SlotTy,
}

#[derive(Clone, Copy)]
enum FieldRef {
    Named(&'static str),
    /// (tuple variant arity, this field's index)
    Pos(usize, usize),
}

/// The pattern-child schema for one matchable NodeKind variant.
struct KindSchema {
    /// The `NodeKind::` variant identifier (e.g. "Send").
    variant: &'static str,
    slots: &'static [Slot],
}
```

**`schema_for(tag: u8) -> Option<&'static KindSchema>`** — `tag` は `NodeKindTag` の `u8`。`KIND_PATTERN_NAMES`(murphy-ast)の tag 番号に対応。本タスクでは 4 種のみ:

| pattern 名 | tag | variant | slots |
|---|---|---|---|
| `send` | 17 | `Send` | `[OptNode(receiver), Sym(method), List(args)]` |
| `csend` | 18 | `Csend` | `[Node(receiver), Sym(method), List(args)]` |
| `const` | 13 | `Const` | `[OptNode(scope), Sym(name)]` |
| `if` | 25 | `If` | `[Node(cond), OptNode(then_), OptNode(else_)]` |

全フィールドは struct variant なので `FieldRef::Named`。`static` な `KindSchema` 配列で表現する(`const`/`static` でハードコード)。

**Step 1: failing test を書く**

`node_pattern_behavior.rs` 末尾に。`nil.foo` 形(receiver が Nil ノード)と入れ子を使う:

```rust
node_pattern!(is_nilrecv_foo, "(send nil :foo)");
node_pattern!(is_nested, "(send (send nil :a) :b)");

/// Build `nil.foo` and return (ast-owning) root id.
fn build_nil_dot_foo() -> Ast {
    let mut b = AstBuilder::new("nil.foo", "t.rb");
    let recv = b.push(NodeKind::Nil, r());
    let m = b.intern_symbol("foo");
    let send = b.push(
        NodeKind::Send {
            receiver: OptNodeId::some(recv),
            method: m,
            args: NodeList::EMPTY,
        },
        r(),
    );
    b.finish(send)
}

#[test]
fn node_match_head_exact() {
    let ast = build_nil_dot_foo();
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };
    assert!(is_nilrecv_foo(ast.root(), &cx));

    // `(nil.a).b` — nested send.
    let mut b = AstBuilder::new("nil.a.b", "t.rb");
    let recv = b.push(NodeKind::Nil, r());
    let a = b.intern_symbol("a");
    let inner = b.push(
        NodeKind::Send { receiver: OptNodeId::some(recv), method: a, args: NodeList::EMPTY },
        r(),
    );
    let bb = b.intern_symbol("b");
    let outer = b.push(
        NodeKind::Send { receiver: OptNodeId::some(inner), method: bb, args: NodeList::EMPTY },
        r(),
    );
    let ast2 = b.finish(outer);
    let raw2 = cx_raw_for(&ast2, &fns);
    let cx2 = unsafe { Cx::from_raw(&raw2) };
    assert!(is_nested(ast2.root(), &cx2));
    assert!(!is_nested(inner_of(&ast2), &cx2)); // inner is (send nil :a), not nested
}

/// The inner `(send nil :a)` of the `nil.a.b` fixture.
fn inner_of(ast: &Ast) -> NodeId {
    let NodeKind::Send { receiver, .. } = *ast.kind(ast.root()) else {
        panic!()
    };
    receiver.get().unwrap()
}
```

**Step 2: test を実行して fail を確認**

Run: `cargo test -p murphy-plugin-macros --test node_pattern_behavior node_match_head_exact`
Expected: コンパイルエラー(`Node` が `compile_error`)。

**Step 3: 実装**

`lower_pat` に `PatKind::Node { head, children }` 腕を追加。

1. `head` を判定。本タスクは `Head::Exact(tag)` のみ対応。`Head::Any` / `Head::OneOf` は Task 5 まで `compile_error`("head not yet supported")。
2. `schema_for(tag.0)` を引く。`None` → `compile_error`("node kind `<name>` is not supported by node_pattern! in v1 — see follow-up issue")。`<name>` は `murphy_ast::pattern_name`。
3. **子→スロット割当**: 固定スロット(`Node`/`OptNode`/`Sym`)を先頭から、`List` スロット(あれば末尾に1個)を残りに割り当てる。
   - 固定スロット数を `f` とする。`children.len() < f` → `compile_error`("too few children")。
   - `List` スロット無し かつ `children.len() != f` → `compile_error`("wrong number of children")。
   - 本タスクでは `children` 中に `Rest` が来たら `compile_error`("`...` not yet supported")(Task 7 で対応)。
4. destructuring パターンを生成: schema の `variant` と全スロットの `field` から
   `let ::murphy_ast::NodeKind::#variant { #f0: __b0, #f1: __b1, .. } = *cx.kind(#subject) else { return #fail; };`
   (struct variant)。各スロットに一意の binding ident `__b{k}`(`subject` ごとに衝突しないよう、再帰の深さを含めたユニーク名にする。例: `gensym` カウンタを `Lower` に持たせ `__b{counter}` を採番)。
5. 各固定スロットについて、対応するパターン子を再帰照合:
   - `SlotTy::Node`: `lower_pat(child, &quote!(__bK), ctx)` をそのまま埋め込む。
   - `SlotTy::OptNode`: パターン子が `PatKind::NilTest` なら
     `match __bK.get() { ::core::option::Option::None => {}, ::core::option::Option::Some(__n) => { /* Nil node check */ if !matches!(*cx.kind(__n), ::murphy_ast::NodeKind::Nil) { return #fail; } } }`。
     それ以外のパターン子なら `let ::core::option::Option::Some(__n) = __bK.get() else { return #fail; };` のあと `lower_pat(child, &quote!(__n), ctx)`。
   - `SlotTy::Sym`: パターン子は `:sym` リテラル か `_` のみ。
     - `PatKind::Wildcard` → ガード無し。
     - `PatKind::Lit(Lit::Sym(s))` → `if cx.symbol_str(__bK) != #s { return #fail; }`。
     - それ以外(`$` capture 含む)→ `compile_error`("symbol slot only accepts a `:sym` literal or `_`")。
6. `List` スロット: 本タスクは「明示要素のみ」。`children[f..]` を List 用パターン子とする。`__bK`(`NodeList` 値)を `let __listK = cx.list(__bK);` で解決。`Rest` を含まないので `if __listK.len() != <count> { return #fail; }` の後、各要素 `lower_pat(lp[i], &quote!(__listK[#i]), ctx)`。
7. 全ガードを順に連結したブロックを返す。

> binding 名衝突に注意。`Lower` に `next: usize` を持たせ `fn gensym(ctx, prefix) -> Ident` を作る。`__b` / `__n` / `__list` すべて gensym 経由にすると入れ子で安全。

**Step 4: test を実行して pass を確認**

Run: `cargo test -p murphy-plugin-macros --test node_pattern_behavior`
Expected: PASS。

**Step 5: commit**

```bash
git add crates/murphy-plugin-macros/src/node_pattern.rs \
        crates/murphy-plugin-macros/tests/node_pattern_behavior.rs
git commit -m "feat(murphy-plugin-macros): lower Head::Exact node matches with slot schema"
```

---

## Task 5: スキーマ表を ~25 種へ拡張 + `Head::Any` / `Head::OneOf`

スキーマ表を設計対象の全 NodeKind へ広げ、`Head::Any`(`(_ ...)`)と `Head::OneOf`(`({a b} ...)`)を kind 判定のみで実装する。

**Files:**
- Modify: `crates/murphy-plugin-macros/src/node_pattern.rs`
- Modify: `crates/murphy-plugin-macros/tests/node_pattern_behavior.rs`

**スキーマ表(全体)** — `crates/murphy-ast/src/node.rs` の `NodeKind` 定義を正典とし、各 variant のフィールドを parser-gem 子順で並べる。tuple variant は `FieldRef::Pos`:

| 名 | tag | variant | slots |
|---|---|---|---|
| `lvasgn` | 14 | `Lvasgn` | `[Sym(name), OptNode(value)]` |
| `ivasgn` | 15 | `Ivasgn` | `[Sym(name), OptNode(value)]` |
| `casgn` | 16 | `Casgn` | `[OptNode(scope), Sym(name), OptNode(value)]` |
| `send` | 17 | `Send` | `[OptNode(receiver), Sym(method), List(args)]` |
| `csend` | 18 | `Csend` | `[Node(receiver), Sym(method), List(args)]` |
| `block` | 19 | `Block` | `[Node(call), Node(args), OptNode(body)]` |
| `const` | 13 | `Const` | `[OptNode(scope), Sym(name)]` |
| `array` | 22 | `Array` | `[List(pos 1/0)]` |
| `hash` | 23 | `Hash` | `[List(pos 1/0)]` |
| `pair` | 24 | `Pair` | `[Node(key), Node(value)]` |
| `if` | 25 | `If` | `[Node(cond), OptNode(then_), OptNode(else_)]` |
| `case` | 26 | `Case` | `[OptNode(subject), List(whens)]` (else_ は v1 では List 末尾扱いにせず無視 — 下記注) |
| `when` | 27 | `When` | `[List(conds)]` (body は v1 無視 — 下記注) |
| `begin` | 28 | `Begin` | `[List(pos 1/0)]` |
| `return` | 29 | `Return` | `[OptNode(pos 1/0)]` |
| `and` | 30 | `And` | `[Node(lhs), Node(rhs)]` |
| `or` | 31 | `Or` | `[Node(lhs), Node(rhs)]` |
| `def` | 32 | `Def` | `[Sym(name), Node(args), OptNode(body)]` (receiver は v1 無視) |
| `class` | 33 | `Class` | `[Node(name), OptNode(superclass), OptNode(body)]` |
| `module` | 34 | `Module` | `[Node(name), OptNode(body)]` |
| `while` | 47 | `While` | `[Node(cond), OptNode(body)]` (post は無視) |
| `until` | 48 | `Until` | `[Node(cond), OptNode(body)]` (post は無視) |
| `gvasgn` | 38 | `Gvasgn` | `[Sym(name), OptNode(value)]` |
| `cvasgn` | 39 | `Cvasgn` | `[Sym(name), OptNode(value)]` |

> **複数 `List`/末尾フィールド問題**: `Case` は `{subject, whens: NodeList, else_: OptNodeId}`、`When` は `{conds: NodeList, body: OptNodeId}`、`Def` は `{receiver, name, args, body}` のように、`NodeList` の後ろにさらにフィールドがある。v1 のスロット規約は「`List` スロットは末尾に高々 1 個」。よって `Case`/`When`/`Def`/`While`/`Until` は **スキーマで一部フィールドを意図的に省く**(上表の通り)。省いたフィールドはパターンから参照できない。これは v1 の既知の制限として design doc の follow-up に追記する。`Def` の `receiver` 省略(def/defs collapse)も同様。
>
> 各スキーマ行に「省いたフィールドとその理由」を Rust のコメントで明記すること。

destructuring で省略フィールドがある場合は struct variant パターンで `..` を使う(`NodeKind::Case { subject: __b0, whens: __b1, .. }`)。tuple variant も省略があれば `..` を併用できる(`NodeKind::Return(__b0)` は省略なし)。

**`Head::Any` / `Head::OneOf` の規約(v1)**: 子は kind 判定のみ。子リストは「空」または「`...` 1 個のみ」に限る。具体的な子パターンや `$...` があれば `compile_error`("(_ ...) / ({…} ...) with concrete children is not supported in v1")。

- `Head::Any` → kind チェック無し(どの NodeKind でも可)。子リスト規約を検査するだけ。
- `Head::OneOf(tags)` → `let __t = cx.kind(#subject).tag().0; if !matches!(__t, #(#tag_u8s)|*) { return #fail; }`。

**Step 1: failing test** — `node_pattern_behavior.rs` 末尾に。`if` / `const` / `def` / `array`(明示要素)/ `Head::OneOf` / `Head::Any` を 1 ケースずつ:

```rust
node_pattern!(is_if, "(if _ _ _)");
node_pattern!(is_top_const, "(const nil? :Foo)");
node_pattern!(is_array2, "(array 1 2)");
node_pattern!(is_send_or_csend, "({send csend} ...)");
node_pattern!(is_any_paren, "(_ ...)");

#[test]
fn schema_table_and_flexible_heads() {
    let mut b = AstBuilder::new("src", "t.rb");
    // if: cond/then/else all Int
    let c = b.push(NodeKind::Int(0), r());
    let t = b.push(NodeKind::Int(1), r());
    let e = b.push(NodeKind::Int(2), r());
    let iff = b.push(
        NodeKind::If { cond: c, then_: OptNodeId::some(t), else_: OptNodeId::some(e) },
        r(),
    );
    // const Foo (no scope)
    let foo = b.intern_symbol("Foo");
    let kons = b.push(NodeKind::Const { scope: OptNodeId::NONE, name: foo }, r());
    // [1, 2]
    let a1 = b.push(NodeKind::Int(1), r());
    let a2 = b.push(NodeKind::Int(2), r());
    let alist = b.push_list(&[a1, a2]);
    let arr = b.push(NodeKind::Array(alist), r());
    // bare puts call: (send nil :puts) with no receiver
    let puts = b.intern_symbol("puts");
    let snd = b.push(
        NodeKind::Send { receiver: OptNodeId::NONE, method: puts, args: NodeList::EMPTY },
        r(),
    );
    let list = b.push_list(&[iff, kons, arr, snd]);
    let root = b.push(NodeKind::Begin(list), r());
    let ast = b.finish(root);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    assert!(is_if(iff, &cx));
    assert!(is_top_const(kons, &cx));
    assert!(is_array2(arr, &cx));
    assert!(is_send_or_csend(snd, &cx));
    assert!(!is_send_or_csend(iff, &cx));
    assert!(is_any_paren(iff, &cx) && is_any_paren(arr, &cx));
}
```

**Step 2–5:** fail 確認 → スキーマ表拡張・head 実装 → PASS → commit
`feat(murphy-plugin-macros): full schema table and Head::Any/OneOf`。

---

## Task 6: capture `$_` / `$(...)` / `$ident`(Node capture)

`PatKind::Capture`(`CaptureKind::Node`)を実装し、`Option<(タプル)>` 戻り値を機能させる。

**Files:**
- Modify: `crates/murphy-plugin-macros/src/node_pattern.rs`
- Modify: `crates/murphy-plugin-macros/tests/node_pattern_behavior.rs`

**Step 1: failing test**

```rust
node_pattern!(cap_receiver, "(send $_ :foo)");
node_pattern!(cap_subpat, "$(send nil :foo)");
node_pattern!(cap_two, "(if $_ $_ _)");

#[test]
fn node_captures_return_tuple() {
    let ast = build_nil_dot_foo(); // (send nil :foo), receiver = Nil node
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };
    let send = ast.root();
    let NodeKind::Send { receiver, .. } = *ast.kind(send) else { panic!() };
    let recv = receiver.get().unwrap();

    // $_ at the receiver slot binds the Nil node id.
    assert_eq!(cap_receiver(send, &cx), Some((recv,)));
    // anonymous $(...) capturing the whole send.
    assert_eq!(cap_subpat(send, &cx), Some((send,)));
    // a non-match returns None.
    assert_eq!(cap_receiver(recv, &cx), None);
}
```

**Step 2: fail 確認。**

**Step 3: 実装**

`lower_pat` に `PatKind::Capture { slot, name: _, body }` 腕を追加:

1. `ctx.capture_allowed` が `false` → `compile_error`("`$` capture is not allowed inside `{}` / `!` / `` ` ``")。
2. `body` が `PatKind::Rest`(`$...`)の場合は **ここでは扱わない** — `$...` は `List` スロット側で処理する(Task 7)。本タスクで `Capture { body: Rest }` を `lower_pat` のトップレベル等で見たら、Task 7 まで `compile_error`("seq capture not yet supported")。
3. それ以外(Node capture): `lower_pat(body, subject, ctx)` のガードを出し、続けて `let cap = cap_ident(*slot as usize); quote!(#cap = #subject;)` を足す。
   - `subject` は呼び出し元が `NodeId` を渡す位置(`Node` スロット・`OptNode` の `Some` 束縛後・トップレベル)でのみ Capture が来る。`Sym` スロットに `$` が来るケースは Task 4 のスロット dispatch が既に `compile_error` 済み。

`cap_ident` は Task 2 で定義済み。capture 変数は `lower_matcher` 冒頭で型付き宣言済みなので、ここでは代入のみ。

**Step 4: PASS 確認。Step 5: commit** `feat(murphy-plugin-macros): lower node captures into the result tuple`。

---

## Task 7: `$...` seq capture と `...` rest

`List` スロットで `Rest`(`...`)と `$...`(seq capture)を扱う。中間位置の rest(`(array $_ ... $_)`)に対応する。

**Files:**
- Modify: `crates/murphy-plugin-macros/src/node_pattern.rs`
- Modify: `crates/murphy-plugin-macros/tests/node_pattern_behavior.rs`

**v1 の rest 規約(設計 doc §lowering 参照、ここで確定):**

- `Rest`(`...`)と `$...`(`Capture{body: Rest}`)は **`List` スロットに割り当たるパターン子の中** にのみ置ける。`List` スロットを持たない NodeKind や、固定スロットの位置に来たら `compile_error`。
- `List` スロットのパターン子列 `lp[0..k]` には rest 系が高々 1 個(parser 保証)。
- rest が無い: `if list.len() != k { fail }` + 各 `list[i]` を `lp[i]` 照合。
- rest がインデックス `r`: `if list.len() < k - 1 { fail }`。`list[0..r]` を `lp[0..r]`、`list[len-(k-1-r)..]` を `lp[r+1..]` 照合。中間 `list[r .. len-(k-1-r)]` が rest。`$...` なら `cap_ident(slot) = &list[r .. len-(k-1-r)];`。

**Step 1: failing test**

```rust
node_pattern!(cap_args, "(send nil? :foo $...)");
node_pattern!(rest_then_cap, "(array ... $_)");
node_pattern!(cap_then_rest, "(array $_ ...)");

#[test]
fn seq_capture_and_rest() {
    let mut b = AstBuilder::new("src", "t.rb");
    // foo(1, 2, 3) with no receiver
    let a1 = b.push(NodeKind::Int(1), r());
    let a2 = b.push(NodeKind::Int(2), r());
    let a3 = b.push(NodeKind::Int(3), r());
    let args = b.push_list(&[a1, a2, a3]);
    let foo = b.intern_symbol("foo");
    let send = b.push(
        NodeKind::Send { receiver: OptNodeId::NONE, method: foo, args },
        r(),
    );
    let earr = b.push_list(&[a1, a2, a3]);
    let arr = b.push(NodeKind::Array(earr), r());
    let list = b.push_list(&[send, arr]);
    let root = b.push(NodeKind::Begin(list), r());
    let ast = b.finish(root);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    // $... binds the whole args slice.
    assert_eq!(cap_args(send, &cx), Some((&[a1, a2, a3][..],)));
    // trailing capture after a leading rest.
    assert_eq!(rest_then_cap(arr, &cx), Some((a3,)));
    // leading capture before a trailing rest.
    assert_eq!(cap_then_rest(arr, &cx), Some((a1,)));
}
```

**Step 2–5:** fail 確認 → `List` スロットの rest 対応実装 → PASS → commit
`feat(murphy-plugin-macros): lower $... seq captures and ... rest in list slots`。

---

## Task 8: union `{}` と negation `!`

`PatKind::Union` と `PatKind::Not` を実装する。どちらも内側は capture 禁止。

**Files:**
- Modify: `crates/murphy-plugin-macros/src/node_pattern.rs`
- Modify: `crates/murphy-plugin-macros/tests/node_pattern_behavior.rs`

**実装方針:** union/not の内側は bool 式へ落とす。ヘルパ `fn lower_as_bool(pat, subject, ctx) -> syn::Result<TokenStream>` を作る:

```rust
// returns an expression of type bool
fn lower_as_bool(pat, subject, parent_ctx) -> syn::Result<TokenStream> {
    let mut inner = Lower { fail: quote!(false), capture_allowed: false, /* gensym 引継ぎ */ };
    let guards = lower_pat(pat, subject, &mut inner)?;
    Ok(quote!({ #guards true }))   // クロージャ不要、ブロック式で十分
}
```

- `PatKind::Union(alts)`: `let __ok = #( #alt_bools )||* ; if !__ok { return #fail; }`。各 `alt_bool` は `lower_as_bool(alt, subject, ctx)`。
- `PatKind::Not(inner)`: `if #inner_bool { return #fail; }`。`inner_bool = lower_as_bool(inner, subject, ctx)`。

> `lower_as_bool` のブロック式は `{ guards...; true }` 形。guards 内の `return false` はこのブロックではなく **囲む関数** から return してしまう。これは誤り。union/not の bool 化には guards の `return` を「ブロック値 false」へ変える必要がある。
>
> 正しい実装: union/not 内は `return` を使わず、各サブパターンを **`&&` 連結の bool 式** に落とす別ルートにする。`lower_pat` とは別に `fn lower_bool(pat, subject, ctx) -> syn::Result<TokenStream>`(bool 式を返す。`matches!` / `&&` / `||` のみ、`return` を出さない)を実装する。capture 不可なので分岐は単純: `Wildcard`→`true`、`Lit`→`matches!`、`Kind`→tag 比較、`Node`→ネストした `matches!` + スロット bool、`Union`→`||`、`Not`→`!`、`NilTest`→`matches!`、`Predicate`→関数呼び出し、`Parent`/`Descend`→式、`Capture`→`compile_error`、`Rest`→`compile_error`(bool 文脈に rest は来ない)。
>
> gensym カウンタは `Lower` 共有で良い(`lower_bool` も `&mut Lower` を取る)。

**Step 1: failing test**

```rust
node_pattern!(is_send_or_int, "{send int}");
node_pattern!(not_nil, "!nil");
node_pattern!(send_nonnil_recv, "(send !nil? :foo)");

#[test]
fn union_and_negation() {
    let mut b = AstBuilder::new("src", "t.rb");
    let i = b.push(NodeKind::Int(9), r());
    let niln = b.push(NodeKind::Nil, r());
    let foo = b.intern_symbol("foo");
    let snd = b.push(
        NodeKind::Send { receiver: OptNodeId::NONE, method: foo, args: NodeList::EMPTY },
        r(),
    );
    let list = b.push_list(&[i, niln, snd]);
    let root = b.push(NodeKind::Begin(list), r());
    let ast = b.finish(root);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    assert!(is_send_or_int(i, &cx) && is_send_or_int(snd, &cx));
    assert!(!is_send_or_int(niln, &cx));
    assert!(not_nil(i, &cx));
    assert!(!not_nil(niln, &cx));
}
```

**Step 2–5:** fail 確認 → `lower_bool` + union/not 実装 → PASS → commit
`feat(murphy-plugin-macros): lower union and negation patterns`。

---

## Task 9: predicate `#name`

`PatKind::Predicate` を実装する。`#name` → 呼出側スコープの自由関数 `name(node, cx) -> bool` 呼び出し。`?` / `!` を含む述語名は `compile_error`。

**Files:**
- Modify: `crates/murphy-plugin-macros/src/node_pattern.rs`
- Modify: `crates/murphy-plugin-macros/tests/node_pattern_behavior.rs`

**実装:** `lower_pat` / `lower_bool` の両方に `PatKind::Predicate(name)` 腕を追加。

- `name` を `syn::parse_str::<syn::Ident>(name)` で Rust 識別子へ。失敗(`?`/`!` 等を含む)→ `compile_error`("predicate name `<name>` is not a valid Rust identifier; `?`/`!` are not allowed in v1")。
- `lower_pat` 文脈: `if !#ident(#subject, cx) { return #fail; }`。
- `lower_bool` 文脈: `#ident(#subject, cx)`。

述語関数のシグネチャは `fn(NodeId, &Cx) -> bool`。`murphy-pattern` の lexer は述語名に `?`/`!` 末尾を許すので、`compile_error` 経路は実際に到達しうる(Task 11 の trybuild で固定)。

**Step 1: failing test**

```rust
node_pattern!(is_big_int, "(int #is_big)");

/// User-provided predicate: a free fn in scope at the matcher call site.
fn is_big(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(*cx.kind(node), NodeKind::Int(v) if v >= 100)
}

#[test]
fn predicate_calls_a_free_function() {
    let mut b = AstBuilder::new("src", "t.rb");
    let big = b.push(NodeKind::Int(500), r());
    let small = b.push(NodeKind::Int(3), r());
    let list = b.push_list(&[big, small]);
    let root = b.push(NodeKind::Begin(list), r());
    let ast = b.finish(root);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    assert!(is_big_int(big, &cx));
    assert!(!is_big_int(small, &cx));
}
```

> 注: `(int #is_big)` は atom `int` のノードマッチに見えるが、schema 表に `int` は無い。**`#predicate` は head ではなく子位置**。`(int #is_big)` は head=`int`(Task 5 で `int` を schema 無し kind と判定 → `compile_error`)。よってテストは `int` をノードマッチに使わない形にする。正しいテストパターンは bare predicate を直接使う `node_pattern!(is_big_int, "#is_big")`(トップレベルで `#is_big` を `node` に適用)。上記テストの `"(int #is_big)"` を **`"#is_big"`** に修正して書くこと。

**Step 2–5:** fail 確認 → 実装 → PASS → commit
`feat(murphy-plugin-macros): lower #predicate to a free function call`。

---

## Task 10: parent `^` と descendant `` ` ``

`PatKind::Parent` と `PatKind::Descend` を実装する。

**Files:**
- Modify: `crates/murphy-plugin-macros/src/node_pattern.rs`
- Modify: `crates/murphy-plugin-macros/tests/node_pattern_behavior.rs`

**実装(`lower_pat`):**

- `PatKind::Parent(inner)`: `let __p = match cx.parent(#subject).get() { Some(p) => p, None => return #fail; };` のあと `lower_pat(inner, &quote!(__p), ctx)`。`inner` 内の capture は許可(親方向は一意なので definite-assignment が壊れない)。
- `PatKind::Descend(inner)`: 子孫を走査し最初の一致で成功。`inner` は capture 禁止(`ctx.capture_allowed = false` を渡す)→ `lower_bool` で bool 式化:

```rust
let __inner_ok = |__d: ::murphy_ast::NodeId| -> bool { #inner_bool };
let mut __hit = false;
for __d in cx.descendants(#subject) {
    if __inner_ok(__d) { __hit = true; break; }
}
if !__hit { return #fail; }
```

`#inner_bool` は `lower_bool(inner, &quote!(__d), ctx)`。

**`lower_bool`** にも `Parent` / `Descend` 腕を追加(bool 式版。`Parent` は `cx.parent(s).get().map_or(false, |p| <inner_bool(p)>)`、`Descend` は `cx.descendants(s).into_iter().any(|d| <inner_bool(d)>)`)。

**Step 1: failing test**

```rust
node_pattern!(parent_is_if, "^if");
node_pattern!(has_nil_descendant, "`nil");

#[test]
fn parent_and_descendant() {
    // if(cond=nil, then=int, else=none)
    let mut b = AstBuilder::new("src", "t.rb");
    let cond = b.push(NodeKind::Nil, r());
    let then_ = b.push(NodeKind::Int(1), r());
    let iff = b.push(
        NodeKind::If { cond, then_: OptNodeId::some(then_), else_: OptNodeId::NONE },
        r(),
    );
    let ast = b.finish(iff);
    let fns = fns();
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    // cond's parent is the if node.
    assert!(parent_is_if(cond, &cx));
    assert!(!parent_is_if(iff, &cx)); // if has no parent
    // the if subtree contains a nil descendant.
    assert!(has_nil_descendant(iff, &cx));
    assert!(!has_nil_descendant(then_, &cx)); // an Int leaf has none
}
```

**Step 2–5:** fail 確認 → 実装 → PASS → commit
`feat(murphy-plugin-macros): lower ^parent and \`descendant traversal`。

---

## Task 11: `compile_error` ケースと trybuild fail fixture

v1 で意図的に拒否するパターンが `compile_error!` になることを trybuild で固定する。

**Files:**
- Create: `crates/murphy-plugin-macros/tests/ui/fail_node_pattern_*.rs`(複数)
- Create: 対応する `.stderr`(`TRYBUILD=overwrite` で生成)

**fail fixture(各 1 ファイル):**

1. `fail_node_pattern_parse_error.rs` — `node_pattern!(m, "(sned _)");`(未知種別名)。
2. `fail_node_pattern_unsupported_kind.rs` — `node_pattern!(m, "(rescue _ _ _)");`(schema 表に無い kind のノードマッチ)。
3. `fail_node_pattern_atom_node_match.rs` — `node_pattern!(m, "(int 5)");`(atom のノードマッチ形式)。
4. `fail_node_pattern_capture_in_union.rs` — `node_pattern!(m, "{$_ int}");`(union 内 capture)。
5. `fail_node_pattern_predicate_question.rs` — `node_pattern!(m, "#odd?");`(述語名に `?`)。
6. `fail_node_pattern_sym_capture.rs` — `node_pattern!(m, "(send nil? $_)");`(`Sym` スロットの `$` capture)。

各ファイルは `fn main() {}` を持ち、先頭にコメントで意図を書く。例:

```rust
// node_pattern! must reject an unknown node kind name with a clear
// compile_error rather than silently producing a never-matching fn.

murphy_plugin_macros::node_pattern!(m, "(sned _)");

fn main() {}
```

**Step 1–2:** まず fixture `.rs` のみ作成し `cargo test -p murphy-plugin-macros --test trybuild` を実行。`.stderr` 不在で fail することを確認。

**Step 3:** 各 `compile_error` メッセージが Task 2–10 で実装済みであることを確認(未実装なら該当箇所を実装)。`TRYBUILD=overwrite cargo test -p murphy-plugin-macros --test trybuild` で `.stderr` を生成。

**Step 4:** `.stderr` を目視し、メッセージが意図通りか確認。`cargo test -p murphy-plugin-macros --test trybuild` PASS。

> trybuild snapshot は `rust-src` 有無で揺れることがある(murphy-8np 参照)。`compile_error!` は単純なメッセージなので影響は小さいはずだが、`.stderr` に標準ライブラリ内部フレームが混ざる場合は fixture を調整する。

**Step 5: commit**

```bash
git add crates/murphy-plugin-macros/tests/ui/fail_node_pattern_*
git commit -m "test(murphy-plugin-macros): trybuild fixtures for node_pattern! errors"
```

---

## Task 12: trybuild pass fixture・最終品質ゲート・design doc 更新

代表パターンのコンパイル成功を固定し、workspace 全体の品質ゲートを通す。

**Files:**
- Create: `crates/murphy-plugin-macros/tests/ui/pass_node_pattern.rs`
- Modify: `docs/plans/2026-05-23-murphy-9cr18-node-pattern-macro.md`(既知の制限を follow-up に追記)

**Step 1: pass fixture**

`crates/murphy-plugin-macros/tests/ui/pass_node_pattern.rs`:

```rust
// Representative node_pattern! invocations that must compile cleanly.

use murphy_plugin_macros::node_pattern;

node_pattern!(p_wildcard, "_");
node_pattern!(p_literal, "42");
node_pattern!(p_send, "(send nil? :puts $...)");
node_pattern!(p_nested_caps, "(if $_ $(send nil? :foo) _)");
node_pattern!(p_union, "{send csend}");
node_pattern!(p_traversal, "^(def :foo _ `nil)");

fn main() {}
```

`cargo test -p murphy-plugin-macros --test trybuild` PASS を確認。

**Step 2: design doc 更新**

`docs/plans/2026-05-23-murphy-9cr18-node-pattern-macro.md` の「非スコープ / follow-up」節に、Task 5 で確定したスキーマ省略フィールド(`Case.else_` / `When.body` / `Def.receiver` / `While.post` / `Until.post`)が v1 ではパターンから参照できない旨を 1 行追記する。

**Step 3: 最終品質ゲート**

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

すべて PASS させる。clippy 警告は実コードを直す(`#[allow]` で握りつぶさない)。proc macro 生成コードが clippy 警告を出す場合は、生成コードに `#[allow(...)]` を**付与してよい**(生成コードの慣用)。ただしマクロ実装本体のコードは警告ゼロにする。

**Step 4: commit**

```bash
git add crates/murphy-plugin-macros/tests/ui/pass_node_pattern.rs \
        crates/murphy-plugin-macros/tests/ui/pass_node_pattern.stderr \
        docs/plans/2026-05-23-murphy-9cr18-node-pattern-macro.md
git commit -m "test(murphy-plugin-macros): node_pattern! pass fixtures; finalize v1"
```

(`pass_*` は通常 `.stderr` を持たないが、warnings が出る構成なら trybuild が要求する。出なければ `.rs` のみ add。)

---

## 完了の定義

beads issue murphy-9cr.18 の達成基準:

- `node_pattern!(name, "pattern")` が module レベルの matcher `fn` を生成する。
- v1 文法全機能(`_` / リテラル / 裸 kind / `nil?` / `(head child…)` の `Exact`/`Any`/`OneOf` / `$_` / `$(...)` / `$ident` / `$...` / `...` / `{}` / `!` / `#predicate` / `^` / `` ` ``)が lowering される。
- capture 0 個 → `bool`、≥1 個 → `Option<(タプル)>`、`$_`→`NodeId`、`$...`→`&[NodeId]`。
- v1 非対応(未対応 kind のノードマッチ・atom のノードマッチ・`Sym` スロット capture・`{}`/`!`/`` ` `` 内 capture・`?`/`!` 付き述語名・パースエラー)が `compile_error!` になる。
- `murphy-plugin-api` に `Cx::list` が追加されている。
- 挙動テスト・trybuild fail/pass テストが揃う。
- `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo fmt --check` が通る。

## 既知の v1 制限(design doc follow-up に記載済み/追記する)

- `Case.else_` / `When.body` / `Def.receiver` / `While.post` / `Until.post` はパターンから参照不可(`List` スロット末尾規約による省略)。
- `Head::Any` / `Head::OneOf` は具体的な子パターンを取れない(kind 判定のみ)。
- 非ノード capture(`Sym` スロット・リテラル値)は不可。
- `Union` / `Not` / `Descend` 内の `$` capture は不可。
- パースエラーの span はパターン文字列リテラル全体。
- B/C 共有セマンティクステストは murphy-9cr.19 で実施。
