# Murphy Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> Scope of this document: **Phase 0 (de-risking spikes) and Phase 1 (walking skeleton) are detailed.**
> Phase 2+ are coarse milestones only — they will be re-planned in detail *after* Phase 0 decisions land,
> because §8 of the design doc (`docs/plans/2026-05-19-murphy-design.md`) has open questions whose answers
> determine the native-primitive IDL and the prism/mruby bridge shape. Writing bite-sized TDD steps for
> Phase 2+ now would be fabricated detail built on an unverified PoC.

**Goal:** Build a from-scratch, high-speed Ruby linter/formatter ("Ruff for Ruby") with a Rust native core and an embedded-mruby path for user cops.

**Architecture:** Single prism parse → one shared immutable AST → two consumers: a native Rust cop engine (standard cops, all-core parallel) and an embedded mruby runtime (user `.rb` cops via Rust native primitives, no serialization). An offense aggregator merges, dedupes, and drives output / autocorrect.

**Tech Stack:** Rust (core, CLI, native cops), prism (parser, Rust binding TBD in Phase 0), embedded mruby (user cop runtime), serde/JSON (offense contract).

**Source of truth for design:** `docs/plans/2026-05-19-murphy-design.md`. This plan implements it; it does not re-decide it.

---

## Phasing Overview

| Phase | Content | Detail level |
|---|---|---|
| **0** | De-risking spikes (prism binding, mruby bridge, deadlines, trust model) | Detailed — but **spikes, not TDD**: deliverable is an ADR + minimal PoC |
| **1** | Walking skeleton: `murphy lint <file>` → 1 native cop → JSON offense → exit code | Detailed — bite-sized TDD |
| 2 | Native cop engine scale-out (multi-core, file discovery, cache key) | Coarse milestone |
| 3 | mruby cop path (SDK, native primitives IDL, isolation, deadlines) | Coarse milestone |
| 4 | Autocorrect (conflict-safe apply, reparse loop, idempotency) | Coarse milestone |
| 5 | Config system + one-way `.rubocop.yml` `murphy migrate` | Coarse milestone |
| 6 | v1 standard cop scope (separate brainstorm) + perf-regression CI | Coarse milestone |
| 7 | Future (§8): third-party cop sandbox, LSP, alternate cop frontends | Out of plan scope |

**Gate rule:** Phase 1 detailed tasks below assume Phase 0 ADRs exist. If a spike's outcome contradicts a Task's sketched code, the Task code is wrong, not the ADR — re-derive from the ADR.

---

## Phase 0 — De-risking Spikes

> These are **time-boxed exploration**, not TDD. "Write a failing test → implement" does not apply to
> "which prism binding should we use". Each spike's deliverable is: (a) a short ADR committed under
> `docs/decisions/`, and (b) a minimal throwaway PoC under `spikes/` proving the decision is viable.
> Box each spike to ~1 focused work-block; if it overruns, the ADR records "unresolved + next step".

### Spike 0.1 — prism Rust binding selection

**Deliverable:**
- `docs/decisions/0001-prism-binding.md` — chosen approach + rejected options + why.
- `spikes/prism_poc/` — parses a `.rb` string to an AST and walks nodes (visits every call node, prints `name` + byte range) from Rust.

**Investigate:** the `prism` Rust crate (ruby/prism official bindings) vs raw FFI to `libprism` vs vendoring prism C and generating bindings. Criteria: node coverage, byte-offset/location fidelity (we key offenses on `{start_offset,end_offset}`), zero-copy access to source, build burden on **us** (never on cop authors).

**Done when:** PoC prints correct byte ranges for a hand-checked snippet, and the ADR names the binding Phase 1 will depend on.

### Spike 0.2 — embedded mruby + native AST handle PoC

**Deliverable:**
- `docs/decisions/0002-mruby-bridge.md` — how a shared in-memory AST node is exposed to mruby as an opaque native handle with **no serialization round-trip**.
- `spikes/mruby_poc/` — Rust embeds mruby; mruby script receives a node handle and calls one native primitive (e.g. `node.name`, `node.receiver_nil?`) backed by the Spike 0.1 AST.

**Investigate:** mruby crate/embedding choice, lifetime/ownership of the AST relative to the mruby VM (AST is immutable and outlives the cop run), how a Rust function is registered as an mruby method on a handle type.

**Done when:** an mruby `.rb` snippet walks at least one node of a real parsed file via native calls only. This PoC's primitive surface seeds the **native-primitive IDL** (design §8) used in Phase 3.

### Spike 0.3 — runaway-cop deadline mechanism

**Deliverable:**
- `docs/decisions/0003-cop-deadlines.md` — can a runaway user cop be interrupted via an mruby instruction hook and/or wall-clock deadline? Concrete mechanism or documented limitation.
- `spikes/deadline_poc/` — a deliberately infinite-loop mruby cop is forcibly interrupted; the host survives and continues.

**Done when:** the PoC proves interruption works (or the ADR records the fallback: e.g. OS-thread + watchdog) — design §6 depends on this for per-cop isolation.

### Spike 0.4 — v1 trust & security posture (decision only)

**Deliverable:** `docs/decisions/0004-trust-model.md` — restates design §2's "no sandbox in v1, trusted self-placed `.rb` only", and **explicitly** notes the residual risk: a third-party/OSS-dependency cop pulled into CI runs unsandboxed. Records this as accepted-for-v1 with a pointer to the Phase 7 sandbox item. No code.

**Done when:** the decision is written and linked from `CLAUDE.md` "What This Project Is".

### Phase 0 Gate

Review all four ADRs together before starting Phase 1. Confirm: the Phase 1 parse adapter (Task 3) can be written against the Spike 0.1 binding, and the Phase 3 IDL has a seed from Spike 0.2.

---

## Phase 1 — Walking Skeleton

**Vertical slice:** `murphy lint <file.rb>` → read file → prism parse once → run **one** native cop (`puts`/`print`/`p` with no receiver) → emit structured offense(s) as JSON → exit `0` (clean) / `1` (offenses) / `2` (cop/config setup error) / `3` (internal).

**Explicitly NOT in Phase 1:** mruby path, autocorrect, config files, multi-core parallelism, `.murphyignore`, caching. Those are Phase 2+.

> Code blocks below are **representative skeletons**. Exact prism API calls depend on Spike 0.1's ADR;
> substitute the real binding's API. The *contract* (offense JSON shape, exit codes, TDD order) is fixed.

### Task 1: Cargo workspace scaffold

**Files:**
- Create: `Cargo.toml` (workspace), `crates/murphy-core/Cargo.toml`, `crates/murphy-core/src/lib.rs`, `crates/murphy-cli/Cargo.toml`, `crates/murphy-cli/src/main.rs`

**Step 1:** Create the workspace with two crates: `murphy-core` (lib) and `murphy-cli` (bin, depends on core).

**Step 2:** Add a trivial `pub fn version() -> &'static str` in core and a smoke test.

**Step 3:** Run: `cargo test` — Expected: PASS (smoke test green, workspace builds).

**Step 4:** Commit.
```bash
git add Cargo.toml crates/
git commit -m "chore: scaffold cargo workspace (core + cli)"
```

### Task 2: Offense contract type

**Files:**
- Create: `crates/murphy-core/src/offense.rs`
- Test: same file `#[cfg(test)]`

**Step 1: Write the failing test** — assert a serialized `Offense` matches the design §5 contract exactly:
```rust
#[test]
fn offense_serializes_to_contract() {
    let o = Offense {
        file: "a.rb".into(),
        cop_name: "Murphy/NoReceiverPuts".into(),
        range: Range { start_offset: 0, end_offset: 4 },
        severity: Severity::Warning,
        message: "Use a logger instead of puts".into(),
    };
    let j: serde_json::Value = serde_json::to_value(&o).unwrap();
    assert_eq!(j["range"]["start_offset"], 0);
    assert_eq!(j["range"]["end_offset"], 4);
    assert_eq!(j["cop_name"], "Murphy/NoReceiverPuts");
}
```

**Step 2:** Run: `cargo test -p murphy-core offense_serializes_to_contract` — Expected: FAIL (type not defined).

**Step 3:** Define `Offense`, `Range { start_offset: u32, end_offset: u32 }`, `Severity` enum with `serde` derives. (No `autocorrect` field yet — Phase 4.)

**Step 4:** Run the test — Expected: PASS.

**Step 5:** Commit.
```bash
git add crates/murphy-core/src/offense.rs crates/murphy-core/src/lib.rs
git commit -m "feat(core): offense contract type matching design §5"
```

### Task 3: Parse adapter

**Files:**
- Create: `crates/murphy-core/src/parse.rs`
- Test: same file

**Step 1: Write the failing test:**
```rust
#[test]
fn parses_valid_ruby_to_ast() {
    let ast = parse("puts 1\n").expect("should parse");
    assert!(ast.has_root());
}
#[test]
fn syntax_error_is_structured_not_panic() {
    let err = parse("def (\n").unwrap_err();
    assert!(matches!(err, ParseError { .. }));
}
```

**Step 2:** Run: `cargo test -p murphy-core parse` — Expected: FAIL.

**Step 3:** Implement `parse(src: &str) -> Result<Ast, ParseError>` wrapping the **Spike 0.1** binding. `Ast` owns/borrows per the 0.1 ADR; expose a node-visiting entry point. Syntax errors → `ParseError { message, range }`, never panic.

**Step 4:** Run the tests — Expected: PASS.

**Step 5:** Commit.

### Task 4: Cop trait + visitor dispatch

**Files:**
- Create: `crates/murphy-core/src/cop.rs`
- Test: same file (with a stub cop)

**Step 1: Write the failing test** — a stub cop that flags every call node pushes offenses into a sink:
```rust
#[test]
fn dispatch_invokes_cop_per_call_node() {
    let ast = parse("foo; bar\n").unwrap();
    let mut sink = Vec::new();
    run_cops(&ast, "t.rb", &[Box::new(CountingStubCop::default())], &mut sink);
    assert_eq!(sink.len(), 2);
}
```

**Step 2:** Run — Expected: FAIL.

**Step 3:** Define `trait Cop { fn name(&self) -> &str; fn on_call_node(&self, node, ctx, sink); }` and `run_cops(ast, file, cops, sink)` that walks the AST once and dispatches call nodes. Read-only; cops only push offenses (design §4).

**Step 4:** Run — Expected: PASS.

**Step 5:** Commit.

### Task 5: First native cop — `NoReceiverPuts`

**Files:**
- Create: `crates/murphy-core/src/cops/no_receiver_puts.rs`
- Test: `crates/murphy-core/tests/cop_no_receiver_puts.rs` (table-driven)

**Step 1: Write the failing table-driven test:**
```rust
// positive: `puts "x"`, `print 1`, `p obj`  → 1 offense each at the message location
// negative: `obj.puts`, `logger.info "x"`, `x = 1` → 0 offenses
```
Each case: input source → expected offense count + expected `range` on the method-name token.

**Step 2:** Run — Expected: FAIL.

**Step 3:** Implement: in `on_call_node`, offense when `receiver` is nil **and** name ∈ {`puts`,`print`,`p`}; range = the message/selector location (byte offsets from Spike 0.1). Message: `"Use a logger instead of puts"`. Severity `Warning`.

**Step 4:** Run — Expected: PASS (all positive + negative rows).

**Step 5:** Commit.

### Task 6: Offense aggregator

**Files:**
- Create: `crates/murphy-core/src/aggregator.rs`
- Test: same file

**Step 1: Write the failing test** — given unsorted offenses across files, output is sorted by `(file, start_offset)` and exact duplicates `(file,cop_name,range,message)` are deduped.

**Step 2:** Run — Expected: FAIL.

**Step 3:** Implement `aggregate(Vec<Offense>) -> Vec<Offense>`: stable sort + dedupe. (Priority/severity resolution across native+mruby is Phase 3 — keep this minimal now.)

**Step 4:** Run — Expected: PASS.

**Step 5:** Commit.

### Task 7: CLI wiring + exit codes

**Files:**
- Modify: `crates/murphy-cli/src/main.rs`
- Test: `crates/murphy-cli/tests/cli.rs` (use `assert_cmd`)

**Step 1: Write the failing tests:**
- `murphy lint clean.rb` (no offenses) → stdout `[]`, exit `0`.
- `murphy lint dirty.rb` (has `puts`) → JSON array with one offense, exit `1`.
- missing file → exit `2`.

**Step 2:** Run: `cargo test -p murphy-cli` — Expected: FAIL.

**Step 3:** Implement: parse `lint <file>` arg, read file, `parse` → `run_cops` (just `NoReceiverPuts`) → `aggregate` → serialize array to stdout. Exit codes: `0` no offenses / `1` offenses / `2` config-or-cop/file-setup error / `3` internal failure (panic guard).

**Step 4:** Run — Expected: PASS.

**Step 5:** Commit.

### Task 8: Syntax-error file behavior

**Files:**
- Modify: `crates/murphy-cli/src/main.rs`
- Test: `crates/murphy-cli/tests/cli.rs`

**Step 1: Write the failing test** — `murphy lint broken.rb` (unparseable) → exactly one offense describing the syntax error, cops **skipped** for that file, exit `1` (design §6).

**Step 2:** Run — Expected: FAIL.

**Step 3:** On `ParseError`, emit one syntax-error offense and skip cop execution for that file; continue (single-file here, but keep the "continue others" shape).

**Step 4:** Run — Expected: PASS.

**Step 5:** Commit.

### Task 9: Multi-file integration snapshot

**Files:**
- Create: `crates/murphy-cli/tests/fixtures/sample_project/` (a few `.rb` files, one clean, one dirty, one broken)
- Test: `crates/murphy-cli/tests/integration_snapshot.rs`

**Step 1: Write the failing test** — `murphy lint <dir-of-files...>` (loop over the explicit file list; no discovery yet) produces a stable JSON snapshot (sorted, deterministic).

**Step 2:** Run — Expected: FAIL.

**Step 3:** Accept multiple file args; loop (still single-threaded); aggregate across files; assert against committed snapshot.

**Step 4:** Run — Expected: PASS.

**Step 5:** Commit.

### Task 10: Quickstart docs + Phase 1 close

**Step 1:** Add a README "Quickstart" (`cargo build`, `murphy lint path/to/file.rb`) and fill the `## Build & Test` section of `CLAUDE.md` with the now-real `cargo` commands (incl. single-test: `cargo test -p murphy-core <name>`).

**Step 2:** Run full suite: `cargo test` — Expected: ALL PASS.

**Step 3:** Commit. Phase 1 walking skeleton complete.

### Phase 1 Gate

Demo the slice end-to-end. The offense JSON contract, exit codes, and TDD harness are now frozen — Phase 2+ build on them without renegotiating the contract.

---

## Phase 2+ — Coarse Milestones (re-plan in detail after their predecessor's gate)

- **Phase 2 — Native engine scale-out:** file discovery (own-config include/exclude + `.murphyignore`), all-core parallel cop execution over the shared immutable AST, `source_digest` cache key (design §3, §5).
- **Phase 3 — mruby cop path:** `Murphy::Cop` base + `on_<prism_node_type>` visitors SDK; the **native-primitive IDL** formalized from Spike 0.2; `add_offense` + `fix.replace/insert/remove`; per-cop independent mruby state; instruction/time deadlines (Spike 0.3); Ruby exception → `error offense`, continue (design §4, §6, §8).
- **Phase 4 — Autocorrect:** descending-offset apply with overlap/conflict detection + conflict log; reparse→rerun loop with max-iteration cutoff and oscillation handling; **idempotency tests written first** (design §5, §6, §7).
- **Phase 5 — Config & migration:** own config format; one-way `murphy migrate` from `.rubocop.yml` with roundtrip tests (design §2, §7).
- **Phase 6 — Standard cop scope:** *separate brainstorm* on which top-adoption Layout/Style/Lint cops ship in v1 (design §8); hyperfine perf-regression CI at N=1/20/100 vs RuboCop; diff-quality watch vs `rubocop -a` (design §7).
- **Phase 7 — Future (§8):** third-party cop sandbox (seccomp etc.), LSP integration, alternate cop frontends (Rune/Roto) — out of this plan's scope.

---

## Execution Notes

- Each Phase 1 task: failing test → run-fail → minimal impl → run-pass → commit. DRY, YAGNI, TDD.
- Phase 0 spikes commit ADRs + `spikes/` PoCs; PoC code is throwaway and **not** carried into `crates/`.
- Do not start Phase 1 Task 3+ until the Phase 0 gate passes (parse adapter depends on Spike 0.1).
