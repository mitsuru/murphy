# ADR 0045 — YAML config syntax evaluation (Rails dev fit)

- Date: 2026-09-25
- Status: Accepted (implemented by murphy-5saj, PR #122)
- Issue: `murphy-ii8`
- Related: ADR 0015 (superseded TOML schema), ADR 0016 (superseded migration mapping),
  ADR 0032/0033/0036 (option schema, syntax-independent)

## Context

`murphy-ii8` asked whether to keep TOML (`murphy.toml`, ADR 0015) or move to
YAML (`.murphy.yml`) for Rails dev fit. This memo records the evaluation;
the decision itself already landed as `murphy-5saj` (PR #122, 2026-05-30).

## Decision

YAML (`.murphy.yml`, RuboCop-compatible). `murphy.toml` is dropped.
Pre-freeze breaking change was allowed and exercised.

## Why YAML won

- Rails config culture is YAML (`.rubocop.yml`, `config/*.yml`, `database.yml`).
  TOML reads as foreign; a pushed `.murphy.toml` invites "what is this?".
- Ruff could ride `pyproject.toml` because Python has a TOML culture.
  Ruby has no equivalent premise.
- A `.rubocop.yml` is now directly usable as `.murphy.yml`
  (`migrate` is a thin normalizer: injects `AllCops.CopsPath`, emits plugin
  rename hints). This is a team-persuasion asset.

## TOML strengths (acknowledged, outweighed)

- Smaller parser, smaller attack surface.
- ADR 0015 used TOML as a "not RuboCop-compatible by design" signal.
- Zero migration cost (already implemented at the time).

## YAML constraints (from murphy-ii8) and status

- Parser: `murphy-ii8` required `saphyr` (`serde_yaml` abandoned).
  Shipped: `yaml-rust2 0.11` (pure-Rust YAML 1.2, replaces `serde_yaml 0.9`).
  `saphyr` is a `yaml-rust2` fork; current choice satisfies the intent.
  Revisit only on maintenance grounds. No change in this memo.
- YAML 1.2 fixed. Done via `yaml-rust2`.
- anchors / aliases / `!tags`: `murphy-ii8` required parser-level reject
  (config error, exit 2; Norway problem / 1.1-vs-1.2 guard).
  Gap: current `yaml_to_json` maps `Yaml::Alias`/`BadValue` to `None`
  (silently dropped), not a hard reject. Follow-up: reject explicitly.
- Filename `.murphy.yml` (Rails dotfile style). Done.
- `murphy.toml` dropped (ABI/config surface pre-freeze). Done.

## Option schema / deprecation (syntax-independent)

Unchanged by this syntax decision; owned by ADR 0032/0033/0036 and
`murphy-9cr.9` (validation gate):

- Cop declares option schema via ABI (`MurphyCopOptionV1`: name/ty/
  default_json/description/enum_values_json/replacement/reason);
  `default_json`/`enum_values_json` are JSON regardless of config syntax.
- unknown key → warn + ignore; deprecated key → warn (+ replacement hint),
  value passes through; type/enum/schema mismatch → warn + default fallback.
- syntax error → exit 2 (config error).
- `--strict-config` (warn → error promotion, CI use): planned, not yet shipped.

## Consequences

- No further TOML/YAML debate; new config work targets `.murphy.yml`.
- Two follow-ups live outside this memo: explicit anchor/alias/tag reject,
  and the `murphy-9cr.9` validation gate + `--strict-config`.
- Pre-`5saj` ADRs mentioning `murphy.toml` (0007, 0017, 0041, 0042, guides)
  stay as historical record; 0015/0016 already carry superseded notes.
