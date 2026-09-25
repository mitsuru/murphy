# Persistent cache (A5)

Murphy keeps two on-disk caches under `$XDG_CACHE_HOME/murphy/v1`
(`$HOME/.cache/murphy/v1` when `XDG_CACHE_HOME` is unset):

- **AST cache** (ADR 0040): parsed arena bytes, `*.ast`. A hit skips prism
  parse + prism→arena translation.
- **Lint-result cache** (ADR 0047, `murphy-fmw.1.1`): per-file offense
  lists, `results/*.json`. A hit skips parse **and** cop dispatch — this
  is what makes the second consecutive `murphy lint` 5x+ faster on
  dispatch-bound corpora (Phase 8 gate #3).

Both are best-effort: any failure (missing file, corrupt entry, I/O
error) is a silent miss. Output is identical whether the run hits or
misses (ADR 0006 unchanged).

## Key rule: content + cop pack + config

Keys mix `sha256(source)` and the file path with the cop set,
per-cop options, severity, and the `AllCops` context. Changing cops or
config misses by construction — stale results are never returned.
Per-path keys mean identical content at differently-scoped paths never
shares an entry. The result cache is enabled for normal runs (not for
mruby user cops, `--fix` intermediate passes, or `--profile`/`--debug`
timing paths); otherwise the run falls back to the AST cache.

A same-name dynamic `.so` pack rebuilt without a version bump is not
detected — run `murphy cache clean` after swapping pack binaries by
hand (same policy as RuboCop).

## CLI

```sh
murphy lint                    # uses both caches
murphy lint --no-cache         # disables both for this run
MURPHY_NO_CACHE=1 murphy lint  # same, via environment
murphy cache stat              # root, entry counts, total bytes
murphy cache clean             # wipe the whole cache tree
```

## Watch/daemon (B1)

`murphy watch` (murphy-fmw.2.1) is the resident consumer of this cache:
the initial pass warms `results/*.json`, then each poll tick re-lints
only added + modified files. See `docs/guides/watch.md`.
