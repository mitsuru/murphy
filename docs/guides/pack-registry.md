# Pack registry + `murphy add` (C1)

The official pack registry is a thin static catalogue over RubyGems
distribution (C2). RubyGems/Bundler own download, versioning, and install;
the registry only overlays Murphy-specific metadata (compat + discovery
hints) so `murphy add <pack>` can resolve a name and check compatibility
before touching `.murphy.yml`. Design: ADR 0049. Gem setup: `gem-distribution.md`.

## Add a pack

```sh
murphy add murphy-rails
# added pack `murphy-rails` to `.murphy.yml` (plugins)
# next: add `gem "murphy-rails"` to your Gemfile, then `bundle install` (or `bundle add murphy-rails`)
bundle install
bundle exec murphy lint
```

What `murphy add` does:

1. Looks up `<pack>` in the bundled official index (`registry/index.toml`).
2. Checks compat: `murphy-api-version` must equal the host ABI
   (`MURPHY_PLUGIN_ABI_VERSION`, currently `4`); the running core version
   must be >= `min-murphy-version`.
3. Appends `plugins: - <pack>` to `.murphy.yml` (creates the file when
   missing; idempotent when already present).
4. Prints the Bundler next step. It never runs `bundle`/`gem` and never
   touches the network.

Useful flags:

```sh
murphy add murphy-rails --dry-run            # show what would change
murphy add murphy-rails --registry ./mirror.toml   # custom index
MURPHY_REGISTRY_PATH=./mirror.toml murphy add murphy-rails
```

Unknown names fail with the available list; incompatible packs fail before
any config write (ABI mismatch → rebuild the pack; core too old → upgrade
murphy first).

## Official packs

| Pack | Gem | Description |
|---|---|---|
| `murphy-rails` | `murphy-rails` | Rails cops (RuboCop-rails port) |
| `murphy-rspec` | `murphy-rspec` | RSpec cops |
| `murphy-example-pack` | `murphy-example-pack` | Demo pack (smoke target) |

The index is the catalogue, not the manifest: per-pack cop lists live in
each pack's `murphy-plugin.toml` (`[cops].provided`, ADR 0046).

## Remote sources (uk7.2 marketplace)

Entries may carry an optional static download source for
`murphy plugins fetch/install` (ADR 0053; full guide:
`plugin-marketplace.md`):

```toml
[[packs]]
name = "murphy-foo"
# ... (gem, version, compat as above)
source-url = "https://example.invalid/packs/murphy-foo-0.1.0.tar.gz"
# source-rev = "v0.1.0"      # git only
# source-subdir = "pack"     # pack root inside checkout/extract
# source-sha256 = "<hex>"    # tarball only
```

`murphy add` ignores these keys (config half); `plugins search` lists the
index, `plugins fetch` materializes a source, and `plugins install`
falls back to it when local gems / `--from` miss.

## Search order reminder

`plugins = ["murphy-rails"]` resolves (ADR 0042, ADR 0048):

1. Same-array `Detailed { name, path }` pin.
2. `MURPHY_PLUGIN_PATH` env dirs.
3. Project-local `.murphy/plugins/`.
4. Installed gems (`MURPHY_GEM_PATH`, then `GEM_HOME`/`GEM_PATH`).
5. User-local `dirs::data_dir()/murphy/plugins/`.

## Troubleshooting

- `unknown pack ... (available: ...)`: typo, or the pack is not official —
  use a `Detailed { name, path }` entry or a custom `--registry` mirror.
- `requires ABI version ...`: rebuild the pack against the host's
  `murphy-plugin-api` version.
- `needs murphy >= ...`: upgrade murphy first (`gem update murphy` /
  `cargo update`), then re-run `murphy add`.
- `murphy add` updated `.murphy.yml` but lint says `not found in search
  path`: the gem is not installed — run `bundle install` and lint under
  `bundle exec` (see `gem-distribution.md`).

## Reference

- ADR 0049 (`docs/decisions/0049-thin-pack-registry.md`): decision.
- ADR 0053 (`docs/decisions/0053-plugin-marketplace-static-index.md`): remote sources.
- ADR 0048 (`docs/decisions/0048-gem-distribution-bundler.md`): gem layer.
- `registry/index.toml`: official index (edit + test to register a pack).
- `murphy-core/src/pack_registry.rs`: index parser + compat + config edit.
- `murphy-cli/src/add.rs`: `murphy add` implementation.
