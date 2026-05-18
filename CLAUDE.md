# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Project Is

**Murphy** is a from-scratch, high-speed Ruby linter/formatter — "Ruff for Ruby". It is **not** a port of RuboCop and shares no code with `rfmt`. The goal is to eliminate RuboCop's slowness (Ruby VM startup + hundreds of cops in Ruby + multi-pass autocorrect reparsing + GVL-bound parallelism) with a native Rust core.

**Status: Phase 1 walking skeleton complete.** `crates/murphy-core` + `crates/murphy-cli` build a working `murphy lint <file>...` (prism parse → `NoReceiverPuts` native cop + `Murphy/Syntax` offenses → aggregated JSON stdout → exit codes 0/1/2/3). The offense-JSON contract, exit codes, and TDD/snapshot harness are **frozen** (ADR 0006); Phase 2+ build on them without renegotiating. The authoritative design — architecture, locked decisions, rejected alternatives with rationale — lives in `docs/plans/2026-05-19-murphy-design.md`. The phased implementation plan is `docs/plans/2026-05-19-murphy-implementation-plan.md`. Resolved Phase-0/gate decisions are ADRs in `docs/decisions/` (read these before Phase 2 / Phase 3 — they carry load-bearing constraints). Spike PoCs under `spikes/` are throwaway and are NOT promoted into `crates/`.

**Security posture (ADR 0004):** v1 ships **no sandbox** for user cops — a `.rb` in `cops/` is **trusted code** run in-process with full host privileges. Per-cop isolation (ADR 0002/0003) is *fault* isolation, not a security boundary. Treat adding a cop like adding a git hook. A real sandbox for third-party cops is a hard Phase 7 prerequisite.

## Architecture (from the design doc)

Single-parse, dual-engine pipeline over one shared immutable AST:

```
source ─▶ prism parse (once) ─▶ shared immutable AST
                                   ├─▶ Native cop engine (standard cops, Rust, all-core parallel)
                                   └─▶ Embedded mruby runtime (user cops, .rb as-is)
                                          ↑ Rust native primitives (traversal / pattern / range)
                                   └─▶ Offense Aggregator ─▶ output / autocorrect
```

Load-bearing decisions (see doc §2 for rationale and rejected options like Spinel/CRuby-embed/Rune):

- **Core in Rust.** Standard cops are reimplemented natively and run across all cores.
- **User cops stay as `.rb`**, run via **in-process embedded mruby** — no daemon, no IPC, no Spinel/CRuby embedding. Authors drop a `.rb` into `cops/`; no build toolchain required of them.
- **"Fast core, scripted glue":** heavy AST work is in Rust native primitives; mruby is a thin visitor layer (`on_<prism_node_type>`). Cops are read-only traversal + text-edit suggestions — **no AST mutation**.
- **One prism parse**, shared in-memory tree exposed to mruby via native handles — **no serialization round-trip**.
- **Isolation is per-cop:** each cop gets an independent mruby state with execution/time deadlines; a crashing or runaway cop degrades to an `error offense` for that cop×file only — everything else continues.
- **Config:** own format + one-way `.rubocop.yml` migration (`murphy migrate`). Not RuboCop-compatible by design.
- Exit codes: `0` clean / `1` offenses / `2` config-or-cop-setup error / `3` internal failure.

## Testing Philosophy (applies once code exists)

TDD is mandatory for cops: write the failing fixture test before implementing. Autocorrect must be **idempotent** — pin the idempotency test (re-running on corrected source yields no change) before writing autocorrect logic. Design doc §7 has the full test-layer matrix: table-driven cop tests, native↔mruby boundary tests, snapshot integration, and hyperfine perf-regression in CI.

## Build & Test

Rust/Cargo workspace: `crates/murphy-core` (lib) + `crates/murphy-cli` (bin `murphy`). Toolchain is mise-pinned (Rust 1.95.0).

```bash
cargo build                                       # debug build (./target/debug/murphy)
cargo build --release                             # release build
cargo run -p murphy-cli -- lint <file>...         # run the linter
cargo test --workspace                            # full suite (Phase 1: 20 tests, all pass)
cargo test -p murphy-core <name>                  # single test, e.g. offense_serializes_to_contract
cargo test -p murphy-cli --test cli               # one integration target (also: --test integration_snapshot)
cargo fmt --check                                 # formatting gate (must be clean)
cargo clippy --all-targets -- -D warnings         # lint gate (must be clean)
```

Exit codes: `0` no offenses / `1` offenses / `2` config-or-cop-or-file-setup error / `3` internal failure.

## Shell Command Safety

`cp`/`mv`/`rm` may be aliased to interactive (`-i`) mode and hang the agent on a y/n prompt. Always use non-interactive forms: `cp -f`, `mv -f`, `rm -f`, `rm -rf`, `cp -rf`. Also `ssh`/`scp` with `-o BatchMode=yes`, `apt-get -y`.

<!-- BEGIN BEADS INTEGRATION v:1 profile:minimal hash:ca08a54f -->
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

## Session Completion

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **PUSH TO REMOTE** - This is MANDATORY:
   ```bash
   git pull --rebase
   bd dolt push
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
