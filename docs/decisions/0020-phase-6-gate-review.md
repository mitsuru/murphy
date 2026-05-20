# ADR 0020 — Phase 6 Gate review (standard-cop + gates)

- Date: 2026-05-20
- Status: Accepted — **GATE PASSED**
- Issue: `murphy-7rg`
- Parent: `murphy-7rg` (Phase 6: v1 standard cop scope + perf-regression CI)
- Gated by: ADR 0017, ADR 0018

## Verdict

**PASS.** Phase 6 is in a usable v1 state with native standard-cop coverage,
diff-quality watch, and perf scripts wired to CI. Remaining work is explicitly
deferred to Phase 7.

## Verification run

Executed as part of the gate review:

- `bash -n scripts/perf/phase6_hyperfine.sh`
- `bash -n scripts/diff/phase6_rubocop_diff.sh`
- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test -p murphy-core`
- `./scripts/perf/phase6_hyperfine.sh`
- `./scripts/diff/phase6_rubocop_diff.sh`

Notable observed outputs:

- Hyperfine benchmark over the phase 6 corpus reported Murphy is significantly faster
  than RuboCop on the sampled workload at N=100 in this environment.
- Diff-quality watch completed and emitted deterministic differences where Murphy
  behavior intentionally differs or is still incomplete.

## Completed scope

- Added local quality/perf artifacts:
  - `scripts/perf/phase6_hyperfine.sh`
  - `scripts/diff/phase6_rubocop_diff.sh`
  - `docs/phase6-diff-watch.md`
  - `crates/murphy-cli/tests/fixtures/phase6_project/mixed.rb`
  - `.github/workflows/phase6-perf.yml`
- Updated `README.md` and `CLAUDE.md` to reflect Phase 6 status and scope.
- `scripts/diff/phase6_rubocop_diff.sh` now intentionally exits zero when
  mismatches are expected, matching watch semantics.
- `murphy` code changes required for this phase remain compatible with `cargo
  clippy` and the existing test suite.

## Deferred / Phase 7 items

- Core formatter (`murphy format`) and broader formatting pipeline.
- Deterministic formatter-like `Layout` expansion beyond v1 subset.
- Third-party/mruby cop sandboxing and richer security boundaries.
- Strict compatibility and CI hard-failure policy for diff-quality gaps; these
  remain manual observation for v1.
- Follow-up cop coverage expansion and implementation refinements discovered by
  ongoing diff reviews.
