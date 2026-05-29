# ADR 0015 — Phase 5 Murphy-owned config schema

- Date: 2026-05-20
- Status: Superseded (see note below)
- Issue: `murphy-3c3.2`

> **Superseded 2026-05-30 (murphy-5saj):** The config file was changed from
> `murphy.toml` (TOML, Murphy-owned schema) to `.murphy.yml` (YAML,
> RuboCop-compatible format). The new schema uses `AllCops: Include/Exclude/CopsPath`
> and top-level cop sections (e.g. `Style/Foo: {Enabled: false}`) instead of
> `[files]`/`[cops.rules."Name"]` TOML tables. The "not RuboCop-compatible by
> design" decision below is reversed — `.murphy.yml` IS designed to be compatible.
> The content below is preserved as historical record.

## Decision

Murphy owns `murphy.toml`. It is not RuboCop-compatible by design.

Supported schema:

```toml
[files]
include = ["**/*.rb"]
exclude = []

[cops]
path = "cops"

[cops.rules."Murphy/NoReceiverPuts"]
enabled = true
severity = "warning" # or "error"
```

Defaults are equivalent to no config: include all Ruby files recursively, exclude
nothing, load user cops from `cops/`, and run all cops at their emitted severity.

Known tables reject unknown fields. This keeps typos loud while allowing Murphy
to grow its own schema deliberately through ADRs.

## Runtime Effects

- `[files]` controls directory/zero-arg discovery only. Explicit file paths are
  still linted exactly as provided.
- `[cops].path` controls where user cops are loaded from and which directory is
  excluded from ordinary directory discovery.
- `[cops.rules.<cop_name>].enabled = false` removes that cop from the run.
- `[cops.rules.<cop_name>].severity` overrides emitted offense severity before
  aggregation, so the configured severity participates in existing deterministic
  severity-precedence rules.

## Non-Goals

- No RuboCop config compatibility in `murphy.toml`.
- No recursive cop loading beyond the configured flat cops directory.
- No per-cop arbitrary option schema beyond the Phase 5 enable/severity fields.

## Follow-up

`murphy-4n9.6` extends `cops.rules` after Phase 5 by preserving arbitrary
per-cop option keys and passing them to native plugin packs through ABI v1. The
Phase 5 decision still owns `enabled` and `severity`; plugin packs interpret the
additional RuboCop-compatible option keys per cop.
