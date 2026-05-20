# ADR 0028 — Sandbox failure integration PoC result

- Date: 2026-05-20
- Status: Accepted
- Issue: `murphy-bn3.1.5`
- Related: ADR 0003, ADR 0011, ADR 0023, ADR 0024
- Feeds: `murphy-bn3.1` Phase 7 third-party cop sandbox

## Decision

Sandbox denial is treated like a cop exception: one isolated error offense for
that cop and file, then execution continues for other cops and files.

## Error shape

The error offense uses the offending cop name, severity `error`, and a message
prefix `Sandbox violation:` followed by the denied capability name. The JSON
contract remains the existing offense shape; no new top-level fields are added.

The range should use the best available location for the denied call when the
runtime can provide it. If the restricted runtime cannot report a precise range,
the existing mruby error-offense fallback range is acceptable for the PoC.

## Determinism

Sandbox denials participate in the existing aggregate ordering and severity
precedence. If a denied cop races with timeout at an exact deadline boundary, ADR
0003's accepted boundary race applies; normal headroom cases must be stable.

## PoC cases

- Denied `File.read` becomes one error offense.
- Denied `require "socket"` becomes one error offense.
- A sibling well-behaved cop still runs on the same file.
- A denied cop in one file does not poison later files.
- Output order remains byte-identical across repeated runs with the same inputs.

## Interaction with existing isolation

The sandbox denial path must reuse the existing per-cop `mrb_state` isolation and
watchdog model. A denial is synchronous and should normally be observed before a
deadline. If a cop reaches a runaway loop after a denial is rescued by user code,
the normal deadline behavior still applies.

## Consequences

- The implementation must not add a second JSON error channel for sandbox
  violations.
- Existing aggregate ordering and ADR 0011 severity precedence remain the single
  output determinism mechanism.
- Docs should teach users to treat `Sandbox violation:` as a cop/package authoring
  error, not as a Murphy internal crash.
