# Metric cops (Reek/rubycritic-style) — brainstorm result

- Date: 2026-09-25
- Issue: `murphy-siq`
- Status: brainstorm-result (pre-ADR)
- Parent roadmap: `docs/plans/2026-05-20-murphy-post-fmw-roadmap-design.md` §7 (D-axis candidate)

## Verdict

**No new std cops needed for the RuboCop `Metrics/*` set — full parity already
shipped.** Reek-only semantic smells (no RuboCop equivalent) remain, and go to
the D axis as a future cop-pack candidate, not `murphy-std`.

## 1. ADR 0018 check (as instructed by the issue)

ADR 0018 (`docs/decisions/0018-phase-6-v1-standard-cop-scope.md`) listed
"Metrics, Naming, Security, ..." under **Non-goals** at v1 decision time.
Implementation has since gone beyond that scope: `murphy-std` now ships all
ten RuboCop `Metrics/*` cops, each verified numerically against RuboCop 1.87.0
(`murphy-parity` headers in source).

## 2. Current coverage (`crates/murphy-std/src/cops/metrics/`)

| Cop | RuboCop analogue | Notes |
|---|---|---|
| `Metrics/AbcSize` | same | mirrors `AbcSizeCalculator` |
| `Metrics/BlockLength` | same | |
| `Metrics/BlockNesting` | same | |
| `Metrics/ClassLength` | same | `CodeLengthCalculator` folding parity |
| `Metrics/CollectionLiteralLength` | same | |
| `Metrics/CyclomaticComplexity` | same | |
| `Metrics/MethodLength` | same | `CountAsOne` ×4, heredoc extension |
| `Metrics/ModuleLength` | same | |
| `Metrics/ParameterLists` | same | |
| `Metrics/PerceivedComplexity` | same | |

That is 10/10 of RuboCop's `Metrics` department — nothing to add here.
`Naming/*` additionally covers part of Reek's `Uncommunicative*` family.

## 3. Remaining gap: Reek-only smells (no RuboCop equivalent)

Candidates, grouped by implementation cost:

- **Cheap (single-file AST)**: `DuplicateMethodCall`, `TooManyStatements`,
  `TooManyMethods`, `NestedIterators`, `RepeatedConditional`,
  `LongYieldList`, `Uncommunicative*` leftovers.
- **Expensive (cross-method/file)**: `FeatureEnvy`, `DataClump`, `LargeClass`
  (needs A2 cross-file engine), Flay-style duplication detection, Flog-style
  aggregate score / rubycritic rating (belongs to B9 report output, not cops).

## 4. Recommendation: D-axis pack, not std

- Ship Reek-style smells (when wanted) as an **opt-in cop pack** on the A6a
  pluggable-pack mechanism (e.g. `murphy-smell`), reusing the existing
  `enabled`/`severity` config schema. No new config keys, no std growth.
- Cheap group first; expensive group only after A2 (cross-file) lands.
  Flog/Flay-style scores go to B9 (`--format html/markdown`) instead of cops.
- No Phase 8/9/10 change, no ADR 0018 revision (0018 scoped v1 only).
  Track as D-axis backlog; no new Phase proposed.
- Constraints respected: docs only, no code/ABI change.

## 5. Next actions

1. Keep this doc as the `murphy-siq` close-out record.
2. If demand appears, file follow-up beads: (a) cheap-group smell pack,
   (b) expensive-group smells gated on A2, (c) Flog-style rating in B9 report.
