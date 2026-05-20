# ADR 0030 — Scope out alternate cop frontends

- Date: 2026-05-20
- Status: Accepted
- Issue: `murphy-bn3.4`
- Related: ADR 0019 (native primitive IDL), ADR 0023 (third-party cop sandbox)
- Parent: `murphy-bn3` Phase 7

## Context

The original design reserved a language-neutral cop API boundary: native
primitives plus visitor-style hooks. That seam intentionally left room for future
Rune, Roto, or similar cop frontends while v1 used embedded mruby only.

The same design already rejected those runtimes for the current product path:
Rune offers little advantage over mruby for Murphy's use case, while Roto/Mun
trade Ruby-like cop authoring for maturity and ecosystem risk. Since then, Phase
7 work has become concrete around two higher-value tracks: LSP integration and a
third-party cop sandbox for the mruby path.

## Decision

Murphy will scope out alternate cop frontends from the current Phase 7 roadmap.
The product remains focused on native Rust cops and embedded mruby cops. The
language-neutral IDL remains a design constraint for the existing boundary, but
Murphy will not implement or prototype Rune, Roto, Mun, WASM, or other alternate
authoring runtimes as part of `murphy-bn3`.

## Rationale

- The current cop authoring value proposition is Ruby-like scripting over a fast
  Rust/prism core. Replacing that with a non-Ruby frontend weakens the migration
  path from RuboCop-style custom cops.
- Third-party distribution is blocked by sandboxing, not by frontend choice. The
  sandbox work should concentrate on the runtime Murphy already ships.
- LSP and sandbox implementation tasks are already large enough for Phase 7.
- Keeping a dormant alternate-runtime task in the ready list creates roadmap
  noise without a near-term user need.

## What remains

The native primitive IDL should stay clean and narrow. Future changes should avoid
unnecessary mruby-specific leakage at the Rust boundary when a generic shape is
equally simple. That preserves optionality without carrying an implementation
commitment.

## Revisit trigger

Open a new epic only if there is a concrete user or distribution need that mruby
cannot satisfy, such as:

- a proven sandbox limitation in restricted mruby,
- a large third-party cop ecosystem requiring another runtime,
- or a performance target that cannot be met with Rust primitives plus mruby
  glue.

Until then, alternate cop frontends are intentionally out of scope.
