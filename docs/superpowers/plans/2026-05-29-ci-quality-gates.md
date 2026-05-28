# CI Quality Gates Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split CI into fast pull-request gates and heavier `main`/manual quality gates.

**Architecture:** Keep GitHub Actions configuration in the existing workflow files. Split `.github/workflows/ci.yml` into focused jobs with a final aggregate `check` gate, and restrict `.github/workflows/phase6-perf.yml` to `main`/manual runs.

**Tech Stack:** GitHub Actions, Rust/Cargo, mise, Swatinem/rust-cache, markdownlint-cli2.

---

## Files

- Modify: `.github/workflows/ci.yml`
- Modify: `.github/workflows/phase6-perf.yml`

### Task 1: Split CI Jobs

**Files:**
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Replace the monolithic Rust job with focused jobs**

Set `.github/workflows/ci.yml` to this content:

```yaml
name: ci

on:
  pull_request:
  push:
    branches: [main]
  workflow_dispatch:

permissions:
  contents: read

concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: ${{ github.event_name == 'pull_request' }}

jobs:
  fmt:
    runs-on: ubuntu-latest
    timeout-minutes: 10
    steps:
      - uses: actions/checkout@v4

      - name: Set up toolchain (mise)
        uses: jdx/mise-action@v2

      - name: Install nightly rustfmt
        run: rustup toolchain install nightly-2026-05-24 --profile minimal --component rustfmt

      - name: Cache cargo
        uses: Swatinem/rust-cache@e18b497796c12c097a38f9edb9d0641fb99eee32 # v2.9.1

      - name: cargo fmt
        run: cargo +nightly-2026-05-24 fmt --check

  cargo-check:
    runs-on: ubuntu-latest
    timeout-minutes: 15
    steps:
      - uses: actions/checkout@v4

      - name: Set up toolchain (mise)
        uses: jdx/mise-action@v2

      - name: Cache cargo
        uses: Swatinem/rust-cache@e18b497796c12c097a38f9edb9d0641fb99eee32 # v2.9.1

      - name: cargo check
        run: cargo check --workspace --all-targets

  test:
    name: test (${{ matrix.os }})
    runs-on: ${{ matrix.os }}
    timeout-minutes: 30
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest]
    steps:
      - uses: actions/checkout@v4

      - name: Set up toolchain (mise)
        uses: jdx/mise-action@v2

      - name: Cache cargo
        uses: Swatinem/rust-cache@e18b497796c12c097a38f9edb9d0641fb99eee32 # v2.9.1

      - name: cargo test
        run: cargo test --workspace

  clippy:
    if: github.event_name != 'pull_request'
    runs-on: ubuntu-latest
    timeout-minutes: 30
    steps:
      - uses: actions/checkout@v4

      - name: Set up toolchain (mise)
        uses: jdx/mise-action@v2
      - name: Install clippy component
        run: rustup component add clippy
      - name: Cache cargo
        uses: Swatinem/rust-cache@e18b497796c12c097a38f9edb9d0641fb99eee32 # v2.9.1

      - name: cargo clippy
        run: cargo clippy --workspace --all-targets -- -D warnings

  full-test:
    if: github.event_name != 'pull_request'
    name: full test (${{ matrix.os }})
    runs-on: ${{ matrix.os }}
    timeout-minutes: 30
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest]
    steps:
      - uses: actions/checkout@v4

      - name: Set up toolchain (mise)
        uses: jdx/mise-action@v2
      - name: Cache cargo
        uses: Swatinem/rust-cache@e18b497796c12c097a38f9edb9d0641fb99eee32 # v2.9.1

      - name: cargo test --no-fail-fast
        run: cargo test --workspace --no-fail-fast

  markdownlint:
    runs-on: ubuntu-latest
    timeout-minutes: 5
    steps:
      - uses: actions/checkout@v4
      - uses: DavidAnson/markdownlint-cli2-action@v20

  check:
    if: always()
    runs-on: ubuntu-latest
    needs: [fmt, cargo-check, test, clippy, full-test, markdownlint]
    steps:
      - name: Check required job results
        env:
          NEEDS: ${{ toJson(needs) }}
        run: |
          python3 - <<'PY'
          import json
          import os
          import sys

          needs = json.loads(os.environ["NEEDS"])
          failed = {
              name: job["result"]
              for name, job in needs.items()
              if job["result"] in {"failure", "cancelled"}
          }
          if failed:
              for name, result in failed.items():
                  print(f"{name}: {result}")
              sys.exit(1)
          PY
```

- [ ] **Step 2: Validate the workflow file changed as intended**

Run: `git diff -- .github/workflows/ci.yml`

Expected: the old monolithic Rust `check` job is replaced by `fmt`, `cargo-check`, `test`, `clippy`, and `full-test`, plus a final aggregate `check` job; `clippy` and `full-test` contain `if: github.event_name != 'pull_request'`, and the aggregate `check` job needs all relevant CI jobs.

### Task 2: Restrict Perf Workflow

**Files:**
- Modify: `.github/workflows/phase6-perf.yml`

- [ ] **Step 1: Remove pull request trigger**

Change the top of `.github/workflows/phase6-perf.yml` to:

```yaml
name: phase6-perf

on:
  push:
    branches:
      - main
  workflow_dispatch:
```

- [ ] **Step 2: Validate the perf workflow trigger**

Run: `git diff -- .github/workflows/phase6-perf.yml`

Expected: the `pull_request:` trigger is removed and `push` plus `workflow_dispatch` remain.

### Task 3: Verify Locally

**Files:**
- Modify: none

- [ ] **Step 1: Run formatting gate**

Run: `cargo +nightly-2026-05-24 fmt --check`

Expected: command exits 0.

- [ ] **Step 2: Run check gate**

Run: `cargo check --workspace --all-targets`

Expected: command exits 0.

- [ ] **Step 3: Run Linux-equivalent test gate**

Run: `cargo test --workspace`

Expected: command exits 0.

- [ ] **Step 4: Run full Linux-equivalent test gate**

Run: `cargo test --workspace --no-fail-fast`

Expected: command exits 0.

- [ ] **Step 5: Run clippy gate**

Run: `cargo clippy --workspace --all-targets -- -D warnings`

Expected: command exits 0.

- [ ] **Step 6: Commit**

Run: `git add .github/workflows/ci.yml .github/workflows/phase6-perf.yml docs/superpowers/specs/2026-05-29-ci-quality-gates-design.md docs/superpowers/plans/2026-05-29-ci-quality-gates.md && git commit -m "ci: split quick and full quality gates"`

Expected: commit succeeds.

## Self-Review

- Spec coverage: the plan covers PR quick gates, main/manual full gates, Linux/macOS tests, Windows exclusion, and perf workflow restriction.
- Placeholder scan: no placeholders remain.
- Type consistency: commands and workflow job names are consistent across tasks.
