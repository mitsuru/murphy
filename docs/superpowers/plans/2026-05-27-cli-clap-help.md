# CLI Clap Help Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Murphy's ad hoc CLI help and top-level parsing with clap while preserving existing command behavior.

**Architecture:** Add typed clap structs in `crates/murphy-cli/src/main.rs` and route parsed commands into existing execution logic. Expose a typed `cops::list_with_format` entry point so `murphy cops list` help and format parsing are owned by clap. Keep runtime lint, migrate, AST, cops rendering, and LSP behavior unchanged.

**Tech Stack:** Rust 2024, `clap` 4.x, existing `assert_cmd` integration tests.

---

### Task 1: Help Surface Tests

**Files:**
- Modify: `crates/murphy-cli/tests/cli.rs`
- Modify: `crates/murphy-cli/tests/cops_subcommand.rs`

- [ ] **Step 1: Write failing tests for top-level and lint help**

Add tests that run `murphy --help` and `murphy lint --help`, assert exit 0,
and check for stable command/flag names rather than exact clap prose.

- [ ] **Step 2: Write failing test for cops list help**

Add a test that runs `murphy cops list --help`, asserts exit 0, and checks for
`--format`, `table`, and `json`.

- [ ] **Step 3: Run focused tests and verify RED**

Run:

```bash
cargo test -p murphy-cli --test cli top_level_help lists_primary_subcommands
cargo test -p murphy-cli --test cops_subcommand cops_list_help_describes_format
```

Expected: the new help tests fail because `--help` is currently treated as an
unknown subcommand or unknown flag.

### Task 2: Clap Parser and Routing

**Files:**
- Modify: `crates/murphy-cli/Cargo.toml`
- Modify: `crates/murphy-cli/src/main.rs`
- Modify: `crates/murphy-cli/src/cops.rs`

- [ ] **Step 1: Add clap dependency**

Add `clap = { version = "4.6.1", features = ["derive"] }` to
`murphy-cli` dependencies.

- [ ] **Step 2: Add typed clap command model**

Define `Cli`, `Command`, `LintArgs`, `MigrateArgs`, `AstArgs`, `CopsArgs`,
`CopsCommand`, `CopsListArgs`, `LintOutputFormatArg`, `AstFormatArg`, and
`CopsFormatArg`.

- [ ] **Step 3: Route clap errors through `main`**

Use `Cli::try_parse_from(args)` inside the existing `catch_unwind` boundary.
If clap returns help/version, print it and return clap's exit code. If clap
returns invalid usage, print it and return setup error code 2.

- [ ] **Step 4: Move existing lint body behind typed args**

Convert the old lint branch into `run_lint(&LintArgs)` and preserve the
existing flag behavior, path discovery, cache handling, output formatting, and
exit codes.

- [ ] **Step 5: Route migrate, ast, cops, and lsp**

Use typed args for `migrate` and `ast`. Add `cops::list_with_format` and call
it from the clap route. Keep `lsp` passing its trailing args to `lsp::run`.

- [ ] **Step 6: Run focused tests and verify GREEN**

Run:

```bash
cargo test -p murphy-cli --test cli
cargo test -p murphy-cli --test cops_subcommand
cargo test -p murphy-cli --test ast
cargo test -p murphy-cli --test migrate
```

Expected: all focused CLI parser tests pass.

### Task 3: Final Verification

**Files:**
- All changed files

- [ ] **Step 1: Format**

Run:

```bash
cargo fmt
```

- [ ] **Step 2: Full murphy-cli test target**

Run:

```bash
cargo test -p murphy-cli
```

Expected: all `murphy-cli` tests pass.

- [ ] **Step 3: Inspect diff**

Run:

```bash
git diff --stat
git diff -- crates/murphy-cli/Cargo.toml crates/murphy-cli/src/main.rs crates/murphy-cli/src/cops.rs crates/murphy-cli/tests/cli.rs crates/murphy-cli/tests/cops_subcommand.rs
```

Expected: diff is limited to clap parsing, help tests, and typed cops list
routing.
