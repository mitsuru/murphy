# Murphy AST Representation Strategy — arena, parser-shaped, typed

**Status**: exploratory design accepted 2026-05-22. Supersedes parts of
the murphy-9cr epic (Tier 1/2 typed node wrappers). A formal ADR and
implementation issues follow.

This document records a design discussion that started inside
murphy-9cr.5 and widened into a decision about Murphy's core AST
representation. It is a snapshot of *direction*, not a finished
implementation spec.

## How this came up

murphy-9cr.5 ("Tier 1 typed Node wrapper, 32 of them") is specified as
newtype wrappers over `ruby_prism::Node<'pr>` variants. Implementing it
surfaced a chain of blockers:

1. The Tier 1 wrappers wrap `ruby_prism::Node`.
2. The plugin ABI never delivers a prism node. `MurphyNodeContext`
   carries only `file` / `source` / `config` / `node_kind` (a string)
   / `range` / `dispatch_id`.
3. ruby-prism cannot rebuild a `Node` from raw pointers — `Node::new`
   is `pub(crate)`.
4. Therefore a "typed view over a prism Node" cannot be handed to a
   third-party plugin across the ABI at all.

So murphy-9cr.5 cannot be implemented as written. That forced a step
back to first principles.

## Reframing: who touches the AST, and for what

There are two distinct consumers, and the epic conflated them:

- **standard cops** — shipped with Murphy (murphy-4gd ports ~834
  RuboCop cops). They run inside `murphy-core` and can touch the AST
  directly; no ABI involved.
- **third-party plugins** — external `.so` files dispatched over the
  C ABI.

Two more constraints came out of the discussion:

- **`run_file` (handing a cop the raw source) is to be removed.** Once
  it is gone, *every* cop — standard and plugin — must work from
  structured AST dispatch. There is no "parse it yourself" escape
  hatch.
- A cop never actually wants the `ruby_prism::Node` struct. It wants
  structured AST information: a node's children, kind, range, and the
  ability to walk to ancestors/descendants.

## Audit: can murphy-rails cops run on AST alone?

We audited all 138 `rubocop-rails` 2.35.2 cops (the rules murphy-rails
ports) against their real RuboCop implementations.

| Category | Meaning | Count |
|---|---|---|
| A | pure AST node dispatch | 132 (96%) |
| B | AST + file path / filename | 2 |
| C | AST + comment list / raw-source line | 4 |
| D | needs something outside AST+comments+filemeta | 0 |

- **Detection for all 138 cops is pure AST.** The category B/C extras
  are confined to autocorrect range computation and deriving a
  migration class name from a filename.
- **No cop uses the lexer token stream.** A token surface is not
  needed.
- **~40 cops (≈29%) require tree traversal** — `each_ancestor`,
  `each_descendant`, multi-level node-pattern search (e.g. the
  `in_migration?` mixin walks `each_ancestor(:class)`). A per-node
  metadata ABI is therefore *insufficient*; the AST surface must
  expose tree navigation.

Conclusion: `run_file` can be removed. AST (with tree navigation) plus
a comment list, raw-source access, and the file path covers 100% of
murphy-rails. But the AST surface must be a navigable tree, not a bag
of per-node metadata.

## The murphy-rails text-matching detour

The current murphy-rails 131 `run_file` cops do **not** parse — they
do text/regex matching. They are a crude scaffold: fast but
inaccurate, and useless as ABI-design evidence (they touch no AST).
They remain valuable only as a *cop inventory* (names, categories,
detection targets). They will be reimplemented on the AST surface once
it exists. The lesson: the AST surface should have been designed
before cops were written. That ordering can still be corrected.

## Decision: arena, parser-shaped, typed AST

Two routes for the AST a cop sees:

- **Route A — prism-native + Tier 1/2 typed wrappers** (the current
  murphy-9cr design). Type-safe, but every RuboCop cop port must
  hand-absorb the prism⇔parser collapse/split differences (the table
  in murphy-9cr.1). Porting ~834 cops stays non-mechanical.
- **Route B — parser-gem-compatible structured AST** (a Rust analogue
  of Ruby's `Prism::Translation::Parser`). Cops see parser-shaped
  nodes whose type names and child positions match what RuboCop cops
  already assume, so porting is mechanical, and `each_ancestor` /
  node-pattern matching land naturally.

**Decision: Route B**, realised as a **type-safe S-expression**: an
**arena-allocated, parser-shaped, typed AST**.

```rust
// The arena owns every node.
struct Ast {
    nodes: Vec<AstNode>,
    // string/symbol interning, comment list, source buffer, file path …
}

struct AstNode {
    kind: NodeKind,
    parent: Option<NodeId>,
    range: Range,
}

// parser-gem node types as a type-safe enum (~100 variants)
enum NodeKind {
    Send  { receiver: Option<NodeId>, method: Symbol, args: NodeList },
    Const { scope: Option<NodeId>, name: Symbol },
    If    { cond: NodeId, then_: Option<NodeId>, else_: Option<NodeId> },
    Int(i64),
    Str(StringId),
    // …
}
```

The load-bearing idea: **children are held as `NodeId` (an arena
index), not `Box`-owned**. That single choice delivers everything the
discussion asked for:

- **Type-safe.** `match node.kind { NodeKind::Send { receiver, method,
  args } => … }` — exhaustive, every variant carries typed fields. The
  S-expression structure expressed directly as an enum.
- **Parent links exist.** A `Box`-owned tree cannot hold child→parent
  links; an arena + `NodeId` holds both directions. `each_ancestor`
  walks `parent` `NodeId`s — satisfying the ~40 traversal cops.
- **Clean C ABI.** `NodeId` is a `u32`. The third-party plugin ABI
  becomes "arena handle + `NodeId` + accessor functions". The
  opaque-handle problem the discussion circled for hours dissolves —
  `NodeId = u32` is the handle.
- **Mechanical RuboCop porting.** parser-shaped node types and child
  layout correspond directly; RuboCop's dynamic `node.type == :send`
  becomes a Rust `match`.

This is the standard Rust AST design (rust-analyzer's rowan, Ruff's
arena AST). Node-pattern matching (`(send _ :foo)`) can be a proc
macro that lowers a pattern to a `match`.

## What this changes

- **murphy-core**: the "shared immutable AST" changes from
  `ruby_prism::Node` to the arena parser-AST. Standard cops and
  plugins all move onto it.
- **New translation layer**: prism parse → arena parser-AST, written
  once. The Rust analogue of `Prism::Translation::Parser`. Runs once
  per file; prism parsing is fast, so the conversion cost is expected
  to be acceptable — **must be measured**.
- **murphy-9cr epic**: Tier 1/2 typed node wrappers (murphy-9cr.5) and
  `#[on_node]` (murphy-9cr.8) are re-designed against the arena AST.
  murphy-9cr.1's frequency data still informs which node kinds deserve
  dispatch ergonomics. murphy-9cr.2/.3/.6/.7 (ABI metadata,
  plugin-api skeleton, `register_cops!`, `#[derive(CopOptions)]`)
  stand — they are about cop registration and options, not AST shape.
- **Plugin ABI**: node dispatch carries `NodeId` + an arena handle;
  accessor functions read kind / children / parent / range. Comment
  list, raw-source access, and file path are added as small extra
  surfaces (audit categories B and C). No lexer token surface.
- **`run_file` removal** becomes reachable: every cop can be expressed
  on the arena AST.

## Cop authoring needs a node-pattern DSL too

The arena AST is only half of what a cop author uses. RuboCop cops are
written against **two** things: the Parser AST *and* NodePattern
(`def_node_matcher "(send nil? :foo)"`). The body of a typical
`on_send` is mostly S-expression pattern matching. An arena AST with no
pattern-matching DSL leaves cop authors hand-writing nested `match`
arms for every rule.

So a **node-pattern matching DSL is a first-class, required component**
alongside the arena AST — not an "open question". Sketch:

- A cop writes `node_pattern!("(send nil? :puts $...)")` (or an
  attribute form).
- A proc macro parses the S-expression pattern **at compile time** and
  lowers it to a `match` over the arena AST's `NodeKind`, plus the
  condition checks and capture extraction.
- Captures (`$`) come back typed — a `NodeId`, a literal, a slice of
  `NodeId`s — so the "type-safe S-expression" idea holds for *both*
  the AST representation and pattern matching.
- The DSL needs an S-expression parser/compiler. This is the Rust
  analogue of RuboCop's NodePattern compiler.

RuboCop's NodePattern features: `_` (any), `...` (rest), `{a b}`
(union), `[a b]` (all), `$` (capture), `#predicate` (method call),
`^` (parent), `nil?`/literals. How much of that to port — and whether
to start with a subset — is the one open scoping question here; the
*existence* of the DSL is not optional.

## Open questions

- prism → arena parser-AST conversion cost (measure against Murphy's
  speed targets; the whole point of Murphy is being fast).
- node-pattern DSL scope — how much of RuboCop's NodePattern grammar
  to support in v1 (subset vs full).
- Exact shape of the comment / raw-source / file-path surfaces
  (audit categories B/C) and how autocorrect range computation uses
  them.
- The plugin ABI for arena access — handle lifetime, accessor function
  set, how `NodeId` validity is guaranteed across the boundary.
- Migration plan for the 131 text-matching murphy-rails cops.
- Interning strategy (`Symbol`, `StringId`) and arena memory layout.
- Whether `~100` `NodeKind` variants follow parser-gem names exactly
  or a Murphy-curated set.

## Next steps

1. Write a formal ADR for "arena parser-shaped typed AST as Murphy's
   core AST representation".
2. Create an implementation epic covering three coupled pieces: the
   arena AST, the prism→arena translation layer, and the node-pattern
   matching DSL. Cops cannot be written without all three.
3. Re-scope murphy-9cr.5 and murphy-9cr.8 against the arena AST (or
   close and re-file them).
4. Prototype the prism→arena conversion and measure the cost before
   committing.
