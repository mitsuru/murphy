# Phase 6 Task 7 Control-Flow Cops Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the Phase 6 v1 control-flow and predicate native cops with conservative autocorrect behavior.

**Architecture:** Extend the existing single-pass `Cop` visitor only with hooks needed by the new cops, keeping every cop in a focused module under `crates/murphy-core/src/cops/style` or `crates/murphy-core/src/cops/lint`. Prefer byte-range source slicing and skip ambiguous/comment-containing cases instead of guessing.

**Tech Stack:** Rust, `ruby-prism`, existing `Cop` trait, `Offense`, `FixEdit`, `run_single_cop`, and `CopRegistry`.

---

### Task 1: Beads Subtasks and Visitor Hooks

**Files:**
- Modify: `crates/murphy-core/src/cop.rs`

- [ ] Create beads subtasks under `murphy-7rg.3` for each cop: `Style/NilComparison`, `Style/AndOr`, `Style/RedundantReturn`, `Style/IfUnlessModifier`, `Lint/EmptyWhen`, `Lint/UnreachableCode`.

Run examples:

```bash
bd create --title="Implement Style/NilComparison" --description="Add conservative v1 native cop for x == nil and x != nil with safe autocorrect." --type=task --priority=2 --deps discovered-from:murphy-7rg.3
bd create --title="Implement Style/AndOr" --description="Add conservative v1 native cop for and/or in conditional contexts." --type=task --priority=2 --deps discovered-from:murphy-7rg.3
```

- [ ] Add failing visitor fan-out tests for the minimum hooks needed by the six cops.

Add test stubs in `crates/murphy-core/src/cop.rs` that push one offense from each new hook and assert the expected count for sources containing matching nodes.

- [ ] Add default no-op hooks to `Cop` and dispatch them from `Dispatcher`.

Expected hook shape:

```rust
fn on_if_node(&self, _node: &ruby_prism::IfNode<'_>, _ctx: &CopContext<'_>, _sink: &mut Vec<Offense>) {}
fn on_return_node(&self, _node: &ruby_prism::ReturnNode<'_>, _ctx: &CopContext<'_>, _sink: &mut Vec<Offense>) {}
fn on_case_node(&self, _node: &ruby_prism::CaseNode<'_>, _ctx: &CopContext<'_>, _sink: &mut Vec<Offense>) {}
```

- [ ] Verify visitor tests.

Run: `cargo test -p murphy-core cop::tests -- --nocapture`

Expected: all `cop::tests` pass.

### Task 2: Style/NilComparison

**Files:**
- Create: `crates/murphy-core/src/cops/style/nil_comparison.rs`
- Modify: `crates/murphy-core/src/cops/style.rs`
- Modify: `crates/murphy-core/src/registry.rs`

- [ ] Write failing tests for `x == nil` and `x != nil`.

Expected behavior:

```ruby
x == nil  # offense, autocorrect to x.nil?
```

- [ ] Implement minimal `on_call_node` logic using call selector and source byte ranges.

Only autocorrect when the left expression and `nil` source ranges are directly available and the comparison has no comments inside the replacement range.

- [ ] Register and verify.

Run: `cargo test -p murphy-core nil_comparison registry::tests -- --nocapture`

Expected: tests pass and registry includes `Style/NilComparison`.

### Task 3: Style/AndOr

**Files:**
- Create: `crates/murphy-core/src/cops/style/and_or.rs`
- Modify: `crates/murphy-core/src/cops/style.rs`
- Modify: `crates/murphy-core/src/registry.rs`

- [ ] Write failing tests for conditional-context `and` / `or`.

Expected behavior:

```ruby
if a and b
end
# offense on and, autocorrect to &&

foo and return
# clean in v1 because it is not a conditional expression context
```

- [ ] Implement minimal conditional-context detection through `on_if_node` source scanning.

Scan only the if condition range. Replace exact ` and ` or ` or ` operator tokens with `&&` or `||` when surrounded by whitespace and not inside a string/comment according to the existing literal/comment scanners.

- [ ] Verify.

Run: `cargo test -p murphy-core and_or registry::tests -- --nocapture`

Expected: tests pass and registry includes `Style/AndOr`.

### Task 4: Style/RedundantReturn

**Files:**
- Create: `crates/murphy-core/src/cops/style/redundant_return.rs`
- Modify: `crates/murphy-core/src/cops/style.rs`
- Modify: `crates/murphy-core/src/registry.rs`

- [ ] Write failing tests for final method-body return.

Expected behavior:

```ruby
def x
  return 1
end
# offense, autocorrect to def x\n  1\nend

def x
  return 1 if cond
end
# clean in v1
```

- [ ] Implement only final, unconditional returns with a directly replaceable expression.

Skip modifier returns, multi-value returns, returns outside method bodies, and any range containing comments.

- [ ] Verify.

Run: `cargo test -p murphy-core redundant_return registry::tests -- --nocapture`

Expected: tests pass and registry includes `Style/RedundantReturn`.

### Task 5: Style/IfUnlessModifier

**Files:**
- Create: `crates/murphy-core/src/cops/style/if_unless_modifier.rs`
- Modify: `crates/murphy-core/src/cops/style.rs`
- Modify: `crates/murphy-core/src/registry.rs`

- [ ] Write failing tests for simple single-line bodies.

Expected behavior:

```ruby
if ok
  run
end
# offense, autocorrect to run if ok\n

if ok # comment
  run
end
# clean in v1
```

- [ ] Implement only single-statement body and single-line condition without comments.

Skip `else`/`elsif`, multi-line conditions, multi-line bodies, nested blocks, and any range containing comments.

- [ ] Verify.

Run: `cargo test -p murphy-core if_unless_modifier registry::tests -- --nocapture`

Expected: tests pass and registry includes `Style/IfUnlessModifier`.

### Task 6: Lint/EmptyWhen and Lint/UnreachableCode

**Files:**
- Create: `crates/murphy-core/src/cops/lint/empty_when.rs`
- Create: `crates/murphy-core/src/cops/lint/unreachable_code.rs`
- Modify: `crates/murphy-core/src/cops/lint.rs`
- Modify: `crates/murphy-core/src/registry.rs`

- [ ] Write failing tests for empty `when` clauses and same-body unreachable statements.

Expected behavior:

```ruby
case x
when 1
when 2
  y
end
# offense on the empty when 1

def x
  return 1
  puts 2
end
# offense on puts 2
```

- [ ] Implement offense-only detection first.

For `EmptyWhen`, inspect `CaseNode` when clauses and flag clauses with no statements. For `UnreachableCode`, inspect same-body statements after unconditional `return`, `break`, `next`, or receiver-less `raise`.

- [ ] Verify.

Run: `cargo test -p murphy-core empty_when unreachable_code registry::tests -- --nocapture`

Expected: tests pass and registry includes both cops.

### Task 7: Integration, Idempotency, and Commit

**Files:**
- Modify: `crates/murphy-core/tests/autocorrect_idempotency.rs` if any new autocorrect needs end-to-end fixpoint coverage.

- [ ] Run focused Task 7 tests.

Run: `cargo test -p murphy-core cops::style cops::lint cop::tests -- --nocapture`

Expected: all targeted tests pass.

- [ ] Run full verification.

Run: `cargo fmt --check`

Run: `cargo test`

Expected: both commands pass.

- [ ] Close completed beads subtasks and commit.

Run: `git status`

Run: `git add crates/murphy-core/src/cop.rs crates/murphy-core/src/cops crates/murphy-core/src/registry.rs crates/murphy-core/tests/autocorrect_idempotency.rs docs/superpowers/plans/2026-05-20-murphy-phase-6-task-7-control-flow-cops.md`

Run: `git commit -m "feat: add control-flow style and lint cops"`

- [ ] Push code and beads.

Run: `git pull --rebase && bd dolt push && git push && git status`

Expected: branch is up to date with origin and working tree is clean.
