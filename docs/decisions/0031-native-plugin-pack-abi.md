# ADR 0031 - Native plugin pack ABI and distribution contract

- Date: 2026-05-20
- Status: Accepted
- Issues: `murphy-fmw.1.3`, `murphy-fmw.1.2`, `murphy-fmw.1.5`

## Context

Phase 8 adds native cop packs so third-party Rust cops can be distributed without
rebuilding Murphy. Dynamic loading gives pack authors flexibility, but it also
turns ABI shape, cop identity, and versioning into public contracts.

## Decision

Native packs are loaded as `cdylib` libraries through a C-compatible ABI. The
current provisional ABI version is `2`. A pack must export `murphy_plugin_abi_version` and
`murphy_register_plugin`. Murphy rejects missing symbols, ABI mismatches,
registration failures, duplicate cop IDs, invalid UTF-8 names, and invalid ranges
as setup errors.

ABI v2 passes RuboCop-compatible cop options to native callbacks as UTF-8 JSON
bytes. `MurphyFileContext.config` contains only the current cop's option object
from `[cops.rules."Cop/Name"]`, excluding Murphy-owned `enabled` and `severity`.
`MurphyCallContext.config` is a pack-level JSON object keyed by cop ID because a
single pack dispatch callback may emit offenses for multiple cops.

Plugin callbacks may run concurrently on multiple OS threads because Murphy keeps
file-level `rayon` parallelism. Pack authors must synchronize shared mutable
state and must not retain host-owned pointers after a callback returns. Rust
panics must not unwind across the plugin ABI boundary.

## Distribution Contract

- Cop IDs are stable public identifiers.
- Renaming a cop means adding a new ID and deprecating the old one.
- Default severity changes require a major pack version bump.
- Config keys are additive within a major version.
- Removing a config key or changing its meaning requires a major version bump.
- Plugin ABI version and pack semantic version are separate contracts. The ABI is
  still pre-1.0 and may break while Murphy's native pack interface is stabilizing.
- Native plugin packs are trusted code with the privileges of the Murphy process.

## Consequences

The C ABI is more verbose than a Rust trait-object boundary, but avoids relying
on Rust ABI stability. Node-level plugin APIs are deferred until Murphy has a
versioned node-handle ABI; the first plugin ABI is file-level.
