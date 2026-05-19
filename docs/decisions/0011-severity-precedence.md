# ADR 0011 — Severity-precedence dedupe (Error > Warning on a 4-tuple collision)

- Date: 2026-05-19
- Status: Accepted
- Issue: Phase 3 walking-skeleton Task 6 (severity-precedence dedupe)
- Effect: changes **which** offense survives a severity-only 4-tuple collision
  in `aggregate` (now max-severity); offense JSON/struct contract UNCHANGED
- Reserved by: ADR 0006 / ADR 0007 ("Phase 3 owns severity precedence");
  ADR 0010 ("Task 6 with its own ADR (renumbered below)")

## Context

Phase 1/2 `aggregate` (`crates/murphy-core/src/aggregator.rs`):

1. Stable `sort_by` on the **total order**
   `(file, start_offset, end_offset, cop_name, message, severity)` — a genuine
   total order over all five `Offense` fields (ADR 0007 determinism keystone;
   `Severity: Ord`, derive order `Warning < Error`).
2. Order-preserving dedupe keyed by the **4-tuple**
   `(file, cop_name, range, message)` — `severity` deliberately **excluded from
   the key** — keeping the FIRST occurrence.

Net Phase-1/2 behavior on a severity-only 4-tuple collision: because
`severity` was the *ascending* final sort tiebreaker, the **enum-min**
severity (`Warning`) sorted first and the keep-first dedupe kept `Warning`.
This was deterministic but asserted **no precedence policy** — it was a
placeholder, and ADR 0006/0007 explicitly reserved the precedence decision for
Phase 3 because no two engines could yet flag the same site.

Phase 3 introduces a second engine (embedded mruby user cops alongside the
native cop engine). Both engines can now legitimately emit an offense identical
on `(file, cop_name, range, message)` but differing in `severity` (e.g. a
native cop reports `Error`, a user cop reports `Warning` at the same site, or
vice versa). Collapsing such a collision to `Warning` would **mask a real
`Error`** behind a duplicate `Warning` — incorrect once two engines coexist.

## Decision

**On a `(file, cop_name, range, message)` 4-tuple collision, the
maximum-severity offense survives. `Error > Warning`.**

- The surviving offense for any set of 4-tuple-equal offenses is the one with
  the **highest severity**, **deterministically and independent of input /
  engine / thread order**.
- **Precedence order (total order over current `Severity` variants):**
  `Warning < Error`, so `Error` wins. The `Severity` enum today has exactly
  `Warning` and `Error`. **Any future `Severity` variant MUST be slotted into
  this total order by its real severity** (more severe = wins a collision); the
  precedence rule is "max by severity", not "max by enum discriminant" — they
  coincide today only because `Severity`'s derive order already runs
  least-severe → most-severe.

### Mechanism (minimal change — approach (a))

The severity component of the `aggregate` sort tiebreaker is flipped from
ascending to **descending** (`b.severity.cmp(&a.severity)` instead of
`a.severity.cmp(&b.severity)`). Within a 4-tuple-equal group the max-severity
offense then sorts FIRST, and the **unchanged** keep-first dedupe yields the
max-severity survivor. This is the minimal correct change: one comparator term
reversed, dedupe logic untouched, key untouched. (The equivalent alternative —
keep the ascending sort, make dedupe keep the max-severity among 4-tuple-equal
entries — is rejected as strictly more code in the load-bearing dedupe loop for
no behavioral gain.)

### The dedupe KEY is unchanged; severity is the *resolution* rule, not identity

`severity` remains **excluded from the dedupe key**. The key is still exactly
the 4-tuple `(file, cop_name, range, message)`. Severity is **not** part of
offense *identity* (two offenses that differ only in severity are still "the
same offense" for collision purposes — we must NOT emit both). Severity is now
the **collision-resolution rule**: when the key collides, the higher severity
is the survivor. This distinction is load-bearing — adding severity to the key
would instead emit *both* a `Warning` and an `Error` for one site, which is
the opposite of the intent.

### JSON / `Offense` struct / serialization UNCHANGED

`crate::Offense`, `crate::Severity`, and their serde representation are
**byte-for-byte unchanged**. No field added (no `autocorrect`, per ADR 0006),
no rename, no serde attribute change. `offense.rs` is **not touched** by this
ADR. ONLY *which* of two collision-equal offenses survives changes (`Error`
instead of `Warning`). Downstream consumers see the identical JSON shape; they
only ever see the more-severe offense at a colliding site, which is strictly
more correct information in the same schema.

## ADR-0007 total-order determinism is preserved

ADR 0007 made the `aggregate` sort a total order and named it the *sole
guarantor* of deterministic output under concurrency. This ADR does **not**
weaken that:

- The sort key still covers all five `Offense` fields; it is still a total
  order; the comparison is still total and antisymmetric (reversing one
  component's direction keeps it a total order).
- **Distinct surviving offenses differ in the 4-tuple** (≥ one of
  `file` / `cop_name` / `range` / `message`). Their relative order is therefore
  decided by an **earlier** sort component (file, then start/end offset, then
  cop_name, then message) — the severity tiebreaker is reached *only* when all
  four 4-tuple components are equal, i.e. *only within a single collision
  group*, every member of which is deduped down to one survivor. Reversing the
  severity direction therefore changes ONLY which member of a collision group
  is kept; it provably **cannot** reorder two offenses that both survive into
  the output. Output ordering is bitwise-identical to before for any input with
  no severity-only collision.
- The result is still **input/engine/thread-order independent**: max-severity
  is a property of the *set* of colliding offenses, not of their arrival order.

## Consequence: one deliberate, predicted test flip

This ADR **deliberately flips** the Phase-1 test
`severity_only_dup_collapses_to_first_phase1_behavior` (renamed to
`severity_collision_resolves_to_higher_severity_phase3`) and case (d) of
`aggregate_total_order_is_input_independent`. Both previously asserted the
enum-min (`Warning`) survivor. Their Phase-1 comments **explicitly predicted
this Phase-3 flip** ("Phase 3 … will redefine which severity wins — flipping
THIS test is correct evolution, not a regression"). The new assertion: the
`Error` survives, identically for forward and reversed input. **This is the
one intended behavior change of Task 6**, fully documented here; it is correct
evolution per ADR 0006/0007's reservation, **not** a regression.

## Regression guard / contract proof

`crates/murphy-cli/tests/snapshots/sample_project.json` MUST remain
**byte-identical**. The sample project exercises only `Murphy/NoReceiverPuts`
(native, single engine) → no cross-engine, no severity-only 4-tuple collision
→ aggregate output unchanged. A byte-identical `sample_project.json` (verified
by `integration_snapshot` and `parallel_determinism`) is the **proof that the
JSON shape / frozen ADR-0006/0007 contract is preserved**: the only thing that
could have changed is collision resolution, and the snapshot has no collision
to resolve, so it must be unchanged. Any diff there would mean the change
leaked beyond collision resolution and is a hard failure.

## Scope (explicitly out)

The Severity-`Ord` ⇄ collision-precedence coupling invariant is documented at
the `Severity` enum in `crates/murphy-core/src/offense.rs` (the trap site where
a variant is added) and pinned by a compile-time assertion there; any new
`Severity` variant MUST be declared in true severity order or the build fails.

This ADR/Task is ONLY the aggregator collision-resolution rule + this ADR + the
flipped/renamed tests + new precedence tests. It does NOT wire mruby cops into
the CLI pipeline (Task 7), introduce `[cops]` config, or change any other
behavior. The new precedence tests construct colliding `crate::Offense` values
directly (the unit level `aggregate` operates at), mirroring existing
aggregator tests; no mruby cop is run.
