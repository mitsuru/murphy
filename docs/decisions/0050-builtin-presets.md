# ADR 0050 — Builtin config presets (`extends:` / `--preset`, C3)

- Date: 2026-09-25
- Status: Accepted
- Issue: `murphy-fmw.3.3` (C3: プリセット / 設定プロファイル)
- Parent: `murphy-fmw.3` (Phase 10 epic)
- Depends on: ADR 0046 (pack format), ADR 0042 (name resolution), ADR 0045 (YAML config)
- Related: ADR 0048 (gem distribution), ADR 0049 (thin registry)

## Context

Phase 10 design §4.3 asks for official presets (`extends = "murphy:rails-strict"`,
`"minimal"`, `"recommended"`, …) and leaves the distribution form open:
part of an official cop pack vs an independent "config pack". A6a (pluggable
cop packs) is done, so presets could ride either form.

## Decision

Official presets ship **builtin with the binary** (`murphy-core::presets`,
no network, no registry, no gem). Third-party presets travel via
`inherit_from:` file includes. Rationale:

- Presets cross pack boundaries (`minimal` mutes Metrics/Naming/packaging;
  `rails-strict` enables curated `Rails/*` + ASE). Binding them to one cop
  pack would split the catalogue or force a synthetic "config pack" gem for
  a few dozen YAML lines.
- A hosted/registry distribution repeats C1's rejected complexity (C1 proved
  a thin catalogue over Gems suffices for packs; presets need even less).
- Builtin keeps `murphy init` (C5) and `--preset` fast-paths offline and
  version-locked to the running core.

Surface:

- `.murphy.yml` top-level `extends:` (string or list): `murphy:<name>` or
  bare `<name>`. Example: `extends: murphy:rails-strict`.
- CLI `--preset <name>` on `lint`, `cops list`, `watch` (same name forms).
- Catalogue v1: `minimal`, `recommended`, `shopify`, `rails-strict`
  (see `docs/guides/presets.md` for deltas).
- Precedence (lowest first): bundled defaults < file `extends:` in order <
  `--preset` < user `Enabled:`/options/`AllCops`/`inherit_from` content.
  Unknown names fail with exit 2 listing available presets.

## Consequences

- Phase 10 gate 4 (`murphy init` + 5s lint) can seed `extends:` without a
  network fetch; C5 reuses this layer.
- No ABI bump (`MURPHY_PLUGIN_ABI_VERSION` stays `4`).
- `rails-strict` needs `murphy-rails` installed to run the `Rails/*` cops
  (`murphy add murphy-rails`); without the pack its `Enabled: true` entries
  are inert but harmless (documented).
- Presets are curated deltas, not wildcard modes: `minimal` lists explicit
  `Enabled: false` entries (Metrics/Naming/Bundler/Gemspec/docs); a future
  `DisabledByDefault` wildcard mode is out of scope.

## Alternatives considered

- Presets as cop-pack-bundled `default.yml` fragments: rejected — presets
  span packs, and pack defaults already occupy that layer.
- Independent "config pack" gems with registry entries: rejected — versioning
  a 30-line YAML via RubyGems + registry adds distribution cost with no
  offline benefit.
- `inherit_gem`-style remote includes: rejected — network fetch at lint time,
  pinning and sandbox questions; `inherit_from:` local files already cover
  team-shared presets.
