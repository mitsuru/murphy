# ADR 0049 — Thin pack registry over Gem distribution (C1)

- Date: 2026-09-25
- Status: Accepted
- Issue: `murphy-fmw.3.1` (C1: cop registry / 公式ディレクトリ)
- Parent: `murphy-fmw.3` (Phase 10 epic)
- Depends on: ADR 0048 (gem distribution), ADR 0046 (pack format), ADR 0042 (name resolution)
- Related: ADR 0038 (single-surface plugin ABI)

## Context

The Phase 10 design (§4.3, §6) left C1 open: does Murphy need a hosted
cop registry, or does Gem distribution (C2) + `Gemfile` discovery suffice?
C2 (ADR 0048) is now adopted: `gem install murphy` ships platform gems,
`bundle exec murphy lint` resolves `murphy-*` packs from installed gems,
and `Gemfile` owns versions (RuboCop parity). Phase 10 gate 3 still needs
a `murphy add <pack>` UX that completes pack installation from a registry.

## Decision

Gem-only suffices for distribution. C1 ships **no hosted service**.
Instead:

1. A thin static catalogue, `registry/index.toml`, overlays
   Murphy-specific metadata RubyGems cannot express per pack:
   `murphy-api-version` (required host ABI) and `min-murphy-version`
   (minimum core version), plus the gem name and catalogue text.
2. `murphy-core::pack_registry` parses the index (bundled at compile
   time, overridable via `--registry <path>` / `$MURPHY_REGISTRY_PATH`
   for mirrors), resolves names, and checks compat **before** any config
   edit or `dlopen`.
3. `murphy add <pack> [--registry PATH] [--dry-run]` appends
   `plugins: - <pack>` to `.murphy.yml` (append-only, comment-preserving)
   and prints the Bundler next step (`bundle add <gem>` / `Gemfile` +
   `bundle install`). It never shells out to `bundle`/`gem` and never
   touches the network — Bundler owns the install, Murphy owns the config.

## Consequences

- Gate 3: `murphy add murphy-rails` completes the config half of pack
  installation; `bundle install` completes the artifact half.
- No ABI bump (`MURPHY_PLUGIN_ABI_VERSION` stays `4`).
- No new service to operate; registering an official pack is an index +
  test edit.
- Incompatible packs fail fast with an actionable message (ABI rebuild
  vs core upgrade) and never write `.murphy.yml`.

## Alternatives considered

- Full hosted registry service (search, publish, auth, versioning):
  rejected — duplicates RubyGems/Bundler, which C2 adoption validates.
- `murphy add` auto-editing `Gemfile` / running `bundle install`:
  rejected — version management is Bundler's job (ADR 0048); Murphy
  prints the command instead.
- Pack cop lists in the index: rejected — `murphy-plugin.toml`
  `[cops].provided` stays the source of truth; the index is a catalogue,
  not a manifest copy.
