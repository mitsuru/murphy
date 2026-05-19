# Murphy Phase 2 — Native Engine Scale-Out Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (or superpowers:subagent-driven-development) to implement this plan task-by-task.
> This re-plans the Phase 2 coarse milestone (`murphy-hgz`) in detail now that the Phase 1 Gate (ADR 0006) has frozen the contract. Phase 3+ stay coarse until their predecessor gate.

**Goal:** Scale the native lint engine from "one file, one cop, single-threaded" to "discover many files, lint them across all cores, deterministically, skipping duplicate work — without renegotiating the Phase 1 frozen contract."

**Architecture:** File discovery (minimal `murphy.toml` include/exclude globs + `.murphyignore`) feeds a rayon-parallel map over the existing `lint_one_file` per-file unit (the AST is immutable and per-file, so file-level parallelism is contention-free); offenses are merged by the existing `aggregate` whose sort key is extended to a *total* order so output stays deterministic regardless of thread interleaving; identical-content files are linted once per run via a `source_digest` memo.

**Tech Stack:** Rust (existing `crates/murphy-core` + `crates/murphy-cli`), `ruby-prism =1.9.0` + `mruby3-sys =3.2.0` (unchanged), **new:** `rayon` (data parallelism), an ignore/glob crate for discovery (the `ignore` crate — ripgrep's — recommended; see Task 6), a hashing crate for `source_digest` (see Task 7).

**Source of truth:** design doc `docs/plans/2026-05-19-murphy-design.md` §3 (all-core parallel) / §5 (source_digest cache key); the frozen contract is `docs/decisions/0006-phase-1-gate-review.md`.

---

## Scope Fences (read before writing any code — these are decided, do not re-litigate)

1. **No second *shipped* cop.** The only shipped native cop stays `NoReceiverPuts`. Multi-cop fan-out, the new aggregator tie-break, and parallel dispatch are exercised by a **`#[cfg(test)]`-only second stub cop** — never compiled into the binary. The native cop *suite* and `murphy-nkq` (name-set scaling, re-export hygiene) remain **Phase 6** (separate brainstorm — do not pre-empt it here).
2. **Config is discovery-only.** Phase 2 introduces a *minimal* `murphy.toml` with **only** `include`/`exclude` glob lists, plus `.murphyignore`. Per-cop enable/disable, severity overrides, cop options, and one-way `.rubocop.yml` migration are **Phase 5** — do not build them.
3. **Cache is in-run memoization only.** Compute `source_digest`; within a single `murphy lint` run, two paths with identical content are linted once and the offenses replayed per path. **No** on-disk persistence, no cache dir, no invalidation strategy, no CLI cache flags — that is a separate post-Phase-2 milestone (Task 8 files the bead).
4. **Parallelism is file-level.** rayon `par_iter` over the discovered file list, calling the existing `lint_one_file`. Cop-level parallelism is deferred until there are many cops (Phase 6+). ADR 0002's Phase-2 forward-flag is realized here: add `Cop: Send + Sync` (the stateless `NoReceiverPuts` unit struct satisfies it automatically).
5. **The Phase 1 frozen contract (ADR 0006) is preserved.** Offense JSON shape, exit codes 0/1/2/3, `SYNTAX_COP_NAME`, byte offsets are unchanged. The aggregator change is an **internal sort tie-break only** — the JSON contract does not change. **Regression guard:** `crates/murphy-cli/tests/integration_snapshot.rs` (the `sample_project.json` snapshot) MUST stay green through every task; if it changes, you broke the contract.

**Phase 3 forward-flag (do not solve here):** Phase 3 mruby-backed cops will also implement `Cop` and run under ADR 0003's per-cop OS-thread+watchdog. The `Cop: Send + Sync` bound added in Task 4 must remain *satisfiable* by a future mruby-backed cop (it moves to a worker thread). Keep the bound minimal (`Send + Sync`, nothing heavier); do not design a bound mruby cops cannot meet. Not fixed here — just don't trap it.

---

## Task Ordering Rationale

`murphy-eu9` tie-break (Task 2) **must precede** the test stub cop (Task 4) — otherwise multi-cop tie ordering cannot be verified. Discovery (Task 6) precedes parallelism payoff but parallelism (Task 5) is wired first against an explicit list so it is testable in isolation. Cache (Task 7) is last before docs because it is an optimization over the now-stable pipeline.

| Task | bead | What |
|---|---|---|
| 1 | `murphy-tdl` | Toolchain pin (`rust-toolchain.toml`) |
| 2 | `murphy-eu9` (Issue 2) | Aggregator total-order tie-break |
| 3 | `murphy-eu9` (Issue 3) | Extra CLI contract guards |
| 4 | ADR 0002 flag | `Cop: Send + Sync` + `#[cfg(test)]` second stub cop |
| 5 | `murphy-hgz` | rayon file-level parallel pipeline |
| 6 | `murphy-hgz` | File discovery (`murphy.toml` + `.murphyignore`) |
| 7 | `murphy-hgz` | `source_digest` in-run memoization |
| 8 | — | Docs + Phase 2 Gate |

---

## Task 1: Toolchain pin (rust-toolchain.toml)

Closes `murphy-tdl`. Infra, no behavior change.

**Files:**
- Create: `rust-toolchain.toml`

**Step 1:** Create `rust-toolchain.toml` aligned with `mise.toml` (Rust 1.95.0):
```toml
[toolchain]
channel = "1.95.0"
components = ["rustfmt", "clippy"]
```

**Step 2:** Verify it does not break the build: `cargo --version` resolves, `cargo test --workspace` still 20 tests green, `cargo fmt --check` exit 0, `cargo clippy --all-targets -- -D warnings` clean.

**Step 3:** Confirm `mise.toml` and `rust-toolchain.toml` agree on `1.95.0` (a mismatch would be worse than neither — assert in the commit message you checked).

**Step 4:** Commit.
```bash
git add rust-toolchain.toml
git commit -m "chore: pin Rust toolchain via rust-toolchain.toml (closes murphy-tdl)"
```

---

## Task 2: Aggregator total-order tie-break

Closes the Issue-2 half of `murphy-eu9`. **Must land before Task 4.**

**Files:**
- Modify: `crates/murphy-core/src/aggregator.rs`
- Test: same file `#[cfg(test)]`

**Context:** Today the sort key is `(file, start_offset)`; ties resolve by stable-sort input order, which becomes cop-registration-order-dependent the moment two cops fire at the same offset in the same file. Extend the key to a **total order**: `(file, start_offset, end_offset, cop_name, message)`. The dedupe key (the 4-tuple `(file, cop_name, range, message)`, severity excluded — design §5/ADR 0006) is **unchanged**. The `severity_only_dup_collapses_to_first_phase1_behavior` test's "first wins" semantics are unchanged.

**Step 1: Write the failing test** — two offenses with identical `(file, start_offset)` but differing `end_offset`/`cop_name`/`message`, supplied in both input orders, must produce the SAME total order out:
```rust
#[test]
fn aggregate_total_order_is_input_independent() {
    let a = off("a.rb", "Murphy/Bbb", 5, 9, Severity::Warning, "msg b");
    let b = off("a.rb", "Murphy/Aaa", 5, 7, Severity::Warning, "msg a");
    // same multiset, opposite input order → identical output
    let o1 = aggregate(vec![a.clone(), b.clone()]);
    let o2 = aggregate(vec![b, a]);
    assert_eq!(o1, o2);
    // total order: (file,start,end,cop,msg) → end 7 before end 9
    assert_eq!(o1[0].range.end_offset, 7);
    assert_eq!(o1[1].range.end_offset, 9);
}
```

**Step 2:** Run: `cargo test -p murphy-core aggregate_total_order_is_input_independent` — Expected: FAIL (current key only `(file,start_offset)`, so input order leaks).

**Step 3:** Extend the sort comparator to `(file, start_offset, end_offset, cop_name, message)`. Keep it `sort_by` (stable, though now the key is total so stability is belt-and-suspenders). Dedupe logic untouched.

**Step 4:** Run the new test + the full aggregator suite + **the Phase 1 snapshot regression guard**:
```bash
cargo test -p murphy-core aggregate
cargo test -p murphy-cli --test integration_snapshot
```
Expected: all PASS, including `integration_snapshot` UNCHANGED (with one cop there are no ties today, so `sample_project.json` must not change — if it does, STOP, you altered the contract).

**Step 5:** Commit.
```bash
git add crates/murphy-core/src/aggregator.rs
git commit -m "feat(core): total-order aggregator tie-break (murphy-eu9 #2; contract preserved)"
```

---

## Task 3: Extra CLI contract guards

Closes the Issue-3 half of `murphy-eu9`. Test-only hardening of the frozen contract.

**Files:**
- Modify: `crates/murphy-cli/tests/cli.rs` (or `tests/integration_snapshot.rs` — keep with the cli behavior tests)

**Step 1: Write three failing/then-passing guards** (they should already pass against current behavior — they pin it so Phase 2's pipeline changes cannot regress it):
- multi-file list where ONE path is missing → exit `2` (pins "an I/O error aborts the run", design §6 / Task 8 behavior).
- clean-only invocation → exit `0` and stdout EXACTLY `[]\n` (pins the empty-array shape).
- an offense-producing run → stderr is empty (stdout-only-JSON / stderr-only-diagnostics machine contract).

**Step 2:** Run: `cargo test -p murphy-cli` — confirm they pass (they pin existing behavior; if any FAILS that is a pre-existing contract bug — STOP and report, do not "fix" by weakening the assertion).

**Step 3:** No production code change expected. If a guard genuinely fails, file a bug bead and surface it — do not silently adjust.

**Step 4:** Commit.
```bash
git add crates/murphy-cli/tests/
git commit -m "test(cli): pin missing-file/clean-only/stderr-empty contract guards (murphy-eu9 #3)"
```

---

## Task 4: `Cop: Send + Sync` + test-only second stub cop

Realizes ADR 0002's Phase-2 forward-flag and provides the multi-cop parallel/tie-break test vehicle. **No shipped cop added** (Scope Fence 1).

**Files:**
- Modify: `crates/murphy-core/src/cop.rs` (trait bound + a `#[cfg(test)]` stub cop)
- Test: `crates/murphy-core/src/cop.rs` `#[cfg(test)]`

**Step 1: Write the failing test** — two *different* stub cops over a multi-call source, asserting deterministic combined output via the Task 2 total order (this both exercises `Send + Sync` usage shape and the tie-break with real cop diversity):
```rust
#[test]
fn two_distinct_cops_dispatch_and_total_order_is_deterministic() {
    // Stub A flags every call node as "Murphy/StubA", Stub B as "Murphy/StubB",
    // both at the call's message_loc range.
    let src = "foo; bar\n";
    let ast = parse(src).unwrap();
    let mut sink = Vec::new();
    let cops: Vec<Box<dyn Cop>> =
        vec![Box::new(StubCopA::default()), Box::new(StubCopB::default())];
    run_cops(&ast, "t.rb", &cops, &mut sink);
    let out = aggregate(sink);
    // 2 call nodes × 2 cops = 4; deterministic by (file,start,end,cop_name,msg)
    assert_eq!(out.len(), 4);
    let names: Vec<&str> = out.iter().map(|o| o.cop_name.as_str()).collect();
    // at each offset StubA precedes StubB by cop_name total order
    assert!(names.windows(2).all(|w| w[0] <= w[1] || true)); // see Step 3 for exact assertion
}
```
(Refine the exact assertion in Step 3 to pin the precise expected `Vec<Offense>` — full-equality is preferred over a weak predicate.)

**Step 2:** Run — Expected: FAIL (`StubCopA`/`StubCopB` undefined; trait may not yet require `Send + Sync`).

**Step 3:** Add `: Send + Sync` to the `Cop` trait (`pub trait Cop: Send + Sync { ... }`). Confirm `NoReceiverPuts` (unit struct, stateless) still implements it with no change (it auto-derives `Send + Sync`). Define `#[cfg(test)] StubCopA`/`StubCopB` (each `#[derive(Default)]`, distinct `name()`), and replace the Step-1 weak predicate with an exact `assert_eq!(out, expected_vec)` that pins the full deterministic sequence.

**Step 4:** Run the cop suite + the snapshot guard:
```bash
cargo test -p murphy-core cop
cargo test -p murphy-cli --test integration_snapshot   # still unchanged
```
Expected: PASS; snapshot unchanged.

**Step 5:** Commit.
```bash
git add crates/murphy-core/src/cop.rs
git commit -m "feat(core): Cop: Send + Sync + test-only dual stub cops (ADR 0002 phase-2 flag)"
```

---

## Task 5: rayon file-level parallel pipeline

The core scale-out. Parallelize the *existing* `lint_one_file` across the file list.

**Files:**
- Modify: `crates/murphy-cli/Cargo.toml` (add `rayon`)
- Modify: `crates/murphy-cli/src/main.rs` (the file loop → `par_iter`)
- Test: `crates/murphy-cli/tests/` (new `parallel_determinism` test)

**Context:** `run()` currently does `for file in files { sink.extend(lint_one_file(f)?) }`. `lint_one_file` parses its own file (immutable AST, no shared mutable state) and returns `Result<Vec<Offense>, AppError>`. This is an embarrassingly parallel map. The `?`-on-`Err` (a missing/unreadable file aborts the run → exit 2, design §6) must be preserved across the parallel boundary.

**Step 1: Write the failing test** — running the binary on the multi-file fixture set repeatedly (and with shuffled arg order) yields BYTE-IDENTICAL stdout every time, and identical to a forced-sequential run:
```rust
// crates/murphy-cli/tests/parallel_determinism.rs
// Run `murphy lint <4 fixtures>` N times in shuffled arg orders;
// assert every stdout is byte-identical to the committed sample_project.json.
```

**Step 2:** Run — Expected: PASS or FAIL depending only on wiring; first ensure it FAILS for the right reason if you stub the parallelism off. (The determinism guarantee comes from `aggregate`'s now-total order — Task 2 — not thread timing.)

**Step 3:** Add `rayon` to `crates/murphy-cli/Cargo.toml` (normal dep). Replace the file loop with a rayon parallel map that preserves the abort-on-first-Err semantics. Recommended shape:
```rust
use rayon::prelude::*;
let results: Result<Vec<Vec<Offense>>, AppError> =
    files.par_iter().map(|f| lint_one_file(f)).collect();
let mut sink: Vec<Offense> = results?.into_iter().flatten().collect();
let offenses = aggregate(sink);
```
`Result<Vec<_>, E>: FromParallelIterator` short-circuits on the first `Err` (deterministic which error? — NO: rayon's first-error is nondeterministic across threads. If exit-code determinism on multi-error input matters, collect all results then pick the error by stable order. For Phase 2, exit code `2` is the same regardless of *which* setup error wins, and stderr is diagnostic-only — document this; only the exit *code* is contract, the chosen message is not). Keep `aggregate` as the single determinism point.

**Step 4:** Run determinism test + full suite + snapshot guard:
```bash
cargo test --workspace
cargo test -p murphy-cli --test integration_snapshot   # unchanged
cargo test -p murphy-cli --test parallel_determinism
```
Expected: all PASS; snapshot byte-identical.

**Step 5:** Commit.
```bash
git add crates/murphy-cli/Cargo.toml crates/murphy-cli/Cargo.lock crates/murphy-cli/src/main.rs crates/murphy-cli/tests/parallel_determinism.rs
git commit -m "feat(cli): rayon file-level parallel lint (determinism via aggregate total order)"
```

---

## Task 6: File discovery (`murphy.toml` + `.murphyignore`)

Discovery-only config (Scope Fence 2). `murphy lint <dir>` / `murphy lint` (no paths) discovers files.

**Files:**
- Create: `crates/murphy-core/src/discovery.rs` (+ wire into `lib.rs`)
- Modify: `crates/murphy-cli/src/main.rs` (accept dir args / zero-arg discovery)
- Modify: `crates/murphy-core/Cargo.toml` (discovery crate)
- Test: `crates/murphy-core/src/discovery.rs` `#[cfg(test)]` + a cli integration test

**Context & crate choice:** recommended `ignore` crate (ripgrep's `ignore`) — it walks directories, honors a custom-named ignore file (`.murphyignore`) with gitignore semantics, and is battle-tested. Alternative: `walkdir` + `globset` + hand-rolled `.murphyignore`. Pick `ignore` unless its dependency surface is objectionable; record the choice + rationale in the discovery module doc (ADR-style one-liner).

**`murphy.toml` schema — EXACTLY this, nothing more (Scope Fence 2):**
```toml
[files]
include = ["**/*.rb"]      # globs; default if absent
exclude = ["vendor/**"]    # globs; applied after include
```
No `[cops]`, no severity, no options. Discovery precedence: explicit CLI file args (current behavior) > directory args (walk them) > zero args (walk cwd). `.murphyignore` (gitignore syntax) and `exclude` globs both prune; `include` selects (default `**/*.rb`).

**Step 1: Write failing tests** (table-driven, in `discovery.rs`):
- given a temp tree + a `murphy.toml` with include/exclude, `discover(root) -> Vec<PathBuf>` returns exactly the expected sorted set.
- a `.murphyignore` line prunes a matching file.
- no `murphy.toml` → default `**/*.rb`, still honors `.murphyignore`.
- explicit file args bypass discovery entirely (unchanged Phase 1 behavior).

**Step 2:** Run — Expected: FAIL (`discover` undefined).

**Step 3:** Implement `pub fn discover(root: &Path) -> Result<Vec<PathBuf>, ConfigError>`: load optional `murphy.toml` (toml crate — likely already transitively available; add `toml` + `serde` derive if needed), build the `ignore` walker with `.murphyignore` as a custom ignore filename, apply include/exclude globs, return a **sorted** Vec (sort here so the file list is deterministic before it even reaches parallel/aggregate — defense in depth). Errors (bad toml, unreadable dir) → a structured `ConfigError` mapping to exit `2` (config-setup error, ADR 0006). Wire `murphy lint` to: file args → as today; dir/zero args → `discover`.

**Step 4:** Run discovery tests + full suite + snapshot guard (the snapshot test passes explicit files, so it must remain unchanged):
```bash
cargo test -p murphy-core discovery
cargo test --workspace
cargo test -p murphy-cli --test integration_snapshot   # unchanged
```

**Step 5:** Commit.
```bash
git add crates/murphy-core/src/discovery.rs crates/murphy-core/src/lib.rs crates/murphy-core/Cargo.toml crates/murphy-core/Cargo.lock crates/murphy-cli/src/main.rs crates/murphy-cli/tests/
git commit -m "feat(core): file discovery — murphy.toml include/exclude + .murphyignore (discovery-only)"
```

---

## Task 7: `source_digest` in-run memoization

Scope Fence 3: in-memory, single-run only. Closes the `murphy-hgz` cache-key item at the agreed depth; files a follow-up bead for persistence.

**Files:**
- Modify: `crates/murphy-core/src/parse.rs` or a new `crates/murphy-core/src/digest.rs` (+ `lib.rs`)
- Modify: `crates/murphy-cli/src/main.rs` (memo before the parallel map)
- Modify: `crates/murphy-core/Cargo.toml` (hash crate)
- Test: cli integration test

**Context:** design §5 keys work on `source_digest`. Phase 2: within ONE run, if two discovered paths have byte-identical content, parse+lint once, then emit the offenses for EACH path (the `file` field differs; offsets are identical because content is). Pure speed/dedup; output MUST be identical to the non-memoized result.

**Crate choice:** `blake3` (fast, no crypto-strength needed — this is a content key) recommended; or std `DefaultHasher` over bytes (zero new dep, weaker but adequate for in-run dedup). Prefer the zero-dep `std::hash` route unless benchmarked insufficient — note the choice.

**Step 1: Write the failing test** — a fixture set containing two files with identical content (e.g. `dup_a.rb` and `dup_b.rb`, both `puts "x"\n`): output contains the offense once per path (2 offenses, differing only in `file`), AND a counter/observable proves `parse` ran once for the duplicated content (e.g. expose a test-only parse counter, or assert via timing-independent instrumentation — prefer a `#[cfg(test)]` hook over timing).

**Step 2:** Run — Expected: FAIL (no memo; parse runs twice — or the counter assertion fails).

**Step 3:** Compute `source_digest(bytes) -> u64/[u8;32]`. Before the parallel map, group discovered paths by digest; parallel-map over UNIQUE contents; fan results back out to every path sharing that digest (rewrite `Offense.file` per path). Keep `aggregate` as the final determinism point. The non-duplicate path must be byte-identical to Task 5's output.

**Step 4:** Run + snapshot guard + the determinism test from Task 5 (still byte-identical) + full suite. The `sample_project.json` fixtures have no dup content → snapshot unchanged.

**Step 5:** File the persistence follow-up and commit.
```bash
# (in the task: bd create the persistent-cache milestone bead, Phase >2)
git add -A
git commit -m "feat(core): source_digest in-run memoization (dup content parsed once; persistence deferred)"
```

---

## Task 8: Docs + Phase 2 Gate

**Files:**
- Modify: `README.md` (directory args, `murphy.toml`, `.murphyignore`, parallel note — keep the honest-status discipline; still no formatter/mruby/autocorrect/persistent-cache claims)
- Modify: `CLAUDE.md` (status → Phase 2 complete; Build & Test unchanged unless commands changed; note discovery + parallel)
- Create: `docs/decisions/0007-phase-2-gate-review.md`

**Step 1:** Update README + CLAUDE.md status. Every documented command/flag VERIFIED by running it (Phase 1 discipline). Do not claim persistent caching (in-run only) or config beyond discovery.

**Step 2:** Run the full gate: `cargo test --workspace` (all green, new count), `cargo fmt --check` (0), `cargo clippy --all-targets -- -D warnings` (clean). End-to-end demo: discover a tree, parallel-lint, deterministic JSON, dup-content memoized; **`integration_snapshot` byte-identical to Phase 1** (the frozen-contract proof).

**Step 3:** Write `0007-phase-2-gate-review.md`: verdict, what was verified, restate that ADR 0006's contract held (snapshot unchanged), list anything deferred (persistent cache bead; `murphy-nkq` re-deferred to Phase 6 since no second cop shipped), and any Phase 3 forward-flags surfaced.

**Step 4:** Commit. Phase 2 complete.
```bash
git add README.md CLAUDE.md docs/decisions/0007-phase-2-gate-review.md
git commit -m "Phase 2 Gate PASSED — native engine scale-out complete (contract preserved)"
```

---

## Phase 2 Gate (exit criteria)

- Discovery (`murphy.toml` include/exclude + `.murphyignore`, dir/zero-arg) works; explicit-file behavior unchanged.
- File-level rayon parallelism; output **deterministic** (total-order `aggregate`) and **byte-identical** to a sequential run and to the Phase 1 `sample_project.json` snapshot — the proof the frozen contract held.
- Identical-content files parsed once per run; output unchanged vs non-memoized.
- `Cop: Send + Sync`; `NoReceiverPuts` unchanged; multi-cop tie-break proven via `#[cfg(test)]` stubs.
- `murphy-tdl`/`murphy-eu9` closed; `murphy-nkq` re-deferred to Phase 6; persistent-cache bead filed.
- 0 clippy warnings, fmt clean, all tests green.

## Deferred out of Phase 2 (tracked, not built)

- **Persistent on-disk `source_digest` cache** (cross-run skip) — separate milestone bead (filed in Task 7).
- **Native cop suite + `murphy-nkq`** (name-set scaling, re-export hygiene) — Phase 6 (separate brainstorm); only relevant once many cops ship.
- **Cop-level parallelism** — Phase 6+ (matters once many cops per file).
- **Per-cop config / severity / `.rubocop.yml` migration** — Phase 5.
- **mruby cop path, ADR 0003 deadlines** — Phase 3 (Phase 2 parallel dispatch is native-only; the `Send + Sync` bound is kept mruby-compatible).
