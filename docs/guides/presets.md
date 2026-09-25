# Config presets (`extends:` / `--preset`, C3)

Official presets are builtin named config layers (no network, no registry).
Design: ADR 0050. They resolve the Phase 10 §4.3 distribution question on the
side of "builtin, not separate config packs".

## Names

| Preset | Meaning |
|---|---|
| `minimal` | Lint + Security + Layout focus; disables Metrics (10), Naming (19), Bundler (7), Gemspec (10), docs cops (3). For large legacy repos / CI fast path. |
| `recommended` | Murphy defaults (no overrides; documents intent). |
| `shopify` | Shopify-style opinions: single quotes, ruby19 hash, docs cops off, relaxed `Metrics/MethodLength Max: 20`, `Metrics/BlockLength Max: 30`. |
| `rails-strict` | Strict Rails: `ActiveSupportExtensionsEnabled: true` + curated `Rails/*` enabled (`Blank`, `FindBy`, `FindEach`, `SaveBang`, `WhereNot`, `CreateTableWithTimestamps`, `DefaultScope`, `HasManyOrHasOneDependent`, `InverseOf`, `TimeZone`, `Validation`, `SkipsModelValidations`). Needs `murphy-rails` to run (`murphy add murphy-rails`); without the pack the entries are inert. |

Both `murphy:<name>` and bare `<name>` are accepted.

## Use

```yaml
# .murphy.yml
extends: murphy:rails-strict
```

```yaml
# list form, later wins
extends:
  - murphy:minimal
  - murphy:shopify
```

```sh
murphy lint --preset minimal
murphy lint --preset murphy:shopify
murphy cops list --preset minimal --format=json
murphy watch --preset recommended --once
```

Precedence (lowest first): bundled defaults < file `extends:` in order <
`--preset` < user `Enabled:`/options/`AllCops`. A user `Enabled: true`
opts back into a preset-disabled cop; a user `Enabled: false` mutes a
preset-enabled cop.

`extends:` composes with `inherit_from:` file includes: inherited files'
preset refs accumulate base-first, the top file's refs win within the preset
layer, and user rules always win overall.

## Errors

Unknown names fail with exit 2 and list available presets:

```sh
murphy lint --preset murphy:nope
# murphy: unknown preset `murphy:nope` (available: minimal, recommended, shopify, rails-strict) ...
```

The same message appears for `extends: murphy:nope` in `.murphy.yml`.

## Team-shared presets

Official presets cover the common cases. Team-shared deltas ship as files
via `inherit_from:` (no registry):

```yaml
# .murphy.yml
inherit_from: config/murphy-team.yml
extends: murphy:recommended
```

## Reference

- ADR 0050 (`docs/decisions/0050-builtin-presets.md`): decision.
- `crates/murphy-core/src/presets.rs`: catalogue + YAML deltas.
- `crates/murphy-core/src/config.rs`: `extends:` parsing + `resolve_preset_layer`.
- `crates/murphy-cli/src/main.rs`: `--preset` on `lint`/`cops list`/`watch`.
