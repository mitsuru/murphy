# Gem distribution + Bundler integration (C2)

`gem install murphy` installs the official gem. `bundle exec murphy lint`
lints with cop packs resolved from the bundle's installed gems.

## Install

```sh
gem install murphy --platform x86_64-linux   # or aarch64-linux, x86_64-darwin, arm64-darwin
murphy lint app.rb
```

Platform gems carry one precompiled native binary each
(`libexec/murphy-<platform>`). The `exe/murphy` Ruby wrapper picks the
binary matching your platform and `exec`s it, so signals, stdio, and the
exit code behave exactly like the native binary. Outside Bundler the
wrapper exports `MURPHY_GEM_PATH` from `Gem.path` so pack discovery
works with plain `gem install` too.

From source (dev checkout):

```sh
cargo build --release -p murphy-cli   # target/release/murphy
./exe/murphy lint app.rb               # wrapper falls back to target/
```

## Bundler

```ruby
# Gemfile
source "https://rubygems.org"
gem "murphy"
gem "murphy-rails"   # cop pack as a gem
```

```sh
bundle install
bundle exec murphy lint
```

`Gemfile` owns gem versions (RuboCop parity); `.murphy.yml` owns cop
configuration. `plugins = ["murphy-rails"]` resolves to the bundled
`murphy-rails` gem — no `path:` needed.

## Cop packs as gems

A `murphy-*` gem is a pack when its directory holds either:

1. `murphy-plugin.toml` at the gem root + cdylibs under `lib/<arch>/`
   (ADR 0046 pack root), or
2. `murphy-plugin.toml` under `<gem>/murphy/` (same layout, avoids
   `lib/` clashing with Ruby files), or
3. legacy `lib/lib<sanitized>.{so,dylib}` single file.

Highest installed version wins. A half-installed gem (manifest but no
cdylib) is skipped, never shadowing a working copy.

## Search order

`plugins = ["murphy-rails"]` resolves in this order:

1. Same-array `Detailed { name, path }` pin (explicit always wins).
2. `MURPHY_PLUGIN_PATH` env dirs.
3. Project-local `.murphy/plugins/`.
4. Installed gems (`MURPHY_GEM_PATH`, then `GEM_HOME` / `GEM_PATH` —
   what `bundle exec` sets).
5. User-local `dirs::data_dir()/murphy/plugins/`.

Local staging shadows the released gem during development; the bundled
gem shadows a stale user-global copy. Misses report every probed dir.

Useful overrides:

```sh
MURPHY_GEM_PATH=/opt/gems bundle exec murphy lint   # extra gem roots
MURPHY_PLUGIN_PATH=./vendor/packs murphy lint       # bypass gems
```

## Troubleshooting

- `native binary not found for platform ...`: the generic `ruby` gem was
  installed with no `libexec/` payload. Reinstall with the platform flag
  (`gem install murphy --platform x86_64-linux`) or build from source.
- `plugin pack ... not found in search path`: the gem is not in the
  bundle (`bundle install`), or the gem lacks `murphy-plugin.toml` /
  cdylib in one of the layouts above. Check with
  `bundle info murphy-rails`.
- `plugin pack requires ABI version ...`: rebuild the pack against the
  `murphy-plugin-api` version the host embeds (`MURPHY_PLUGIN_ABI_VERSION`).
- Same-name `.so` rebuilt without a version bump is not detected —
  run `murphy cache clean` (same policy as RuboCop).

## Reference

- ADR 0048 (`docs/decisions/0048-gem-distribution-bundler.md`): design.
- ADR 0046 (`docs/decisions/0046-plugin-pack-format.md`): pack layout.
- `scripts/build-gem.sh`: build one platform gem locally.
- `.github/workflows/release-gem.yml`: 4-platform matrix release.
