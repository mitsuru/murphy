# ADR 0024 — Third-party cop capability policy

- Date: 2026-05-20
- Status: Accepted
- Issue: `murphy-bn3.1.2`
- Related: ADR 0004, ADR 0021, ADR 0023
- Feeds: `murphy-bn3.1` Phase 7 third-party cop sandbox

## Context

ADR 0023 defines the first third-party cop sandbox milestone as a
capability-restricted mruby runtime. This ADR turns that threat model into the
initial allow/deny policy for Ruby-visible APIs and code loading.

## Decision

The MVP third-party cop sandbox uses a deny-by-default Ruby capability policy.
Third-party cops may use only Murphy's cop DSL, AST read primitives,
offense/autocorrect reporting primitives, package-root require, and allowlisted
pure Ruby stdlib supplied by Murphy.

Host capabilities are denied: `File`, `Dir`, `IO`, `Socket`, `Process`, `ENV`,
`Kernel#system`, backticks, `exec`, `spawn`, `Open3`, unrestricted `load`,
absolute-path require, package-root escapes, non-allowlisted stdlib, native
extensions, FFI, and `dlopen`.

## Require policy

`require` and `require_relative` are allowed only through Murphy's resolver.
Resolution has two namespaces:

- `murphy_stdlib`: immutable, Murphy-managed pure Ruby stdlib modules.
- `package`: files under the canonical package root.

Allowlisted stdlib names resolve before package-local bare names. A package-local
`json.rb` cannot shadow Murphy's allowlisted `json`. Package-local files should
use explicit relative require when they intend package resolution.

## Initial allowlist

The first allowlist is intentionally small: `set` and `json` only if Murphy ships
pure Ruby implementations that do not expose filesystem, process, network, or
native extension access. If Murphy cannot ship a safe pure Ruby implementation,
the name remains denied.

## Denied behavior contract

Denied capability use raises a Ruby exception that the existing mruby cop runner
maps to one isolated error offense. It must not abort the host process or affect
other cops.

## Future capabilities

Any new host-facing capability must be named, package-scoped, testable, and
documented. For example, bundled read-only package assets may be added later as a
specific package-asset API, not by re-enabling unrestricted `File.read`.

## Consequences

- `murphy-bn3.1.3` must prove a restricted custom mruby runtime can enforce the
  denied API set while preserving Murphy's legitimate cop APIs.
- `murphy-bn3.1.4` must prove the require policy is implementable without ambient
  `$LOAD_PATH` behavior.
- `murphy-bn3.1.8` must prove package-local caches cannot reuse this policy under
  stale package content or another package's identity.
