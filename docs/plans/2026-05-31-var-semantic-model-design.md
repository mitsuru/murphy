# VarSemanticModel — design

Issue: `murphy-es99.5`
Scope: `crates/murphy-core`, `crates/murphy-plugin-api`
Status: design approved 2026-05-31

## 1. Why

Several RuboCop cops require knowledge of how local variables are declared,
assigned, and referenced across a scope — information that goes beyond what
a single-node `NodeCop` visit can observe. In RuboCop this is supplied by
`VariableForce`, a shared analysis engine that tracks the full lifecycle of
every local variable within its scope.

Murphy currently implements each variable-tracking cop independently with its
own ad-hoc dataflow logic. `Lint/UselessAssignment` has grown to 1 000+ lines,
re-implementing branch-chain computation and dominating-overwrite detection
inline. `Lint/UnusedMethodArgument` has its own separate scan. There is no
foundation for `Lint/ShadowingOuterLocalVariable`, `Lint/UnusedBlockArgument`,
or `Lint/UnderscorePrefixedVariableName`.

`VarSemanticModel` provides the shared engine that all of these cops can build
on, eliminating duplication and enabling the remaining cops to be implemented.

### Ecosystem scope

Research across the rubocop ecosystem (rubocop, rubocop-rails, rubocop-rspec,
rubocop-performance, sevencop, and others) found exactly **9 cops** that join
`VariableForce`, across only two hook types:

| Hook | Count | Key cops |
|---|---|---|
| `after_leaving_scope` | 8 | `UselessAssignment`, `UnusedMethodArgument`, `UnusedBlockArgument`, `InfiniteLoop`, `MapIntoArray`, `SaveBang`, `LeakyLocalVariable`, `UnderscorePrefixedVariableName` |
| `before_declaring_variable` | 1 | `ShadowingOuterLocalVariable` |

## 2. Scope

In:

- `VarSemanticModel` built once per file in `murphy-core`, stored in `CxRaw`
  as a raw pointer, accessible to cops via `cx.var_model()`
- Branch-aware `is_referenced` pre-computed for every `Assignment`
- Migration of `Lint/UselessAssignment` and `Lint/UnusedMethodArgument` to use
  the shared engine
- New cops: `Lint/ShadowingOuterLocalVariable`, `Lint/UnusedBlockArgument`,
  `Lint/UnderscorePrefixedVariableName`

Out (follow-up issues):

- Level 2 (ClassModel / Reek-equivalent class-level aggregation)
- Level 3 (ProgramModel / Brakeman-equivalent cross-file call index)
- Level 4 (TaintModel / inter-procedural data flow)
- Instance variable, class variable, or global variable tracking
- Inter-procedural / cross-scope data flow

## 3. Data structures

```rust
/// File-level variable scope analysis. Built once per file before the cop
/// loop; cops access it via `cx.var_model()`.
pub struct VarSemanticModel {
    /// Scope-boundary NodeId (Def / Defs / Block / Lambda / Sclass /
    /// Class / Module) → ScopeInfo.
    scopes: HashMap<NodeId, ScopeInfo>,
}

pub struct ScopeInfo {
    /// The enclosing scope's boundary NodeId, or None for the root scope.
    pub parent_scope: Option<NodeId>,
    pub variables: Vec<Variable>,
}

pub struct Variable {
    pub name: Symbol,
    /// True for Arg / Optarg / Restarg / Kwarg / Kwoptarg / Kwrestarg /
    /// Blockarg nodes declared in the scope's argument list.
    pub is_argument: bool,
    /// The NodeId of the declaration node (first assignment or argument node).
    pub declaration_node: NodeId,
    pub assignments: Vec<Assignment>,
    pub references: Vec<Reference>,
}

pub struct Assignment {
    pub node_id: NodeId,
    /// Pre-computed by the engine: true iff at least one Reference to this
    /// variable is reachable from this assignment on every possible
    /// execution path (branch-aware; see §5).
    pub is_referenced: bool,
}

pub struct Reference {
    pub node_id: NodeId,
}
```

### Public API

```rust
impl VarSemanticModel {
    pub fn build(ast: &Ast) -> Self;
    pub fn scope(&self, boundary_node: NodeId) -> Option<&ScopeInfo>;
}

impl ScopeInfo {
    pub fn parent_scope<'a>(&self, model: &'a VarSemanticModel)
        -> Option<&'a ScopeInfo>;
    pub fn variables(&self) -> &[Variable];
}
```

## 4. Build algorithm

`VarSemanticModel::build` performs a **single DFS** over the arena AST with an
explicit scope stack.

```
push root scope (NodeId = ast.root())

DFS pre-order:
  Def / Defs / Block / Lambda / Sclass / Class / Module:
    push new ScopeInfo { parent_scope: stack.top() }

  Arg / Optarg / Restarg / Kwarg / Kwoptarg / Kwrestarg / Blockarg:
    declare Variable { is_argument: true } in current scope

  Lvasgn / OpAsgn / OrAsgn / AndAsgn / Masgn target / For.var /
  Resbody.var:
    append Assignment to current scope's Variable (create if absent)

  Lvar:
    append Reference to current scope's Variable (create if absent)

DFS post-order (scope boundary nodes):
  run branch-aware dominance analysis on current scope (§5)
  pop scope
```

`AstNode::parent` (filled by `AstBuilder::finish`) is used to walk branch
chains without storing a separate parent map.

## 5. Branch-aware dominance analysis

Run once per scope after all nodes in the scope have been collected. Ported
from `useless_assignment.rs` `analyze_scope` / `branch_chain` /
`is_branch_barrier`.

For each `Variable`, for each `Assignment w`:

1. **No later reference** — `w.is_referenced = false`.

2. **Later reference exists** — find the first `Reference r` after `w`.
   Then find the first `Assignment o` that lies between `w` and `r` and
   dominates `w` (same or shallower branch, not in an exclusive sibling
   branch). If such an `o` exists, `w.is_referenced = false`; otherwise
   `w.is_referenced = true`.

**Branch chain** — for a node `n`, walk `n.parent` up to the scope root,
collecting `(barrier, child)` pairs at each branch-introducing ancestor:
`If`, `Case`, `When`, `While`, `Until`.

**Exclusive branches** — two nodes are in exclusive branches when their
branch chains diverge at the same barrier but in different children.

**Loop special case** — assignments inside `While`, `Until`, or `For` bodies
are always marked `is_referenced = true` because later iterations may
observe the value.

## 6. CxRaw integration

```rust
// murphy-plugin-api/src/abi.rs — appended to CxRaw (ADR 0037 tail-append)
pub struct CxRaw {
    // ... existing fields ...

    /// Pointer to the file's VarSemanticModel, built before the cop loop.
    /// Always non-null during native cop dispatch; null only in bare FFI
    /// tests that construct a minimal CxRaw.
    pub var_model: *const VarSemanticModel,
}

// murphy-plugin-api/src/cx.rs
impl<'a> Cx<'a> {
    pub fn var_model(&self) -> Option<&VarSemanticModel> {
        unsafe { self.raw.var_model.as_ref() }
    }
}

// murphy-core/src/dispatch.rs
pub fn run_cops_with_options(ast: &Ast, ...) {
    let var_model = VarSemanticModel::build(ast);   // once per file
    let mut base = build_cx_raw(ast, sink);
    base.var_model = &var_model as *const _;
    // cop loop ...
}
```

`VarSemanticModel` lives on the stack of `run_cops_with_options` for the
duration of the cop loop; the raw pointer is valid for that lifetime.
`MURPHY_PLUGIN_ABI_VERSION` is bumped on merge (tail-append to `CxRaw` is
still an ABI change under ADR 0038).

## 7. Cop usage examples

### Lint/UselessAssignment (migrated)

```rust
fn check(&self, node: NodeId, cx: &Cx<'_>) {
    let Some(model) = cx.var_model() else { return };
    let Some(scope) = model.scope(node) else { return };
    for var in scope.variables() {
        for asgn in var.assignments() {
            if !asgn.is_referenced {
                cx.emit_offense(asgn.node_id, /* message */);
            }
        }
    }
}
```

### Lint/ShadowingOuterLocalVariable (new)

```rust
fn check(&self, node: NodeId, cx: &Cx<'_>) {
    let Some(model) = cx.var_model() else { return };
    let Some(scope) = model.scope(node) else { return };
    for var in scope.variables().iter().filter(|v| v.is_argument) {
        let mut outer = scope.parent_scope(model);
        while let Some(s) = outer {
            if s.variables().iter().any(|v| v.name == var.name) {
                cx.emit_offense(var.declaration_node, /* message */);
                break;
            }
            outer = s.parent_scope(model);
        }
    }
}
```

## 8. Testing

Unit tests for `VarSemanticModel` live in `crates/murphy-core` under a
`#[cfg(test)]` module. Each test builds an `Ast` via `murphy_translate::translate`,
calls `VarSemanticModel::build`, and asserts on `is_referenced` values.

The existing 800+ lines of branch-aware tests in `useless_assignment.rs` serve
as the primary regression suite for the engine's dominance analysis. They are
preserved without modification; only the cop body that calls them is simplified.

## 9. Implementation order

1. Define data structures + `build` skeleton (DFS, scope stack, raw
   assignment/reference collection) — no branch analysis yet.
2. Port branch analysis from `useless_assignment.rs` into the engine.
3. Wire `CxRaw::var_model` and `cx.var_model()`.
4. Migrate `Lint/UselessAssignment` to use the engine.
5. Migrate `Lint/UnusedMethodArgument` to use the engine.
6. Implement `Lint/ShadowingOuterLocalVariable`.
7. Implement `Lint/UnusedBlockArgument`.
8. Implement `Lint/UnderscorePrefixedVariableName`.
