# ADR 0029 — Phase 7 sandbox gate review

- Date: 2026-05-20
- Status: Accepted
- Issue: `murphy-bn3.1.7`
- Parent: ADR 0023

## Gate result

The Phase 7 sandbox milestone is ready to be re-planned as implementation work.
The accepted MVP sandbox is capability-restricted mruby with package-scoped
require, package fingerprint cache invalidation, and isolated error-offense
handling for denied capabilities.

The gate does not claim third-party distribution is safe today. It records that
the design is specific enough to split into implementation tasks with TDD and
feature-gated integration tests.

## Implementation tasks to create next

- Build restricted mruby runtime profile.
- Add runtime boot self-check for denied constants and methods.
- Implement Murphy-managed require resolver.
- Implement package fingerprinting and package-scoped cache keys.
- Map sandbox denials to isolated error offenses.
- Add feature-gated integration tests for denied APIs, require policy, cache
  isolation, and deterministic output.

## Third-party distribution gate

Third-party cop distribution remains blocked until the implementation tasks pass
with tests proving denied APIs, restricted require, cache invalidation, and
failure integration.

## Non-MVP follow-up

seccomp, Landlock, namespaces, and separate worker processes remain future
defense-in-depth. They should be revisited if the restricted mruby surface cannot
close a required host capability or if third-party package distribution grows a
threat model beyond Ruby-visible capability control.
