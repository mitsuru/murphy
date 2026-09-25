# `murphy plugins install` (user-local pack install)

`murphy add <pack>` (C1) edits `.murphy.yml` only — Bundler owns download
and install. `murphy plugins install <pack>` is the artifact half for
non-Bundler users: it copies an already-local pack into the user-local
dir (`~/.local/share/murphy/plugins/`, search-path layer 5 of ADR 0042)
and verifies the installed copy.

Design: murphy-uk7.1. No network, no `bundle`/`gem` subprocess, no ABI
bump. Remote download (marketplace) and multi-arch auto-selection are
follow-ups.

## Usage

```sh
murphy plugins install murphy-rails --from ./dist   # local pack dir (or parent of <name>/)
murphy plugins install murphy-rails                 # from installed gems (C2 discovery)
murphy plugins install murphy-rails --dry-run       # resolve + compat only, no copy
murphy plugins install murphy-rails --force         # overwrite an existing install
murphy plugin install murphy-rails --from ./dist    # singular alias
```

What it does:

1. Validates `<name>` (same alphabet as the resolver; traversal rejected).
2. Registry compat gate (same as `murphy add`): when `<name>` is listed in
   `registry/index.toml` (or `--registry` / `$MURPHY_REGISTRY_PATH`),
   `murphy-api-version` must equal the host ABI and the core must be >=
   `min-murphy-version`. Unlisted names fall through to manifest-only
   verification so third-party / local packs stay installable.
3. Finds the source: `--from <dir>` (the dir itself, `<dir>/<name>/`, or
   legacy `<dir>/lib<sanitized>.so`) or installed gems (pack-root walk-up
   for pack-dir gems, direct file for legacy gems).
4. Copies to `<user-dir>/<name>/` (pack dir, recursive) or
   `<user-dir>/lib<sanitized>.so` (legacy). Refuses to clobber without
   `--force`.
5. Verifies: manifest parses, ABI matches, a cdylib resolves and exists.
6. Prints the config next step when `.murphy.yml` lacks the entry
   (`plugins = ["<name>"]` now resolves with no `path:` pin).

Useful overrides:

```sh
murphy plugins install murphy-foo --registry ./mirror.toml   # custom index
MURPHY_USER_PLUGINS_DIR=/tmp/packs murphy plugins install murphy-foo --from ./dist
```

## Troubleshooting

- `pack ... not found in installed gems`: run `bundle install` / `gem
  install <pack>`, or pass `--from <dir>` with a local pack.
- `requires ABI version ...`: rebuild the pack against the host's
  `murphy-plugin-api` version.
- `already installed ... (pass --force)`: the user dir already holds the
  pack; re-run with `--force`.
- `source pack ... holds manifest name ...`: the `--from` dir's manifest
  names a different pack — check the path.
- `not found in ... after install`: the copy lost its manifest/cdylib;
  re-run with `--force` from a clean source.

## Reference

- ADR 0046 (`docs/decisions/0046-plugin-pack-format.md`): pack layout.
- ADR 0049 (`docs/decisions/0049-thin-pack-registry.md`): registry gate.
- ADR 0048 (`docs/decisions/0048-gem-distribution-bundler.md`): gem layer.
- `murphy-core/src/plugin_install.rs`: resolve + copy + verify.
- `murphy-cli/src/plugins.rs`: `plugins install` implementation.
