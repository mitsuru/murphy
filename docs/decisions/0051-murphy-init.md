# ADR 0051 — `murphy init` setup UX (C5)

- Date: 2026-09-26
- Status: Accepted
- Issue: `murphy-fmw.3.4` (C5: murphy init / セットアップ UX)
- Parent: `murphy-fmw.3` (Phase 10 epic)
- Depends on: ADR 0045 (YAML config), ADR 0049 (thin registry), ADR 0050 (builtin presets)
- Related: B8 git-hook scaffolds (`docs/guides/git-hooks.md`), baseline (`docs/guides/baseline.md`), `murphy migrate`

## Context

Phase 10 design §4.3 gates C5 on: `murphy init` in an existing repo, then
`murphy lint` returns a sensible result in under 5s (existing Rails app
premise). The May roadmap text names `murphy.toml` + `.murphyignore`; ADR
0045 has since dropped TOML in favour of RuboCop-compatible YAML
(`.murphy.yml`). B8 (`murphy install --git-hook`), C1 (`murphy add`), C3
(`extends:` / `--preset`), and `murphy migrate` already exist — `init` must
compose with them, not duplicate them.

## Decision

`murphy init [--preset <name>] [--force] [--hook [TOOL]] [--from <.rubocop.yml>]`:

- Writes `.murphy.yml` (fresh commented template, `extends:
murphy:<preset>`, default `recommended`) + `.murphyignore` (gitignore
syntax, `db/schema.rb` active, common Rails paths as commented examples).
`murphy.toml` is NOT generated (ADR 0045).
- `--preset` reuses the C3 builtin catalogue (ADR 0050; `murphy:<name>` or
bare `<name>`); unknown names exit 2 listing presets. The template enables
nothing extra, so post-init lint stays fast (gate 4 is a lint property the
template preserves via `Exclude` / `.murphyignore` narrowing only).
- `--from <path>` migrates a `.rubocop.yml` (same
`migrate_rubocop_yml_to_murphy_yml` as `murphy migrate`) and prepends the
chosen `extends:` (migrated cop rules always win — presets resolve below
user rules). Covers the `migrate`-then-tidy flow from the C5 issue.
- `--hook [TOOL]` reuses the B8 scaffold bodies (`lefthook` default for bare
`--hook`); without it no hook files are touched.
- Baseline files are NOT generated: `murphy lint --generate-baseline` owns
that flow; `init` prints the hint so `init` itself stays instant.
- Atomic no-clobber: without `--force`, any existing target (config, ignore,
or hook scaffolds) aborts the whole run with exit 2 before anything is
written (mirrors `install --tool all`). Generated config is validated
(`MurphyConfig::from_yaml_str`) before writing.
- No ABI bump (`MURPHY_PLUGIN_ABI_VERSION` untouched; CLI-only scaffolding).

## Consequences

- Phase 10 gate 4 is met by construction: `init` is instant (no lint, no
network) and the template keeps discovery narrow; e2e asserts
init-then-lint returns valid JSON (exit 0 clean / 1 offenses, never 2).
- `murphy migrate` stays the stdout converter; `init --from` is the writing
variant. Both share one migration function (no drift).
- Hook ownership stays with B8 (`install --git-hook`); `init --hook` is a
thin alias over the same bodies.

## Alternatives considered

- Generating `murphy.toml`: rejected — ADR 0045 dropped TOML; the linter
reads `.murphy.yml`.
- Auto-generating a baseline during `init`: rejected — requires a full lint
run (slow on large repos, against the instant-setup goal); the hint suffices.
- Auto-detecting Rails and picking `rails-strict`: rejected — magic preset
selection surprises; `--preset rails-strict` + `murphy add murphy-rails` is
explicit and documented.
- Interactive prompts: rejected — non-interactive CLI (flags only), consistent
with `install` / `add`.
