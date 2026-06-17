# Liveness: exception-CFG dominance + retry back-edge (murphy-l1iy) Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Eliminate 5 `Lint/UselessAssignment` false positives on Mastodon caused by Murphy's liveness not modeling begin/rescue branch exclusivity or the `retry` back-edge.

**Architecture:** Two independent fixes, landed as two commits with the full suite between them.
**A (commit 1, contained to `var_semantic_model.rs`):** make `Rescue` an *asymmetric* branch barrier — its arms (body / each resbody / else) are mutually exclusive for *domination* (`chain_is_prefix`), but the `body` arm stays *compatible* with every sibling for *read observation* (`barrier_condition_is_compatible`), because exception flow carries begin-body writes into the handlers.
**B (commit 2, `translate.rs` + `var_semantic_model.rs`):** translate `retry` to the existing `NodeKind::Retry` (currently `Unknown`), then treat a `begin..rescue..end` whose resbody contains a `Retry` as a loop body (always-referenced), matching RuboCop's `process_rescue`.

**Tech Stack:** Rust workspace (`murphy-plugin-api`, `murphy-std`, `murphy-translate`). Tests via `murphy_plugin_api::test_support::{test, indoc}` with inline `^^^` caret offense markers. Build/test require `eval "$(mise activate bash)"` in the worktree shell.

**Ground truth (rubocop 1.87 VariableForce):** `BRANCH_NODES = [if, case, case_match, rescue]`; `mark_last_as_reassigned!` kills the prior write only when same-branch; `branch.rb` ExceptionHandler makes the main body `may_run_incompletely?`/`may_jump_to_other_branch?`; `process_rescue` loop-ifies a rescue containing `retry`.

**Key files (read first):**
- `crates/murphy-plugin-api/src/var_semantic_model.rs` — `is_branch_barrier` (~82), `barrier_condition_is_compatible` (~120), `is_in_protected_begin_body` (~132), `is_in_loop_body` (~157), `analyze_scope_is_referenced` (~228).
- `crates/murphy-std/src/cops/lint/useless_assignment.rs` — tests module (`mod tests`, harness `test::<UselessAssignment>()`).
- `crates/murphy-translate/src/translate.rs` — `translate_node` catch-all `Unknown` arm (~1465); `Break`/`Next` arms (~1200) as the sibling pattern.
- `crates/murphy-pattern/tests/rubocop_pattern_compat.rs:334` — `(retry)` GapProbe case.

**Shell preamble for every build/test command (worktree needs mise):**
```bash
cd /home/ubuntu/projects/murphy/.claude/worktrees/liveness-exc-cfg-retry && eval "$(mise activate bash)"
```

---

## PHASE A — Rescue as an asymmetric branch barrier

### Task A1: Write the failing Pattern-A tests

**Files:**
- Modify (tests only): `crates/murphy-std/src/cops/lint/useless_assignment.rs` (inside `mod tests`)

**Step 1: Add these tests** at the end of the `mod tests` block (before its closing `}`):

```rust
    #[test]
    fn rescue_alt_branch_does_not_overwrite_begin_body_write() {
        // request.rb `encoding`: begin-body write and rescue write are
        // mutually exclusive; the begin-body value reaches the read on the
        // no-exception path. RuboCop 1.87 reports nothing.
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
            if charset.nil?
              encoding = Encoding::BINARY
            else
              begin
                encoding = Encoding.find(charset)
              rescue ArgumentError
                encoding = Encoding::BINARY
              end
            end
            String.new(encoding: encoding)
        "#});
    }

    #[test]
    fn rescue_alt_branch_does_not_overwrite_pre_begin_write() {
        // request.rb `addresses`: a pre-begin init plus a begin-body write,
        // both with a rescue alternative. None are useless per RuboCop 1.87.
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
            addresses = []
            begin
              addresses = [resolve(host)]
            rescue StandardError
              addresses = lookup(host)
              addresses = addresses.first(2)
            end
            addresses.each { |a| p a }
        "#});
    }

    #[test]
    fn rescue_alt_branch_nested_in_conditional_not_flagged() {
        // process_mentions_service.rb `mentioned_account`.
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
            mentioned_account = find_remote(username)
            if undeliverable?(mentioned_account)
              begin
                mentioned_account = resolve(match)
              rescue Error
                mentioned_account = nil
              end
            end
            use(mentioned_account)
        "#});
    }

    #[test]
    fn multi_statement_begin_body_write_observed_by_rescue() {
        // CANARY for body-arm identity: with a MULTI-statement begin body the
        // body wraps in a `(begin ...)` stmt-list node, so the arm recorded in
        // barrier_chain must still `==` Rescue.body. `value` is only read in
        // the handler via exception flow.
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
            begin
              setup
              value = compute
            rescue
              log(value)
            end
        "#});
    }

    #[test]
    fn genuinely_unused_rescue_handler_write_still_flagged() {
        // Guard against over-suppression: a never-read write inside a resbody
        // is still useless (RuboCop flags it too).
        test::<UselessAssignment>().expect_offense(indoc! {r#"
            begin
              work
            rescue
              x = 1
              ^ Useless assignment to variable - `x`.
            end
        "#});
    }
```

**Step 2: Run and confirm the four no-offense tests FAIL, the guard PASSES**

```bash
cargo test -p murphy-std --lib rescue_alt_branch multi_statement_begin_body genuinely_unused_rescue_handler 2>&1 | tail -25
```
Expected: `rescue_alt_branch_*` and `multi_statement_begin_body_write_observed_by_rescue` FAIL (offense reported where none expected); `genuinely_unused_rescue_handler_write_still_flagged` PASSES. If the canary already passes, the body-arm identity assumption held even pre-fix — note it and continue.

**Step 3: Commit the failing tests**
```bash
git add crates/murphy-std/src/cops/lint/useless_assignment.rs
git commit -m "test(liveness): pin begin/rescue alternative-branch UselessAssignment FPs (murphy-l1iy)"
```

---

### Task A2: Make `Rescue` an asymmetric branch barrier

**Files:**
- Modify: `crates/murphy-plugin-api/src/var_semantic_model.rs`

**Step 1: Add `Rescue` to `is_branch_barrier`** (~line 82). Update the doc comment and the match:

```rust
/// Returns `true` for nodes that introduce exclusive branches.
/// `Rescue` is a barrier whose arms (body / each resbody / else) are mutually
/// exclusive for *domination*; the body arm is kept compatible for *reads* in
/// `barrier_condition_is_compatible` (exception flow). `Resbody`/`Ensure` are
/// not barriers themselves.
fn is_branch_barrier(ast: &Ast, node: NodeId) -> bool {
    matches!(
        *ast.kind(node),
        NodeKind::If { .. }
            | NodeKind::Case { .. }
            | NodeKind::When { .. }
            | NodeKind::CaseMatch { .. }
            | NodeKind::While { .. }
            | NodeKind::Until { .. }
            | NodeKind::Block { .. }
            | NodeKind::Numblock { .. }
            | NodeKind::Itblock { .. }
            | NodeKind::Rescue { .. }
    )
}
```

**Step 2: Extend `barrier_condition_is_compatible`** (~line 120) so the Rescue `body` arm is compatible with every sibling arm (exception flow carries begin-body writes into the handlers / else / fall-through):

```rust
fn barrier_condition_is_compatible(ast: &Ast, barrier: NodeId, a: NodeId, b: NodeId) -> bool {
    match *ast.kind(barrier) {
        NodeKind::If { cond, .. } | NodeKind::While { cond, .. } | NodeKind::Until { cond, .. } => {
            a == cond || b == cond
        }
        // The begin body (`body`) flows into every rescue/else/after arm via
        // exception control flow, so a begin-body write stays observable by a
        // read in any sibling arm. Resbody-vs-resbody and resbody-vs-else stay
        // exclusive. (Domination via `chain_is_prefix` is unaffected — only
        // read-compatibility relaxes here.)
        NodeKind::Rescue { body, .. } => body.get() == Some(a) || body.get() == Some(b),
        _ => false,
    }
}
```

**Step 3: Run the Task A1 tests — expect PASS**
```bash
cargo test -p murphy-std --lib rescue_alt_branch multi_statement_begin_body genuinely_unused_rescue_handler 2>&1 | tail -15
```
Expected: all 5 PASS.

**Step 4: Run the existing begin/rescue regression tests — expect PASS**
```bash
cargo test -p murphy-std --lib useless_assignment 2>&1 | tail -20
cargo test -p murphy-plugin-api --lib 2>&1 | tail -10
```
Expected: especially `begin_body_write_interrupted_by_exception_is_observed_by_rescue` and `rescue_handler_read_observes_begin_body_write` PASS. If `rescue_handler_read_observes_begin_body_write` fails, the body-arm identity in Step 2 is wrong — debug with `murphy ast --format sexp` on a multi-statement begin body and confirm `Rescue.body.get()` equals the arm node recorded by `barrier_chain`.

**Step 5: Run the full affected-crate suites + clippy**
```bash
cargo test -p murphy-plugin-api -p murphy-std --lib 2>&1 | tail -5
cargo clippy -p murphy-plugin-api -p murphy-std --all-targets -- -D warnings 2>&1 | tail -5
```
Expected: 0 failed; no clippy warnings.

**Step 6: Commit Phase A**
```bash
git add crates/murphy-plugin-api/src/var_semantic_model.rs
git commit -m "fix(liveness): model Rescue as an asymmetric branch barrier (murphy-l1iy)

Rescue arms (body/resbody/else) are mutually exclusive for domination, but
the begin body stays read-compatible with all arms via exception flow. Fixes
begin/rescue alternative-branch Lint/UselessAssignment FPs."
```

---

## PHASE B — retry back-edge

### Task B1: Write the failing Pattern-B tests

**Files:**
- Modify (tests only): `crates/murphy-std/src/cops/lint/useless_assignment.rs`

**Step 1: Add these tests** in `mod tests`:

```rust
    #[test]
    fn retry_accumulator_op_assign_not_flagged() {
        // request_pool.rb `retries`: `retries += 1; retry` — the op-assign
        // is read on the next iteration via the retry back-edge.
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
            retries = 0
            begin
              do_work
            rescue StandardError
              if retries.positive?
                raise
              else
                retries += 1
                retry
              end
            end
        "#});
    }

    #[test]
    fn retry_accumulator_simple_not_flagged() {
        // snowflake.rb `tries`.
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
            tries = 0
            begin
              insert_record
            rescue RecordNotUnique
              raise if tries > 100
              tries += 1
              retry
            end
        "#});
    }
```

**Step 2: Run and confirm FAIL**
```bash
cargo test -p murphy-std --lib retry_accumulator 2>&1 | tail -15
```
Expected: both FAIL — `retries += 1` / `tries += 1` reported as useless (`Use \`+\` instead of \`+=\``).

**Step 3: Commit failing tests**
```bash
git add crates/murphy-std/src/cops/lint/useless_assignment.rs
git commit -m "test(liveness): pin retry-accumulator UselessAssignment FPs (murphy-l1iy)"
```

---

### Task B2: Translate `retry` to `NodeKind::Retry`

**Files:**
- Modify: `crates/murphy-translate/src/translate.rs`
- Modify: `crates/murphy-pattern/tests/rubocop_pattern_compat.rs:334`
- Test: `crates/murphy-translate/tests/` (add a translate test)

**Step 1: Write a failing translate test.** Add to the translate test suite (e.g. a unit `#[test]` in `translate.rs`'s test module, mirroring existing keyword tests):

```rust
    #[test]
    fn translates_retry_keyword_to_retry_node() {
        let ast = translate("begin\nrescue\n  retry\nend\n", "t.rb");
        let has_retry = (0..ast.len())
            .map(NodeId::from_index)
            .any(|id| matches!(ast.kind(id), murphy_ast::NodeKind::Retry));
        assert!(has_retry, "retry keyword must lower to NodeKind::Retry, not Unknown");
    }
```
(If `translate`/`NodeId::from_index`/`ast.len()` helpers differ, match the conventions already used in that test module — check neighboring tests first.)

**Step 2: Run — expect FAIL** (`retry` currently lowers to `Unknown`):
```bash
cargo test -p murphy-translate translates_retry_keyword_to_retry_node 2>&1 | tail -10
```

**Step 3: Add the translate arm.** Find how prism `RetryNode` reaches `translate_node`; add an arm before the `Unknown` catch-all (mirror the `Redo`/`Break`/`Next` shape):

```rust
            prism::Node::RetryNode { .. } => self.builder.push(NodeKind::Retry, range),
```
(Confirm the exact prism variant/match style used for other leaf keywords in this file; `Retry` carries no children.)

**Step 4: Run translate test — expect PASS**, plus the existing coverage/snapshot tests:
```bash
cargo test -p murphy-translate 2>&1 | tail -15
```
Expected: `translates_retry_keyword_to_retry_node` PASS; `coverage.rs` (`unknown_ratio_under_5_percent`, `translate_never_panics_on_diverse_input`) PASS; `control_flow.sexp` snapshot unaffected (it contains the method `retry_later`, not the keyword). If any snapshot legitimately changed (a fixture containing the `retry` keyword), review the diff and bless it only if it changed `(unknown)` → `(retry)`.

**Step 5: Recategorize the pattern-compat probe.** In `crates/murphy-pattern/tests/rubocop_pattern_compat.rs:334`, the `(retry)` case is `category: GapProbe` (asserts the pattern finds nothing). Now that `retry` lowers to a real node, this probe must reflect that it matches. Read the surrounding test logic + the `GapProbe`/non-gap category semantics, then change the `(retry)` case to the appropriate "supported/matches" category (mirror a sibling supported case). Run:
```bash
cargo test -p murphy-pattern 2>&1 | tail -15
```
Expected: PASS.

**Step 6: Run unreachable_code / unreachable_loop suites** (now-live `NodeKind::Retry` arms):
```bash
cargo test -p murphy-std --lib unreachable_code unreachable_loop 2>&1 | tail -15
```
Expected: PASS. (Code after a *bare* `retry` is now correctly unreachable; `retry` is treated as a continue for unreachable-loop. If a test pinned the old Unknown behavior, update it to the correct RuboCop-parity expectation and note it in the commit.)

---

### Task B3: Treat retry-rescue as a loop body in liveness

**Files:**
- Modify: `crates/murphy-plugin-api/src/var_semantic_model.rs`

**Step 1: Add the helper** near `is_in_loop_body` (~line 180). Compute "contains retry" once per Rescue (perf: no per-assignment subtree DFS — walk up to each enclosing Rescue, and for each, scan its resbodies once):

```rust
/// Returns `true` if `node` is inside a `Rescue` whose resbody subtree contains
/// a `Retry`. RuboCop treats such a `begin..rescue..end` as a loop
/// (`process_rescue` → `process_loop`), so writes inside it may be read on the
/// next iteration via the retry back-edge and must not be flagged.
fn is_in_retry_rescue(ast: &Ast, root: NodeId, node: NodeId) -> bool {
    let mut current = node;
    while let Some(parent) = ast.parent(current).get() {
        if parent == root {
            return false;
        }
        if let NodeKind::Rescue { resbodies, .. } = *ast.kind(parent) {
            if ast
                .list(resbodies)
                .iter()
                .any(|&rb| subtree_contains_retry(ast, rb))
            {
                return true;
            }
        }
        current = parent;
    }
    false
}

/// DFS over `node`'s subtree for a `Retry`, not descending into nested scopes.
fn subtree_contains_retry(ast: &Ast, node: NodeId) -> bool {
    if matches!(*ast.kind(node), NodeKind::Retry) {
        return true;
    }
    ast.children(node)
        .iter()
        .any(|&c| subtree_contains_retry(ast, c))
}
```
(Confirm `ast.list(...)` / `ast.children(...)` signatures against neighboring helpers in this file; `barrier_chain`/`is_in_loop_body` already use `ast.parent`/`ast.kind`.)

**Step 2: Wire it into `analyze_scope_is_referenced`** (~line 247), alongside the loop-body short-circuit:

```rust
            // Loop body OR retry-rescue (RuboCop process_loop): always
            // referenced — the next iteration may read it.
            if is_in_loop_body(ast, scope_root, asgn_node)
                || is_in_retry_rescue(ast, scope_root, asgn_node)
            {
                var.assignments[i].is_referenced = true;
                continue;
            }
```

**Step 3: Run the Task B1 tests — expect PASS**
```bash
cargo test -p murphy-std --lib retry_accumulator 2>&1 | tail -10
```

**Step 4: Full affected-crate suites + clippy**
```bash
cargo test -p murphy-plugin-api -p murphy-std -p murphy-translate -p murphy-pattern 2>&1 | tail -8
cargo clippy -p murphy-plugin-api -p murphy-std -p murphy-translate --all-targets -- -D warnings 2>&1 | tail -5
```
Expected: 0 failed; no warnings.

**Step 5: Commit Phase B**
```bash
git add crates/murphy-translate/src/translate.rs crates/murphy-plugin-api/src/var_semantic_model.rs crates/murphy-pattern/tests/rubocop_pattern_compat.rs crates/murphy-translate/tests
git commit -m "fix(liveness): model retry back-edge (translate retry + loop-ify retry-rescue) (murphy-l1iy)

Lower retry to NodeKind::Retry (was Unknown) and treat a begin..rescue..end
whose resbody contains retry as a loop body, so accumulators read on the next
iteration are not flagged. Mirrors RuboCop process_rescue."
```

---

## PHASE C — Integration verification & quality gates

### Task C1: Full workspace gates

```bash
cargo test --workspace 2>&1 | tail -15
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -5
cargo +nightly fmt --check 2>&1 | tail -5
```
Expected: all green. Fix any fallout before proceeding.

### Task C2: Mastodon parity check (the real acceptance gate)

**Step 1:** Build release and lint the 4 previously-excluded files directly (the `.murphy_todo.yml` excludes them; lint the repro shapes via the release binary against copies to bypass discovery, OR temporarily remove the 4 `Lint/UselessAssignment` Exclude entries):

```bash
cargo build --release -p murphy-cli 2>&1 | tail -2
BIN=/home/ubuntu/projects/murphy/.claude/worktrees/liveness-exc-cfg-retry/target/release/murphy
# Edit ~/mastodon/.murphy_todo.yml: remove the 4 paths under Lint/UselessAssignment.
$BIN lint ~/mastodon 2>&1 | tail -20
```
Expected: **0 offenses** across the whole repo (parity baseline preserved; no new unreachable_code/unreachable_loop offenses from the retry flip).

**Step 2:** If 0, leave the `.murphy_todo.yml` edit in place (it is a Mastodon-repo change, committed separately in that repo, not in murphy). Record the before/after offense count. If non-zero, triage each new offense: confirm whether RuboCop 1.87 also reports it (use standalone `rubocop`, not Mastodon's bundle — see memory `rubocop-ground-truth`). Genuine new parity gaps → new bd issue; FPs → fix here.

### Task C3: Update beads + finish branch

- `bd update murphy-l1iy --notes "..."` with the before/after FP counts and the commits.
- REQUIRED SUB-SKILL: `superpowers:finishing-a-development-branch`.
- Then ask whether to `bd close murphy-l1iy`.

---

## Notes / known limitations (document, don't fix here)
- retry-rescue is treated as a whole-loop body (matches RuboCop), so a genuinely-dead write inside a retry-rescue is not flagged — same approximation Murphy already makes for `while`/`until`/`for`.
- Ensure is intentionally left non-barrier (covered by `is_in_protected_begin_body`); no FP requires changing it. Revisit only if a future FP demands it.
