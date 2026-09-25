# ADR 0052 — Benchmark publishing (C6)

- Date: 2026-09-26
- Status: Accepted
- Issue: `murphy-fmw.3.5` (C6: benchmark publish)
- Parent: `murphy-fmw.3` (Phase 10 epic)
- Depends on: ADR 0020 (Phase 6 gate; hyperfine suite + perf-CI)
- Related: `docs/guides/benchmarks.md`, `docs/guides/profile.md`, `.github/workflows/phase6-perf.yml`

## Context

Phase 10 design §4.3 gate 5 requires benchmark results viewable from
the official docs. Phase 6 (ADR 0020) measures murphy vs RuboCop with
`scripts/perf/phase6_hyperfine.sh` on every `main` push, but the JSON
exports went to a temp dir and were discarded — nothing reached the
docs, so the "Ruff for Ruby" claim had no standing public data.

## Decision

- `phase6_hyperfine.sh` gains `--export-dir <dir>` (any position);
  without it behaviour is unchanged (temp dir, discarded).
- `scripts/perf/render_benchmarks.py` (stdlib only) turns the three
  hyperfine exports (N = 1/20/100) into two published artifacts:
  `docs/benchmarks/results.json` (`schema_version: 1`: timestamp,
  commit, runner, method, per-scale mean/stddev/speedup) and a results
  table spliced into `docs/guides/benchmarks.md` between
  `BENCHMARK-RESULTS` markers (idempotent re-splice, never duplicated).
- `phase6-perf.yml` gains a `publish` job (only on `push` to `main`):
  benchmark → render → upload raw exports as artifacts → force-push the
  refreshed page + snapshot to a rolling `docs/bench-snapshot` branch and
  open/update a snapshot PR. PRs run only the fast checks (`bash -n`,
  renderer unit tests).
- `docs/guides/benchmarks.md` is the canonical page: results table,
  methodology, local repro, snapshot-schema note, and the
  hyperfine-vs-`--profile` split (outside "how slow", inside "which
  cop x file"). README links to it; no numbers are duplicated there.
- Renderer tests live in `scripts/perf/tests/test_render_benchmarks.py`
  (stdlib `unittest`, `python3 <file>` — no pytest dependency), covering
  export parsing, speedup math, markdown rendering, marker splicing,
  and a main() end-to-end pass.
- No ABI bump (no Rust surface touched).

## Consequences

- Gate 5 is met structurally: results are one click from the docs, and
  every `main` push refreshes them. Absolute times are runner-relative
  (`ubuntu-latest`, shared vCPU) — the murphy/RuboCop ratio is the
  portable signal, stated on the page.
- The rolling snapshot PR keeps docs fresh without a Pages deployment.
  `main` is ruleset-protected ("changes via pull request"), so direct
  commit-back is rejected — the PR is the compliant path. PR-triggered
  runs execute only fast checks (`perf-scripts-check`; `perf-regression`
  and `publish` skip on `pull_request`), so there is no retrigger loop.
- Timing noise stays visible: means ship with stddev, raw exports are
  kept as CI artifacts for audit.

## Alternatives considered

- GitHub Pages + chart.js trend graphs: rejected — needs a Pages setup
  and a history store; the checked-in snapshot + artifacts cover v1,
  trend history can layer on `results.json` later.
- Badges/shields with latest speedup in README: rejected — badge
  services need a hosted endpoint; a docs link is honest and sufficient.
- Failing CI on slowdown (hard perf gate): rejected — shared-runner
  noise would flake; Phase 6 deliberately keeps the gate observational
  (ADR 0020).
