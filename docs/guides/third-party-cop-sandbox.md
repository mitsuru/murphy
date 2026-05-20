# Third-party cop sandbox

Murphy's planned third-party cop sandbox is a capability-restricted mruby
runtime. Third-party cops are not allowed to access host filesystem,
environment, network, process execution, native extensions, or arbitrary code
loading.

This guide describes the intended Phase 7 contract. v1 cops in `cops/` remain
trusted code until the sandbox implementation gate passes.

## Allowed

- Murphy cop DSL.
- Murphy AST reader methods.
- Murphy offense and autocorrect reporting methods.
- Package-local Ruby helpers under the package root.
- Murphy-managed allowlisted pure Ruby stdlib modules.

## Denied

- `File`, `Dir`, `IO`, `Socket`, `Process`, `ENV`.
- `system`, backticks, `exec`, `spawn`, `Open3`.
- `load`, absolute-path require, package-root escapes.
- Non-allowlisted stdlib, native extensions, FFI, `dlopen`.

## Require

Use package-local relative require for package helpers. Bare stdlib names resolve
only when Murphy ships a safe pure Ruby implementation in the allowlist. Package
files cannot shadow Murphy-managed stdlib modules.

`load` is not part of the third-party cop package contract because it re-runs
arbitrary paths and makes cache/fingerprint behavior harder to reason about.

## Cache invalidation

Murphy fingerprints third-party cop package content. If any package Ruby file,
manifest, config, or vendored helper changes, package-scoped require and sandbox
caches are invalidated for that package only.

Package-local caches also include the sandbox policy version and stdlib allowlist
version. A policy change invalidates stale decisions even when package content is
unchanged.

## Denied API behavior

A denied capability becomes a cop error offense with a `Sandbox violation:`
message. The host process should not crash, sibling cops should continue, and
output ordering should remain deterministic.

## OS sandboxing

seccomp, Landlock, namespaces, and separate worker processes are reserved as
future defense-in-depth. They are not the MVP sandbox mechanism.
