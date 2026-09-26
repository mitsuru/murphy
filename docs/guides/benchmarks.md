# Benchmarks (murphy vs RuboCop)

C6 (murphy-fmw.3.5). Phase 6 perf-CI numbers, published continuously so
the "Ruff for Ruby" claim stays backed by current data. The table below is
refreshed automatically: every push to `main` runs the hyperfine suite
(`.github/workflows/phase6-perf.yml`) and commits the new snapshot back
to this page plus `docs/benchmarks/results.json`.

<!-- BENCHMARK-RESULTS-BEGIN -->

Last updated: 2026-09-26T08:51:24Z (commit `15d7ac168592564adb33d1b0d1e43918c4a4696c`, ubuntu-latest).

| Files (N) | murphy (mean) | RuboCop (mean) | Speedup |
| --- | --- | --- | --- |
| 1 | 14ms (±0ms) | 958ms (±11ms) | 69.7× |
| 20 | 15ms (±1ms) | 1.03s (±8ms) | 68.0× |
| 100 | 19ms (±1ms) | 1.06s (±5ms) | 55.8× |

Means with standard deviation over hyperfine runs (--warmup 2).
Higher speedup is better for murphy. Raw exports are kept as CI
artifacts; docs/benchmarks/results.json holds the machine-readable snapshot.

<!-- BENCHMARK-RESULTS-END -->

## Methodology

- Corpus: `crates/murphy-cli/tests/fixtures/builtin_only_project`
  (3 small Ruby files), replicated to N = 1, 20, 100 project copies.
- Tool: `hyperfine --warmup 2 --ignore-failure`, comparing
  `./target/release/murphy lint <dir>` against `rubocop --format json <dir>`.
- Runner: `ubuntu-latest` GitHub Actions (2 vCPU class, shared) — treat
  absolute times as runner-relative; the murphy/RuboCop **ratio** is the
  portable signal.
- murphy runs from a release build (`cargo build --release`); RuboCop
  runs with default configuration on the same tree.
- murphy's persistent result cache stays warm across hyperfine repetitions
  within one command, so repeat runs mostly measure cache-hit lint while
  RuboCop runs uncached — this matches the shipped default configuration
  (see [cache.md](cache.md)).

## Reproduce locally

```bash
cargo install hyperfine   # once
gem install rubocop       # once
./scripts/perf/phase6_hyperfine.sh --export-dir /tmp/bench
python3 scripts/perf/render_benchmarks.py --input-dir /tmp/bench \
  --out-json docs/benchmarks/results.json \
  --docs-page docs/guides/benchmarks.md
```

Without `--export-dir`, exports stay in a temp dir and are discarded.
The renderer is covered by `python3 scripts/perf/tests/test_render_benchmarks.py`
(stdlib only, no pytest needed).

## Machine-readable snapshot

`docs/benchmarks/results.json` (`schema_version: 1`) holds the latest
snapshot: timestamp, commit, runner, method, and per-scale means,
standard deviations, and speedups. Consume it for badges, release notes,
or trend tracking; the schema version bumps if fields change.

## Relationship to `--profile`

Hyperfine says *how slow* the whole run is from the outside; `murphy lint
--profile` says *which cop x file* is slow from the inside (see
[profile.md](profile.md)). Use them together: this page tracks the
headline ratio over time, the profiler explains a regression.
