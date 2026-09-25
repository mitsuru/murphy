# Plugin marketplace (static index + remote fetch)

Remote pack discovery over the thin registry (C1), without a hosted
service. Design: ADR 0053. Local install base: `plugin-install.md`.
Registry catalogue: `pack-registry.md`.

A marketplace is a static TOML index plus downloadable pack sources
(git repos and/or tarballs on plain HTTPS, or `file://` paths for local
mirrors/tests). `murphy` searches the index, fetches the source with
system `git` / `curl` / `tar`, verifies it (manifest + ABI + cdylib),
and installs it to the user-local dir.

## Search

```sh
murphy plugins search rails              # substring over name/description/gem
murphy plugins search                    # list every pack in the index
murphy plugins search foo --registry ./mirror.toml   # custom index
MURPHY_REGISTRY_PATH=./mirror.toml murphy plugins search foo
```

Entries with a remote source print a `[remote]` marker.

## Fetch

```sh
murphy plugins fetch murphy-foo         # to the fetch cache (<cache>/murphy-foo/)
murphy plugins fetch murphy-foo --to /tmp/murphy-foo   # explicit dest
murphy plugins fetch murphy-foo --registry ./mirror.toml
```

What fetch does:

1. Registry lookup + compat gate (same as `murphy add`: host ABI equality,
   core `>= min-murphy-version`).
2. Requires `source-url` (else it fails with the `--from`/gems hint).
3. Materializes a fresh copy (dest cleared first):
   `*.tar.gz`/`*.tgz` via `curl` + `tar` (SHA-256 checked when
   `source-sha256` is set), git URLs via `git clone` (+ `checkout
   --detach <source-rev>`), `file://` dirs via recursive copy.
4. Resolves the pack root: `source-subdir` when set, else the dest
   itself, `<dest>/<name>/`, or the first one-level-deep pack dir
   (tarballs/repos often wrap the pack in `name-version/`).
5. Verifies manifest + ABI + cdylib before reporting the path.

Cache root: `$MURPHY_PLUGIN_CACHE_DIR` when set, else the system cache
dir (`~/.cache/murphy/plugins/` on Linux).

## Install with remote fallback

```sh
murphy plugins install murphy-foo --registry ./mirror.toml   # gems → remote
murphy plugins install murphy-foo --no-remote                # uk7.1 behaviour
murphy plugins install murphy-foo --cache-dir /tmp/pcache    # cache override
murphy plugins install murphy-foo --dry-run                  # reports the fetch, no network
```

Order: `--from <dir>` / installed gems first (uk7.1, unchanged); only
when those miss and the registry entry has `source-url` does murphy
fetch to the cache and install from there. `--dry-run` prints `would
fetch ...` without downloading.

## Publish to a static index

No account, no upload API — a marketplace is files:

```sh
# 1. Validate the pack and get the index snippet.
murphy plugins publish --from ./dist/murphy-foo
# pack `murphy-foo` 0.1.0 is publishable (verified: .../lib...so)
# --- append to your static index (`registry/index.toml`) ---
# [[packs]]
# ...

# 2. Tarball + checksum.
tar czf murphy-foo-0.1.0.tar.gz -C ./dist/murphy-foo murphy-plugin.toml lib
sha256sum murphy-foo-0.1.0.tar.gz   # → source-sha256

# 3. Upload the tarball (any static HTTPS host), set source-url (+ sha256),
#    append the snippet to index.toml, share the index.
#    Consumers: murphy plugins install murphy-foo --registry https://.../index.toml
#    (or MURPHY_REGISTRY_PATH).
```

Git sources skip the tarball: push the repo (pack root at the repo root
or under `source-subdir`), set `source-url` to the clone URL (+ optional
`source-rev` tag).

Index entry reference (`registry/index.toml`):

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
# source-rev = "v0.1.0"      # git only
# source-subdir = "pack"     # pack root inside checkout/extract
# source-sha256 = "<hex>"    # tarball only
```

## Troubleshooting

- `has no remote source`: the index entry lacks `source-url` — use
  `--from <dir>` / installed gems, or pick an index that publishes one.
- `cannot run \`git\`/\`curl\`/\`tar\``: install the missing tool and retry.
- `checksum mismatch`: the tarball bytes differ from `source-sha256` —
  re-download, or the publisher must update the hash.
- `has no manifest at ... (checked source-subdir)`: the subdir is wrong —
  check the checkout/extract layout.
- `requires ABI version ...` / `needs murphy >= ...`: same compat gate as
  `murphy add` — rebuild the pack / upgrade murphy first.

## Reference

- ADR 0053 (`docs/decisions/0053-plugin-marketplace-static-index.md`).
- ADR 0049 (`docs/decisions/0049-thin-pack-registry.md`): thin registry.
- `murphy-core/src/plugin_marketplace.rs`: search + fetch + publish.
- `murphy-core/src/pack_registry.rs`: index + `source-*` fields.
- `murphy-cli/src/plugins.rs`: `search` / `fetch` / `publish` / install fallback.
