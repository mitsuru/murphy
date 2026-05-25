# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Project Is

**Murphy** is a from-scratch, high-speed Ruby linter/formatter — "Ruff for Ruby". It is **not** a port of RuboCop and shares no code with `rfmt`. The goal is to eliminate RuboCop's slowness with a native Rust core.

**Status: Phase 6 — v1 standard-cop scope + perf gates (complete).** `crates/murphy-core` + `crates/murphy-cli` build a working `murphy lint` that runs built-in standard cops from ADR 0018 by default across `Murphy`, `Lint`, `Style`, and limited `Layout` namespaces. Directory discovery uses `murphy.toml` `[files] include`/`exclude` plus `.murphyignore`; `.gitignore` is deliberately not honored. User cops are loaded from configured `[cops].path` (default `cops/`, cwd-relative, ADR 0004) and run in-process via embedded mruby with per-cop isolation, wall-clock deadline guarding, and exception isolation. `murphy.toml` is Murphy-owned, not RuboCop-compatible: `[cops.rules."Cop/Name"]` supports `enabled = false` and `severity = "warning" | "error"`; configured cops path is excluded from directory discovery. `murphy migrate <.rubocop.yml>` remains the one-way bootstrap helper. Autocorrect is available via `murphy lint --fix`/`-a`. Default offense JSON and exit-code contracts remain frozen (ADR 0006); ADR 0020 records a passing Phase 6 gate. Config schema is ADR 0015; migration mapping is ADR 0016.

**Security posture (ADR 0004):** v1 ships **no sandbox** for user cops — a `.rb` in the configured cops path is **trusted code** run in-process with full host privileges. Per-cop isolation is fault isolation, not a security boundary.

## Architecture

```text
source -> prism parse once -> shared immutable AST
                         |-> native Rust cops
                         |-> embedded mruby user cops via native primitives
                         -> offense aggregator -> output / autocorrect
```

Load-bearing decisions:

- Core in Rust; standard cops are native and run across all cores.
- User cops stay as `.rb`, run via embedded mruby, and are loaded from configured `[cops].path`.
- One prism parse is shared in-memory; no AST serialization round-trip.
- Offense aggregation is the determinism point and applies severity precedence.
- Config is Murphy-owned; `.rubocop.yml` migration is one-way and lossy.
- Exit codes: `0` clean / `1` offenses / `2` config-or-cop-setup error / `3` internal failure.

## Testing Philosophy

TDD is mandatory for behavior changes: write the failing test before implementing. Autocorrect must remain idempotent. Snapshot and determinism tests protect the JSON contract.

## Build & Test

Rust/Cargo workspace: `crates/murphy-core` (lib) + `crates/murphy-cli` (bin `murphy`). Toolchain is pinned via `rust-toolchain.toml`.

```bash
cargo build                                       # debug build
cargo build --release                             # release build
cargo run -p murphy-cli -- lint <file>...         # lint explicit files
cargo run -p murphy-cli -- lint <dir>             # lint a directory
cargo run -p murphy-cli -- lint                   # discover from cwd
cargo run -p murphy-cli -- lint --fix <file>...   # apply fix blocks
cargo run -p murphy-cli -- lint -a <file>...      # alias for --fix
cargo run -p murphy-cli -- lint --fix --debug ... # print fixpoint debug info
cargo run -p murphy-cli -- lint --fix -- <file>   # -- separates files from flags
cargo run -p murphy-cli -- migrate .rubocop.yml   # one-way migration to stdout
cargo test --workspace                            # full suite
cargo test -p murphy-core <name>                  # single core test
cargo test -p murphy-cli --test cli               # CLI integration target
cargo test -p murphy-cli --test migrate           # migration integration target
cargo +nightly fmt --check                        # formatting gate (nightly for rustfmt.toml `ignore`; CI pins nightly-2026-05-24)
cargo clippy --workspace --all-targets -- -D warnings
```

## Shell Command Safety

`cp`/`mv`/`rm` may be aliased to interactive (`-i`) mode and hang the agent. Always use non-interactive forms: `cp -f`, `mv -f`, `rm -f`, `rm -rf`, `cp -rf`. Also use `ssh`/`scp` with `-o BatchMode=yes`, and `apt-get -y`.

## Worktree Setup

New git worktrees (`.claude/worktrees/*`, `.worktrees/*`) need `mise trust` inside the worktree before tools are visible. mruby3-sys's `build.rs` invokes `make -C mruby` which requires Ruby; without `mise` activation, the script silently emits no `libmruby.a` and later test links fail with `-lmruby` not found.

```bash
mise trust                       # one-time per worktree
eval "$(mise activate bash)"     # per shell — exposes ruby/etc.
cargo clean -p mruby3-sys && cargo build  # if libmruby.a is missing
```

## Test Parallelism

`cargo test` runs lib tests in parallel. A `static AtomicUsize` shared across multiple `#[test]` fns will race — `store(0)` in one test interleaves with `fetch_add` in another. Use per-test static atomics (one tagged per test) when a dispatch-thunk or callback needs to observe call counts.

<!-- BEGIN BEADS INTEGRATION v:1 profile:minimal hash:7510c1e2 -->
## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking. Run `bd prime` to see full workflow context and commands.

### Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work
bd close <id>         # Complete work
```

### Rules

- Use `bd` for ALL task tracking — do NOT use TodoWrite, TaskCreate, or markdown TODO lists
- Run `bd prime` for detailed command reference and session close protocol
- Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files

**Architecture in one line:** issues live in a local Dolt DB; sync uses `refs/dolt/data` on your git remote; `.beads/issues.jsonl` is a passive export.

## Session Completion

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **PUSH TO REMOTE** - This is MANDATORY:

   ```bash
   git pull --rebase
   git push
   git status  # MUST show "up to date with origin"
   ```

5. **Clean up** - Clear stashes, prune remote branches
6. **Verify** - All changes committed AND pushed
7. **Hand off** - Provide context for next session

**CRITICAL RULES:**

- Work is NOT complete until `git push` succeeds
- NEVER stop before pushing - that leaves work stranded locally
- NEVER say "ready to push when you are" - YOU must push
- If push fails, resolve and retry until it succeeds
<!-- END BEADS INTEGRATION -->
