# ADR 0048 — Gem distribution + Bundler integration (C2)

- Date: 2026-09-25
- Status: Accepted
- Issue: `murphy-fmw.3.2` (C2: Gem 配布 + Bundler 統合)
- Parent: `murphy-fmw.3` (Phase 10 epic)
- Depends on: ADR 0046 (pack format), ADR 0042 (name resolution)
- Related: ADR 0038 (single-surface plugin ABI)

## Context

Phase 10 gate 1–2 require `gem install murphy` to deliver the official
gem and `bundle exec murphy lint` to work with cop-pack discovery from
gems. Murphy is a Rust binary; Ruby users install via RubyGems and
version-manage via `Gemfile` (RuboCop parity). The resolver (ADR 0042)
knew only `MURPHY_PLUGIN_PATH` / project-local / user-local — it could
not see `bundle install`ed `murphy-*` packs.

## Decision

### 1. `murphy` gem ships precompiled per-platform binaries

- One platform gem per target: `x86_64-linux`, `aarch64-linux`,
  `x86_64-darwin`, `arm64-darwin` (`MURPHY_GEM_PLATFORM` at `gem build`
  time; the string doubles as the `libexec/murphy-<platform>` suffix).
- `exe/murphy` (Ruby) resolves the matching `libexec/` binary and
  `exec`s it: signals, stdio, and exit codes pass through unchanged.
  Fallbacks in order: `$MURPHY_NATIVE_BINARY`, `libexec/murphy-<tag>`,
  `target/release/murphy`, `target/debug/murphy` (source checkout).
- Outside Bundler (no `GEM_HOME`/`GEM_PATH`), the wrapper exports
  `MURPHY_GEM_PATH` from `Gem.path` so the native resolver still finds
  installed packs.
- `scripts/build-gem.sh` builds one platform gem locally;
  `.github/workflows/release-gem.yml` builds the 4-platform matrix and
  pushes to RubyGems on `v*` tags.

### 2. Resolver gains a gem layer between project-local and user-local

`plugins = ["murphy-rails"]` now resolves:

1. `Detailed` pin, 2. `MURPHY_PLUGIN_PATH`, 3. `.murphy/plugins/`,
4. **installed gems** (`MURPHY_GEM_PATH`, then `GEM_HOME`/`GEM_PATH`),
5. user-local.

`MURPHY_GEM_PATH` is the explicit/test override; `GEM_HOME`/`GEM_PATH`
are what `bundle exec` sets, so no `bundle show` subprocess is needed.
Each gemdir's `gems/<name>-<version>/` candidates sort by version
descending (numeric-aware: `0.10.0` beats `0.9.0`); the first candidate
whose probe yields a cdylib wins. Probes per gem dir: `<gem>/` as pack
dir, `<gem>/murphy/` as pack dir, legacy `<gem>/lib/<lib_filename>`.
Broken gems are skipped, never shadowing later layers.

`Gemfile` owns versions, `.murphy.yml` owns cop config — the same split
RuboCop uses.

### 3. Out of scope

- `murphy add <pack>` registry UX (C1 decides registry-vs-Gemfile after
  C2 adoption data).
- Pack signing (ADR 0004 trust model).
- `cargo install` parity and multi-arch auto-selection beyond the
  ADR 0046 narrow rule.

## Consequences

- Gate 1: `gem install murphy --platform <tag>` delivers a working
  `murphy` on all four targets.
- Gate 2: `bundle exec murphy lint` resolves `Gemfile`-installed packs
  with no `path:` configuration.
- No ABI bump (`MURPHY_PLUGIN_ABI_VERSION` stays `4`).

## Alternatives considered

- Shell out to `bundle info --paths` at lint time: rejected — slow,
  fragile outside `bundle exec`, and redundant since Bundler already
  exports `GEM_HOME`/`GEM_PATH`.
- Parse `Gemfile` directly for versions: rejected — version selection
  is Bundler's job; the resolver reads the installed result.
- Single fat gem with four binaries: rejected — 4x download for every
  user; platform gems are the RubyGems convention.
- Gem root `lib/` collision avoidance by mandating `<gem>/murphy/`:
  rejected as mandatory — both `<gem>/` and `<gem>/murphy/` probe, so
  existing gem layouts keep working.
