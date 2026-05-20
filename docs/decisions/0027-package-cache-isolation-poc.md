# ADR 0027 — Package cache isolation PoC result

- Date: 2026-05-20
- Status: Accepted
- Issue: `murphy-bn3.1.8`
- Related: ADR 0023, ADR 0024, ADR 0026
- Feeds: `murphy-bn3.1` Phase 7 third-party cop sandbox

## Decision

All third-party cop package caches are scoped by package identity and package
fingerprint. A cache hit may reuse work only for the same package, same content,
same sandbox policy version, and same stdlib allowlist version.

## Fingerprint inputs

The package fingerprint is a content hash over canonical package-root-relative
paths and bytes for `.rb` files, package manifest/config, and vendored helpers.
mtime is ignored. Symlink targets are canonicalized; targets outside the package
root are rejected instead of fingerprinted.

The hash input must include both the normalized relative path and file bytes so a
rename cannot collide with an unchanged byte stream at a different path.

## Cache key

Every package-local cache key must include:

- `package_id`
- `package_fingerprint`
- `sandbox_policy_version`
- `stdlib_allowlist_version`

The same fingerprint under a different package id is a different cache entry.
This prevents package A from poisoning package B even if they contain identical
files.

## PoC cases

- A one-byte change in package A changes package A's fingerprint.
- A package A change does not invalidate package B.
- `$LOAD_PATH`, `$LOADED_FEATURES`, constants, and monkey patches from package A
  are not visible to package B.
- Package-local `json.rb` cannot shadow Murphy's allowlisted stdlib `json`.
- Symlink and canonicalization escapes are rejected before cache insertion.

## Cache classes

Package-scoped caches include require-resolution results, loaded feature sets,
manifest/config parsing, allowlist decisions, and future compiled mruby bytecode.
Global caches such as Murphy-managed immutable stdlib bytes, host source parse
results, native cop registry state, and policy version constants are separate and
must not be invalidated by unrelated package content changes.

## Consequences

- Any future performance cache for third-party cops must first declare whether it
  is package-scoped or global.
- Package fingerprinting must run before any package-local cache lookup.
- Cache hits are never allowed to widen capabilities; they can only reuse a
  decision already valid for the same policy and package fingerprint.
