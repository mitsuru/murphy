# Murphy Native Plugin Node ABI Design

Date: 2026-05-20
Status: approved design draft
Scope: `murphy-ary` (`A6b.2`), follow-up to ADR 0031

## Context

ADR 0031 introduced native `cdylib` cop packs with a C-compatible ABI. The first
ABI is file-level only: a plugin receives file/source bytes and can emit offenses,
but it cannot inspect Murphy's parsed AST. That is enough to prove dynamic pack
loading, not enough to port RuboCop core cops.

RuboCop core cops are AST-driven. Even simple cops use concepts such as node
kind, parent/child traversal, method call name, receiver, arguments, block body,
and byte ranges. Murphy must expose those through a stable host-query ABI without
leaking Rust ABI, `ruby_prism` Rust types, or borrowed node pointers across a
dynamic library boundary.

Existing internal precedents:

- mruby primitives use opaque handles and host queries, never raw prism pointers.
- native `Cop` dispatch walks the AST once and exposes typed Rust node borrows
  only inside Murphy.
- plugin v1 keeps libraries loaded and treats callbacks as concurrent and
  trusted.

## Goal

Add native plugin ABI v2 with node handles and host query functions, then prove
it by porting one existing Murphy standard cop into an external native plugin
pack. The first target is `Style/NilComparison` because it exercises call nodes,
receiver/argument relationships, ranges, and existing output comparison without
requiring framework-specific state.

## Non-Goals

- No Rust trait objects, `ruby_prism` Rust types, or borrowed AST pointers across
  the plugin ABI.
- No full RuboCop NodePattern DSL in this step.
- No cross-file analysis, constant resolution, Rails/RSpec semantic model, or
  type information.
- No pack distribution registry, checksum, or lockfile changes.
- No native plugin sandbox. Native packs remain trusted code.

## Architecture

ABI v2 adds a host query table to the registered plugin descriptor. A v2 plugin
still registers cops through `murphy_register_plugin`, but each cop may provide a
node-aware file callback. Murphy passes a per-file `MurphyNodeHostV2` table and a
file-scoped AST handle space.

```text
PluginFileCop::inspect_file
        |
        v
build per-file NodeTable from Ast
        |
        v
MurphyNodeHostV2 { query fns + userdata }
        |
        v
plugin run_file_v2(ctx, host, emit, sink)
        |
        v
plugin queries node handles and emits offenses
```

Handles are `u32` indexes into a host-owned per-file node table. They are valid
only during the callback. Plugins must not retain handles, host pointers, source
pointers, or returned slices after the callback returns.

## Node Table

For ABI v2, Murphy snapshots only node metadata needed for stable lookup, not the
entire AST as plugin-owned data. The table is built per file before the plugin
callback and dropped after it returns.

Each entry stores:

- node kind constant
- byte range for the full node
- parent handle or `MURPHY_NODE_NONE`
- child handles in source/walk order
- kind-specific fields for supported nodes

This table avoids exposing raw prism lifetimes and avoids O(N) re-walk per
query. It is also deterministic because it is built by one AST walk in source
order.

## Node Kind Scope

The first v2 ABI should include the node classes needed by existing Murphy cops
and near-term RuboCop core ports:

- common/control: `ROOT`, `STATEMENTS`, `IF`, `UNLESS`, `CASE`, `WHEN`, `RETURN`
- definitions: `DEF`, `CLASS`, `MODULE`, `BLOCK`
- calls/constants: `CALL`, `CONST_READ`, `CONSTANT_PATH`, `LOCAL_VARIABLE_READ`
- assignments: `LOCAL_VARIABLE_WRITE`, `INSTANCE_VARIABLE_WRITE`
- literals: `STRING`, `SYMBOL`, `INTEGER`, `TRUE`, `FALSE`, `NIL`, `ARRAY`, `HASH`
- fallback: `UNKNOWN`

Kind constants are `u32` values exported by `murphy-core`. `UNKNOWN` nodes still
have range, parent, and children so traversal can continue even before a kind is
fully modeled.

## Host Query ABI

Use C-compatible function pointers. The v2 host table field names are fixed as:

```c
uint32_t node_count(void *userdata);
uint32_t node_kind(void *userdata, uint32_t handle);
uint32_t node_start(void *userdata, uint32_t handle);
uint32_t node_end(void *userdata, uint32_t handle);
uint32_t node_parent(void *userdata, uint32_t handle);
uint32_t node_child_count(void *userdata, uint32_t handle);
uint32_t node_child(void *userdata, uint32_t handle, uint32_t index);
MurphySlice node_text(void *userdata, uint32_t handle);
```

Invalid handles return neutral sentinels:

- kind: `MURPHY_NODE_UNKNOWN`
- handle: `MURPHY_NODE_NONE` (`u32::MAX`)
- count/range: `0`
- slice: null/zero

All offsets are byte offsets into the original source. `node_text` returns a
borrowed source slice valid only for the callback.

## Call Query ABI

Call/send is the most important RuboCop core primitive. Add dedicated queries:

```c
MurphySlice call_name(void *userdata, uint32_t handle);
uint32_t call_receiver(void *userdata, uint32_t handle);
uint32_t call_arg_count(void *userdata, uint32_t handle);
uint32_t call_arg(void *userdata, uint32_t handle, uint32_t index);
uint32_t call_message_start(void *userdata, uint32_t handle);
uint32_t call_message_end(void *userdata, uint32_t handle);
```

For non-call handles, these return neutral sentinels. `call_receiver` returns
`MURPHY_NODE_NONE` for receiver-less calls.

`Style/NilComparison` needs this shape:

- find `CALL` nodes named `==` or `!=`
- inspect receiver and first argument
- detect either side as `NIL`
- emit offense at the operator/message range or full call range if message range
  is unavailable

## Block Query ABI

Blocks are common in RuboCop core cops. Add these in v2 even if the first PoC
does not consume them:

```c
uint32_t block_call(void *userdata, uint32_t handle);
uint32_t block_args(void *userdata, uint32_t handle);
uint32_t block_body(void *userdata, uint32_t handle);
```

This is intentionally structural only. No NodePattern DSL or semantic block type
classification is included.

## Plugin Registration

ABI v1 stays supported. ABI v2 is additive.

Registration shape:

- `murphy_plugin_abi_version()` returns `2` for node-aware packs.
- `MurphyPluginCopV2` includes `run_file_v2`.
- v1 packs still load through the current file-level path.
- v2 packs may still register file-level cops if `run_file_v2` is absent and
  `run_file` is present.

Murphy rejects malformed v2 descriptors as setup errors: bad sizes, null required
callbacks, duplicate IDs, invalid UTF-8 names, invalid table lengths, or ABI
version greater than supported.

## Error Handling

Host queries never panic for plugin mistakes. Invalid handles and wrong-kind
queries return sentinels. Emitted offenses keep the current validation:

- UTF-8 cop name/message required
- `start <= end`
- `end <= source.len()`
- invalid emissions are ignored, not process-fatal

Plugin callback nonzero return still produces one error offense for that cop/file.
Plugin panics across FFI remain undefined by contract and are forbidden.

## Testing Strategy

- Unit-test NodeTable construction on representative Ruby snippets.
- Unit-test parent/child relationships and byte ranges, including multibyte
  source.
- Unit-test call query behavior for receiver-less calls, explicit receivers,
  operator calls, arguments, and nil literals.
- Integration-test an external v2 plugin pack that ports `Style/NilComparison`.
- Compare plugin `Style/NilComparison` output against the built-in cop by running
  fixtures with the built-in disabled and the plugin enabled.
- Snapshot-test native-only runs to confirm ABI v1/v2 additions do not change
  existing output.

## Success Criteria

- `murphy-example-pack` or a new `murphy-example-node-pack` loads as ABI v2.
- The example pack implements `Style/NilComparison` using only node host queries.
- The plugin cop detects `x == nil`, `nil == x`, `x != nil`, and `nil != x` with
  byte ranges matching the built-in cop's contract.
- Existing v1 plugin pack tests still pass.
- Existing native-only snapshot tests remain unchanged.

## Follow-Ups

- Add pack-specific config once node ABI is proven.
- Add helper/resource loading for richer plugin packs.
- Add Rails/RSpec-specific query helpers after core AST shape is stable.
- Consider a small NodePattern-like helper crate for plugin authors, built on top
  of the C ABI rather than added to the ABI itself.
