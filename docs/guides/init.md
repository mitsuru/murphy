# `murphy init` setup UX (C5)

Scaffold `.murphy.yml` + `.murphyignore` in an existing repo so the next
`murphy lint` is fast and sensible. Design: ADR 0051. Presets: `presets.md`.
Hooks: `git-hooks.md`. Baseline: `baseline.md`. Registry: `pack-registry.md`.

## Quick start

```sh
murphy init
murphy lint
```

`init` writes two files (never overwrites without `--force`):

- `.murphy.yml` — commented template with `extends: murphy:recommended`,
`AllCops.Exclude` for `db/schema.rb` / `vendor` / `tmp` (on top of the
bundled defaults that already skip `node_modules`, `tmp`, `vendor`, `.git`),
and per-cop tuning examples.
- `.murphyignore` — gitignore syntax (`db/schema.rb` active; common Rails
paths as commented examples). Only this file prunes discovery besides
`AllCops.Exclude` (`.gitignore` is NOT consulted).

Next steps are printed after every run (lint check, baseline freeze, hook).

## Options

```sh
murphy init --preset minimal          # legacy repo / CI fast path
murphy init --preset murphy:rails-strict  # strict Rails opinions
murphy init --force                   # overwrite existing files
murphy init --hook                    # + lefthook scaffold (B8 templates)
murphy init --hook pre-commit         # + pre-commit scaffold
murphy init --from .rubocop.yml       # migrate instead of fresh template
murphy init --from .rubocop.yml --preset minimal --force
```

- `--preset`: builtin preset for the generated `extends:` (`minimal`,
`recommended` (default), `shopify`, `rails-strict`; `murphy:<name>` or bare).
Unknown names exit 2 and list presets.
- `--hook`: also scaffold a git-hook config (`lefthook` | `pre-commit` |
`overcommit` | `all`; bare `--hook` means `lefthook`). Same bodies as
`murphy install --git-hook`.
- `--from`: migrate a `.rubocop.yml` into `.murphy.yml` (same conversion as
`murphy migrate`; the chosen `extends:` is prepended and migrated cop rules
win over the preset layer). For the migrate-then-tidy flow.
- `--force`: overwrite existing targets. Without it, any existing target
aborts the run (exit 2) before anything is written (atomic, mirrors
`install --tool all`).

## Rails flow

```sh
murphy init --preset rails-strict
murphy add murphy-rails        # `rails-strict` needs the Rails pack to run
murphy lint
murphy lint --generate-baseline .murphy-baseline.toml  # freeze legacy offenses (optional)
murphy lint --baseline .murphy-baseline.toml           # only new offenses
```

## Errors

- Existing file without `--force` → exit 2 (`File ./.murphy.yml already
exists (pass --force to overwrite)`), nothing written.
- Unknown `--preset` → exit 2 (`unknown preset ... (available: minimal, ...)`).
- Unreadable `--from` / invalid generated config → exit 2.

## Reference

- ADR 0051 (`docs/decisions/0051-murphy-init.md`): decision.
- `crates/murphy-cli/src/init.rs`: implementation + unit tests.
- `crates/murphy-cli/tests/init_e2e.rs`: init-then-lint gate tests.
