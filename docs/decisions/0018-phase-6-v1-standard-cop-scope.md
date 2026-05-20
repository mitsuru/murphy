# ADR 0018 — Phase 6 v1 standard cop scope

- Date: 2026-05-20
- Status: Accepted
- Issue: `murphy-7rg.1`
- Parent: `murphy-7rg` (Phase 6 — v1 standard cop scope + perf-regression CI)
- Gated by: ADR 0017 (Phase 5 PASSED; Phase 6 may start)
- Feeds: `murphy-7rg.2`, `murphy-nkq`, `murphy-7rg.3`, `murphy-7rg.4`, `murphy-7rg.5`

## Context

Murphy currently ships one native standard cop, `Murphy/NoReceiverPuts`. The
original design intentionally deferred the v1 standard-cop catalogue to Phase 6:

> standard cop v1 scope: how much of top-adoption Layout/Style/Lint to ship.

The Phase 6 scope decision must happen before implementation. `murphy-7rg.3`
explicitly implements the cop suite decided here, and any per-cop subtasks are
spawned from this ADR at execution time.

## Decision

Murphy v1 will ship a **balanced RuboCop-approximation set**: about twenty
standard cops across `Lint`, `Style`, and a limited, safe subset of `Layout`.

The selection criteria are:

1. Prioritize RuboCop's default, commonly-seen `Layout` / `Style` / `Lint` cop
   experience over Murphy-specific novelty.
2. Keep Phase 6 medium-sized: large enough to feel useful, small enough to
   implement with table-driven tests and perf gates.
3. Include `Layout` only where byte-range detection and autocorrect can be made
   deterministic and idempotent.
4. Implement autocorrect aggressively for cops whose replacement can be made
   deterministic under the Phase 4 fixpoint engine.
5. File follow-up beads for implementation discoveries instead of expanding this
   ADR ad hoc.

## v1 cop set

### Lint

These cops target high-signal production bugs or accidental debug artifacts.

| Murphy cop | RuboCop analogue | v1 behavior |
|---|---|---|
| `Murphy/NoReceiverPuts` | Murphy-specific debug-output cop | Existing native cop; keep enabled by default. |
| `Lint/Debugger` | `Lint/Debugger` | Flag common debugger calls and debugger requires. |
| `Lint/DeprecatedClassMethods` | `Lint/DeprecatedClassMethods` | Flag and autocorrect known deprecated class method calls such as `File.exists?`. |
| `Lint/DuplicateHashKey` | `Lint/DuplicateHashKey` | Flag duplicate literal hash keys when determinable in one file. |
| `Lint/EmptyWhen` | `Lint/EmptyWhen` | Flag empty `when` branches. |
| `Lint/UnreachableCode` | `Lint/UnreachableCode` | Flag statements after unconditional control-flow exits within the same body. |
| `Lint/UnusedMethodArgument` | `Lint/UnusedMethodArgument` | Flag unused method parameters; autocorrect by prefixing `_` when safe. |
| `Lint/UselessAssignment` | `Lint/UselessAssignment` | Flag assigned local variables that are never read in the same local scope. |

### Style

These cops give v1 a familiar RuboCop-style default experience while staying
inside predictable AST/text transforms.

| Murphy cop | RuboCop analogue | v1 behavior |
|---|---|---|
| `Style/FrozenStringLiteralComment` | `Style/FrozenStringLiteralComment` | Enforce and autocorrect the file-level magic comment. |
| `Style/HashSyntax` | `Style/HashSyntax` | Prefer Ruby 1.9 hash syntax where safe. |
| `Style/StringLiterals` | `Style/StringLiterals` | Prefer single-quoted strings when interpolation/escapes do not require double quotes. |
| `Style/SymbolArray` | `Style/SymbolArray` | Prefer `%i[...]` for simple symbol arrays. |
| `Style/WordArray` | `Style/WordArray` | Prefer `%w[...]` for simple string arrays. |
| `Style/RedundantReturn` | `Style/RedundantReturn` | Flag/autocorrect redundant final `return` in method bodies where behavior is preserved. |
| `Style/NilComparison` | `Style/NilComparison` | Prefer `nil?` over direct `== nil` / `!= nil` where safe. |
| `Style/IfUnlessModifier` | `Style/IfUnlessModifier` | Prefer modifier form for simple single-line bodies. |
| `Style/RedundantSelf` | `Style/RedundantSelf` | Remove redundant `self.` only when Ruby semantics are not changed. |
| `Style/AndOr` | `Style/AndOr` | Prefer `&&` / `||` in conditional contexts. |

### Layout

Layout scope is intentionally limited. Murphy v1 is not a formatter replacement;
these cops cover high-frequency whitespace issues with deterministic byte edits.

| Murphy cop | RuboCop analogue | v1 behavior |
|---|---|---|
| `Layout/TrailingWhitespace` | `Layout/TrailingWhitespace` | Remove trailing spaces/tabs outside heredoc-sensitive ranges. |
| `Layout/EmptyLines` | `Layout/EmptyLines` | Collapse excessive blank lines. |
| `Layout/SpaceAroundOperators` | `Layout/SpaceAroundOperators` | Enforce spaces around common binary operators when token context is unambiguous. |
| `Layout/SpaceInsideParens` | `Layout/SpaceInsideParens` | Remove spaces immediately inside parentheses. |
| `Layout/DotPosition` | `Layout/DotPosition` | Prefer leading dot for multiline method chains. |

## Autocorrect policy

RuboCop-approximation is the v1 product direction, so standard cops should emit
autocorrect edits whenever the transformation can be made deterministic and
idempotent.

Every autocorrecting cop must have:

1. Table-driven offense tests.
2. Table-driven correction tests.
3. A fixpoint/idempotency test that feeds corrected source back through the cop
   and observes no further changes or a documented fixed-point outcome.
4. A multibyte-source test when the cop slices by source byte range.

If an individual RuboCop analogue has unsafe autocorrection, Murphy may still
implement the offense in v1 but must either omit autocorrect or narrow it to a
safe subset. The implementation task must file a beads follow-up for any omitted
RuboCop behavior that matters but is not safe to include in v1.

## Implementation consequences

`murphy-7rg.2` remains next because the mruby/native primitive IDL is still
stringly in places. The v1 native cop suite can proceed after the IDL and native
cop-suite hygiene are ready.

`murphy-nkq` should harden the native cop layout before `murphy-7rg.3` starts:

- Module naming and re-export conventions must scale beyond one cop.
- Registry construction remains the single source of the enabled native cop set.
- Aggregator ordering from ADR 0007 must not be weakened as more cops run.

`murphy-7rg.3` should spawn concrete per-cop beads from the table above. The
spawned tasks should group cops only when they share an implementation seam; do
not batch unrelated cops just to reduce issue count.

## Deferred / discovered work protocol

During implementation, if a cop exposes missing parser access, token access,
scope analysis, heredoc handling, or autocorrect conflict semantics, create a
new beads issue linked to the active task using a discovered-from dependency or
notes. Do not silently expand the current task or weaken the v1 set without a
recorded beads decision.

Examples of expected follow-ups:

- Token-level infrastructure needed by `Layout` cops.
- Local-scope dataflow needed by `Lint/UselessAssignment`.
- Safe-subset limits for `Style/RedundantSelf`.
- Heredoc/comment-sensitive exclusions for whitespace cops.
- RuboCop behavior gaps found by the diff-quality watch.

## Non-goals

- Full RuboCop compatibility.
- Configurable per-cop style options beyond the existing enabled/severity schema.
- Metrics, Naming, Security, Rails, RSpec, Performance, or other extension cops.
- A general formatter pipeline beyond the listed Layout cops.
- Third-party cop sandboxing.

## Verification expectation

The Phase 6 gate must verify:

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test`
- Perf-regression CI from `murphy-7rg.4` at N=1/20/100 vs RuboCop
- Diff-quality watch from `murphy-7rg.5` vs `rubocop -a`
