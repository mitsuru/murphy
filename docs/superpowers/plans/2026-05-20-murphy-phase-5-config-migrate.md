# Murphy Phase 5 Config + Migrate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Murphy-owned configuration, one-way `.rubocop.yml` migration, configured cop enable/severity controls, and Phase 5 documentation without changing default lint output.

**Architecture:** Keep `murphy-core` as the config/discovery/registry contract owner and `murphy-cli` as orchestration. Add a small config module that loads `murphy.toml`, merges defaults, and exposes concrete runtime decisions to discovery, registry, aggregation, and migration. Preserve the existing offense JSON fields and byte-identical default snapshots.

**Tech Stack:** Rust workspace, `serde`, `toml`, `serde_json`, `ignore`, `globset`, `rayon`, existing `assert_cmd` integration tests.

---

## File Structure

- Modify `crates/murphy-core/src/discovery.rs`: keep file discovery and config error display, extend discovery to accept loaded config and exclude configured cops path.
- Create `crates/murphy-core/src/config.rs`: define `MurphyConfig`, `[files]`, `[cops]`, severity parsing, config loading, and migration output model.
- Modify `crates/murphy-core/src/registry.rs`: accept configured cops path and cop enable rules.
- Modify `crates/murphy-core/src/aggregator.rs`: add a wrapper that applies configured severity overrides before canonical aggregation.
- Modify `crates/murphy-core/src/lib.rs`: export config APIs.
- Modify `crates/murphy-cli/src/main.rs`: load config once, thread it through lint/fix flows, add `murphy migrate`.
- Add/modify CLI tests under `crates/murphy-cli/tests/`: config behavior, migration behavior, and discovery exclusion.
- Add ADRs under `docs/decisions/`: Phase 5 schema, migration mapping, gate review.
- Modify `README.md` and `CLAUDE.md`: document Phase 5 commands and scope.

## Tasks

### Task 1: ConfigError Cleanup (`murphy-3c3.1`)

**Files:**
- Modify: `crates/murphy-core/src/discovery.rs`
- Modify: `crates/murphy-core/src/registry.rs`

- [ ] Write a failing test proving registry cops-dir errors do not say `cannot discover files`.
- [ ] Run `cargo test -p murphy-core registry::tests::cops_is_a_regular_file_yields_config_error_io -- --nocapture` and confirm the old message fails.
- [ ] Change `ConfigError::Io(String)` display to use the supplied message verbatim or add a context enum.
- [ ] Update discovery call sites to include `cannot discover files` in their own message where appropriate.
- [ ] Run `cargo test -p murphy-core discovery::tests registry::tests`.

### Task 2: Murphy Config Schema + ADR (`murphy-3c3.2`)

**Files:**
- Create: `crates/murphy-core/src/config.rs`
- Modify: `crates/murphy-core/src/lib.rs`
- Modify: `crates/murphy-core/src/discovery.rs`
- Add: `docs/decisions/0015-phase-5-config-schema.md`

- [ ] Add tests for parsing default config, `[files]`, `[cops]`, unknown field rejection inside known tables, and severity values `warning`/`error`.
- [ ] Implement `MurphyConfig` with `files.include`, `files.exclude`, `cops.path`, and `cops.rules` keyed by cop name.
- [ ] Preserve default behavior when `murphy.toml` is absent.
- [ ] Write ADR 0015 documenting Murphy-owned schema and non-RuboCop compatibility.
- [ ] Run `cargo test -p murphy-core config::tests discovery::tests`.

### Task 3: Config Loading + Precedence (`murphy-3c3.3`)

**Files:**
- Modify: `crates/murphy-core/src/config.rs`
- Modify: `crates/murphy-core/src/discovery.rs`
- Modify: `crates/murphy-cli/src/main.rs`

- [ ] Add CLI tests proving `murphy lint <dir>` loads `<dir>/murphy.toml` for discovery and zero-arg lint loads `./murphy.toml`.
- [ ] Load config once per discovery root and preserve explicit-file behavior bypassing discovery config.
- [ ] Keep CLI flags `--fix`, `-a`, and `--debug` precedence unchanged.
- [ ] Run `cargo test -p murphy-cli --test cli`.

### Task 4: Wire Cop Enable/Severity (`murphy-3c3.7`)

**Files:**
- Modify: `crates/murphy-core/src/config.rs`
- Modify: `crates/murphy-core/src/registry.rs`
- Modify: `crates/murphy-core/src/aggregator.rs`
- Modify: `crates/murphy-cli/src/main.rs`

- [ ] Add tests where `Murphy/NoReceiverPuts.enabled = false` suppresses native offenses.
- [ ] Add tests where `Murphy/NoReceiverPuts.severity = "error"` changes emitted severity before aggregation.
- [ ] Add tests where an mruby cop can be disabled by its derived host cop name.
- [ ] Implement registry filtering for enabled/disabled cops.
- [ ] Implement severity override before aggregate dedupe so configured severity participates in precedence.
- [ ] Run `cargo test -p murphy-cli --test cli --test mruby_e2e` and `cargo test -p murphy-core aggregator::tests registry::tests`.

### Task 5: Exclude Configured Cops Path From Discovery (`murphy-3c3.8`)

**Files:**
- Modify: `crates/murphy-core/src/discovery.rs`
- Modify: `crates/murphy-cli/tests/cli.rs`

- [ ] Add a failing directory-lint test with `cops/broken.rb` showing only the cop error on target files, not an ordinary lint target offense for `cops/broken.rb`.
- [ ] Exclude configured `cops.path` from directory discovery after include and before output.
- [ ] Preserve explicit-file behavior: `murphy lint cops/foo.rb` still lints that file as an explicit target.
- [ ] Run `cargo test -p murphy-cli --test cli --test mruby_e2e`.

### Task 6: Migrate Mapping ADR (`murphy-3c3.4`)

**Files:**
- Add: `docs/decisions/0016-rubocop-yml-migration-mapping.md`

- [ ] Document one-way migration only.
- [ ] Map RuboCop `AllCops.Include`/`AllCops.Exclude` to `[files] include/exclude`.
- [ ] Map per-cop `Enabled` and `Severity` to `[cops.rules.<name>] enabled/severity`.
- [ ] State unsupported keys are dropped with warnings, not represented as compatibility claims.

### Task 7: `murphy migrate` Implementation (`murphy-3c3.5`)

**Files:**
- Modify: `crates/murphy-core/src/config.rs`
- Modify: `crates/murphy-cli/src/main.rs`
- Add: `crates/murphy-cli/tests/migrate.rs`

- [ ] Add failing tests for `murphy migrate .rubocop.yml` writing Murphy TOML to stdout.
- [ ] Add roundtrip tests: migrate output parses as `MurphyConfig` and lint behavior matches the mapped settings.
- [ ] Add setup-error tests for missing/unreadable/malformed `.rubocop.yml`.
- [ ] Implement hand-rolled `migrate` subcommand without changing `lint` behavior.
- [ ] Run `cargo test -p murphy-cli --test migrate --test cli`.

### Task 8: Docs + Gate (`murphy-3c3.6`)

**Files:**
- Modify: `README.md`
- Modify: `CLAUDE.md`
- Add: `docs/decisions/0017-phase-5-gate-review.md`

- [ ] Document `murphy.toml` Phase 5 schema and `murphy migrate`.
- [ ] Record Phase 5 gate verdict and deferred Phase 6 items.
- [ ] Run `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test`.
- [ ] Close completed beads children and parent with reasons.
- [ ] Commit and push branch plus beads data.

## Self-Review

- Spec coverage: all eight child issues have a task. Config error cleanup, schema, loading, runtime wiring, cops-path exclusion, migration ADR, migration implementation, and docs/gate are covered.
- Placeholder scan: no TBD/TODO/fill-in placeholders remain.
- Type consistency: config names are consistently `MurphyConfig`, `[files]`, `[cops]`, `enabled`, `severity`, and `cops.path`.
