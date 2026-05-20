# ADR 0023 — Third-party cop sandbox threat model

- Date: 2026-05-20
- Status: Accepted
- Issue: `murphy-bn3.1.1`
- Related: ADR 0003 (cop deadlines), ADR 0004 (v1 trust model), ADR 0021
  (custom mruby build path)
- Feeds: `murphy-bn3.1` Phase 7 third-party cop sandbox

## Context

ADR 0004 deliberately ships v1 without a security sandbox: a `.rb` file in
`cops/` is trusted code and runs in-process with Murphy's privileges. That is
acceptable only while cops are first-party or deliberately vendored. It is not an
acceptable permanent posture for third-party cop distribution.

Phase 7 already introduced a custom mruby runtime seam (ADR 0021) for
instruction-hook and runtime customization work. The sandbox design should use
that seam before introducing a separate process or OS sandbox, unless the mruby
capability surface proves impossible to close.

## Decision

The first third-party cop sandbox milestone will target a
**capability-restricted mruby runtime**. The primary security boundary is the
Ruby-visible capability surface, not a syscall boundary.

For the MVP sandbox, Murphy must make it impossible for third-party cop Ruby code
to directly reach host filesystem, environment, network, process execution, or
arbitrary code loading capabilities. Murphy will expose only the explicit cop DSL
and linting primitives required to inspect AST nodes and report offenses.

OS sandboxing such as seccomp, Landlock, namespaces, or a separate worker process
is deferred as defense-in-depth. It is not part of the first proof because it
would force a larger IPC/AST transport redesign while the more immediate risk is
the mruby API surface Murphy chooses to expose.

## Assets to protect

- Developer and CI secrets in environment variables, credential files, and tool
  config.
- Repository contents outside the analyzed source text visible to the cop API.
- Filesystem integrity, including source files, lockfiles, build artifacts, and
  generated outputs.
- Network boundary: a cop must not exfiltrate source or credentials.
- Process boundary: a cop must not spawn shell commands or helper processes.
- Murphy host process integrity: a cop failure must remain an isolated offense,
  not a host crash or corrupted shared runtime state.
- Deterministic lint output: one cop package must not influence another cop's
  diagnostics through shared mutable runtime/cache state.

## In-scope threats

- Direct filesystem access through `File`, `Dir`, `IO`, path-oriented stdlib, or
  native extensions.
- Environment access through `ENV` or equivalent host bindings.
- Network access through socket APIs or networking stdlib.
- Process execution through `Kernel#system`, backticks, `exec`, `spawn`,
  `Process`, `Open3`, or equivalent bindings.
- Arbitrary code loading through unrestricted `require`, `require_relative`,
  `load`, absolute paths, package-root escapes, or native extension loading.
- Cache pollution across cop packages: `$LOAD_PATH`, `$LOADED_FEATURES`, loaded
  constants, monkey patches, require-resolution caches, compiled-code caches, or
  host-side package metadata from package A must not affect package B.
- Standard-library shadowing: a package-local `json.rb` or similar file must not
  replace an allowlisted Murphy-managed stdlib module for another package or for
  the same package when stdlib resolution is requested.
- Symlink/canonicalization escapes that make a path appear package-local while it
  resolves outside the package root.
- Stale trust decisions: if any package code, manifest, or vendored helper changes,
  package-scoped sandbox caches must not be reused under the old fingerprint.

## Non-goals for the first sandbox milestone

- Protecting against arbitrary native Rust bugs in Murphy itself.
- Running third-party cops from unreviewed package registries before the sandbox
  gate passes.
- Allowing third-party cops to read arbitrary project files as a feature.
- Providing a Linux-only seccomp/Landlock guarantee in the first milestone.
- Supporting C extensions, FFI, `dlopen`, or host-native Ruby gems in third-party
  cops.
- Preserving compatibility with unrestricted RuboCop extensions that depend on
  filesystem, process, or network access.

## Initial capability policy

The next design task must convert this threat model into a precise allow/deny
list. The intended direction is:

- Allow Murphy's cop DSL.
- Allow AST read primitives exposed by Murphy.
- Allow offense and autocorrect reporting primitives exposed by Murphy.
- Allow `require` only for package-root files and allowlisted pure Ruby stdlib
  supplied by Murphy.
- Deny `File`, `Dir`, `IO`, `Socket`, `Process`, `ENV`, `Kernel#system`,
  backticks, unrestricted `load`, absolute-path require, package-root escapes,
  non-allowlisted stdlib, and native extensions.

Any future capability expansion must be explicit and testable. For example, if a
cop package needs read-only access to bundled assets, that should be a named
package-asset capability, not unrestricted `File.read`.

## Cache isolation requirements

Third-party cop package caches must be package-scoped. The cache key for any
package-local sandbox decision must include at least:

- `package_id`
- `package_fingerprint`
- `sandbox_policy_version`
- `stdlib_allowlist_version`

The package fingerprint must be content-based, not mtime-based, and must cover
the package files that can affect execution: cop `.rb` files, manifest/config,
and vendored helpers. A one-byte change in those inputs invalidates that package's
require-resolution, loaded-feature, manifest/allowlist, and future compiled-code
caches. It must not invalidate unrelated packages.

Murphy-managed immutable stdlib caches, host source parse caches, native cop
registry data, and global policy versions are separate caches. They are not keyed
by package fingerprint unless they directly depend on package input.

## Failure handling contract

A sandbox violation must integrate with existing mruby cop isolation:

- Denied capability use becomes a structured cop failure/error offense.
- The host process must not crash.
- Other cops and files must continue running.
- Output ordering and aggregation must remain deterministic.
- Timeout behavior from ADR 0003 remains a separate fault-isolation boundary, not
  a security capability boundary.

## Consequences

- `murphy-bn3.1.2` must specify the exact capability whitelist/denylist and
  restricted `require` semantics.
- `murphy-bn3.1.3`, `.1.4`, and `.1.8` must prove the restricted runtime,
  require allowlist, and package cache isolation are feasible before full
  implementation planning.
- `murphy-bn3.1.5` must prove denied capability usage maps to isolated error
  offenses.
- Third-party cop distribution remains blocked until the Phase 7 sandbox gate
  records that these proofs are sufficient.
