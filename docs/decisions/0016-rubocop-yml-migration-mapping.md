# ADR 0016 — One-way `.rubocop.yml` migration mapping

- Date: 2026-05-20
- Status: Accepted
- Issue: `murphy-3c3.4`

## Decision

`murphy migrate <.rubocop.yml>` performs a one-way, lossy conversion to Murphy's
own `murphy.toml` schema. It is a bootstrap helper, not a compatibility layer.

## Mapping

| RuboCop key | Murphy key | Notes |
|---|---|---|
| `AllCops.Include` | `[files].include` | Defaults to `**/*.rb` when absent. |
| `AllCops.Exclude` | `[files].exclude` | Glob strings are copied as provided. |
| `<CopName>.Enabled` | `[cops.rules."<CopName>"].enabled` | Boolean only. |
| `<CopName>.Severity` | `[cops.rules."<CopName>"].severity` | `warning` and `error` only. |

Unsupported RuboCop keys are dropped. The migrated output intentionally makes no
claim that Murphy implements the corresponding RuboCop cop semantics.

## CLI Contract

- Output is written to stdout.
- The input file is not modified.
- Missing or unreadable input exits 2 and writes no stdout.
- Output must parse as `MurphyConfig`.
