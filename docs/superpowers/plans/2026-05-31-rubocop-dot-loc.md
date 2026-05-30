# RuboCop Dot Loc Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `cx.loc(node).dot()` and `cx.is_dot(node)` use Prism's parser-provided call operator location instead of byte scanning.

**Architecture:** Add a sparse `CallOperatorLoc` side table parallel to the existing `CallClosingLoc` table. Translation records `CallNode::call_operator_loc()` for calls that have an explicit `.` or `&.`; plugin API lookups use binary search by `NodeId`, keeping `NodeLoc` compact and preserving RuboCop-like call-specific semantics.

**Tech Stack:** Rust workspace, `murphy-ast`, `murphy-translate`, `murphy-plugin-api`, Prism bindings, existing test-support harness.

---

## File Structure

- Modify `crates/murphy-ast/src/node.rs`: add `CallOperatorLoc { node, operator }` next to `CallClosingLoc`.
- Modify `crates/murphy-ast/src/builder.rs`: store, sort, and finish `call_operator_locs`.
- Modify `crates/murphy-ast/src/ast.rs`: add owned field and accessor plumbing.
- Modify `crates/murphy-ast/src/serialize.rs`: serialize/deserialize the new sparse table after `call_closing_locs`; bump `FORMAT_VERSION` by one.
- Modify `crates/murphy-ast/src/lib.rs`: re-export `CallOperatorLoc`.
- Modify `crates/murphy-translate/src/translate.rs`: record `call.call_operator_loc()` after call translation.
- Modify `crates/murphy-plugin-api/src/abi.rs`: add pointers/lengths for `call_operator_locs`; do not bump `MURPHY_PLUGIN_ABI_VERSION` without explicit user approval.
- Modify CxRaw construction sites: `crates/murphy-core/src/dispatch.rs`, `crates/murphy-plugin-api/src/internal.rs`, `crates/murphy-plugin-api/src/test_support.rs`, and macro tests that manually construct `CxRaw`.
- Modify `crates/murphy-plugin-api/src/cx.rs`: pass the side-table range into `LocRef`; make `LocRef::dot()` return it directly; keep `call_operator_loc()` as `Option<Range>` wrapper.

## Task 1: AST Side Table

**Files:**
- Modify: `crates/murphy-ast/src/node.rs`
- Modify: `crates/murphy-ast/src/builder.rs`
- Modify: `crates/murphy-ast/src/ast.rs`
- Modify: `crates/murphy-ast/src/lib.rs`

- [ ] **Step 1: Add the side-table record type**

In `crates/murphy-ast/src/node.rs`, add this after `CallClosingLoc`:

```rust
/// Parser-provided call operator for a call node (`.` or `&.`).
///
/// Stored out-of-line so [`NodeLoc`] stays compact and call nodes without an
/// explicit operator pay no per-node storage cost.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CallOperatorLoc {
    pub node: NodeId,
    pub operator: Range,
}
```

- [ ] **Step 2: Thread the field through `AstBuilder`**

In `crates/murphy-ast/src/builder.rs`, add `CallOperatorLoc` to the import list. Add a `call_operator_locs: Vec<CallOperatorLoc>` field, initialize it in `AstBuilder::new`, and add:

```rust
/// Record Prism's parser-provided `CallNode::call_operator_loc()` for a call.
pub fn add_call_operator_loc(&mut self, node: NodeId, operator: Range) {
    self.call_operator_locs
        .push(CallOperatorLoc { node, operator });
}
```

In `finish`, sort it next to `call_closing_locs`:

```rust
self.call_operator_locs
    .sort_unstable_by_key(|entry| entry.node.0);
```

Pass it to `Ast { call_operator_locs: self.call_operator_locs, ... }`.

- [ ] **Step 3: Thread the field through `Ast`**

In `crates/murphy-ast/src/ast.rs`, add `CallOperatorLoc` to imports. Add this field after `call_closing_locs`:

```rust
pub(crate) call_operator_locs: Vec<CallOperatorLoc>,
```

Add an accessor near `call_closing_locs()`:

```rust
/// Parser-provided call operator ranges, sorted by node id.
pub fn call_operator_locs(&self) -> &[CallOperatorLoc] {
    &self.call_operator_locs
}
```

Also add the corresponding borrowed field wherever the file defines the raw borrowed AST view used by serialization/ABI plumbing.

- [ ] **Step 4: Re-export the type**

In `crates/murphy-ast/src/lib.rs`, add `CallOperatorLoc` beside `CallClosingLoc` in the public re-export.

- [ ] **Step 5: Run focused AST compile check**

Run: `cargo check -p murphy-ast`

Expected: it may fail in serialization because the new field has not been encoded yet. No unrelated compile errors should appear.

## Task 2: Serialization

**Files:**
- Modify: `crates/murphy-ast/src/serialize.rs`

- [ ] **Step 1: Add serialization helpers**

In `crates/murphy-ast/src/serialize.rs`, import `CallOperatorLoc`. Add helpers parallel to `write_call_closing_loc` / `read_call_closing_loc`:

```rust
fn write_call_operator_loc(entry: CallOperatorLoc, out: &mut Vec<u8>) {
    write_node_id(entry.node, out);
    write_range(entry.operator, out);
}

fn read_call_operator_loc(cur: &mut &[u8]) -> Result<CallOperatorLoc, SerError> {
    Ok(CallOperatorLoc {
        node: read_node_id(cur)?,
        operator: read_range(cur)?,
    })
}
```

- [ ] **Step 2: Bump `FORMAT_VERSION`**

Change `pub const FORMAT_VERSION: u32 = 6;` to `7`.

- [ ] **Step 3: Encode the new table after call closing locs**

In the writer, after the `call_closing_locs` count and entries, write:

```rust
put_u64(&mut out, self.call_operator_locs.len() as u64);
for entry in &self.call_operator_locs {
    write_call_operator_loc(*entry, out);
}
```

- [ ] **Step 4: Decode the new table with backward tolerance for pre-v7 tails**

In the reader, after reading `call_closing_locs`, read `call_operator_locs` only if bytes remain:

```rust
let mut call_operator_locs = Vec::new();
if !cur.is_empty() {
    let count = read_u64(&mut cur)? as usize;
    call_operator_locs = Vec::with_capacity(count);
    for _ in 0..count {
        call_operator_locs.push(read_call_operator_loc(&mut cur)?);
    }
}
```

Pass `call_operator_locs` into `Ast`.

- [ ] **Step 5: Add round-trip tests**

Add tests next to `round_trip_call_closing_locs`:

```rust
#[test]
fn round_trip_call_operator_locs() {
    let mut b = AstBuilder::new("foo.bar", "test.rb");
    let method = b.intern_symbol("bar");
    let root = b.push_named(
        NodeKind::Send {
            receiver: OptNodeId::NONE,
            method,
            args: NodeList::EMPTY,
        },
        r(0, 7),
        r(4, 7),
    );
    b.add_call_operator_loc(root, r(3, 4));
    let ast = b.finish(root);

    let restored = Ast::from_bytes(&ast.to_bytes()).unwrap();
    assert_eq!(restored.call_operator_locs(), ast.call_operator_locs());
}
```

- [ ] **Step 6: Run AST tests**

Run: `cargo test -p murphy-ast`

Expected: PASS.

## Task 3: Translation

**Files:**
- Modify: `crates/murphy-translate/src/translate.rs`

- [ ] **Step 1: Record Prism call operator locs**

In `translate_call`, after the existing `closing_loc` recording, add:

```rust
if let Some(operator) = call.call_operator_loc() {
    self.builder.add_call_operator_loc(id, Self::range(&operator));
}
```

- [ ] **Step 2: Add translator test**

Add a test that translates `foo.bar` and `foo&.bar` separately and asserts each has one call operator loc with raw source `.` and `&.`. Also assert `foo + bar` and `bar` have no call operator locs.

- [ ] **Step 3: Run translate tests**

Run: `cargo test -p murphy-translate`

Expected: PASS.

## Task 4: Plugin ABI Plumbing

**Files:**
- Modify: `crates/murphy-plugin-api/src/abi.rs`
- Modify: `crates/murphy-core/src/dispatch.rs`
- Modify: `crates/murphy-plugin-api/src/internal.rs`
- Modify: `crates/murphy-plugin-api/src/test_support.rs`
- Modify: `crates/murphy-plugin-macros/tests/register_modes_equivalence.rs`
- Modify: `crates/murphy-plugin-macros/tests/cross_backend_conformance.rs`
- Modify: `crates/murphy-plugin-macros/tests/node_pattern_behavior.rs`
- Modify: `crates/murphy-plugin-macros/tests/cop_attr_behavior.rs`

- [ ] **Step 1: Extend `CxRaw`**

In `abi.rs`, import `CallOperatorLoc` and append fields after `call_closing_locs_len`:

```rust
/// Sparse parser-provided call operators for call nodes.
pub call_operator_locs: *const CallOperatorLoc,
pub call_operator_locs_len: usize,
```

Do not change `MURPHY_PLUGIN_ABI_VERSION` unless the user explicitly approves an ABI bump.

- [ ] **Step 2: Update `CxRaw` construction sites**

Wherever `CxRaw` is constructed from a parsed AST, add:

```rust
call_operator_locs: p.call_operator_locs.as_ptr(),
call_operator_locs_len: p.call_operator_locs.len(),
```

Where tests construct empty contexts, use:

```rust
call_operator_locs: std::ptr::null(),
call_operator_locs_len: 0,
```

- [ ] **Step 3: Update ABI layout test**

Adjust `offset_of!` assertions in `abi.rs` to include the two new fields at the end. Keep existing field order unchanged.

- [ ] **Step 4: Run plugin API/macro compile checks**

Run: `cargo check -p murphy-plugin-api && cargo test -p murphy-plugin-macros`

Expected: PASS after all construction sites are updated.

## Task 5: `LocRef::dot()` Semantics

**Files:**
- Modify: `crates/murphy-plugin-api/src/cx.rs`

- [ ] **Step 1: Add side-table accessor on `Cx`**

Add:

```rust
fn call_operator_locs(&self) -> &'a [CallOperatorLoc] {
    unsafe { slice(self.raw.call_operator_locs, self.raw.call_operator_locs_len) }
}
```

- [ ] **Step 2: Change `LocRef` fields**

Replace `receiver_end: Option<u32>` and `source: &'a [u8]` usage for `dot()` with:

```rust
call_operator: Range,
```

Keep `source` only if another `LocRef` method still needs it. If only `token_text()` needs it, keep `source` and remove `receiver_end`.

- [ ] **Step 3: Make `dot()` return the parser-provided range**

Replace the byte scan body with:

```rust
pub fn dot(&self) -> Range {
    self.call_operator
}
```

- [ ] **Step 4: Populate `call_operator` in `Cx::loc`**

In `Cx::loc`, look up the side table:

```rust
let call_operator = self
    .call_operator_locs()
    .binary_search_by_key(&id.0, |entry| entry.node.0)
    .map(|idx| self.call_operator_locs()[idx].operator)
    .unwrap_or(Range::ZERO);
```

Pass `call_operator` into `LocRef`.

- [ ] **Step 5: Keep `call_operator_loc()` as a compatibility wrapper**

Leave:

```rust
pub fn call_operator_loc(&self, id: NodeId) -> Option<Range> {
    let r = self.loc(id).dot();
    if r == Range::ZERO { None } else { Some(r) }
}
```

- [ ] **Step 6: Update tests that mention byte scanning**

Update comments and test names so they assert parser-provided behavior, not byte scan behavior. Keep tests for explicit dot, safe navigation, multiline chain, implicit send, operator method, bracket method, and non-call kinds.

- [ ] **Step 7: Run plugin API tests**

Run: `cargo test -p murphy-plugin-api`

Expected: PASS.

## Task 6: Workspace Verification

**Files:**
- No new files.

- [ ] **Step 1: Run focused tests**

Run: `cargo test -p murphy-ast && cargo test -p murphy-translate && cargo test -p murphy-plugin-api`

Expected: PASS.

- [ ] **Step 2: Run affected cop tests**

Run: `cargo test -p murphy-std dot_position`

Expected: PASS.

- [ ] **Step 3: Run formatting**

Run: `cargo fmt --check`

Expected: PASS.

- [ ] **Step 4: Run clippy**

Run: `cargo clippy --workspace --all-targets -- -D warnings`

Expected: PASS.

## Self-Review

- Spec coverage: the plan moves `loc.dot()` from byte scan to Prism parser-provided memory, preserves RuboCop-style call-site access, and avoids widening `NodeLoc`.
- Placeholder scan: no `TBD` / `TODO` / unspecified implementation steps remain.
- Type consistency: `CallOperatorLoc`, `call_operator_locs`, and `call_operator` naming is consistent across AST, ABI, translation, and `LocRef`.
