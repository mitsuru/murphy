# ADR 0017 — Phase 5 Gate review (config + migrate complete)

- Date: 2026-05-20
- Status: Accepted — **GATE PASSED**
- Epic: `murphy-3c3` (Phase 5 — config + one-way migration)
- Preserves: ADR 0006/0007/0012/0014 default-output determinism and JSON shape
- Adds: ADR 0015 config schema; ADR 0016 migration mapping
- Effect: Phase 6 (`murphy-7rg`) may start

## Verdict

**PASS.** Murphy now has an owned configuration format and a one-way migration
helper from `.rubocop.yml`.

Completed scope:

- `ConfigError` display no longer forces discovery wording onto registry/cop
  setup failures.
- `murphy.toml` supports `[files]` and `[cops]` with `path`, per-cop `enabled`,
  and per-cop `severity`.
- Native and mruby cops honor per-cop enable/disable.
- Severity overrides are applied before aggregation.
- Directory discovery excludes the configured cops path, while explicit cop-file
  targets are still linted when named directly.
- `murphy migrate <.rubocop.yml>` emits Murphy TOML to stdout and the output
  round-trips into lint behavior.
- Default configuration preserves existing sample-project output.

## Verification

Phase 5 added focused tests for config parsing, CLI config behavior, cops-path
discovery exclusion, and migration roundtrip. The final gate requires:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test
```

## Deferred

- Broader RuboCop key coverage is intentionally deferred; migration remains
  lossy and one-way.
- Standard cop expansion and perf-regression CI remain Phase 6 work.
- Third-party cop sandboxing remains a later security milestone.
