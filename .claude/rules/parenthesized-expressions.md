# Unwrap parenthesized expressions before checking node kind

When a node is wrapped in parentheses, its AST node is `NodeKind::Begin(list)`.
Without unwrapping, `cx.kind(node)` returns `Begin`, missing the real node type
inside and causing false negatives (or false positives when `Begin` is
misinterpreted).

## The two rules

### 1. Always unwrap before matching on node kind

Use `crate::cops::util::unwrap_parenthesized(node, cx)` which fully unwraps
arbitrarily nested parentheses with a `while` loop. Never unwrap only one
level — `((expr))` requires two levels.

```rust
// Good — fully unwrap nested parentheses with a loop
pub fn unwrap_parenthesized(mut node_id: NodeId, cx: &Cx<'_>) -> NodeId {
    while crate::cops::util::is_parenthesized(node_id, cx) {
        let NodeKind::Begin(list) = cx.kind(node_id) else { break; };
        match cx.list(*list) {
            [single] => node_id = *single,
            _ => break,
        }
    }
    node_id
}
```

Apply it before checking the effective node kind of receivers, arguments, or
default values:

```rust
// Good
fn is_acceptable_default(node: NodeId, cx: &Cx<'_>) -> bool {
    let node = crate::cops::util::unwrap_parenthesized(node, cx);
    match *cx.kind(node) {
        NodeKind::Nil => true,
        // ...
    }
}

// Good — unwrap receiver before checking its kind
let receiver_id = crate::cops::util::unwrap_parenthesized(receiver_id, cx);
if !is_shuffle_call(receiver_id, cx) {
    return;
}
```

### 2. Use `is_parenthesized` from `crate::cops::util` — never match on `NodeKind::Begin(_)` directly

`NodeKind::Begin` represents both parenthesized expressions AND `begin...end`
blocks. Matching on it directly conflates the two, causing:

- **False negatives:** parenthesized safe-assignments inside `begin...end` blocks
  are incorrectly treated as parenthesized and skipped.
- **False positives:** `begin...end` blocks are incorrectly treated as
  parenthesized expressions.

```rust
// Good — use the util helper
fn is_safe_assignment(cx: &Cx<'_>, id: NodeId) -> bool {
    let Some(parent_id) = cx.parent(id).get() else { return false; };
    crate::cops::util::is_parenthesized(parent_id, cx)
}

// Avoid — matches begin...end blocks too
matches!(*cx.kind(parent_id), NodeKind::Begin(_))
```

## When traversing parenthesized receivers in chains

If an intermediate `fetch` / send call in a chain is parenthesized,
`cx.call_receiver` returns the `Begin` node. Attempting to get arguments or
method name on that `Begin` node breaks the chain. Unwrap at each step:

```rust
let mut current = node;
while let Some(recv) = cx.call_receiver(current).get() {
    let unwrapped_recv = crate::cops::util::unwrap_parenthesized(recv, cx);
    if !is_diggable_fetch(unwrapped_recv, cx) {
        break;
    }
    let recv_args = cx.call_arguments(unwrapped_recv);
    keys.insert(0, recv_args.first().copied()?);
    current = unwrapped_recv;
}
```

## See also

- `crates/murphy-std/src/cops/util.rs` — `unwrap_parenthesized` and
  `is_parenthesized`.
- `crates/murphy-std/src/cops/style/hash_fetch_chain.rs` — chains of fetch
  calls that each must unwrap parenthesized receivers.
- `crates/murphy-std/src/cops/style/sample.rs` — unwrapping receiver before
  checking shuffle call.
- `crates/murphy-std/src/cops/style/hash_slice.rs` — unwrapping vs. skipping
  parenthesized receivers.
