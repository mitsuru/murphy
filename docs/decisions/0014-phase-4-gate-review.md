# ADR 0014 — Phase 4 Gate review (autocorrect complete; frozen contract preserved)

- Date: 2026-05-20
- Status: Accepted — **GATE PASSED**
- Epic: `murphy-hwe` (Phase 4 — autocorrect)
- Reviews: ADR 0013 (autocorrect contract extension); P4 Tasks 1–8, each implement → roborev-refine (review → fix → re-review until Pass) → close
- Preserves: ADR 0006/0007/0012 (frozen offense-JSON shape / determinism contract)
- Extends: ADR 0013 (deliberate `Offense.autocorrect` contract addition)
- Effect: Phase 5 (`murphy-3c3`, config + `.rubocop.yml` migrate) may start

## Verdict

**PASS.** A cop's `fix` block now actually rewrites source. The autocorrect path
is complete end-to-end:

- **Task .1 (ADR 0013):** `Offense.autocorrect` wire contract pinned — `Edit` /
  `Autocorrect` types, `skip_serializing_if = "Option::is_none"` byte-identity
  guarantee, `#[non_exhaustive]` Rust-surface claim made mechanical.
- **Task .2:** mruby `Murphy::Fix` edit blob marshalled into real `Edit` records
  on `Offense.autocorrect`; `sdk::FixEdit` / `CopRun.fixes` removed; ADR 0009
  field-disjointness now covers only the `ctx ↔ sink` pair.
- **Task .3:** idempotency harness + fixtures written test-first (TDD mandate,
  design §7); `apply_edits` stub added as Phase 4 seam.
- **Task .4:** `apply_edits_logged` implemented with conflict detection (overlapping
  edits logged, one winner kept; descending-offset apply for safety).
- **Task .5:** `run_to_fixpoint` reparse-rerun loop with oscillation detection and
  max-iter cutoff; oscillation semantics (APIN1) recorded in ADR 0013.
- **Task .6:** CLI `murphy lint --fix`/`-a` write-back with atomic, mode-preserving,
  symlink-safe replace; `--debug` observability line; exit codes; `--` separator.
- **Task .7:** `autocorrect_project` fixture + snapshot; idempotency + determinism
  e2e tests; Phase-3-bounded soft-(a) tests inverted to Phase-4 reality.
- **Task .8 (this ADR):** honest docs update to Phase 4 + gate review.

Every task ran implement → roborev-refine (review → fix → re-review until Pass)
→ close. The two contract risks (byte-identity, oscillation safety) were decided
upfront (ADR 0013, APIN1) before any application code.

## End-to-end demo (binary, observed)

All runs below used the `autocorrect_project` fixture
(`crates/murphy-cli/tests/fixtures/autocorrect_project/`) copied to a temp
directory. Cops loaded from the fixture's `cops/` subdirectory (contains
`puts_to_logger.rb` → `Murphy/PutsToLogger`, `delete_pp.rb` → `Murphy/DeletePp`).

**Replace case — `replace_me.rb` (bare `puts` → `logger.info`, fully fixable):**

```console
$ ./target/debug/murphy lint --fix replace_me.rb
murphy: fixed 1 of 1 files
[]
$ echo $?
0
```

File content after fix:

```ruby
# Fixture: replace_me — a bare puts call that PutsToLoggerCop replaces
# with "logger.info". Fully fixable: post-fix no offenses remain → exit 0.
logger.info "hello"
```

**Idempotency — 2nd run on the already-fixed file:**

```console
$ ./target/debug/murphy lint replace_me.rb
[]
$ echo $?
0
```

**Delete case — `delete_me.rb` (`pp` selector removed, fully fixable):**

```console
$ ./target/debug/murphy lint --fix delete_me.rb
murphy: fixed 1 of 1 files
[]
$ echo $?
0
```

**Mixed case — `mixed.rb` (`puts` fixed, `print` offense has no fix → residual):**

```console
$ ./target/debug/murphy lint --fix mixed.rb
murphy: fixed 1 of 1 files
[{"file":"mixed.rb","cop_name":"Murphy/NoReceiverPuts","range":{"start_offset":249,"end_offset":254},"severity":"warning","message":"Use a logger instead of puts"}]
$ echo $?
1
```

**`--debug` observability line (on `replace_me.rb` before fixing):**

```console
$ ./target/debug/murphy lint --fix --debug replace_me.rb
murphy: fixed 1 of 1 files
murphy: debug: replace_me.rb iterations=1 status=Converged conflicts=0 written=true
[]
$ echo $?
0
```

**`-a` short flag (alias for `--fix`):**

```console
$ ./target/debug/murphy lint -a replace_me.rb
murphy: fixed 1 of 1 files
[]
$ echo $?
0
```

**`--` separator — explicitly list files after `--fix`:**

```console
$ ./target/debug/murphy lint --fix -- replace_me.rb delete_me.rb
murphy: fixed 2 of 2 files
[]
$ echo $?
0
```

## Frozen-contract proof (the load-bearing check)

`crates/murphy-cli/tests/snapshots/sample_project.json` has exactly **one**
change commit in its entire history — `8a820ca` (Phase 1 Task 9). `git diff
421e367 HEAD -- …/sample_project.json` (Phase 3 Gate → end of Phase 4, spanning
**all of Phase 4**) is **empty**. The native-only offense JSON / exit codes /
`SYNTAX_COP_NAME` / byte offsets are byte-identical through the entire
autocorrect path.

The reason is exactly what ADR 0013 guaranteed: the sample project has no cops
that emit a fix, so every offense has `autocorrect: None`, so the
`skip_serializing_if = "Option::is_none"` attribute omits the key, so the
serialized output is unchanged. `integration_snapshot` + `parallel_determinism`
pass. ADR 0006/0007/0012 frozen shape holds unchanged through Phase 4.

## ADR consistency + cross-ADR interlocks (record; do not re-derive)

- ADR 0013 pinned the `Offense.autocorrect` wire contract upfront (Task .1) before
  any application code, following the Phase-0 pattern of de-risking the contract
  before implementation.
- ADR 0013 §"Byte-identity guarantee" (Task .1) + the `#[serde(skip_serializing_if
  = "Option::is_none")]` attribute are the mechanical guarantees that byte-identity
  holds. The frozen-contract proof above is the empirical confirmation.
- APIN1 oscillation semantics (Task .5): `FixpointOutcome::corrected` = the
  re-visited state at cycle detection (`next`), NOT the previous round's output.
  Re-feeding this value to `run_to_fixpoint` immediately re-detects the oscillation
  → weakly idempotent. Recorded in ADR 0013 Phase 4 Task 5 update.
- ADR 0009 field-disjointness soundness: after Task .2 removed `sdk::FixEdit` /
  `CopRun.fixes`, the argument now covers only the `ctx ↔ sink` `UnsafeCell` pair.
  The prior `sink`/`fixes` single-writer clause is no longer applicable and was
  removed from the `CopRun` doc (recorded in ADR 0013 Task 2 update).
- ADR 0013's `Offense` is `#[non_exhaustive]`: forbids struct-literal construction
  from outside `murphy-core`, so Phase 5+ may add contract fields without a Rust
  source break for downstream crates.

## Known limitations (carried forward)

- Directory linting discovers `cops/*.rb` as ordinary source files (Phase-2 glob
  `**/*.rb`), so a broken cop's error offense also appears against the cop file.
  Not a frozen-contract violation; tracked for natural resolution in Phase 5
  `[cops]`/discovery work (see Phase-deferred below).
- Per-cop `--debug` timing and deadline observability (APIN4 from Task .6) was
  deferred; only the file-level `--debug` line is emitted today. Tracked below.
- ThreadSanitizer verification of the mruby path remains recommended future CI
  (carried forward from ADR 0012 Carried-forward UNPROVEN). Soundness rests on
  ADR 0009's read-only-immutable-arena reasoning + field-disjointness + spike
  concurrent-stress + the `murphy-cql` late-finish stress guard.

## Phase-deferred (tracked; none weaken the frozen contract)

- **Phase 5 (`murphy-3c3`):** `[cops]` config / per-cop enable/severity-override /
  `.rubocop.yml` one-way migration (`murphy migrate`); structured `ConfigError`;
  `cops/`-self-lint exclusion; document/normalize the `Murphy/<PascalCase(stem)>`
  derived-name contract.
- **Phase 6 (`murphy-7rg`, `murphy-nkq`):** native cop suite breadth; lift
  `lint_source`'s per-call cop vec to a registry; node-message-loc stringly-IDL
  hardening.
- **Phase 6 (deferred from Task .6 APIN4):** per-cop `--debug` timing and
  deadline observability (beyond the current file-level line).
- **Phase 6 (epic note B):** I-2 redundant-parse collapse — the fixpoint loop
  re-parses after each round; collapsing to a single shared parse where no edits
  overlap is a performance free-win deferred from Phase 4.
- **Phase 7 (ADR 0003/0004):** real sandbox for third-party cops;
  instruction-step deadline via a custom mruby build.
- Post-Phase-2 perf: `murphy-fvh` persistent cache, `murphy-3ui` streaming memo,
  `murphy-dfl` fast-abort.

## Final gate metrics

- **`cargo test --workspace`:** 131 passed / 0 failed / 0 ignored
- **`cargo fmt --check`:** clean (exit 0)
- **`cargo clippy --all-targets -- -D warnings`:** clean (exit 0)
- **Frozen snapshot:** `git diff 421e367 HEAD -- crates/murphy-cli/tests/snapshots/sample_project.json` is empty; single-commit file history (`8a820ca`) unchanged
- **`integration_snapshot` + `parallel_determinism`:** pass (included in the 131)
