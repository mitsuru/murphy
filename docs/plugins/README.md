# Writing a Murphy plugin

Third-party cops ship as **native plugin packs**: a Rust `cdylib` crate plus
a `murphy-plugin.toml` manifest, loaded at runtime via `dlopen`. This page is
the pack-author entry point — scaffolding, loading, and distribution. Cop
authorship (`#[cop]`, `#[on_node]`, options, testing) lives in
[`docs/guides/plugin-cop-infrastructure.md`](../guides/plugin-cop-infrastructure.md);
the manifest schema and directory convention live in
[ADR 0046](../decisions/0046-plugin-pack-format.md).

## Scaffold with `cargo generate`

```sh
cargo install cargo-generate
cargo generate --git https://github.com/murphy-rs/plugin-template --name murphy-foo
```

The template (mirrored in-repo at [`template/`](template/) — that directory *is*
the future standalone repo root, so splitting it out is a plain copy) contains:

| File | Role |
|---|---|
| `Cargo.toml.liquid` | `cdylib` crate on the single-surface API (`murphy-plugin-api` only) |
| `murphy-plugin.toml.liquid` | Pack manifest: identity, `murphy-api-version`, `[cops].provided` |
| `src/lib.rs.liquid` | Minimum viable cop (`Example/NoTodo`, one `send` match) + `register_cops!` |
| `tests/smoke.rs.liquid` | Harness test + `PACK_COPS`-vs-manifest registration guard |
| `README.md.liquid` | Generated-pack readme (build / load / delete-the-demo) |
| `LICENSE` | MIT (update the copyright holder line) |
| `cargo-generate.toml` | Built-in placeholders only (`project-name`, `crate_name`, `authors`) |
| `.github/workflows/ci.yml.liquid` | `cargo test` + `clippy -D warnings` + `fmt --check` |

The bundled `Example/NoTodo` cop is a smoke target, not a starting point:
delete it (code, manifest `provided` entry, and smoke assertions together)
when the real cops land.

Until `murphy-plugin-api` is published to crates.io, generated packs point
its `Cargo.toml` entries at a git/path dependency on the Murphy repo's
`crates/murphy-plugin-api` (the template's `Cargo.toml` comment shows how).

## Build, test, load

```sh
cargo test                              # smoke: demo cop + registration guard
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo build                             # target/debug/libmurphy_foo.so
```

Stage the pack directory form (the distributable unit per ADR 0046) and
reference it from `.murphy.yml`:

```yaml
plugins:
  - name: murphy-foo
    path: /tmp/murphy-packs/murphy-foo   # dir holding murphy-plugin.toml + lib/
```

`murphy-api-version` in the manifest must equal the host's
`MURPHY_PLUGIN_ABI_VERSION` (currently `4`); a mismatch is rejected before
`dlopen`. Rebuilt packs staged under a project's `.murphy/plugins/` are
refreshed with `murphy plugins sync --from target/debug` (see the
infrastructure guide §10).

## Distribution

An installed pack is a directory: `murphy-plugin.toml` at the root,
per-arch binaries under `lib/<arch>/` (`linux-x86_64`, `darwin-arm64`, …).
Multi-arch matrix releases are a future option — the template's CI builds
and tests the host arch only; the `lib/<arch>/` slots are where cross-built
binaries go. There is no marketplace yet.

Install a pack to the user-local dir (`~/.local/share/murphy/plugins/`,
search-path layer 5) with `murphy plugins install` (`murphy plugin
install` is an alias). It resolves `<name>` via the thin registry (compat
check, same gate as `murphy add`) plus installed gems, copies the pack,
and verifies the installed copy (manifest + ABI + cdylib):

```sh
murphy plugins install murphy-rails --from ./dist   # local pack dir
murphy plugins install murphy-rails                 # from installed gems
murphy plugins install murphy-rails --dry-run       # no copy
murphy plugins install murphy-rails --force         # overwrite
```

After install, `plugins = ["murphy-rails"]` resolves with no `path:` pin.
`murphy add murphy-rails` remains the `.murphy.yml` config half; `plugins
install` is the artifact half for non-Bundler users (Bundler users resolve
gems directly, no copy needed). Remote download (marketplace) and
multi-arch auto-selection are follow-ups.

Packs also ship as Ruby gems (C2; ADR 0048): a `murphy-*` gem holding
`murphy-plugin.toml` (+ `lib/<arch>/` cdylibs) at its root, under
`<gem>/murphy/`, or as a legacy `lib/lib<name>.so` is auto-discovered
from `plugins = ["murphy-foo"]` under `bundle exec` (highest installed
version wins). The `murphy` binary itself installs via
`gem install murphy --platform <tag>`. Full guide:
`docs/guides/gem-distribution.md`.

Official packs are catalogued in the thin registry (C1; ADR 0049) and
installed into `.murphy.yml` with `murphy add murphy-rails` (guide:
`docs/guides/pack-registry.md`).

Official config presets (`minimal` / `recommended` / `shopify` /
`rails-strict`) are builtin layers selected via `extends: murphy:<name>` or
`--preset` (C3; ADR 0050, guide: `docs/guides/presets.md`).

## Future: `murphy plugin new`

A `murphy plugin new murphy-foo` subcommand — a thin wrapper over
`cargo generate murphy-rs/plugin-template` that also wires the generated
pack into the current project's `.murphy.yml` — is **design-only** in this
task (murphy-1f7) and explicitly not implemented. It stays a follow-up until
the template repo exists standalone and `murphy-plugin-api` is published,
at which point the wrapper's UX (`--name`, `--path`, `--no-config-wiring`)
can be specified against real usage.
