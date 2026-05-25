# Dispatch attributes & `Cx<'_>` cheat-sheet

Reference companion for SKILL.md §2 ("Decide the dispatch shape") and §3
("Scaffold the cop file"). Source of truth is infra guide §3 and §4 plus
`crates/murphy-plugin-api/src/cx.rs`.

## Dispatch attributes (inside `#[cop]` impl)

### `#[on_node(kind = "…")]`

Per-kind dispatch. The method is invoked once for every AST node whose
kind matches. Multiple `#[on_node]` methods are allowed inside one cop
— the macro merges their kind set into the subscription, and the host
dispatches to the matching method by node tag.

Each dispatch method must have signature:

```rust
fn check_<thing>(&self, node: NodeId, cx: &Cx<'_>)
```

Common kind strings (see `docs/guides/rubocop-hook-dispatch.md` for the
full map):

| Kind | Used for |
|---|---|
| `"send"` | All method calls — `puts(x)`, `obj.foo`, `a + b`, `arr[i]`. Pair with `methods = […]` when filtering by name. |
| `"block"` | `do … end` and `{ … }` blocks. |
| `"def"` | `def foo …`. |
| `"defs"` | `def self.foo …` (singleton). |
| `"class"` / `"module"` | Class / module definitions. |
| `"if"` / `"case"` | Conditional shapes. |
| `"str"` / `"dstr"` / `"sym"` | String / interpolated string / symbol literals. |
| `"int"` / `"float"` | Numeric literals. |
| `"array"` / `"hash"` / `"pair"` | Container literals (`"pair"` is one key-value entry inside a hash). |
| `"const"` | Constant reference; `scope` walks the namespace chain. |

### `#[on_node(kind = "send", methods = […])]`

`Send`-only method-name pre-filter (infra guide §3 "Method-name
filtering"). The host skips the FFI hop for `Send` nodes whose method
symbol is not in the list.

- `methods` is **only** valid on `kind = "send"` — the macro rejects it
  on any other kind at parse time.
- `methods = []` is rejected (silent-cop guard).
- Same kind cannot dispatch twice; combine multiple allow-lists into one
  attribute rather than splitting.
- Always keep the defensive `let NodeKind::Send { … } = *cx.kind(node) else { return };`
  destructure even when the filter is present — it guards against a
  future kind-aliasing accident.

### `#[on_new_investigation]`

File-visit dispatch. Called exactly once per file with `node == cx.root()`.
Mirrors RuboCop's `on_new_investigation` lifecycle hook. Use this for
whole-file scanners (raw-source cops like
`Layout/TrailingWhitespace` whose root kind can be any of `Begin`,
`Nil`, `Send`, `Def`, …).

The method receives `node = root` so it can iterate `cx.comments()`,
read `cx.source()`, or walk `cx.descendants(root)`.

## `Cx<'a>` — reading the AST

`Cx<'a>` is a borrowed, direct-read view of the arena. Traversal and
`NodeKind` matching are pure memory reads — zero FFI (infra guide §4).
The lifetime `'a` forbids retaining any part past the `check` call.

| Method | Returns | Use case |
|---|---|---|
| `root()` | `NodeId` | The file's root node. |
| `node(id)` | `&'a AstNode` | The packed node header. |
| `kind(id)` | `&'a NodeKind` | The typed payload — match on this. |
| `range(id)` | `Range` | Byte range `[start, end)` into source. |
| `parent(id)` | `OptNodeId` | `OptNodeId::NONE` at the root. |
| `list(NodeList)` | `&'a [NodeId]` | Dereference a variable-length child list (e.g. `Send { args }`). |
| `children(id)` | `Vec<NodeId>` | Direct children, in source order. |
| `ancestors(id)` | `impl Iterator<Item = NodeId>` | Walks up to the root. |
| `descendants(id)` | `Vec<NodeId>` | Subtree pre-order, including `id`. |
| `symbol_str(Symbol)` | `&'a str` | Interned identifier text. |
| `string_str(StringId)` | `&'a str` | Interned string literal text. |
| `comments()` | `&'a [Comment]` | All file comments, source order. |
| `raw_source(Range)` | `&'a str` | Bytes in source covered by `Range`. |
| `source()` | `&'a str` | Whole file source. |
| `emit_offense(Range, &str, Option<Severity>)` | `()` | Report a finding. |
| `emit_edit(Range, &str)` | `()` | Autocorrect replacement. |

Source is UTF-8 (panics otherwise). `Range::{start, end}` are `u32` byte
offsets; converting to/from char/column positions is the cop's
responsibility — `test_support`'s `char_indices()` translation is the
canonical pattern.

## Common `NodeKind` shapes

These are the ones cops match on most often. Use the in-tree examples
as authoritative — `cx.kind(id)` returns `&NodeKind` and the actual
variants live in `murphy-plugin-api` re-exports.

```rust
// Method call: every `puts x`, `obj.foo(y)`, `a + b`, `arr[i]`.
NodeKind::Send { receiver: OptNodeId, method: Symbol, args: NodeList }

// Block: `do…end` or `{…}`. `call` is the lead Send.
NodeKind::Block { call: NodeId, args: NodeList, body: OptNodeId }

// Method definition. `body` may be None for `def foo; end`.
NodeKind::Def { name: Symbol, args: NodeList, body: OptNodeId }

// Constant reference. `scope` walks `Foo::Bar::Baz` left-to-right.
NodeKind::Const { scope: OptNodeId, name: Symbol }

// String literal (single segment).
NodeKind::Str(StringId)

// Interpolated string `"#{x}"`.
NodeKind::Dstr(NodeList)

// Symbol literal `:foo`.
NodeKind::Sym(Symbol)
```

For unfamiliar kinds, look up the corresponding RuboCop hook in
`docs/guides/rubocop-hook-dispatch.md` and then grep
`crates/murphy-plugin-api/src/` for the variant.

## Standard destructure pattern

The defensive `let-else` form is the house style — every dispatch body
starts with it even when the kind is statically guaranteed by the
attribute:

```rust
fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Send { receiver, method, args } = *cx.kind(node) else {
        return; // unreachable today, but cheap insurance against kind-aliasing.
    };
    // …
}
```

The `*` dereferences the `&NodeKind` returned by `cx.kind`. `NodeKind`'s
variants are `Copy` for the small fields and contain handles (`Symbol`,
`NodeId`, `StringId`) for the rest, so the deref is free.
