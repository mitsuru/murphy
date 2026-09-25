# Murphy baseline (`.murphy-baseline.toml`)

The baseline freezes *existing* violations so `murphy lint` reports only
*new* ones. It is the RuboCop `.rubocop_todo.yml` equivalent, in TOML form
consistent with the historical `murphy.toml` (ADR 0015). Use it for legacy
adoption: freeze today's violations, keep the lint green, fix incrementally.

## Legacy adoption (end to end)

```bash
# 1. See what a legacy tree reports today.
murphy lint --format json

# 2. Freeze it. The run still reports all offenses; it also writes the file.
murphy lint --generate-baseline .murphy-baseline.toml

# 3. Check the file in, then lint only new violations from now on.
git add .murphy-baseline.toml
murphy lint --baseline .murphy-baseline.toml   # exit 0: nothing new
```

Fix a frozen violation and the next `--baseline` run stays green (fewer
offenses than the frozen count is fine). Add a *new* violation — a new file,
a new cop, or a further offense of a frozen cop in a frozen file — and only
that offense is reported (exit 1). When you intentionally fix a batch,
regenerate the file and commit the smaller baseline.

## File format

```toml
# Generated header (comments are preserved by hand edits, ignored on load).
version = 1

[[entries]]
file = "app/models/user.rb"
cop = "Style/FrozenStringLiteralComment"
count = 2
```

- `version` must be `1`.
- Each `[[entries]]` suppresses up to `count` offenses of `cop` in `file`.
  The first `count` offenses per `(file, cop)` in aggregate order are treated
  as known; the rest are reported.
- Omit `count` to suppress *all* offenses of that cop in that file, present
  and future (hand-written freeze, RuboCop todo parity).
- Paths are relative to the directory you lint from; a leading `./` is
  ignored, so `./a.rb` and `a.rb` share one entry. Generate and lint from the
  same working directory.
- Duplicate entries for one `(file, cop)` merge: any `count`-less entry wins
  (suppress-all), otherwise counts add.

## Granularity: why file + cop, not line-level

Entries are keyed by **(file, cop ID)** with a count — deliberately *not*
line- or range-level. Line numbers shift on every edit and messages change
across cop versions, so line fingerprints rot within days and regenerate
noise. `(file, cop)` matches RuboCop's todo granularity (per-cop `Exclude:`
file lists) and stays stable while the file is refactored. The count keeps
the useful half of line precision: a *new* offense of an already-frozen cop
in the same file still surfaces once the frozen count is exceeded.

## `.murphyignore` vs baseline: responsibility split

| Question | Answer |
|---|---|
| File should never be linted (vendored, generated)? | `.murphyignore` (gitignore syntax, discovery exclusion) |
| File is real code with violations to fix later? | `.murphy-baseline.toml` (still linted, known offenses filtered) |

`.murphyignore` removes files from discovery — they are never parsed and never
appear in output. The baseline keeps files in the run and filters only the
*frozen* `(file, cop)` offenses from output, so new violations in those files
still surface. Ignore by default only what no cop should ever see.

## CI usage

```yaml
- run: murphy lint --baseline .murphy-baseline.toml --format json
```

The offense JSON shape is unchanged (ADR 0006): the baseline filters *output
only*, so existing JSON consumers keep working. A missing or malformed
baseline file fails the run with exit `2` (setup error), never silently.
