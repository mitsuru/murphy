# ADR 0026 — Restricted require allowlist PoC result

- Date: 2026-05-20
- Status: Accepted
- Issue: `murphy-bn3.1.4`
- Related: ADR 0023, ADR 0024
- Feeds: `murphy-bn3.1` Phase 7 third-party cop sandbox

## Decision

Third-party cop `require` uses Murphy's resolver rather than mruby's ambient load
path. The resolver accepts package-root files and Murphy-managed allowlisted pure
Ruby stdlib only.

## Resolution rules

1. Canonicalize the package root once before execution.
2. Reject absolute require paths from third-party code.
3. Resolve allowlisted stdlib names from Murphy's immutable stdlib root.
4. Resolve relative package requires only if the canonical result remains under
   the package root.
5. Reject `load` for third-party packages.
6. Reject native extensions and files outside `.rb` source form.

The resolver is the only authority. Runtime modifications to `$LOAD_PATH` or
`$LOADED_FEATURES` cannot grant access beyond this policy.

## PoC cases

- `require_relative "lib/helper"` succeeds for a package-local helper.
- `require "set"` succeeds only when `set` is shipped in Murphy's pure stdlib
  allowlist.
- `require "/tmp/evil"`, `require_relative "../evil"`, and symlink escapes fail.
- `require "socket"`, `require "open3"`, and native extension require fail.
- Package-local `json.rb` does not shadow allowlisted stdlib `json`.

## Failure mode

Denied requires raise the same sandbox-denial exception shape used by other
denied capabilities. The cop runner converts the denial to an isolated error
offense in the failure integration milestone.

## Consequences

- The implementation must not rely on ambient cwd-relative mruby require search.
- The package root must be canonicalized before any package code executes.
- The stdlib allowlist version is part of package cache keys so policy changes
  invalidate stale require decisions.
