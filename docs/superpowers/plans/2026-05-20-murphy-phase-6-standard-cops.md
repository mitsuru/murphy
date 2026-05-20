# Murphy Phase 6 Standard Cops Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the ADR 0018 v1 standard-cop scope, harden the native/mruby cop boundary, and add Phase 6 perf/diff gates.

**Architecture:** Keep `murphy-core` as the cop-engine owner: native cops live in focused modules, shared visitor hooks and source helpers live in small support modules, and `CopRegistry` remains the single source of enabled built-in cops. Keep `murphy-cli` as orchestration: it loads config, registry, mruby cops, and fixpoint logic, then surfaces perf/debug output without changing the offense JSON contract.

**Tech Stack:** Rust workspace, `ruby-prism`, embedded `mruby3-sys`, `serde_json`, `rayon`, existing `assert_cmd` tests, shell-based CI scripts, RuboCop/hyperfine for optional perf and diff-quality checks.

---

## File Structure

- Modify `crates/murphy-core/src/mruby/primitives.rs`: replace stringly `node_msg_range` return with typed range primitives and document the formal native primitive IDL.
- Modify `crates/murphy-core/src/mruby/cop_prelude.rb`: consume typed range primitives instead of parsing comma-separated strings.
- Modify `crates/murphy-core/src/mruby/sdk.rs`: keep native primitive registration and offense emission in sync with the IDL ADR.
- Add `docs/decisions/0019-phase-6-native-primitive-idl.md`: accepted IDL for mruby/native primitives.
- Modify `crates/murphy-core/src/cop.rs`: add visitor hooks only when a cop needs them; preserve single traversal and `Cop: Send + Sync`.
- Modify `crates/murphy-core/src/cops.rs`: replace the one-file native-cop module list with grouped submodules.
- Create `crates/murphy-core/src/cops/lint/*.rs`: `Debugger`, `DeprecatedClassMethods`, `DuplicateHashKey`, `EmptyWhen`, `UnreachableCode`, `UnusedMethodArgument`, `UselessAssignment`.
- Create `crates/murphy-core/src/cops/style/*.rs`: `FrozenStringLiteralComment`, `HashSyntax`, `StringLiterals`, `SymbolArray`, `WordArray`, `RedundantReturn`, `NilComparison`, `IfUnlessModifier`, `RedundantSelf`, `AndOr`.
- Create `crates/murphy-core/src/cops/layout/*.rs`: `TrailingWhitespace`, `EmptyLines`, `SpaceAroundOperators`, `SpaceInsideParens`, `DotPosition`.
- Create `crates/murphy-core/src/cops/support.rs`: source-line helpers, byte-range edit helpers, token-lite scans, and shared table-test harness utilities.
- Modify `crates/murphy-core/src/registry.rs`: register the ADR 0018 native cop set in deterministic order.
- Modify `crates/murphy-core/src/lib.rs`: re-export only public types that are part of existing contracts; avoid re-exporting every cop type unless tests need it.
- Modify `crates/murphy-core/tests/autocorrect_idempotency.rs`: add standard-cop fixpoint coverage.
- Modify `crates/murphy-cli/tests/integration_snapshot.rs` and related snapshots: add a Phase 6 fixture separate from the frozen sample-project contract.
- Create `scripts/perf/phase6_hyperfine.sh`: run Murphy vs RuboCop at N=1/20/100.
- Create `scripts/diff/phase6_rubocop_diff.sh`: compare Murphy `--fix` output against `rubocop -a` on the watch corpus.
- Add `.github/workflows/phase6-perf.yml` if CI is available in the repository; otherwise add a documented local perf script and file a beads follow-up for CI wiring.
- Modify `README.md`: update status, standard cop list, and Phase 6 limitations.
- Add `docs/decisions/0020-phase-6-gate-review.md`: final Phase 6 gate verdict.

## Tasks

### Task 1: Native Primitive IDL Hardening (`murphy-7rg.2`)

**Files:**
- Modify: `crates/murphy-core/src/mruby/primitives.rs`
- Modify: `crates/murphy-core/src/mruby/cop_prelude.rb`
- Modify: `crates/murphy-core/src/mruby/sdk.rs`
- Add: `docs/decisions/0019-phase-6-native-primitive-idl.md`
- Test: `crates/murphy-core/tests/cop_no_puts_mruby.rs`

- [ ] Claim the beads issue.

Run: `bd update murphy-7rg.2 --claim --json`

Expected: JSON shows `status` as `in_progress`.

- [ ] Write failing tests for typed `message_loc` access.

Add or update a test in `crates/murphy-core/tests/cop_no_puts_mruby.rs` with this shape:

```rust
#[test]
fn mruby_node_message_loc_is_typed_range() {
    let source = "puts 'x'\n";
    let cop = r#"
      class TypedRangeCop < Murphy::Cop
        def on_call_node(node)
          return unless node.name == :puts

          range = node.message_loc
          add_offense(range, message: "typed range") do |fix|
            fix.replace(range, "logger.info")
          end
        end
      end
    "#;

    let ctx = murphy_core::AstContext::new(source.as_bytes().to_vec());
    let offenses = murphy_core::run_mruby_cop(ctx, "TypedRangeCop", "typed.rb", cop)
        .expect("run cop");

    assert_eq!(offenses.len(), 1);
    assert_eq!(offenses[0].range.start_offset, 0);
    assert_eq!(offenses[0].range.end_offset, 4);
    assert_eq!(offenses[0].autocorrect.as_ref().unwrap().edits[0].replacement, "logger.info");
}
```

- [ ] Run the test and confirm it fails for the old stringly path.

Run: `cargo test -p murphy-core --test cop_no_puts_mruby mruby_node_message_loc_is_typed_range -- --nocapture`

Expected: failure because `node.message_loc` still depends on string parsing or lacks the typed return path.

- [ ] Implement typed primitives.

In `crates/murphy-core/src/mruby/primitives.rs`, add native callbacks with this IDL:

```text
Murphy.node_msg_start(handle) -> Integer
Murphy.node_msg_end(handle) -> Integer
Murphy.source_slice(start, end) -> String
Murphy.node_name(handle) -> String
Murphy.node_receiver_nil?(handle) -> true | false
Murphy.node_count -> Integer
```

Keep `Range::from_prism_location(&loc)` as the only prism-location narrowing site. Return `-1` for missing/out-of-range message locations, and let Ruby glue convert that into `nil` rather than panic.

- [ ] Update Ruby glue.

In `crates/murphy-core/src/mruby/cop_prelude.rb`, make `Node#message_loc` build a `Murphy::Range` from the two typed integer primitives:

```ruby
def message_loc
  start_offset = Murphy.node_msg_start(@handle)
  end_offset = Murphy.node_msg_end(@handle)
  return nil if start_offset < 0 || end_offset < 0

  Murphy::Range.new(start_offset, end_offset)
end
```

Remove any `split(',').map(&:to_i)` logic.

- [ ] Write the IDL ADR.

Create `docs/decisions/0019-phase-6-native-primitive-idl.md` documenting the exact primitives above, integer sentinel behavior, byte-offset contract, and the reason stringly range transport was removed.

- [ ] Run focused checks.

Run: `cargo test -p murphy-core mruby::primitives mruby::sdk -- --nocapture`

Run: `cargo test -p murphy-core --test cop_no_puts_mruby`

Expected: all pass.

- [ ] Commit and close.

Run: `git add crates/murphy-core/src/mruby/primitives.rs crates/murphy-core/src/mruby/cop_prelude.rb crates/murphy-core/src/mruby/sdk.rs crates/murphy-core/tests/cop_no_puts_mruby.rs docs/decisions/0019-phase-6-native-primitive-idl.md && git commit -m "fix: formalize mruby primitive IDL"`

Run: `bd close murphy-7rg.2 --reason "Formalized typed native primitive IDL and removed stringly node message range path." --json`

Expected: commit succeeds and issue closes.

### Task 2: Native Cop Suite Hygiene (`murphy-nkq`)

**Files:**
- Modify: `crates/murphy-core/src/cops.rs`
- Move: `crates/murphy-core/src/cops/no_receiver_puts.rs` to `crates/murphy-core/src/cops/murphy/no_receiver_puts.rs`
- Create: `crates/murphy-core/src/cops/{lint,style,layout,support}.rs`
- Modify: `crates/murphy-core/src/registry.rs`
- Modify: `crates/murphy-core/src/lib.rs`

- [ ] Claim the beads issue.

Run: `bd update murphy-nkq --claim --json`

Expected: JSON shows `status` as `in_progress`.

- [ ] Add a registry test for scalable native names.

In `crates/murphy-core/src/registry.rs`, update the native-cop test so it asserts the registry has no duplicate native names and contains the existing cop:

```rust
#[test]
fn native_cop_names_are_unique_and_include_existing_cop() {
    let names = CopRegistry::native_cop_names();
    let unique: std::collections::BTreeSet<_> = names.iter().cloned().collect();

    assert_eq!(names.len(), unique.len(), "native cop names must be unique");
    assert!(names.iter().any(|name| name == "Murphy/NoReceiverPuts"));
}
```

- [ ] Run the registry test.

Run: `cargo test -p murphy-core registry::tests::native_cop_names_are_unique_and_include_existing_cop -- --nocapture`

Expected: pass before refactor; this is a safety net.

- [ ] Refactor cop modules without behavior changes.

Use this module shape in `crates/murphy-core/src/cops.rs`:

```rust
//! Native cop implementations.

pub mod layout;
pub mod lint;
pub mod murphy;
pub mod style;
pub(crate) mod support;

pub use murphy::no_receiver_puts::NoReceiverPuts;
```

Move the current `no_receiver_puts.rs` under `crates/murphy-core/src/cops/murphy/no_receiver_puts.rs` and create `crates/murphy-core/src/cops/murphy.rs` with:

```rust
pub mod no_receiver_puts;
```

Create empty group modules with explanatory comments and no shipped cops yet.

- [ ] Keep public exports stable.

In `crates/murphy-core/src/lib.rs`, keep:

```rust
pub use cops::NoReceiverPuts;
```

Do not export every future cop type from `lib.rs` unless an integration test needs it.

- [ ] Run behavior-preserving checks.

Run: `cargo test -p murphy-core registry::tests cops::murphy::no_receiver_puts -- --nocapture`

Run: `cargo test -p murphy-cli --test integration_snapshot -- --nocapture`

Expected: all pass; sample-project output remains unchanged.

- [ ] Commit and close.

Run: `git add crates/murphy-core/src/cops.rs crates/murphy-core/src/cops crates/murphy-core/src/registry.rs crates/murphy-core/src/lib.rs && git commit -m "refactor: organize native cop modules"`

Run: `bd close murphy-nkq --reason "Native cop modules and registry tests now scale beyond one built-in cop." --json`

Expected: commit succeeds and issue closes.

### Task 3: Shared Native Cop Test Harness

**Files:**
- Create: `crates/murphy-core/src/cops/support.rs`
- Modify: `crates/murphy-core/src/cop.rs`
- Test: `crates/murphy-core/src/cops/support.rs`

- [ ] Add support helpers with tests first.

Add tests in `support.rs` for byte line ranges, trailing whitespace detection, and edit construction:

```rust
#[test]
fn line_ranges_are_byte_offsets_for_multibyte_source() {
    let ranges = line_ranges("# 日本語\nputs 1\n".as_bytes());
    assert_eq!(ranges.len(), 2);
    assert_eq!(ranges[0].start_offset, 0);
    assert_eq!(ranges[0].end_offset, "# 日本語\n".len() as u32);
}

#[test]
fn replace_edit_uses_byte_range() {
    let edit = replace_edit(2, 5, "x");
    assert_eq!(edit.range.start_offset, 2);
    assert_eq!(edit.range.end_offset, 5);
    assert_eq!(edit.replacement, "x");
}
```

- [ ] Run tests and confirm failure.

Run: `cargo test -p murphy-core cops::support -- --nocapture`

Expected: fail because helpers do not exist.

- [ ] Implement minimal helpers.

Add functions with these signatures:

```rust
pub(crate) fn line_ranges(source: &[u8]) -> Vec<crate::Range>;
pub(crate) fn replace_edit(start: u32, end: u32, replacement: &str) -> crate::Edit;
pub(crate) fn offense_with_edit(
    file: &str,
    cop_name: &str,
    range: crate::Range,
    message: &str,
    edit: crate::Edit,
) -> crate::Offense;
```

Keep all helper offsets in bytes and return `Severity::Warning` by default.

- [ ] Add a standard test runner helper only if it removes duplication.

If three or more cops need identical parse/run/assert code, add this test-only helper:

```rust
#[cfg(test)]
pub(crate) fn run_single_cop(cop: Box<dyn crate::Cop>, source: &str) -> Vec<crate::Offense> {
    let ast = crate::parse(source).expect("parse source");
    let mut sink = Vec::new();
    crate::run_cops(&ast, "test.rb", &[cop], &mut sink);
    crate::aggregate(sink)
}
```

- [ ] Run focused checks.

Run: `cargo test -p murphy-core cops::support -- --nocapture`

Expected: pass.

- [ ] Commit.

Run: `git add crates/murphy-core/src/cops/support.rs crates/murphy-core/src/cop.rs crates/murphy-core/src/cops.rs && git commit -m "test: add native cop support harness"`

Expected: commit succeeds.

### Task 4: Text-Based Layout Cops (`murphy-7rg.3` subset)

**Files:**
- Create: `crates/murphy-core/src/cops/layout/trailing_whitespace.rs`
- Create: `crates/murphy-core/src/cops/layout/empty_lines.rs`
- Create: `crates/murphy-core/src/cops/layout/space_inside_parens.rs`
- Modify: `crates/murphy-core/src/cops/layout.rs`
- Modify: `crates/murphy-core/src/registry.rs`
- Modify: `crates/murphy-core/tests/autocorrect_idempotency.rs`

- [ ] Create beads subtasks for the three cops.

Run three `bd create` commands with parent/dependency metadata matching project conventions, one each for `Layout/TrailingWhitespace`, `Layout/EmptyLines`, and `Layout/SpaceInsideParens`. Each description must mention ADR 0018 and this plan.

Expected: three new open child tasks or discovered subtasks linked to `murphy-7rg.3`.

- [ ] Add failing tests for `Layout/TrailingWhitespace`.

In `trailing_whitespace.rs`, add unit tests:

```rust
#[test]
fn flags_and_removes_trailing_spaces() {
    let offenses = run_single_cop(Box::new(TrailingWhitespace), "x = 1  \n");

    assert_eq!(offenses.len(), 1);
    assert_eq!(offenses[0].cop_name, "Layout/TrailingWhitespace");
    assert_eq!(offenses[0].range.start_offset, 5);
    assert_eq!(offenses[0].range.end_offset, 7);
    assert_eq!(offenses[0].autocorrect.as_ref().unwrap().edits[0].replacement, "");
}
```

- [ ] Implement `Layout/TrailingWhitespace`.

Implement a stateless `TrailingWhitespace` cop. Because `Cop` currently dispatches AST nodes only, add a `fn inspect_file(&self, ctx: &CopContext<'_>, sink: &mut Vec<Offense>) {}` default hook to `Cop`, call it once before AST traversal in `run_cops`, and use it for source-wide text cops.

- [ ] Add and implement `Layout/EmptyLines`.

Test source:

```ruby
class A


  def x
  end
end
```

Expected: one offense covering the second blank line, with autocorrect replacing it with an empty string so the result has one blank line.

- [ ] Add and implement `Layout/SpaceInsideParens`.

Test source: `foo( 1, 2 )\n`

Expected: two offenses or one offense with two edits; corrected source `foo(1, 2)\n`.

- [ ] Register the three cops.

In `CopRegistry::native_cops_list()`, add the new cops in deterministic ADR order after `NoReceiverPuts` unless an existing test demands lexical order.

- [ ] Add idempotency tests.

In `autocorrect_idempotency.rs`, run `run_to_fixpoint` for each cop source and assert `FixpointStatus::Converged` and a stable corrected string.

- [ ] Run checks and commit.

Run: `cargo test -p murphy-core cops::layout autocorrect_idempotency -- --nocapture`

Run: `cargo test -p murphy-cli --test integration_snapshot -- --nocapture`

Expected: all pass; the frozen sample fixture changes only if it newly contains offenses from default-enabled cops. If it changes, inspect whether the new offenses are intended and update only Phase 6-specific snapshots, not the Phase 1 frozen-contract explanation.

Run: `git add crates/murphy-core/src/cop.rs crates/murphy-core/src/cops crates/murphy-core/src/registry.rs crates/murphy-core/tests/autocorrect_idempotency.rs crates/murphy-cli/tests && git commit -m "feat: add initial layout cops"`

### Task 5: Call-Pattern Lint Cops (`murphy-7rg.3` subset)

**Files:**
- Create: `crates/murphy-core/src/cops/lint/debugger.rs`
- Create: `crates/murphy-core/src/cops/lint/deprecated_class_methods.rs`
- Modify: `crates/murphy-core/src/cops/lint.rs`
- Modify: `crates/murphy-core/src/registry.rs`

- [ ] Create beads subtasks for `Lint/Debugger` and `Lint/DeprecatedClassMethods`.

Expected: subtasks linked to `murphy-7rg.3`.

- [ ] Add failing tests for `Lint/Debugger`.

Test cases:

```rust
#[test]
fn flags_common_debugger_calls() {
    let offenses = run_single_cop(Box::new(Debugger), "binding.pry\nbyebug\n");
    assert_eq!(offenses.iter().map(|o| o.cop_name.as_str()).collect::<Vec<_>>(), vec!["Lint/Debugger", "Lint/Debugger"]);
}
```

Cover at least `binding.pry`, `binding.irb`, `debugger`, `byebug`, and `require 'debug/start'` if the parser exposes the string literal argument through current APIs. If `require` argument extraction needs a new visitor/accessor, file a beads follow-up and implement call-name-only detection first.

- [ ] Implement `Lint/Debugger` on `on_call_node`.

Use receiver/name gates. For bare calls, flag names `debugger`, `byebug`, `pry`. For receiver chains, flag `binding.pry`, `binding.irb`, `binding.b`, `binding.break` when the receiver source slice equals `binding`.

- [ ] Add failing tests for `Lint/DeprecatedClassMethods`.

Test cases:

```rust
#[test]
fn corrects_deprecated_file_and_dir_exists() {
    let offenses = run_single_cop(Box::new(DeprecatedClassMethods), "File.exists?(path)\nDir.exists?(path)\n");
    let replacements: Vec<_> = offenses
        .iter()
        .map(|o| o.autocorrect.as_ref().unwrap().edits[0].replacement.as_str())
        .collect();
    assert_eq!(replacements, vec!["exist?", "exist?"]);
}
```

- [ ] Implement `Lint/DeprecatedClassMethods`.

Start with a safe v1 table:

```rust
const DEPRECATED: &[(&str, &str, &str)] = &[
    ("File", "exists?", "exist?"),
    ("Dir", "exists?", "exist?"),
];
```

If adding `iterator?`, `attr`, `ENV.dup`, or socket methods requires argument-sensitive transforms, file beads follow-ups and keep v1 to the safe table above.

- [ ] Register, test, and commit.

Run: `cargo test -p murphy-core cops::lint -- --nocapture`

Run: `git add crates/murphy-core/src/cops crates/murphy-core/src/registry.rs && git commit -m "feat: add call-pattern lint cops"`

### Task 6: Literal Style Cops (`murphy-7rg.3` subset)

**Files:**
- Create: `crates/murphy-core/src/cops/style/frozen_string_literal_comment.rs`
- Create: `crates/murphy-core/src/cops/style/string_literals.rs`
- Create: `crates/murphy-core/src/cops/style/symbol_array.rs`
- Create: `crates/murphy-core/src/cops/style/word_array.rs`
- Modify: `crates/murphy-core/src/cops/style.rs`
- Modify: `crates/murphy-core/src/registry.rs`

- [ ] Create beads subtasks for the four cops.

Expected: subtasks linked to `murphy-7rg.3`.

- [ ] Add failing tests for `Style/FrozenStringLiteralComment`.

Cases:

```rust
#[test]
fn inserts_frozen_string_literal_comment_at_file_start() {
    let offenses = run_single_cop(Box::new(FrozenStringLiteralComment), "puts 'x'\n");
    let edit = &offenses[0].autocorrect.as_ref().unwrap().edits[0];
    assert_eq!(edit.range.start_offset, 0);
    assert_eq!(edit.replacement, "# frozen_string_literal: true\n\n");
}
```

Also test shebang preservation: `#!/usr/bin/env ruby\nputs 'x'\n` inserts after the shebang line.

- [ ] Implement `Style/FrozenStringLiteralComment` as an `inspect_file` cop.

Detect existing `# frozen_string_literal: true` or `false` in the first comment block and do not duplicate it. Autocorrect inserts the true comment at byte offset 0 or just after shebang.

- [ ] Add failing tests for `Style/StringLiterals`.

Cases:

```rust
#[test]
fn converts_simple_double_quoted_string_to_single_quotes() {
    let offenses = run_single_cop(Box::new(StringLiterals), "x = \"abc\"\n");
    assert_eq!(offenses[0].autocorrect.as_ref().unwrap().edits[0].replacement, "'abc'");
}

#[test]
fn leaves_interpolated_strings_alone() {
    let offenses = run_single_cop(Box::new(StringLiterals), "x = \"#{name}\"\n");
    assert!(offenses.is_empty());
}
```

- [ ] Implement `Style/StringLiterals`.

Use source byte scanning around prism string nodes if node-specific literal quote access is available. If prism binding lacks enough literal metadata, file a beads token-infrastructure follow-up and implement no behavior for this cop until that dependency is solved.

- [ ] Add and implement `Style/SymbolArray` and `Style/WordArray`.

Start with simple literal arrays where every item is a static symbol or static single-word string. Autocorrect to `%i[...]` or `%w[...]` only when no item needs escaping.

- [ ] Run and commit.

Run: `cargo test -p murphy-core cops::style -- --nocapture`

Run: `cargo test -p murphy-core --test autocorrect_idempotency -- --nocapture`

Run: `git add crates/murphy-core/src/cops crates/murphy-core/src/registry.rs crates/murphy-core/tests/autocorrect_idempotency.rs && git commit -m "feat: add literal style cops"`

### Task 7: Control-Flow and Predicate Style Cops (`murphy-7rg.3` subset)

**Files:**
- Create: `crates/murphy-core/src/cops/style/redundant_return.rs`
- Create: `crates/murphy-core/src/cops/style/nil_comparison.rs`
- Create: `crates/murphy-core/src/cops/style/if_unless_modifier.rs`
- Create: `crates/murphy-core/src/cops/style/and_or.rs`
- Create: `crates/murphy-core/src/cops/lint/empty_when.rs`
- Create: `crates/murphy-core/src/cops/lint/unreachable_code.rs`
- Modify: `crates/murphy-core/src/cop.rs`
- Modify: `crates/murphy-core/src/registry.rs`

- [ ] Create beads subtasks for the six cops.

Expected: subtasks linked to `murphy-7rg.3`.

- [ ] Extend visitor hooks with tests.

Add only the hooks needed by these cops, for example:

```rust
fn on_if_node(&self, _node: &ruby_prism::IfNode<'_>, _ctx: &CopContext<'_>, _sink: &mut Vec<Offense>) {}
fn on_return_node(&self, _node: &ruby_prism::ReturnNode<'_>, _ctx: &CopContext<'_>, _sink: &mut Vec<Offense>) {}
fn on_case_node(&self, _node: &ruby_prism::CaseNode<'_>, _ctx: &CopContext<'_>, _sink: &mut Vec<Offense>) {}
```

Run: `cargo test -p murphy-core cop::tests -- --nocapture`

Expected: visitor fan-out tests still pass.

- [ ] Implement `Style/NilComparison` first.

Test cases: `x == nil` -> `x.nil?`; `x != nil` -> `!x.nil?`. Do not autocorrect if the left expression has ambiguous source range; file a beads follow-up if source slicing is not available.

- [ ] Implement `Style/AndOr` in conditional contexts.

Test cases: `if a and b\nend\n` -> offense with replacement `&&`; `foo and return` -> no offense for default conditional-only v1 behavior.

- [ ] Implement `Style/RedundantReturn` for final method body returns.

Test cases: `def x\n  return 1\nend\n` autocorrects to `def x\n  1\nend\n`; `def x\n  return 1 if cond\nend\n` is left alone.

- [ ] Implement `Style/IfUnlessModifier` for single-line bodies.

Test cases: `if ok\n  run\nend\n` -> `run if ok\n` only when condition and body are each single-line and contain no comments.

- [ ] Implement `Lint/EmptyWhen` and `Lint/UnreachableCode` without autocorrect first.

For `EmptyWhen`, flag `when a` with no statements. For `UnreachableCode`, start with statements after `return`, `break`, `next`, or `raise` in the same body.

- [ ] Run and commit.

Run: `cargo test -p murphy-core cops::style cops::lint cop::tests -- --nocapture`

Run: `git add crates/murphy-core/src/cop.rs crates/murphy-core/src/cops crates/murphy-core/src/registry.rs && git commit -m "feat: add control-flow style and lint cops"`

### Task 8: Scope-Aware Lint Cops (`murphy-7rg.3` subset)

**Files:**
- Create: `crates/murphy-core/src/cops/lint/unused_method_argument.rs`
- Create: `crates/murphy-core/src/cops/lint/useless_assignment.rs`
- Create: `crates/murphy-core/src/cops/lint/duplicate_hash_key.rs`
- Create: `crates/murphy-core/src/cops/style/hash_syntax.rs`
- Create: `crates/murphy-core/src/cops/style/redundant_self.rs`
- Modify: `crates/murphy-core/src/cop.rs`
- Modify: `crates/murphy-core/src/cops/support.rs`
- Modify: `crates/murphy-core/src/registry.rs`

- [ ] Create beads subtasks for the five cops and any scope-analysis support issue.

Expected: subtasks linked to `murphy-7rg.3`; if scope analysis is larger than one cop, create a support issue that blocks `UnusedMethodArgument` and `UselessAssignment`.

- [ ] Add failing tests for `Lint/DuplicateHashKey`.

Test cases: `{ a: 1, a: 2 }` flags the second key; `{ a: 1, b: 2 }` is clean. Start with literal symbol/string/integer keys only.

- [ ] Implement `Lint/DuplicateHashKey`.

Use hash-node visitor support. If `ruby-prism` exposes assoc/key nodes differently than expected, add the minimal visitor hook and keep the cop to static literal keys.

- [ ] Add failing tests for `Style/HashSyntax`.

Test cases: `{ :a => 1 }` autocorrects to `{ a: 1 }`; `{ "a" => 1 }` remains clean for v1.

- [ ] Implement `Style/HashSyntax`.

Only autocorrect symbol keys that are valid bare labels.

- [ ] Add failing tests for `Style/RedundantSelf`.

Test cases: `self.foo` in a method autocorrects to `foo`; `self.foo = 1` and `self.class` remain clean.

- [ ] Implement `Style/RedundantSelf` safe subset.

Remove `self.` only for method calls that are not setters, not keyword-sensitive, and not known semantic exceptions. File beads follow-ups for skipped RuboCop behavior.

- [ ] Add failing tests for `Lint/UnusedMethodArgument`.

Test cases: `def x(a)\n  1\nend\n` flags `a` and autocorrects to `_a`; `def x(a)\n  a\nend\n` is clean.

- [ ] Add failing tests for `Lint/UselessAssignment`.

Test cases: `x = 1\nputs 2\n` flags `x`; `x = 1\nputs x\n` is clean.

- [ ] Implement minimal lexical scope analysis.

Support method-local scopes first. Track local variable writes and reads by byte range and name. Do not cross method/class/module boundaries. If block parameter shadowing is not straightforward, file a beads follow-up and skip those cases in v1 with explicit tests.

- [ ] Run and commit.

Run: `cargo test -p murphy-core cops::lint cops::style -- --nocapture`

Run: `cargo test -p murphy-core --test autocorrect_idempotency -- --nocapture`

Run: `git add crates/murphy-core/src/cop.rs crates/murphy-core/src/cops crates/murphy-core/src/registry.rs crates/murphy-core/tests/autocorrect_idempotency.rs && git commit -m "feat: add scope-aware standard cops"`

### Task 9: Remaining Layout Operator Cops (`murphy-7rg.3` subset)

**Files:**
- Create: `crates/murphy-core/src/cops/layout/space_around_operators.rs`
- Create: `crates/murphy-core/src/cops/layout/dot_position.rs`
- Modify: `crates/murphy-core/src/cops/layout.rs`
- Modify: `crates/murphy-core/src/registry.rs`

- [ ] Create beads subtasks for `Layout/SpaceAroundOperators` and `Layout/DotPosition`.

Expected: subtasks linked to `murphy-7rg.3`.

- [ ] Add failing tests for `Layout/SpaceAroundOperators`.

Test cases: `x=1+2\n` corrects to `x = 1 + 2\n`. Do not alter unary `-x`, `!x`, keyword arguments, or hash labels.

- [ ] Implement `Layout/SpaceAroundOperators` with a safe token-lite scanner.

Use byte scanning plus a conservative operator set: `=`, `+`, `-`, `*`, `/`, `%`, `==`, `!=`, `&&`, `||`, `<`, `>`, `<=`, `>=`. If scanner confidence is low for an occurrence, skip it and file a beads follow-up if the gap is important.

- [ ] Add failing tests for `Layout/DotPosition`.

Test case: `foo.\n  bar\n` corrects to `foo\n  .bar\n`; `foo\n  .bar\n` is clean.

- [ ] Implement `Layout/DotPosition`.

Use source byte scanning for a `.` immediately before a newline followed by indentation and an identifier. Move the dot to the first non-space byte of the next line.

- [ ] Run and commit.

Run: `cargo test -p murphy-core cops::layout -- --nocapture`

Run: `cargo test -p murphy-core --test autocorrect_idempotency -- --nocapture`

Run: `git add crates/murphy-core/src/cops crates/murphy-core/src/registry.rs crates/murphy-core/tests/autocorrect_idempotency.rs && git commit -m "feat: add operator layout cops"`

### Task 10: Standard Cop Integration Snapshots (`murphy-7rg.3` completion)

**Files:**
- Add: `crates/murphy-cli/tests/fixtures/phase6_project/*.rb`
- Add/modify: `crates/murphy-cli/tests/integration_snapshot.rs`
- Add: `crates/murphy-cli/tests/snapshots/phase6_project.json`
- Modify: `README.md`

- [ ] Add a Phase 6 fixture project.

Create files that exercise multiple cops without touching the historical sample-project fixture:

```ruby
# crates/murphy-cli/tests/fixtures/phase6_project/mixed.rb
puts "debug"  
File.exists?(path)
if value == nil
  return true
end
```

- [ ] Add failing integration snapshot test.

Add a CLI test that runs `murphy lint fixtures/phase6_project` and compares JSON to a checked-in snapshot.

- [ ] Generate and inspect expected snapshot.

Run: `cargo test -p murphy-cli --test integration_snapshot phase6_project_snapshot -- --nocapture`

Expected: initial failure prints actual JSON. Inspect ordering: it must follow ADR 0007 aggregate order.

- [ ] Check in the snapshot only after inspecting all offenses.

Expected snapshot includes offenses from multiple cop namespaces and any autocorrect payloads that should be emitted in lint mode.

- [ ] Update README standard cop list.

Document the ADR 0018 list and explicitly state that Murphy is still not full RuboCop compatibility.

- [ ] Close `murphy-7rg.3`.

Run: `cargo test -p murphy-cli --test integration_snapshot -- --nocapture`

Run: `cargo test -p murphy-core cops -- --nocapture`

Run: `git add crates/murphy-cli/tests crates/murphy-core/src README.md && git commit -m "feat: integrate phase 6 standard cops"`

Run: `bd close murphy-7rg.3 --reason "ADR 0018 standard cop suite implemented with integration snapshots." --json`

### Task 11: Hyperfine Perf Regression Gate (`murphy-7rg.4`)

**Files:**
- Create: `scripts/perf/phase6_hyperfine.sh`
- Add: `.github/workflows/phase6-perf.yml` if CI secrets/tooling allow it
- Modify: `README.md`

- [ ] Claim the issue.

Run: `bd update murphy-7rg.4 --claim --json`

Expected: JSON shows `status` as `in_progress`.

- [ ] Add a local perf script.

Create `scripts/perf/phase6_hyperfine.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

if ! command -v hyperfine >/dev/null 2>&1; then
  echo "hyperfine is required" >&2
  exit 2
fi

if ! command -v rubocop >/dev/null 2>&1; then
  echo "rubocop is required" >&2
  exit 2
fi

cargo build --release

corpus_dir=${1:-crates/murphy-cli/tests/fixtures/phase6_project}
tmp_dir=$(mktemp -d)
trap 'rm -rf "$tmp_dir"' EXIT

for n in 1 20 100; do
  run_dir="$tmp_dir/n$n"
  mkdir -p "$run_dir"
  for i in $(seq 1 "$n"); do
    cp -R "$corpus_dir" "$run_dir/project_$i"
  done

  hyperfine \
    --warmup 2 \
    --export-json "$tmp_dir/phase6-n$n.json" \
    "./target/release/murphy lint $run_dir" \
    "rubocop --format json $run_dir"
done

echo "perf results written under $tmp_dir"
```

- [ ] Make it executable.

Run: `chmod +x scripts/perf/phase6_hyperfine.sh`

- [ ] Run locally if tools are installed.

Run: `scripts/perf/phase6_hyperfine.sh`

Expected: exits 0 and prints hyperfine summaries. If `hyperfine` or `rubocop` is absent, record that in beads notes and keep the script tested by shell syntax only.

- [ ] Add CI wiring or file follow-up.

If GitHub Actions is in use, add `.github/workflows/phase6-perf.yml` that installs Rust, Ruby, RuboCop, and hyperfine, then runs the script on push/PR. If CI setup is intentionally deferred, run `bd create` for the CI follow-up and link it from `murphy-7rg.4`.

- [ ] Commit and close.

Run: `git add scripts/perf/phase6_hyperfine.sh .github/workflows README.md && git commit -m "ci: add phase 6 perf gate"`

Run: `bd close murphy-7rg.4 --reason "Added Phase 6 hyperfine perf gate at N=1/20/100 vs RuboCop." --json`

### Task 12: Diff Quality Watch (`murphy-7rg.5`)

**Files:**
- Create: `scripts/diff/phase6_rubocop_diff.sh`
- Add: `docs/phase6-diff-watch.md`

- [ ] Claim the issue.

Run: `bd update murphy-7rg.5 --claim --json`

Expected: JSON shows `status` as `in_progress`.

- [ ] Add a diff-watch script.

Create `scripts/diff/phase6_rubocop_diff.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

if ! command -v rubocop >/dev/null 2>&1; then
  echo "rubocop is required" >&2
  exit 2
fi

cargo build --release

source_dir=${1:-crates/murphy-cli/tests/fixtures/phase6_project}
work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT

cp -R "$source_dir" "$work_dir/murphy"
cp -R "$source_dir" "$work_dir/rubocop"

./target/release/murphy lint --fix "$work_dir/murphy" >/tmp/murphy-phase6-fix.json
rubocop -a "$work_dir/rubocop" >/tmp/rubocop-phase6-fix.txt || true

diff -ru "$work_dir/rubocop" "$work_dir/murphy" || true
```

- [ ] Make it executable and run it if RuboCop is installed.

Run: `chmod +x scripts/diff/phase6_rubocop_diff.sh`

Run: `scripts/diff/phase6_rubocop_diff.sh`

Expected: exits 0; diff output is informational, not a hard failure in v1.

- [ ] Document watch semantics.

Create `docs/phase6-diff-watch.md` explaining that this is a quality watch, not a compatibility promise. Mention that gaps become beads issues when they represent intended ADR 0018 behavior.

- [ ] Commit and close.

Run: `git add scripts/diff/phase6_rubocop_diff.sh docs/phase6-diff-watch.md && git commit -m "test: add phase 6 rubocop diff watch"`

Run: `bd close murphy-7rg.5 --reason "Added diff-quality watch against rubocop -a." --json`

### Task 13: Docs and Phase 6 Gate ADR (`murphy-7rg.6`)

**Files:**
- Modify: `README.md`
- Modify: `CLAUDE.md`
- Add: `docs/decisions/0020-phase-6-gate-review.md`

- [ ] Claim the issue.

Run: `bd update murphy-7rg.6 --claim --json`

Expected: JSON shows `status` as `in_progress`.

- [ ] Update docs.

README must state:

```markdown
**Phase 6 — v1 standard cops + perf gates (complete).** Murphy ships the ADR 0018 built-in cop set across `Murphy`, `Lint`, `Style`, and limited `Layout`, with autocorrect where transformations are deterministic and idempotent.
```

Also list limitations: no full RuboCop compatibility, no configurable style options beyond enabled/severity, and layout scope remains limited.

- [ ] Write Phase 6 gate ADR.

Create `docs/decisions/0020-phase-6-gate-review.md` with sections:

```markdown
# ADR 0020 — Phase 6 Gate review (standard cops + perf gates complete)

- Date: 2026-05-20
- Status: Accepted — **GATE PASSED**
- Epic: `murphy-7rg`
- Preserves: ADR 0006/0007/0013 JSON contract and deterministic aggregation
- Adds: ADR 0018 standard cop scope; ADR 0019 native primitive IDL

## Verdict

## Completed scope

## Verification

## Deferred
```

Use actual verification output from the next step; do not write speculative pass text before running commands.

- [ ] Run full quality gates.

Run: `cargo fmt --check`

Run: `cargo clippy --workspace --all-targets -- -D warnings`

Run: `cargo test`

Run: `scripts/perf/phase6_hyperfine.sh` if required tools are installed; otherwise include the exact missing-tool note in the gate ADR and linked beads issue.

Run: `scripts/diff/phase6_rubocop_diff.sh` if RuboCop is installed; otherwise include the exact missing-tool note in the gate ADR and linked beads issue.

Expected: Rust gates pass. Perf/diff gates either run or have tracked environment follow-ups.

- [ ] Close Phase 6 issues.

Run: `bd close murphy-7rg.6 --reason "Phase 6 gate ADR written and verification recorded." --json`

Run: `bd close murphy-7rg --reason "Phase 6 standard cops, IDL hardening, perf gate, diff watch, and gate ADR complete." --json`

- [ ] Commit and push.

Run: `git add README.md CLAUDE.md docs/decisions/0020-phase-6-gate-review.md && git commit -m "docs: record phase 6 gate"`

Run: `git pull --rebase && bd dolt push && git push && git status`

Expected: git status says the branch is up to date with origin and working tree is clean.

## Self-Review

- Spec coverage: ADR 0018 cop scope is covered by Tasks 4-10; IDL hardening is Task 1; native suite hygiene is Task 2; perf regression CI is Task 11; diff-quality watch is Task 12; docs and gate ADR are Task 13.
- Red-flag scan: no incomplete markers or open-ended implementation gaps remain. Where implementation may expose missing parser/token/scope support, the plan gives a concrete first slice and requires a beads follow-up.
- Type consistency: all standard offense construction uses existing `Offense`, `Range`, `Edit`, `Autocorrect`, `Severity`, `Cop`, `CopContext`, `run_cops`, and `CopRegistry` names. New helper names are consistently `line_ranges`, `replace_edit`, `offense_with_edit`, and `run_single_cop`.
- Scope check: Phase 6 includes several subsystems, but they are already represented as separate beads children and the plan preserves that split. `murphy-7rg.3` is further split into per-cop subtasks during execution.
