# CI Quality Gates Design

## Goal

Improve GitHub Actions so pull requests stay fast while `main` and manual runs keep stronger quality coverage.

## Current State

The existing `.github/workflows/ci.yml` has one Linux Rust job that runs nightly `rustfmt`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace --no-fail-fast`. It also has a separate markdownlint job. `.github/workflows/phase6-perf.yml` runs performance comparison work on pull requests, `main`, and manual dispatch.

## Target Behavior

Pull requests should aim for roughly 2-3 minutes. They must run cheap, high-signal checks:

- `cargo +nightly-2026-05-24 fmt --check` on Linux.
- `cargo check --workspace --all-targets` on Linux.
- `cargo test --workspace` on Linux and macOS.
- markdownlint.

Full checks should run on `main` pushes and manual dispatch:

- `cargo clippy --workspace --all-targets -- -D warnings` on Linux.
- `cargo test --workspace --no-fail-fast` on Linux and macOS.
- The existing formatting and markdown gates.

Windows is intentionally out of scope for this change. It can be added later if runtime and dependency setup are acceptable.

## Workflow Shape

Keep a single `ci.yml` workflow and split work into smaller jobs:

- `fmt`: Linux-only, runs on pull requests, `main`, and manual dispatch.
- `cargo-check`: Linux-only, runs on pull requests, `main`, and manual dispatch.
- `test`: Linux/macOS matrix, runs on pull requests, `main`, and manual dispatch.
- `clippy`: Linux-only, runs only on `main` and manual dispatch.
- `full-test`: Linux/macOS matrix, runs only on `main` and manual dispatch.
- `markdownlint`: unchanged trigger behavior.

Keep a final aggregate `check` job as the required branch-protection gate. It depends on `fmt`, `cargo-check`, `test`, `clippy`, `full-test`, and `markdownlint`, runs with `if: always()`, and fails only when a needed job is `failure` or `cancelled`. `success` and `skipped` are acceptable so pull requests can intentionally skip the heavier `clippy` and `full-test` jobs.

Use the same Rust setup pattern as the current workflow: checkout, `jdx/mise-action@v2`, Rust components, pinned nightly rustfmt where needed, and `Swatinem/rust-cache`.

## Perf Workflow

The perf workflow is heavier than the desired PR feedback loop because it installs RuboCop and hyperfine and builds release binaries. Move it off pull requests and keep it on `main` and manual dispatch.

## Acceptance Criteria

- Pull request CI includes Linux `fmt`, Linux `cargo check --workspace --all-targets`, Linux/macOS `cargo test --workspace`, and markdownlint.
- The required `check` status remains an aggregate gate covering all relevant CI jobs.
- `clippy -D warnings` no longer runs on every pull request.
- `clippy -D warnings` still runs on `main` pushes and manual dispatch.
- `cargo test --workspace --no-fail-fast` runs on Linux and macOS for `main` pushes and manual dispatch.
- `phase6-perf` no longer runs on pull requests.
- Existing Actions versions and pinned nightly rustfmt date remain unchanged.
