# `def_node_matcher!` — RuboCop-subset S-expression matcher

`def_node_matcher!` lowers a RuboCop-subset S-expression pattern to a free
function at compile time (ADR 0033). It is Murphy's analogue of
RuboCop's `def_node_matcher` and is the right tool when the cop needs
to test the shape of a node across multiple kinds at once.

Authoritative reference: infra guide §3 ("Reusable matchers:
`def_node_matcher!`") plus the v1 grammar doc at
`docs/plans/2026-05-22-murphy-9cr17-pattern-grammar.md`. The runtime IR
lives in `crates/murphy-pattern`.

## When to reach for `def_node_matcher!`

- The shape spans more than one node kind (`(send nil :describe (const …) …)`).
- The same pattern is reused from multiple dispatch methods.
- The RuboCop original used `def_node_matcher` or `def_node_search`.
- The captures would be awkward to extract by hand-walking
  `cx.children(id)`.

Stay with manual destructure (`let NodeKind::Send { … } = *cx.kind(id)`)
when the shape is one level deep and there are no captures — a single
match arm is clearer than a pattern function.

## Invocation

The macro is invoked at module top level — once per pattern — and emits
a free function `(node: NodeId, cx: &Cx<'_>) -> R`. Call the function
from any cop's dispatch body.

```rust
use murphy_plugin_macros::def_node_matcher;

// Zero captures → fn(NodeId, &Cx) -> bool. Tests shape only.
def_node_matcher!(is_bare_expect_call, "(send nil :expect _)");

// Captured atoms → fn(NodeId, &Cx) -> Option<(Capture1, Capture2, …)>.
def_node_matcher!(
    describe_first_arg,
    "(send {nil (const nil :RSpec)} :describe $_ ...)",
);
```

Then inside a dispatch:

```rust
#[on_node(kind = "send")]
fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
    if !is_bare_expect_call(node, cx) {
        return;
    }
    // …
}
```

## v1 grammar (RuboCop subset)

Each element of the pattern is one of:

| Form | Meaning |
|---|---|
| `42`, `:foo`, `true`, `nil` | Atom literals. Match the corresponding `NodeKind` payload. |
| `int`, `send`, `block`, `str`, `class`, … | Node-kind head. Matches a node whose kind is the named one. |
| `_` | Wildcard (matches any one node / atom). |
| `{a b c}` | Alternation — any of `a`, `b`, `c` matches. |
| `...` | Rest — zero-or-more nodes in a sequence (commonly trailing). |
| `$_`, `$sym`, `$(send …)` | Capture — emits the matched node / atom into the function's return tuple. Capture order matches source order. |
| `$...` | Sequence capture — captures the remaining nodes of a sequence as `&'a [NodeId]` (borrowed slice into the arena, not owned). |

Worked examples (paired with the RuboCop form they replace):

```text
RuboCop: (send nil :require _)
Murphy : same — def_node_matcher!(is_require_call, "(send nil :require _)");

RuboCop: (send {nil (const nil :Foo)} :bar _ ...)
Murphy : same — receiver alternation, one captured-shape arg, then rest.

RuboCop: (block (send nil :it $_) _ _)
Murphy : def_node_matcher!(it_block_name, "(block (send nil :it $_) _ _)");
         → returns Option<NodeId> for the name argument.
```

## What is *not* in v1

The following appear in RuboCop's `def_node_matcher` grammar and are
**not** supported by `def_node_matcher!` v1 — translate them out by hand:

- Named captures (`$_name`). Use positional captures in tuple order.
- Predicate calls (`#some_method?`). Re-write as an `if` on the
  captured value after the match.
- Type-tagged captures (`$_str`). Match the kind in the head and let
  the cop body assert further.
- Union captures (`${a b}` capturing across alternation). Lift to two
  patterns or two arms.
- Repeated atoms with `*` inside `{ }`. The only repetition form is
  `...` at the tail of a sequence.

If the RuboCop pattern can't be expressed in this subset, write the
shape check by hand against `NodeKind` and `cx.list(...)` and leave a
note in the file's doc comment.

## Return-type rules

The signature of the generated function depends on the captures:

- **No captures** — returns `bool`.
- **One or more captures** — returns `Option<(C1, C2, …)>`. The capture
  types are: `NodeId` for `$_` / `$(…)` / `${…}`, the atom type for
  `$sym` / `$int` / etc., and `&'a [NodeId]` for `$...` (the macro
  lowers `CaptureKind::Seq` to a borrowed slice into the arena, not an
  owned `Vec`).

Always handle the `Option` with `let Some((a, b)) = pattern(node, cx) else { return; }`
— do not `.unwrap()`.

## Performance notes

Compile-time lowering means there is no per-call interpretation cost;
the generated function is straight-line destructuring against `NodeKind`
plus the captures. Pattern matchers are as cheap as hand-written
destructure and should be the default whenever the shape is non-trivial.
