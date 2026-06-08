# Always resolve AST node properties through Cx helpers — never destructure NodeKind inline

Do NOT pattern-match on `NodeKind::Send { .. }`, `NodeKind::Def { .. }`,
`NodeKind::Block { call, .. }`, etc. directly. Use the centralized `Cx` helper
methods instead.

This is the single most frequent review finding across 170+ Gemini Code Assist
comments on merged PRs (#280–#328).

## Why

- **API consistency.** The Cx helpers encapsulate AST structure knowledge in one
  place. Inline destructuring scatters structural assumptions across dozens of
  files, making AST refactors break silently.
- **DRY.** Common checks like "is this a bare method call with no receiver?"
  are reimplemented by every cop unless the Cx helper is used.
- **Block delegation.** `cx.method_name(node)` automatically delegates through
  `Block → call`, `Numblock → send`, and `Itblock → send`, so you never need
  to destructure each block variant by hand.

## Helper reference

| Use this | Not this |
|---|---|
| `cx.method_name(node)` | `let NodeKind::Send { method, .. } = *cx.kind(node)` |
| `cx.call_receiver(node)` | Destructuring `receiver` from `NodeKind::Send` |
| `cx.call_arguments(node)` | Destructuring `args` from `NodeKind::Send` |
| `cx.is_assignment(node)` | Custom assignment-kind switch (missing `Masgn`, `OpAsgn`, `OrAsgn`, `AndAsgn`) |
| `cx.is_recursive_literal(node)` | Custom recursive-literal walker (missing `Str`, `Dstr`, `Regexp`, `Dsym`, etc.) |
| `cx.is_global_const(node, "ClassName")` | Matching `NodeKind::Const { name, .. }` by name only (misses `MyModule::ClassName`) |
| `cx.is_void_context(node)` | Custom void-context tree-walk |
| `cx.is_bare_access_modifier(node)` | Manual `Send` destructure for `public`/`private`/`protected` |
| `cx.is_access_modifier(node)` | Same as above |
| `cx.block_call(node)` | `NodeKind::Block { call, .. }` destructure |
| `cx.def_receiver(node)` | `NodeKind::Def { receiver, .. }` destructure |
| `cx.is_any_def_type(node)` | Matching `NodeKind::Def` + `NodeKind::Defs` separately |
| `cx.is_assignment_method(node)` | Checking `method_predicates::is_setter` directly |
| `cx.const_name(node)` | Manual `Const` destructure + scope resolution |

## Pattern

```rust
// Good — use Cx helpers
fn is_static_method_definition(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(*cx.kind(node), NodeKind::Def { .. })
        || (cx.call_receiver(node).get().is_none()
            && cx.method_name(node).is_some_and(|m| matches!(m, "attr" | "attr_reader" | "attr_writer" | "attr_accessor")))
}

fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
    let Some(method_str) = cx.method_name(node) else { return; };
    let receiver_opt = cx.call_receiver(node);
    let args_list = cx.call_arguments(node);
    // ...
}
```

## Anti-pattern

```rust
// Avoid — destructuring NodeKind::Send inline
let NodeKind::Send { method, receiver, args, .. } = *cx.kind(node) else {
    return;
};
let Some(recv_id) = receiver.get() else { return; };
let args_list = cx.list(args);
```

```rust
// Avoid — destructuring NodeKind::Def inline
let NodeKind::Def { name, .. } = *cx.kind(node) else {
    return;
};
let method_str = cx.symbol_str(name);
// ... manual scope checks via cx.def_receiver(node), etc.
```

```rust
// Avoid — custom literal walker
fn is_literal_value(child: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(child) {
        NodeKind::Int(_) | NodeKind::Float(_) | ... => true,
        _ => false,
    }
}
```

## See also

- `crates/murphy-plugin-api/src/cx.rs` — all Cx method signatures and docs.
- `crates/murphy-std/src/cops/lint/useless_access_modifier.rs` — canonical
  example where every `Send`/`Block`/`Def` property is resolved through helpers.
- `crates/murphy-std/src/cops/lint/void.rs` — uses `cx.is_void_context`,
  `cx.method_name`, and `cx.is_any_def_type` to eliminate duplicated void-check
  logic.
