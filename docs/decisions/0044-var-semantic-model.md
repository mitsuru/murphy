# ADR 0044 — VarSemanticModel: shared variable scope analysis engine

- Date: 2026-05-31
- Status: Accepted
- Issue: `murphy-es99.5`
- Related: ADR 0037 (arena AST), ADR 0038 (single-surface plugin ABI),
  ADR 0043 (murphy-prism token access)

## Context

Murphy needs a shared engine for local variable scope and data flow analysis,
equivalent to RuboCop's `VariableForce`. Without it, each cop re-implements
its own ad-hoc dataflow logic. `Lint/UselessAssignment` has grown to 1 000+
lines; `Lint/UnusedMethodArgument` has a separate implementation; and
`Lint/ShadowingOuterLocalVariable`, `Lint/UnusedBlockArgument`, and
`Lint/UnderscorePrefixedVariableName` cannot be implemented at all without a
shared scope table.

Ecosystem research (rubocop, rubocop-rails, rubocop-rspec, rubocop-performance,
sevencop) found **9 cops** that use `VariableForce`, using only two hook types:
`after_leaving_scope` (8 cops) and `before_declaring_variable` (1 cop).

## Decision

Implement `VarSemanticModel`, a Ruff-style pre-computed query model built once
per file from the arena AST, stored in `CxRaw` as a raw pointer, and accessed
by cops via `cx.var_model()`.

Key properties:

- Scope boundaries identified by `NodeId` (the boundary node itself).
- `ScopeInfo` holds `parent_scope: Option<NodeId>` for scope chain traversal.
- Each `Assignment` carries a pre-computed `is_referenced: bool` (branch-aware
  dominance analysis, ported from `useless_assignment.rs`).
- Built in `murphy-plugin-api`, unconditionally, before the cop loop in
  `run_cops_with_options`. No lazy initialization.
- Cops query the model; no callback/hook protocol is exposed.

See `docs/plans/2026-05-31-var-semantic-model-design.md` for the full data
structures, build algorithm, and branch analysis specification.

## Alternatives considered

### RuboCop-style interleaved hook system

RuboCop's `VariableForce` interleaves its own AST traversal with the cop
dispatch loop. Cops implement `after_leaving_scope` / `before_declaring_variable`
callbacks and receive scope data through those hooks.

Rejected. Murphy's cop dispatch is driven by `NodeKind` tags through a C FFI
boundary (`CxRaw`, `DispatchFn`). Introducing a second traversal that fires
callbacks into cop code would require a separate non-FFI trait or a second
dispatch protocol. The interleaved model also makes the order of analysis
and offense emission harder to reason about in a Rust ownership context.

### Ruff-style SemanticModel with broader scope (ClassModel)

Ruff's `SemanticModel` covers the full module including imports, symbol
resolution, and binding kinds. Murphy could start with a similarly broad model
that also tracks instance variables and class-level structure (Reek-equivalent
analysis), laying groundwork for Level 2 (ClassModel) and Level 3
(ProgramModel / Brakeman-equivalent).

Rejected for this issue. The 9 ecosystem cops that need variable force all
operate on local variable scope only. Expanding scope to class-level aggregation
now would add significant complexity without a concrete consumer. A future
`ClassModel` can build on top of `VarSemanticModel`'s `NodeId`-keyed scope
boundaries without requiring changes to this layer.

### Dedicated `ScopeId` type (separate from `NodeId`)

Ruff uses a dedicated `ScopeId` (dense `u32` index into a `Vec<ScopeInfo>`)
rather than keying on the AST node's `NodeId`. The dense `Vec` gives better
cache locality when iterating all scopes.

Rejected. Murphy files have on the order of 5–50 scopes. The memory and
performance difference between `HashMap<NodeId, ScopeInfo>` and
`Vec<ScopeInfo> + HashMap<NodeId, ScopeId>` is in the low-hundreds-of-bytes
range and unmeasurable in practice. `ScopeId` also introduces a new identifier
type that cops must convert to/from, whereas `NodeId` is already the universal
identifier in the Murphy API. The B option actually uses slightly more memory
(two structures instead of one).

### Lazy construction via `OnceLock` inside `Cx`

`VarSemanticModel` could be built lazily when `cx.var_model()` is first called,
avoiding build cost when no variable-analysis cop is active.

Rejected. `Cx<'a>` is a thin `&'a CxRaw` wrapper; `CxRaw` is `#[repr(C)]` and
cannot hold a `OnceLock<VarSemanticModel>`. The model must live somewhere with
a longer lifetime than the `Cx` borrow, which means it must be constructed and
owned outside of `Cx`. Building it unconditionally at the top of
`run_cops_with_options` is the simplest correct approach; the build cost for a
typical file is negligible.

### Build inside `translate()` (alongside the arena AST)

`VarSemanticModel` could be computed during `translate()` and returned as
`(Ast, VarSemanticModel)`. This avoids a second traversal of the AST.

Rejected. `translate()` already has a clear, narrow contract: take source bytes
and a path, return an `Ast`. Adding `VarSemanticModel` to the return type
couples a higher-level analysis concept into the low-level translation layer.
The second-pass cost (a single DFS over an already-warm arena) is far smaller
than the `translate()` cost itself (Prism parse + full arena construction).

### Build from Prism's Visit callbacks (before arena translation)

`VarSemanticModel` could be built during the Prism `Visit` traversal, before
`translate()` converts to the arena AST.

Rejected. Prism nodes have no `NodeId`; they are identified only by byte
ranges and borrowed C pointers. Building the model from Prism would require
byte-range-keyed maps and a subsequent translation step to connect them to
arena `NodeId`s, adding complexity with no benefit. The Prism lifetime is
dropped at the end of `translate()`; any model built there would need to be
fully converted before return.

### Restrict to variable-analysis cops only (cop-registration flag)

`VarSemanticModel` could be built only when at least one variable-analysis cop
is active, using a flag in `PluginCopV1` or a separate registration mechanism.

Deferred / not adopted for now. The build cost is small and the added
complexity of per-cop opt-in is not justified at current scale. This can be
revisited if profiling shows the build is a measurable bottleneck with all
variable-analysis cops disabled.

### Extend to Level 2–4 (ClassModel, ProgramModel, TaintModel)

Reek requires class-level aggregation (too many instance variables, data
clumps across methods). Brakeman requires cross-file call indexing and
inter-procedural taint tracking. These could be designed together with
`VarSemanticModel` as a unified `SemanticModel`.

Deliberately deferred. The analysis layers are:

```
Level 1: VarSemanticModel   — intra-scope local variable tracking (this ADR)
Level 2: ClassModel         — class-level method/ivar aggregation (Reek)
Level 3: ProgramModel       — cross-file call index (Brakeman)
Level 4: TaintModel         — inter-procedural data flow (Brakeman security)
```

Level 2 can be added later by building on top of Level 1's `NodeId`-keyed
scope boundaries without modifying this layer. Levels 3 and 4 are
fundamentally different in nature (cross-file, inter-procedural) and belong
in separate infrastructure.

## Consequences

### Positive

- `Lint/UselessAssignment` and `Lint/UnusedMethodArgument` shed their inline
  dataflow implementations; the branch-aware logic lives in one place.
- `Lint/ShadowingOuterLocalVariable`, `Lint/UnusedBlockArgument`, and
  `Lint/UnderscorePrefixedVariableName` become implementable.
- Future variable-analysis cops (e.g., `Style/InfiniteLoop` autocorrect guard,
  `RSpec/LeakyLocalVariable`) have a ready foundation.
- The pre-computed query API (`cx.var_model().scope(node).variables()`) is
  simpler for cop authors than a callback/hook protocol.

### Negative

- `VarSemanticModel::build` runs on every file, even when no variable-analysis
  cop is active. Acceptable at current scale; revisit if profiling demands it.
- `CxRaw` gains a new `var_model` pointer field, requiring an ABI version bump
  (`MURPHY_PLUGIN_ABI_VERSION`).
- The branch-aware dominance algorithm is non-trivial; it must be ported
  carefully from `useless_assignment.rs` and validated against the existing
  test suite.
