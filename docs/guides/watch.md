# Watch / daemon (`murphy watch`)

B1 (murphy-fmw.2.1). `murphy watch` stays resident and re-lints only the
files you just saved, so save-to-feedback is one short poll after the
write (Phase 9 gate 1). The initial pass lints every discovered file —
warming the A5 on-disk result cache (ADR 0047) — and each later pass
re-lints only added + modified files. Unchanged files are never re-read,
re-parsed, or re-dispatched in the incremental pass.

## CLI

```bash
# Watch the current directory (discovery, like `murphy lint`).
murphy watch

# Watch specific files or dirs.
murphy watch app.rb lib/

# Machine-readable differential output per pass.
murphy watch --format json app.rb

# Poll every 200 ms; clear the screen before each pass.
murphy watch --interval 0.2 --clear app/

# Lint once through the watch pipeline and exit (CI/tests).
murphy watch --once app.rb
```

| Flag | Meaning |
|---|---|
| `--format <name>` | Same shapes as `murphy lint --format` (`human` default; frozen `json` per ADR 0006). Each pass prints only the changed subset. |
| `--no-cache` | Disable both on-disk caches (arena AST + lint results). |
| `--baseline <PATH>` | Suppress frozen offenses; the file is reloaded every pass, and a change triggers a full re-lint. |
| `--interval <secs>` | Poll interval, 0.05–60 s (default 0.5). |
| `--clear` | Clear the screen (ANSI) before each pass. |
| `--once` | Lint once and exit with `0` clean / `1` offenses (uses the watch pipeline; for CI/tests). |

A typical session (stderr trimmed):

```text
$ murphy watch app.rb
…human lint output for app.rb…
murphy watch: initial pass: 1 files, 1 offenses
murphy watch: watching 1 files every 0.50s (Ctrl-C to stop)
# …edit + save app.rb…
…human lint output for app.rb (changed subset only)…
murphy watch: 1 changed (a.rb) — 0 offenses
```

Stop with Ctrl-C. Exit codes in `--once` mode are `0` clean /
`1` offenses / `2` setup / `3` internal — the same contract as
`murphy lint`. A resident run has no lint exit code; it ends on signal.

## Behaviour

- **Differential passes.** After the initial full pass, each tick
  re-discovers the file list (new files are picked up), diffs
  `mtime` + length snapshots, and lints only added + modified files.
  Removals are logged (`N removed`) with nothing to lint.
- **Config tracking.** `.murphy.yml` in the cwd is watched: a change
  reloads the config + registry + caches and triggers a full re-lint.
  The new config fingerprint misses old result-cache keys by
  construction (ADR 0047), so reloaded results are never stale.
  Files pulled in via `inherit_from:` and plugin-pack changes are NOT
  tracked — restart `murphy watch` after those.
- **No fixes.** `murphy watch` never applies autocorrect (a fix
  write-back would immediately re-trigger the watcher). Run
  `murphy lint --fix` separately.
- **Polling, no new dependency.** Snapshots use file `mtime` + length,
  so editors that save via atomic rename are handled the same as
  in-place writes. Sub-100 ms latency is deliberately not promised;
  "feels instant" means one short poll after save.
- **Transient errors don't kill the loop.** A file deleted mid-pass or
  a discovery hiccup is reported on stderr and the loop keeps watching.
  Only `--baseline` load failures exit (they would fail every future
  pass identically, so failing loud matches `murphy lint`).

## Flag interactions

- `--format` accepts every `murphy lint --format` value; the per-pass
  document covers the changed subset while each shape (including the
  ADR 0006 default JSON) is unchanged.
- `watch --once --format json` prints byte-identical output to
  `murphy lint --format json` on the same files (covered by e2e).
- `--no-cache` / `MURPHY_NO_CACHE=1` disable both caches, as with lint.
- With mruby user cops loaded, the result cache stays off (their sources
  live outside the fingerprint) and watch falls back to the AST cache —
  same output, just slower per pass.
- `--baseline` composes with every format (filtering happens before
  formatting, as with lint).

## A5 cache relationship

`murphy watch` is the consumer ADR 0047 unblocked: the initial
`--once` pass populates `results/*.json`, so even a restarted watcher
(or a parallel `murphy lint`) hits the cache for unchanged files. See
`docs/guides/cache.md` for the key rule (content + cop pack + config)
and `murphy cache stat` / `murphy cache clean`.
