# Phase E: NodePattern Ruby-side params (`%var` / `%N`) Design

beads: `murphy-aow` (Phase E of epic `murphy-3gt7`).

## Goal

Add RuboCop NodePattern's `%var` (named) and `%1`/`%2` (positional) runtime
parameter mechanism to Murphy's pattern language, integrating with the existing
`CopOptions` machinery so cop authors can write patterns like
`(send _ %method ...)` and have the value bound to `cx.options::<MyOpts>().method`
at match time.

## Background

RuboCop's `def_node_matcher` accepts inline runtime arguments:

```ruby
def_node_matcher :interesting_call?, '(send _ %method ...)', method: Set[:foo, :bar]
```

Murphy's idiom for "runtime cop configuration" is `#[derive(CopOptions)]` on a
struct accessed via `cx.options::<MyOpts>()`. Phase E bridges the two: pattern
`%method` is resolved at match time by looking up `method` on the cop's
`CopOptions` struct.

Phase E is the 8th and final sub-task of epic murphy-3gt7. The first seven
(qpf9/2ouf/l448/wsep/kq57/nnr8/t8km, PRs #95-101) ported the parser to LALRPOP
and closed the remaining grammar gaps (`[ ... ]`, `|`, `Foo`, `_name`, `/.../`).
Lex/AST scaffolding for `%var`/`%N` was prototyped during the stacked-epic run
and is preserved in this branch's initial commit.

## Syntax

```
(send _ %method)            # named: subject Sym matches cop options.method
(send _ :foo %1)            # positional: 1st arg matches caller's positional[0]
(send _ %method %1)         # mixed is allowed
```

`%var` and `%N` sit in **atom positions** (subject `Lit::Sym(s)` / `Lit::Str(s)`
/ `Lit::Int(n)` / `Lit::True` / `Lit::False`). Type compatibility between the
subject `Lit` and the runtime `Param` is checked at match time; a mismatch
(e.g. `%method: i64` against `Lit::Sym(...)`) is a **runtime miss**, not a panic.

`%0` is rejected at lex time (positional indices are 1-based, matching
RuboCop). `%` followed by any other byte (e.g. `%@`) is a lex error.

## Param type & CopOptions field correspondence

A new `Param<'a>` enum in `murphy-plugin-api` represents a resolved runtime
value:

```rust
pub enum Param<'a> {
    Str(&'a str),              // single string; matches Lit::Sym / Lit::Str
    StrSet(&'a [String]),      // set membership against Lit::Sym / Lit::Str
    Int(i64),                  // matches Lit::Int
    IntSet(&'a [i64]),         // set membership against Lit::Int
    Bool(bool),                // matches Lit::True / Lit::False
    None,                      // Option<T>::None — always miss
}
```

Conversion from `CopOptions` field types is via an `IntoParam<'a>` trait:

| CopOptions field type | `Param` variant produced | Match semantics |
|---|---|---|
| `&String` | `Str(&str)` | string equality with subject Sym/Str |
| `&Vec<String>` | `StrSet(&[String])` | contains check (RuboCop `Set[...]` analog) |
| `i64` | `Int(i64)` | integer equality with subject Int |
| `&Vec<i64>` | `IntSet(&[i64])` | contains check |
| `bool` | `Bool(bool)` | bool equality with subject True/False |
| `&CopOptionEnum` | `Str(e.as_str())` | string equality via enum wire form |
| `&Option<T>` (T: IntoParam) | `match self { Some(t) => t.into_param(), None => None }` | inner type or always-miss |

Other CopOptions field types (e.g. structs, maps) have no `IntoParam` impl, so
referencing them with `%var` is a compile error (Rust trait-bound unsatisfied).

A `match_lit_against_param` helper in `murphy-plugin-api` is the central
comparison function reused by both backends and called from generated code.

## `def_node_matcher!` macro signature

The generated function's signature varies by what params the pattern uses. The
macro inspects the parsed AST to decide:

```rust
// Pattern A: no params → unchanged from existing API
def_node_matcher!(name, "(send _ :foo)")
//  fn name(node: NodeId, cx: &Cx<'_>) -> bool

// Pattern B: at least one %var → opts: required, positional arg added
def_node_matcher!(name, "(send _ %method)", opts: MyOpts)
//  fn name(node: NodeId, cx: &Cx<'_>, positional: &[Param<'_>]) -> bool

// Pattern C: only %1/%2 (no %var) → positional arg added, no opts:
def_node_matcher!(name, "(send _ :foo %1)")
//  fn name(node: NodeId, cx: &Cx<'_>, positional: &[Param<'_>]) -> bool

// Pattern D: mixed %var and %N → opts: required, positional arg added
def_node_matcher!(name, "(send _ %method %1)", opts: MyOpts)
//  fn name(node: NodeId, cx: &Cx<'_>, positional: &[Param<'_>]) -> bool
```

There are only **two** generated signatures — the legacy 2-arg form (no
runtime params) and the 3-arg form that takes `positional`. Patterns B and D
share the same signature: the macro adds `positional: &[Param<'_>]` whenever
*any* runtime param (`%var` or `%N` or both) appears, even if a particular
pattern only references `%var`. Cop authors writing the B form can simply
pass `&[]` for `positional`.

Macro-time validation rules:

- pattern has `%var` AND no `opts:` → compile error (named param used but no Options type provided)
- pattern has `opts:` AND no `%var` → compile error (unused `opts:`)
- pattern has any `%var` or `%N` → signature switches to the positional-bearing form

Existing cops that never use `%var`/`%N` keep their current signature and
require no changes.

## B-backend codegen sketch

For `def_node_matcher!(is_interesting_call, "(send _ %method %1)", opts: MyOpts)`
the macro emits roughly:

```rust
pub fn is_interesting_call(
    node: NodeId,
    cx: &Cx<'_>,
    positional: &[Param<'_>],
) -> bool {
    // Decode options once per call.
    let __opts = cx.options_or_default::<MyOpts>();

    // Pre-resolve every named %var into a local Param binding (lifetimes tied
    // to __opts, which lives for the duration of this call).
    let __p_method: Param<'_> = (&__opts.method).into_param();

    // Pattern body — same shape as existing lower_pat output, except %method
    // and %1 are emitted as match_lit_against_param calls.
    let n = cx.ast().node(node);
    if n.kind() != NodeKind::Send { return false; }
    let children = cx.ast().children(node);
    if !match_lit_against_param(cx.ast().lit(children[1]), &__p_method) {
        return false;
    }
    if !match_lit_against_param(
        cx.ast().lit(children[2]),
        positional.get(0).unwrap_or(&Param::None),
    ) { return false; }
    true
}
```

Out-of-bounds positional access (caller passed too few `Param`s) collapses to
`Param::None` — soft-fail, no panic.

## C-backend matcher

The standalone `matches()` API in `murphy-pattern` doesn't know about `Cx` or
`CopOptions`, so it grows a new `ParamHost` trait that abstracts the same
lookup:

```rust
pub trait ParamHost {
    fn named(&self, name: &str) -> Option<Param<'_>>;
    fn positional(&self, index: usize) -> Option<Param<'_>>;
}

pub struct NoParams;
impl ParamHost for NoParams { /* always None */ }
```

`matches()` gains a `params: &Q` parameter (`Q: ParamHost + ?Sized`). The
existing 4-arg `matches(ir, ast, node, predicates)` survives as a delegate that
plugs in `&NoParams`.

`match_pat`'s switch grows `IrNode::ParamNamed`/`IrNode::ParamNumber` arms that
call into `ParamHost`, build a `Param`, then delegate to
`match_lit_against_param` against the current node's `Lit` shape.

The mruby bridge (`murphy-9cr.24`) will be the production `ParamHost`
implementation but is **out of scope** for this issue; tests use an in-memory
`HashMap`-backed `ParamHost`.

## Validation walks

Post-parse walks in `parser.rs` (`convert_named_captures`,
`validate_bare_predicate_position`, `assign_capture_slots`,
`resolve_pred_capture_refs`, `validate_quantifier_*`, `validate_rest_placement`,
`validate_capture_position`) all need to recognise `PatKind::ParamNamed` and
`PatKind::ParamNumber` as **leaf** kinds — no children to descend into, no
capture interaction, no validation rule beyond what the lexer already enforces
(`%0` reject, etc.).

## Test plan

- **Lexer** (extending stash scaffolding): `%method` / `%1` / `%99` / `%0` (error) / `%@` (error)
- **Parser**: `%method` and `%1` parse to the expected `PatKind`; mixed atom positions work; `({%a %b} ...)` (union head) rejects (head must be a NodeKind)
- **Matcher**: in-memory `ParamHost` exercising every `Param` variant and the
  type-mismatch soft-fail; out-of-bounds positional → miss
- **B-backend**: trybuild compile-fail tests for `%var` without `opts:` and
  `opts:` without `%var`; runtime tests for each `CopOptions` field type
- **Cross-backend conformance** (`tests/cross_backend_conformance.rs` §16):
  the same in-memory `ParamHost` examples on both backends

## Scope

In: %var/%N parsing, AST, IR, both backends, `Param` enum, `IntoParam` trait,
`match_lit_against_param`, `ParamHost` trait, `matches()` API extension,
cross-backend conformance, follow-up issue for mruby-bridge integration.

Out: mruby bridge (`murphy-9cr.24`) plumbing of `ParamHost`; pattern-level
extensions like RuboCop's `%CONST_NAME`; CopOptions support for `Vec<i64>` or
other not-yet-derived field types — the `IntoParam` impls land here but the
derive side may need a separate issue.
