# ADR 0025 — Restricted mruby runtime PoC result

- Date: 2026-05-20
- Status: Accepted
- Issue: `murphy-bn3.1.3`
- Related: ADR 0021, ADR 0023, ADR 0024
- Feeds: `murphy-bn3.1` Phase 7 third-party cop sandbox

## Context

Murphy already has a feature-gated custom runtime seam that can load an alternate
mruby runtime from `MURPHY_MRUBY_CUSTOM_BUILD_PATH`. The sandbox runtime should
extend that path rather than attempting to load unrestricted Ruby APIs and filter
dangerous calls after the fact.

## Decision

Murphy will implement the restricted runtime by extending the custom mruby build
path, not by filtering after loading unrestricted Ruby APIs.

## PoC criteria

The implementation PoC must prove these expressions fail inside the restricted
runtime: `File.read`, `Dir.entries`, `IO.read`, `ENV.to_h`, `system`, backticks,
`Process.pid`, `Socket.tcp`, `Open3.capture3`, `load`, and native extension
loading. The same runtime must still run a minimal Murphy cop that uses the DSL,
reads node data, reports an offense, and registers an autocorrect edit.

## Expected mechanism

Build-time configuration removes or never loads dangerous mruby gems/classes.
Runtime boot validates that denied constants/methods are absent before executing
third-party code. Validation failure is a setup error for the custom runtime, not
a silently weakened sandbox.

The custom runtime remains responsible for returning an owned `mrb_state` and, if
needed, a matching close hook. Murphy's existing custom-open tracking prevents a
fallback default state from being closed by the custom runtime.

## Evidence required before full implementation

Tests must run with `--features murphy-core/mruby-custom-build` and a test
runtime path. The test fixture must attempt each denied expression and assert the
failure is surfaced as an isolated cop error, while a legitimate cop still
returns deterministic offenses.

## Consequences

- The restricted runtime PoC must be feature-gated and must not change default
  `mruby3-sys` behavior.
- The runtime must fail closed: if a denied API is still present at boot, Murphy
  treats that custom runtime as unusable for sandboxed third-party cops.
- Runtime restriction and instruction-budget hooks remain one custom mruby build
  work item, matching ADR 0005's Phase 7 guidance.
