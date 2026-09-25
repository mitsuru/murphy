# Profiler (`murphy lint --profile`)

B6 (murphy-fmw.2.6). `--profile` replaces the lint output on stdout with
profiling JSON: per-cop wall time, p95 over cop x file invocations, the
cop x file matrix, and hot-file detection (Phase 9 gate 5).

## CLI

```bash
# Summary JSON (gate shape) to stdout; redirect to keep it.
murphy lint --profile app/ > profile.json

# Speedscope trace for browser flame analysis.
murphy lint --profile --profile-format speedscope app/ > trace.json

# Profile one file.
murphy lint --profile dirty.rb > profile.json
```

Stdout is the profile document — NOT the offense list. The exit code still
reflects lint offenses (`0` clean / `1` offenses / `2` setup / `3`
internal), so `murphy lint --profile ... > profile.json` stays CI-usable:
a red lint still exits 1 even though stdout holds the profile.

## Summary shape (`--profile-format summary`, the default)

```json
{
  "cop_wall_micros": { "Lint/Debugger": 1420 },
  "cop_file_micros": { "Lint/Debugger": { "./app.rb": 1420 } },
  "p95_micros": 1420,
  "hot_files": [{ "file": "./app.rb", "wall_micros": 1420 }],
  "invocation_count": { "Lint/Debugger": 1 }
}
```

| Key | Meaning |
|---|---|
| `cop_wall_micros` | Cop -> total wall time across all files (microseconds). |
| `cop_file_micros` | Cop -> file -> wall time for that pair: the cop x file matrix. |
| `p95_micros` | p95 over recorded cop x file invocation wall times. |
| `hot_files` | Top 5 files by total cop wall time (ties broken by path). |
| `invocation_count` | Cop -> number of recorded cop x file invocations. |

## Cop x file matrix and hot files

Each linted file is dispatched once per cop (outer = cop, inner = matched
node), and each cop's full loop is timed. The matrix answers "which cop is
slow on which file" without re-running: sort `cop_file_micros[cop]` for a
cop's worst files, or `hot_files` for the repo's slowest files overall.

Hot files rank by cop wall time only — parse time is excluded (it feeds the
Speedscope trace but never the wall maps). Timer granularity is
microseconds: a cop x file invocation under 1us records `0` and is skipped
in the maps, so very fast cops on tiny files may be absent from
`cop_wall_micros`. Sums are exact otherwise.

## Speedscope shape (`--profile-format speedscope`)

A Speedscope-compatible `traceEvents` payload: one thread per file
(deterministic ids from sorted file order), `parse` plus per-cop spans
(`native_cop:<Cop>` / `mruby_cop:<Cop>`). Load the file in
[Speedscope](https://www.speedscope.app/) or any trace viewer:

```bash
murphy lint --profile --profile-format speedscope app.rb > /tmp/profile.json
# open /tmp/profile.json in Speedscope
```

`--profile-format` without `--profile` exits 2.

## Flag interactions

- `--format` is accepted but ignored with `--profile`: stdout is always the
  profile JSON. (The offense JSON shape of ADR 0006 is untouched — profiling
  never alters lint output, it replaces the stdout document.)
- `--fix` + `--profile` profiles the post-fix lint pass only; fixpoint
  iterations are not timed.
- `--explain` wins over `--profile` (no lint run to profile).
- `--debug` + `--profile` works: debug lines stay on stderr, stdout remains
  pure profile JSON.
- Baseline filtering still applies to the exit code (frozen offenses do not
  flip the profile run green/red); the profile numbers themselves are
  pre-filter dispatch wall times.
- Profiling bypasses content memoization: identical-content files are linted
  independently so every real file appears in the matrix. Profile runs are
  therefore slower than memoized lint runs — profile a subset when triaging.
- Wall times are measured under parallel lint (files in parallel across
  cores), so numbers include thread scheduling like any real run.

## Phase 6 perf CI relationship

`scripts/perf/phase6_hyperfine.sh` measures end-to-end wall time
(murphy vs rubocop) from outside; `--profile` explains it from inside with
the per-cop breakdown. Use them together: hyperfine says "how slow",
the profile matrix says "which cop x file". Both report microseconds /
wall time, so profile `cop_wall_micros` sums are directly comparable
against hyperfine totals minus parse/discovery overhead.

## Implementation notes

- No `MURPHY_PLUGIN_ABI_VERSION` bump: timing wraps the host dispatch loop;
  `PluginCopV1` is untouched.
- The untimed lint path shares the same inner loop with timing disabled
  (one branch per cop, no `Instant` reads), so profiled and normal runs
  cannot drift in dispatch behavior.
