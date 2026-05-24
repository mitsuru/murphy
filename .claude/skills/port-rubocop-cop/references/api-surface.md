# `murphy_plugin_api` import cheat-sheet

The single-surface ABI re-exports everything a cop file needs from one path
(infra guide §2). When porting a cop, the imports almost always come from
this set. Anything reached through `murphy-core`, `murphy-ast`,
`murphy-translate`, `murphy-pattern` etc. is **off limits at runtime** —
`tests/dep_boundary.rs` fails the build.

## Always-needed

```rust
use murphy_plugin_api::{Cx, NodeId, NodeKind, cop};
```

- `Cx<'a>` — borrowed view of the arena; the cop reads the AST through it.
  See `references/dispatch-and-cx.md`.
- `NodeId` — opaque arena handle for a node.
- `NodeKind` — the typed payload; cops `match` on this after `cx.kind(id)`.
- `cop` — the `#[cop(...)]` attribute macro. Always required on the cop
  `impl` block.

## Frequently needed

- `NoOptions` — marker type for cops with no configuration. Pass as
  `options = NoOptions` to `#[cop(...)]`.
- `OptNodeId` — `Option<NodeId>` analogue with `NONE` constant. Used for
  optional fields like `Send { receiver }` and `Block { body }`.
- `Range` — `[start, end)` byte range into source; emitted with offenses
  and edits.
- `Symbol` — interned identifier; resolve to text via `cx.symbol_str(sym)`.
- `Severity` — `Warning` / `Error`. Pass `Some(Severity::Error)` to
  `emit_offense` only when the cop overrides per match site; otherwise
  pass `None` and let the host default chain apply.

## Options

```rust
use murphy_plugin_api::CopOptions;          // for #[derive(CopOptions)]
```

Plus the field-level `#[option(default = …, description = "…")]` attribute
(handled by the derive — no separate import).

## Dispatch attributes

`on_node` and `on_new_investigation` are re-exported from
`murphy_plugin_api` (so they *can* appear in the `use` list — the infra
guide §2 spells them out for completeness), but **in-tree cops do not
import them**. The `#[cop(...)]` proc-macro consumes the inner
`#[on_node]` / `#[on_new_investigation]` attributes itself, so the
import is unnecessary noise. Follow the house style and leave them out:

- `#[on_node(kind = "<canonical-name>")]`
- `#[on_node(kind = "send", methods = ["foo", "bar"])]`
- `#[on_new_investigation]`

If the cop file ever needs the attributes outside a `#[cop]` impl block
(it should not — that's not a supported authoring shape), `use
murphy_plugin_api::{on_node, on_new_investigation};` is what to add.

## Pattern matchers

```rust
use murphy_plugin_macros::node_pattern;
```

Note: `node_pattern!` is re-exported through `murphy-plugin-macros`, not
`murphy-plugin-api`. The dep boundary allows it because the macros crate
is part of the single-surface ABI's macro half. See
`references/node-pattern.md`.

## Test harness (dev-only)

```rust
#[cfg(test)]
use murphy_plugin_api::test_support::{expect_offense, expect_no_offenses, indoc, run_cop};
```

Gated behind the `test-support` cargo feature. The pack's `Cargo.toml`
[dev-dependencies] must enable it:

```toml
[dev-dependencies]
murphy-plugin-api = { path = "../murphy-plugin-api", features = ["test-support"] }
```

## What `#[cop(...)]` arguments mean

| Arg | Required | Notes |
|---|---|---|
| `name` | yes | `"Pack/CopName"`. Must match `murphy.toml` and emit JSON. |
| `description` | optional | One-line summary surfaced in `murphy cops list`. |
| `default_severity` | optional | `"warning"` or `"error"`. Omit to leave host default. |
| `default_enabled` | optional | `true` / `false`. Omit to leave host default. |
| `options` | yes | `NoOptions` or a `#[derive(CopOptions)]` struct. |

## What is *not* on the surface

These exist in the codebase but are **not** part of the plugin ABI and
must not be imported from a cop file:

- `murphy_ast::*` — internal AST representation. Reach it via
  `murphy_plugin_api::NodeKind` / `Cx` only.
- `murphy_core::*` — host orchestration. Not callable from packs.
- `murphy_translate::*` — parser. Only test_support pulls it in.
- `murphy_pattern::*` — pattern IR runtime; lowered into by
  `node_pattern!` at compile time and reached through the generated
  function, not directly.
