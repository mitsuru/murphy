# mruby Node Field Arena Primitives Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the old prism-call-only mruby primitive surface with arena AST read primitives for `murphy-9cr.24.2`.

**Architecture:** `crates/murphy-core/src/mruby/primitives.rs` will expose read-only `Murphy.node_*` functions backed by the owned `murphy_ast::Ast` arena instead of re-walking prism `CallNode`s. `crates/murphy-core/src/mruby/cop_prelude.rb` will keep Ruby user cops thin by wrapping integer node ids in `Murphy::Node` and delegating field access to the native primitives.

**Tech Stack:** Rust, `mruby3-sys`, `murphy_ast::{Ast, NodeId, NodeKind}`, `murphy-plugin-api` surface conventions, Ruby prelude loaded through mruby.

---

## File Map

- Modify: `crates/murphy-core/src/mruby/primitives.rs`
  - Remove old live-prism `node_count`, `node_name`, `node_receiver_nil?`, `node_msg_start`, `node_msg_end`, and `source_slice` implementation.
  - Add `node_kind`, `node_parent`, `node_children`, `node_ancestors`, `node_descendants`, `node_range`, `node_field`, `symbol_str`, `string_str`, `raw_source`, and `comments`.
  - Add small mruby conversion helpers for integers, booleans, nil, strings, symbols, arrays, and argument extraction.
- Modify: `crates/murphy-core/src/mruby/cop_prelude.rb`
  - Update `Murphy::Node` to call new primitive names.
  - Keep `Murphy::Cop#__emit_offense` wire format unchanged.
- Modify: `crates/murphy-core/tests/cop_no_puts_mruby.rs`
  - Update fixture cops from old call-node APIs to `Node#kind` and `Node#field` APIs.
  - Preserve existing offense range and severity assertions.
- Optionally modify: `crates/murphy-core/src/mruby/state.rs`
  - Only if the current `mrb_state.ud` payload cannot expose the arena `Ast` needed by primitives. Do not use `ud` as a liveness mechanism.

## Task 1: Baseline And Failing API Test

**Files:**
- Modify: `crates/murphy-core/tests/cop_no_puts_mruby.rs`

- [ ] **Step 1: Write the failing Ruby-cop test path**

Update the no-receiver puts cop fixture in `cop_no_puts_mruby.rs` so it uses the new wrapper surface:

```ruby
class NoReceiverPuts < Murphy::Cop
  def on_send(node)
    return unless node.kind == :send
    return unless node.field(:method) == :puts
    return unless node.field(:receiver).nil?

    add_offense(node, message: 'Use logger instead of puts')
  end
end
```

Keep the existing Rust assertions for `puts 1\n`, `obj.puts\n`, severity, and selector/message range. This should fail before implementation because `Murphy.node_kind` and `Murphy.node_field` are not registered.

- [ ] **Step 2: Run the targeted test and verify failure**

Run:

```bash
CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo test -p murphy-core --features mruby-user-cops --test cop_no_puts_mruby
```

Expected: FAIL with an mruby error mentioning an undefined `Murphy.node_kind`, `Murphy.node_field`, or missing `Murphy::Node` wrapper method.

- [ ] **Step 3: Record the failure in beads**

Run:

```bash
bd update murphy-9cr.24.2 --notes "Baseline failing test confirmed: cop_no_puts_mruby now expects node_kind/node_field arena primitives."
```

Expected: issue notes update succeeds.

## Task 2: Primitive Registration Skeleton

**Files:**
- Modify: `crates/murphy-core/src/mruby/primitives.rs`

- [ ] **Step 1: Add registered function stubs**

Replace the old `register` entries with the new names. Each native function may initially return `nil` or an empty array to make registration visible before behavior is filled in:

```rust
mrb_define_module_function(mrb, murphy, c"node_kind".as_ptr(), native_node_kind, args_req(1));
mrb_define_module_function(mrb, murphy, c"node_parent".as_ptr(), native_node_parent, args_req(1));
mrb_define_module_function(mrb, murphy, c"node_children".as_ptr(), native_node_children, args_req(1));
mrb_define_module_function(mrb, murphy, c"node_ancestors".as_ptr(), native_node_ancestors, args_req(1));
mrb_define_module_function(mrb, murphy, c"node_descendants".as_ptr(), native_node_descendants, args_req(1));
mrb_define_module_function(mrb, murphy, c"node_range".as_ptr(), native_node_range, args_req(1));
mrb_define_module_function(mrb, murphy, c"node_field".as_ptr(), native_node_field, args_req(2));
mrb_define_module_function(mrb, murphy, c"symbol_str".as_ptr(), native_symbol_str, args_req(1));
mrb_define_module_function(mrb, murphy, c"string_str".as_ptr(), native_string_str, args_req(1));
mrb_define_module_function(mrb, murphy, c"raw_source".as_ptr(), native_raw_source, args_req(2));
mrb_define_module_function(mrb, murphy, c"comments".as_ptr(), native_comments, args_req(0));
```

Remove registration of the old primitive names unless another in-tree test still depends on them. If such a dependency appears, update that test rather than keeping compatibility aliases.

- [ ] **Step 2: Run the targeted test and verify the failure moved**

Run:

```bash
CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo test -p murphy-core --features mruby-user-cops --test cop_no_puts_mruby
```

Expected: failure no longer says primitive is undefined; it should fail because stubbed return values do not detect `puts`.

## Task 3: Arena Context Access And Basic Node Traversal

**Files:**
- Modify: `crates/murphy-core/src/mruby/primitives.rs`
- Optionally modify: `crates/murphy-core/src/mruby/state.rs`

- [ ] **Step 1: Implement arena access from the existing cop run context**

Add a helper near the existing `ctx(mrb)` equivalent that projects the arena AST owned by the current cop run. Use the existing `CopRun`/`AstContext` ownership; do not store borrowed data in `mrb_state.ud`.

The helper should have the shape:

```rust
unsafe fn ast_ctx<'a>(mrb: *mut mrb_state) -> &'a AstContext {
    let ud = unsafe { (*mrb).ud } as *const crate::mruby::sdk::CopRun;
    assert!(!ud.is_null(), "mrb_state.ud must hold the CopRun pointer");
    let run: &crate::mruby::sdk::CopRun = unsafe { &*ud };
    run.ctx()
}
```

Then use the arena accessor already available on `AstContext`. If the accessor does not exist, add the smallest read-only method required, for example `AstContext::ast(&self) -> &murphy_ast::Ast`.

- [ ] **Step 2: Implement `node_parent`, `node_children`, `node_ancestors`, `node_descendants`, and `node_range`**

Use `Ast::parent`, `Ast::children`, `Ast::ancestors`, `Ast::descendants`, and `Ast::range`. Return `nil` for invalid ids where possible instead of panicking. Return arrays of integer node ids for node-list APIs.

- [ ] **Step 3: Add focused unit tests for traversal primitives**

Add tests in `primitives.rs #[cfg(test)]` or an integration test that parse a small source such as:

```ruby
if cond
  puts 1
else
  puts 2
end
```

Assert that root has descendants, child ids round-trip through `parent`, and `node_range` returns `[start, end]` byte offsets.

- [ ] **Step 4: Run traversal tests**

Run:

```bash
CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo test -p murphy-core --features mruby-user-cops mruby::primitives
```

Expected: traversal tests pass; `cop_no_puts_mruby` may still fail until `node_kind` and `node_field` are complete.

## Task 4: Node Kind Symbols

**Files:**
- Modify: `crates/murphy-core/src/mruby/primitives.rs`

- [ ] **Step 1: Add exhaustive `NodeKind` to symbol-name mapping**

Implement a pure Rust helper:

```rust
fn node_kind_symbol_name(kind: &murphy_ast::NodeKind) -> &'static CStr {
    match kind {
        NodeKind::Error => c"error",
        NodeKind::Nil => c"nil",
        NodeKind::True_ => c"true",
        NodeKind::False_ => c"false",
        NodeKind::SelfExpr => c"self",
        NodeKind::Int(_) => c"int",
        NodeKind::Float(_) => c"float",
        NodeKind::Str(_) => c"str",
        NodeKind::Sym(_) => c"sym",
        NodeKind::Send { .. } => c"send",
        NodeKind::Csend { .. } => c"csend",
        NodeKind::RangeExpr { .. } => c"irange",
        NodeKind::Defined(_) => c"defined?",
        // Continue exhaustively for every NodeKind variant.
    }
}
```

Use an exhaustive match with no wildcard so new AST variants fail compilation.

- [ ] **Step 2: Return Ruby symbols from `native_node_kind`**

Use mruby symbol interning (`mrb_intern_cstr`/available binding) or a Ruby literal fallback to return `:send`, `:int`, etc. Prefer direct symbol APIs if present in `mruby3_sys`; use `mrb_load_string` only as a fallback if bindings lack value boxing.

- [ ] **Step 3: Add `kind_symbol_mapping` test**

Create enough AST nodes through `AstBuilder` or parser fixtures to cover every `NodeKind` variant present in `murphy_ast::NodeKind`. At minimum, add a pure Rust unit test for `node_kind_symbol_name` that constructs one variant of each kind and asserts the expected string.

- [ ] **Step 4: Run kind tests**

Run:

```bash
CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo test -p murphy-core --features mruby-user-cops kind_symbol_mapping
```

Expected: PASS.

## Task 5: `node_field` Minimal Send Path

**Files:**
- Modify: `crates/murphy-core/src/mruby/primitives.rs`
- Modify: `crates/murphy-core/src/mruby/cop_prelude.rb`

- [ ] **Step 1: Implement field argument extraction**

Read `(id, field_sym)` from mruby using the existing `mrb_get_args` pattern. Convert field symbols to a comparable value through `mrb_sym` or by converting to a string if `mrb_sym` helpers are unavailable.

- [ ] **Step 2: Implement `Send` and `Csend` fields**

For `NodeKind::Send` and `NodeKind::Csend`, support:

```text
:receiver  -> Integer node id or nil
:method    -> Ruby Symbol
:arguments -> Array<Integer>
```

If the current `NodeKind` does not yet carry `block`, do not invent one; return `nil` for `:block`.

- [ ] **Step 3: Update `Murphy::Node` wrapper**

In `cop_prelude.rb`, implement:

```ruby
class Node
  attr_reader :id

  def initialize(id)
    @id = id
  end

  def kind
    Murphy.node_kind(@id)
  end

  def field(name)
    value = Murphy.node_field(@id, name)
    self.class.wrap(value)
  end

  def self.wrap(value)
    case value
    when Integer then new(value)
    when Array then value.map { |v| wrap(v) }
    else value
    end
  end
end
```

Keep existing offense/fix methods intact.

- [ ] **Step 4: Run no-puts test**

Run:

```bash
CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo test -p murphy-core --features mruby-user-cops --test cop_no_puts_mruby
```

Expected: tests for bare `puts` and `obj.puts` pass, including message/range and severity assertions.

## Task 6: Complete `node_field` Payload Coverage

**Files:**
- Modify: `crates/murphy-core/src/mruby/primitives.rs`

- [ ] **Step 1: Add field cases from the beads design table**

Implement the remaining field mappings for literals, variables, constants, assignments, blocks, containers, control flow, definitions, rescue/ensure, dynamic strings, regexps, and multiple assignment.

Use the exact return rules from the issue:

```text
NodeId payload        -> Integer
OptNodeId::NONE      -> nil
NodeList             -> Array<Integer>
Symbol/String handle -> Ruby Symbol/String when inline, or handle only where the AST stores an interned id that must be decoded by symbol_str/string_str
Bool                 -> true/false
Unsupported          -> nil
```

- [ ] **Step 2: Add unsupported-field tests**

Assert examples such as `Int.receiver`, `Send.value`, and `Nil.value` return `nil`.

- [ ] **Step 3: Add representative supported-field tests**

Use parser fixtures or `AstBuilder` to test at least one field in each family:

```text
literal: Int.value, Str.value, Sym.value
variable: Lvar.name
assignment: Lvasgn.name/value
container: Array.elements, Hash.pairs, Pair.key/value
control flow: If.condition/then/else, While.condition/body/post
definition: Def.name/parameters/body
rescue: Rescue.body/rescues/else
range: RangeExpr.start/end/exclusive
```

- [ ] **Step 4: Run primitive tests**

Run:

```bash
CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo test -p murphy-core --features mruby-user-cops mruby::primitives
```

Expected: PASS.

## Task 7: Source, Interner, And Comments Primitives

**Files:**
- Modify: `crates/murphy-core/src/mruby/primitives.rs`

- [ ] **Step 1: Implement `raw_source(start, end)`**

Return a Ruby string for valid byte ranges. Return `nil` for negative, inverted, out-of-bounds, or non-UTF-8 boundary ranges that would panic Rust slicing.

- [ ] **Step 2: Implement `symbol_str(handle)` and `string_str(handle)`**

Use `Ast::interner()` APIs. If no public lookup exists, add the smallest method to `murphy_ast::Interner` required to return `&str` by handle. Raise `ArgumentError` for invalid handles.

- [ ] **Step 3: Implement `comments()`**

Return an array of `[start, end, text]` triples in source order, using `Ast::comments()` and `Ast::raw_source(comment.range)` where appropriate.

- [ ] **Step 4: Run source/interner/comment tests**

Run:

```bash
CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo test -p murphy-core --features mruby-user-cops raw_source symbol_str string_str comments
```

Expected: PASS.

## Task 8: Full Verification And Cleanup

**Files:**
- Modify only files changed by previous tasks.

- [ ] **Step 1: Format**

Run:

```bash
cargo fmt --check
```

Expected: PASS. If it fails, run `cargo fmt`, then rerun `cargo fmt --check`.

- [ ] **Step 2: Run feature tests**

Run:

```bash
CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo test -p murphy-core --features mruby-user-cops
```

Expected: PASS.

- [ ] **Step 3: Run clippy**

Run:

```bash
CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo clippy --workspace --all-targets --features mruby-user-cops -- -D warnings
```

Expected: PASS.

- [ ] **Step 4: Update beads and commit**

Run:

```bash
bd update murphy-9cr.24.2 --notes "Implemented arena-backed mruby node primitives and Node prelude wrapper; verification commands passed."
bd close murphy-9cr.24.2 --reason "Arena-backed mruby node primitive surface implemented and verified."
bd dolt pull
git status --short
git add crates/murphy-core/src/mruby/primitives.rs crates/murphy-core/src/mruby/cop_prelude.rb crates/murphy-core/tests/cop_no_puts_mruby.rs docs/superpowers/plans/2026-05-24-mruby-node-field-arena-primitives.md .beads/issues.jsonl
git commit -m "feat: add mruby arena node primitives"
```

Expected: commit succeeds. This worktree is ephemeral and has no upstream; follow the project-specific merge handoff instead of pushing unless explicitly requested.

## Self-Review

- Spec coverage: covers all 11 primitives, prelude wrapper, no-puts integration, field payload expansion, source/interner/comments, verification, and beads lifecycle.
- Placeholder scan: no `TBD`/`TODO` placeholders remain; Task 6 intentionally references the existing beads design table to avoid duplicating the entire field matrix while still listing required families and return rules.
- Type consistency: primitive names match `murphy-9cr.24.2` acceptance criteria; Ruby wrapper methods call the same native names registered in Task 2.
