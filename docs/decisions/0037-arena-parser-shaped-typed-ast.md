# ADR 0037: Arena, parser-shaped, typed AST as the core representation

**Status**: Accepted (2026-05-22)
**Issue**: murphy-9cr.13 (parent epic: murphy-9cr)
**Related**: ADR 0001 (prism binding), ADR 0006 (offense JSON / exit-code contract), ADR 0019 (native primitive IDL), murphy-9cr.1 (RuboCop dispatch frequency analysis), murphy-9cr.5 (prism Tier 1 typed wrappers)
**Supersedes**: murphy-9cr.5 (prism Tier 1 typed wrappers — the wrapper-over-`ruby_prism::Node` design)

## Context

Murphy's architecture (CLAUDE.md) describes "one prism parse shared in-memory"
as a `ruby_prism::Node` tree borrowed by every cop. murphy-9cr.5 specified
32 Tier 1 typed wrappers over `ruby_prism::Node` variants. Implementing it
surfaced a hard blocker: the plugin ABI never delivers a prism node, and
ruby-prism cannot rebuild a `Node` from raw pointers — `Node::new` is
`pub(crate)`. A "typed view over a borrowed prism node" therefore cannot
cross the plugin ABI at all.

The AST representation strategy memo (`docs/plans/2026-05-22-ast-representation-strategy.md`)
explored the alternative — an owned, arena-allocated, parser-shaped, typed
AST that Murphy controls end to end. The plugin-reboot design
(`docs/plans/2026-05-22-plugin-reboot-design.md`) concretized it into an
implementable shape. This ADR formalizes the **core AST representation**
half of that design. The plugin ABI half is ADR 0038.

## Decision

Murphy's shared immutable AST is an **owned arena**, not a borrowed
`ruby_prism::Node` tree.

1. **Arena ownership.** A new crate `murphy-ast` owns the AST. One
   `murphy_ast::Ast` value owns one file: a flat `Vec<AstNode>`, a
   `node_lists` side table, an interner, comments, the source buffer, and
   the file path. murphy-core's "shared immutable AST" type changes from
   `ruby_prism::Node` to `murphy_ast::Ast`.

2. **Fixed-size POD nodes.** `AstNode` is `#[repr(C)]` and fixed-length
   (sized to the largest variant): `kind`, `parent`, `range`. `NodeId` is a
   `u32` index into `nodes`. Variable-length children are referenced by
   `NodeList = (u32 start, u32 len)` into `node_lists`. `OptNodeId` uses the
   sentinel `u32::MAX` for `None` rather than relying on a niche. No
   `Box`/`Vec`/pointer lives inside a node.

3. **`NodeKind` is `#[repr(C, u8)]`.** A payload-carrying enum with a fixed
   `u8` discriminant, so its layout is stable and ABI-describable.

4. **Parser-shaped, not prism-shaped.** The AST follows the node shapes that
   the Ruby `parser` gem (and therefore RuboCop cops) assume — not prism's.
   A new crate `murphy-translate` performs `prism AST → arena AST` in a
   single one-file DFS pass and is the **only** place the prism↔parser
   collapse/split divergence (catalogued in murphy-9cr.1) is absorbed. Cops
   never see that divergence.

5. **The `NodeKind` variant set is frozen at v1, parser-gem-shaped.** Once
   `murphy-ast` ships, adding, reordering, or changing a `NodeKind` variant
   is a breaking ABI change (a `#[repr(C, u8)]` payload enum has a defined
   but rigid layout). The variant set must therefore be designed to
   completion before `murphy-ast` ships. v1 follows the `parser` gem because
   Route B (mechanical RuboCop port) makes parser-gem naming and child
   layout the target.

6. **Bidirectional traversal.** Every node carries a `parent` back-link
   (the root uses a sentinel). `murphy-ast` exposes `parent()` /
   `children()` / `ancestors()` / `descendants()` iterators — enough for the
   ~40 traversal cops (`each_ancestor` and friends) identified in the
   strategy memo.

7. **Error nodes pass through.** prism parse errors map to
   `NodeKind::Error`; dispatch skips them so a syntax error never crashes a
   cop.

8. **Serializability is a v1 design goal of `murphy-ast`.** Because nodes
   are POD, side tables are flat, and the interner blobs to an offset array,
   the in-memory arena is already nearly its own serialized form. The
   *binary cache feature itself* (cache key, invalidation, CLI integration)
   is a deferred fast-follow sub-task, **not v1-blocking** — but `murphy-ast`
   must be designed so that feature is possible without reshaping the arena.

## Translation cost gate (reversal trigger)

The `prism AST → arena AST` pass is new work on the hot path, and Murphy
exists to be fast. Before the arena AST is committed to as the production
representation, a prototype must **measure the translation cost** against
the baseline (prism parse only).

- The gate metric is the translation pass's added wall-clock time as a
  percentage of the prism-parse baseline.
- If the increment exceeds the agreed threshold, the **fallback is to
  abandon the translation pass and thinly wrap prism nodes instead** —
  accepting a narrower plugin surface.
- The binary cache (decision 8) is the insurance: even a borderline
  translation cost is amortized across repeated runs (editor / CI /
  pre-commit re-lint the same files).

This is an explicit decision point, not a passing note: a failed gate
reverses decision 4.

## Reasons

1. **The plugin ABI cannot carry a borrowed prism node.** ruby-prism's
   `Node::new` is `pub(crate)`; nothing can rebuild a `Node` from the
   pointers an ABI would pass. An owned arena that Murphy controls is the
   only representation that can be *both* the core AST and the plugin
   surface — see ADR 0038.
2. **Flat POD bytes are the shared foundation.** Single-surface direct-read
   ABI (ADR 0038), the binary cache, and any future zero-copy `mmap` all
   require the AST to be flat bytes with no embedded pointers. Fixed-size
   nodes plus side tables deliver exactly that.
3. **Parser-shaped serves Route B.** Mechanical RuboCop porting needs cop
   code to see the shape RuboCop cops were written against. Absorbing the
   collapse/split divergence in `murphy-translate` keeps every cop free of
   prism-specific quirks.
4. **Cache-friendliness compounds.** Linters re-run the same files
   constantly. A serializable arena lets a cache hit skip *both* the prism
   parse and the translation pass.

## Alternatives considered

- **Keep `ruby_prism::Node` as the core AST with thin typed wrappers**
  (the original murphy-9cr.5 plan). Rejected: the wrappers cannot cross the
  plugin ABI, and ruby-prism will not rebuild nodes from raw pointers.
- **A prism-shaped arena** (mirror prism's node kinds instead of the
  parser gem's). Rejected: it pushes the collapse/split divergence into
  every cop and defeats the Route B mechanical-port goal.
- **A pointer-rich idiomatic Rust tree** (`Box`/`Vec` inside nodes).
  Rejected: not flat, not serializable, and impossible to direct-read
  across a `.so` boundary.

## Out of scope

- **Pattern DSL** (design doc §4 — `murphy-pattern`, the B/C backends).
  A separate forthcoming ADR.
- **murphy-9cr epic restructure** (design doc §6 DAG). Operational
  sequencing, not an architectural decision; handled when the epic is
  re-planned.
- **The plugin ABI surface** (`Cx`, `NodeCop`, the `.so` boundary).
  ADR 0038.

## Consequences

- **murphy-9cr.5 is superseded.** The prism Tier 1 typed-wrapper set is
  replaced by `murphy-ast`'s `NodeKind`; murphy-9cr.5 should be closed as
  superseded during the epic restructure.
- **Two new crates**: `murphy-ast` and `murphy-translate`. murphy-core's
  shared-AST type changes accordingly.
- **ADR 0001 (prism binding) is unaffected.** prism still performs the
  actual parse; translation is a new downstream pass that consumes prism's
  output.
- **ADR 0006 (offense JSON / exit codes) is unchanged** and stays protected
  by snapshot and determinism tests.
- **A failed cost gate reverses decision 4** (see the reversal trigger
  above). The arena AST is a conditional commitment until the prototype
  measurement passes.
- **The v1 `NodeKind` freeze front-loads design effort**: the variant set
  must be designed to completion — parser-gem-shaped — before `murphy-ast`
  ships, because post-ship variant changes are breaking.

## Implementation status

- Decision recorded; this ADR is the deliverable.
- `murphy-ast` and `murphy-translate` implementation, the prototype, and the
  cost-gate measurement are downstream issues sequenced by the design doc
  §6 DAG (`murphy-ast` crate creation tracked as murphy-9cr.14).
- No code change in this issue.
