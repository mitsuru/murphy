# Murphy Phase 8 cdylib Plugin Pack Design

Date: 2026-05-20
Status: approved design draft
Scope: `murphy-fmw.1.3`, `murphy-fmw.1.2`, `murphy-fmw.1.5`

## Context

Phase 8 turns Murphy from a fixed linter into an extension platform. The chosen
A6b direction is native plugin loading via `cdylib`, not static Cargo features or
WASM. This unlocks third-party Rust cop packs without rebuilding Murphy, but it
also makes the ABI and failure model part of the public contract.

Existing code has one central native cop boundary:

- `murphy_core::Cop` is the in-process Rust trait used by built-in cops.
- `CopRegistry::native_cops_list()` statically constructs the built-in cop set.
- mruby user cops are loaded separately from configured `cops/*.rb` paths.
- config is strict TOML via `#[serde(deny_unknown_fields)]`.

The design keeps existing cop IDs and default JSON output stable while adding a
new pack/plugin layer around the registry.

## Goals

- Load a native `cdylib` pack from `murphy.toml` at runtime.
- Reject unsupported plugins deterministically as setup errors (`exit 2`).
- Repackage built-in native cops as the `builtin` pack without changing cop IDs.
- Provide one external PoC pack (`murphy-example-pack`) that loads and runs.
- ADR the distribution contract: cop ID stability, versioning, config evolution,
  and plugin ABI compatibility.

## Non-Goals

- No Rust trait objects across the dynamic library boundary.
- No Rust ABI dependency between Murphy and plugin crates.
- No sandbox guarantee for native plugins. Loading a native plugin is trusted code
  execution in the host process.
- No stable AST pointer ABI in the first implementation step. The public plugin
  boundary starts with a narrow C ABI and grows only through versioned structs.

## Architecture

Introduce a native plugin layer below `CopRegistry` and above concrete cops.

```text
murphy.toml
  [[cop_packs]] name/path/version
        |
        v
CopPackLoader -- dlopen -- cdylib exports
        |                 murphy_plugin_abi_version
        |                 murphy_register_plugin
        v
LoadedCopPack
        |
        +-- builtin pack cops (existing Rust cops)
        +-- external native plugin cops
        |
        v
CopRegistry native slice + mruby cop paths
```

`CopRegistry` remains the one run-level owner of cops. The internal native list
becomes a list of loaded packs, with `builtin` always present unless disabled by
future config. Existing call sites still consume `&[Box<dyn Cop>]` so the lint
pipeline can remain unchanged while the pack layer is introduced.

## Plugin ABI

Plugins export exactly these symbols in v1:

```c
uint32_t murphy_plugin_abi_version(void);
int32_t murphy_register_plugin(const MurphyHostV1 *host, MurphyPluginV1 *plugin);
```

Rules:

- `murphy_plugin_abi_version()` must return `1` for the initial implementation.
- Missing symbols, ABI mismatch, or registration failure are setup errors.
- All exported structs use `#[repr(C)]` equivalents and explicit size fields.
- Strings crossing the boundary are UTF-8 pointer/length pairs owned by the side
  documented in the struct field.
- Plugin libraries stay loaded for the full Murphy process lifetime; registered
  function pointers are never used after unload.
- Plugin callbacks may be invoked concurrently from multiple OS threads because
  Murphy keeps its existing file-level `rayon` parallelism. A plugin must treat
  callbacks as thread-safe, synchronize any shared mutable state itself, and must
  not retain host-owned pointers after the callback returns.

The first executable cop ABI should be intentionally narrow: file-level callback
with source bytes and a host offense sink. Node-level callbacks can be added in a
future ABI minor only after the stable node handle model is designed. This avoids
exporting `ruby_prism` internals or borrowed AST lifetimes across `cdylib`.

## Configuration

Extend `murphy.toml` with a top-level array:

```toml
[[cop_packs]]
name = "murphy-example-pack"
path = "packs/murphy-example-pack/libmurphy_example_pack.so"
version = "0.1.0"
```

Fields:

- `name`: stable pack name used in diagnostics and cache keys.
- `path`: project-root-relative path to the dynamic library.
- `version`: declared pack version. Murphy records it for cache invalidation and
  diagnostics but does not fetch or resolve dependencies in Phase 8.

Config stays strict. Unknown pack fields are errors. Existing `[cops.rules]` keys
continue to use cop IDs, not pack-local names, so disabling `Style/StringLiterals`
works the same before and after built-in repackaging.

## Built-In Pack

The current native cops become a logical `builtin` pack:

- `builtin` is always loaded first.
- Existing cop IDs are unchanged.
- Existing registry order is preserved inside the pack.
- Reserved-name checks for mruby cops use all loaded native cop names plus
  `Murphy/Syntax`, not just hardcoded built-ins.

This is an internal reorganization; snapshots and exit codes must stay stable.

## Error Handling

All plugin load and registration failures are setup errors (`exit 2`) with
diagnostics on stderr and no stdout JSON.

Examples:

- configured library path does not exist
- `dlopen` fails
- required symbol is missing
- ABI version mismatch
- plugin registers a duplicate cop ID
- plugin registers invalid UTF-8 or an empty cop ID
- plugin callback returns an invalid offense range

Runtime panics in Rust plugin code cannot be safely recovered across FFI. Plugin
callbacks must be `extern "C"` functions that do not unwind. Murphy should treat
an explicit plugin error return as a setup/runtime plugin error and exit 2 for
the first implementation.

Plugin authors should avoid process-global mutable state unless it is protected
with normal Rust synchronization primitives such as `Mutex`, `RwLock`, or
atomics. Thread-local state must not be used to infer cop identity because Murphy
may reuse worker threads across files and cops.

## Data Flow

1. CLI loads `MurphyConfig` from the project root.
2. `CopRegistry::discover_with_config` constructs the `builtin` pack and loads
   configured native plugin packs.
3. Registry applies `[cops.rules]` enablement across all native cops.
4. Existing mruby cop discovery still reads configured `cops.path`.
5. Linting calls `run_cops` on the combined native cop slice.
6. mruby cops run after native cops as they do today.
7. `aggregate_with_config` remains the single deterministic ordering and
   severity-override point.

## Distribution Contract ADR

Create an ADR for A6a/C4 with these rules:

- Cop IDs are stable public identifiers. Rename means deprecate old ID and add a
  new ID, not mutate in place.
- Default severity changes require a major pack version bump.
- Config keys are additive within a major version. Removing a key or changing its
  meaning requires a major bump.
- Plugin ABI version and pack semantic version are separate. A pack version bump
  does not imply an ABI bump, and an ABI bump does not imply a cop behavior bump.
- Native plugin packs are trusted code and must be documented as equivalent to
  running arbitrary code in the linting process.

## Testing

- Unit-test TOML parsing for `[[cop_packs]]`, defaults, and unknown-field errors.
- Unit-test built-in pack preserves exact native cop names and order.
- Unit-test duplicate cop ID rejection across built-in and plugin cops.
- Integration-test missing library and missing symbol errors map to exit 2 with
  empty stdout.
- Integration-test `murphy-example-pack` loads and emits one deterministic
  offense.
- Snapshot-test native-only projects to prove existing output is unchanged.

## Implementation Order

1. Add config shape for `[[cop_packs]]`.
2. Refactor built-in native cops into a logical `builtin` pack with unchanged
   public behavior.
3. Add `CopPackLoader` and C ABI declarations behind the existing non-Windows
   `libloading` dependency.
4. Add plugin load validation and duplicate ID checks.
5. Add the minimal external example pack fixture and integration test.
6. Write the A6a/C4 ADR and connect pack/version rules to diagnostics.

## Open Constraints

- Windows dynamic loading is not part of the first pass; existing `libloading`
  dependency is already non-Windows-gated in `murphy-core`.
- Persistent cache keys from A5 must include pack name, pack version, plugin ABI
  version, and relevant `[cops.rules]` config once the cache exists.
- A future node-level plugin ABI must use explicit node handles and host queries,
  not `ruby_prism` Rust types or borrowed pointers across FFI.
