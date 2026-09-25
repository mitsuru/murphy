# ADR 0053 — Static-index plugin marketplace (remote discovery)

- Date: 2026-09-25
- Status: Accepted
- Issue: `murphy-uk7.2` (uk7: plugin marketplace / index)
- Parent: `murphy-uk7` (Plugin ecosystem post-MVP)
- Depends on: ADR 0049 (thin registry C1), ADR 0048 (gem distribution C2),
  ADR 0046 (pack format), ADR 0042 (name resolution)
- Related: ADR 0038 (single-surface plugin ABI)

## Context

`murphy-uk7.1` gave non-Bundler users an artifact install
(`murphy plugins install <name>` → user-local dir) but only from
already-local sources: installed gems (C2 discovery) or `--from <dir>`.
The C1 registry (`registry/index.toml`) is a local catalogue with compat
metadata and no download source. The umbrella asks for remote pack
discovery + download (search, fetch, publish flow) without building a
hosted service.

## Decision

Extend the C1 static index with an optional download source per pack and
fetch it with system tools. No hosted service, no new Cargo deps, no ABI
bump.

### 1. Index: four optional `source-*` keys

```toml
[[packs]]
name = "murphy-foo"
gem = "murphy-foo"
version = "0.1.0"
description = "Foo cops"
homepage = "https://example.invalid"
murphy-api-version = 4
min-murphy-version = "0.1.0"
source-url = "https://example.invalid/packs/murphy-foo-0.1.0.tar.gz"
# source-rev = "v0.1.0"      # git sources only (tag/branch/SHA)
# source-subdir = "pack"     # pack root inside checkout/extract
# source-sha256 = "<hex>"    # tarball sources only
```

`source-url` schemes:

| URL shape | Kind | Tool |
|---|---|---|
| `*.tar.gz` / `*.tgz` (`http(s)://`, `file://`, plain path) | tarball | `curl` (remote) + `tar xzf`; SHA-256 checked when `source-sha256` is set |
| `https://` / `http://` / `ssh://` / `git://` / `git@` / `*.git`, or any URL with `source-rev` | git | `git clone` + `checkout --detach <rev>` |
| `file://<dir>` / plain dir (itself or `<dir>/<name>/` a pack dir) | local dir | recursive copy |

A `file://` path that is neither a pack dir nor a `<name>/` parent but
holds `.git/` falls through to `git clone` (local-git mirrors work with
no special scheme).

### 2. Search / fetch / publish flows

- `murphy plugins search [query]` — case-insensitive substring over
  `name`/`description`/`gem` (empty query lists all, sorted by name).
- `murphy plugins fetch <name> [--to DIR]` — compat-gate, fetch to
  `<cache>/<name>/` (`$MURPHY_PLUGIN_CACHE_DIR` else system cache dir) or
  `--to`, then verify manifest + ABI + cdylib.
- `murphy plugins publish [--from DIR]` — validate the pack dir and print
  the `[[packs]]` snippet to append to a static index, plus the
  `tar czf` / upload / `source-sha256` next steps.
- `murphy plugins install <name>` — unchanged local-first order
  (`--from` → gems); when that misses and the registry entry has a
  `source-url`, fetch to the cache and install from there (opt out with
  `--no-remote`; override the cache with `--cache-dir`). `--dry-run`
  reports the fetch without touching the network.

### 3. Static-index hosting (no service)

A marketplace is a git repo / HTTPS dir holding `index.toml` (+ optional
tarballs). Consumers point at it with `--registry <path>` /
`$MURPHY_REGISTRY_PATH`. Publishing is: `tar czf`, upload, append the
`plugins publish` snippet with `source-url` (+ `sha256sum` output as
`source-sha256`), push. Mirrors work the same way.

## Consequences

- `install` gains remote reach with local-first priority preserved;
  `--no-remote` restores exact uk7.1 behaviour.
- No ABI bump (`MURPHY_PLUGIN_ABI_VERSION` stays `4`).
- No service to operate; supply chain stays checksummed-but-unsigned
  (signing remains future scope per the umbrella).
- `git`/`curl`/`tar` become runtime fetch deps with explicit errors when
  missing.

## Alternatives considered

- Hosted registry service (search/publish/auth/versioning): rejected —
  duplicates RubyGems/Bundler per ADR 0049; static files + git/HTTPS
  cover discovery.
- New HTTP/tar Cargo deps (`reqwest`, `flate2/tar`): rejected — system
  `curl`/`tar`/`git` are already on dev/CI hosts and keep the core
  dependency surface unchanged.
- Bundler-owned remote (`gem install` only): rejected for this flow —
  C2 stays the Bundler path; the marketplace serves non-Bundler users
  and version-pinned tarball/git installs.
