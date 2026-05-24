# ADR 0013 — Phase 4 autocorrect contract extension (`Offense.autocorrect`)

- Date: 2026-05-20
- Status: Accepted
- Issue: Phase 4 Task 1 (`murphy-hwe.1`) — autocorrect contract extension
- Effect: intentionally extends the frozen ADR 0006/0007 offense-JSON shape with
  an optional `autocorrect` field; five frozen fields are unchanged; byte-identity
  of existing snapshots is preserved
- Gated by: ADR 0012 (Phase 3 PASSED; Phase 4 may start)
- Referenced by: ADR 0014 (Phase 4 Gate, `murphy-hwe.8`, to be written)

## Context

ADR 0006 froze the Phase-1 offense-JSON shape as five fields:

```text
{ file, cop_name, range:{start_offset,end_offset}, severity, message }
```

ADR 0007 (Phase 2 gate) and ADR 0012 (Phase 3 gate) confirmed that the shape
was byte-for-byte unchanged through all of Phase 2 and Phase 3. ADR 0012
explicitly records:

> soft-(a) honored end-to-end: the `Murphy::Cop` `fix` block is captured but
> **not applied**; `Offense` JSON is the ADR-0006 frozen shape with **no
> `autocorrect`** field. **Phase 4 owns autocorrect application + the deliberate
> `Offense.autocorrect` contract extension.**

Phase 4 now opens. This ADR documents the intended, deliberate extension of that
contract and the constraints that preserve byte-identity for existing tooling.

## Decision

### The five frozen fields are immutable

`file`, `cop_name`, `range`, `severity`, `message` — their names, types, and
serde representations are **unchanged**. Any downstream consumer that reads only
these five fields sees identical JSON to Phase 1–3. This invariant is permanent
and is not renegotiated by Phase 4.

### A sixth field `autocorrect` is added as an optional extension

The wire shape for an offense with a fix:

```json
{
  "file": "a.rb",
  "cop_name": "Murphy/Foo",
  "range": { "start_offset": 0, "end_offset": 4 },
  "severity": "warning",
  "message": "use foo",
  "autocorrect": {
    "edits": [
      { "range": { "start_offset": 0, "end_offset": 4 }, "replacement": "foo" }
    ]
  }
}
```

The wire shape for an offense **without** a fix is byte-identical to the
Phase 1–3 frozen form — the `"autocorrect"` key is **absent**, not present with
`null`:

```json
{
  "file": "a.rb",
  "cop_name": "Murphy/Foo",
  "range": { "start_offset": 0, "end_offset": 4 },
  "severity": "warning",
  "message": "use foo"
}
```

### Byte-identity guarantee via serde attributes

The `autocorrect: Option<Autocorrect>` field carries two serde attributes:

- `#[serde(skip_serializing_if = "Option::is_none")]` — omits the key entirely
  when the value is `None`; the key is **absent**, not `"autocorrect": null`.
- `#[serde(default)]` — when deserializing older JSON that lacks the key, the
  field defaults to `None` without error (forward-compatible deserialization).

Together these ensure:

1. `sample_project.json` (the ADR 0006/0007/0012 regression anchor) is
   **byte-identical** after this change. The sample project's offenses have no
   fix, so `autocorrect` is `None`, so the key is absent, so the serialized
   output is unchanged.
2. Older tooling that parses the five-field shape continues to work without
   modification.

### `Edit` is a separate wire-contract type, not a promotion of `sdk::FixEdit`

Phase 3 introduced `mruby::sdk::FixEdit` as a crate-private synthetic
placeholder to capture the `fix` block without applying it. That type was
deliberately kept internal and is **not** the wire type.

`offense::Edit` (introduced by this ADR) is an independent, `pub` wire-contract
type with its own stability guarantee. It serialises identically to the design
§5 shape. The bridge between a mruby cop's `fix` block and `offense::Edit`
values — i.e. marshalling `sdk::FixEdit` into `Edit` and wiring the result into
`Offense.autocorrect` — is **Phase 4 Task 2's responsibility** and is out of
scope for this task.

This separation is deliberate: the wire contract is pinned now (this ADR), and
the mruby marshal path is implemented independently without renegotiating the
shape.

### JSON field-order convention

There is no ADR that pins JSON key order. `serde`'s struct-field order is the
de-facto contract (serde_json serialises struct fields in declaration order).
`autocorrect` is declared as the **last** field in `Offense`, so it appears last
in JSON. This convention must be preserved if the field list is ever extended
further.

### `Offense::new` signature is unchanged; `Offense` is `#[non_exhaustive]`

The existing five-argument `Offense::new(file, cop_name, range, severity,
message)` is preserved exactly. `autocorrect` is initialised to `None`. Callers
that need to attach a fix use the new builder method
`Offense::with_autocorrect(ac: Autocorrect) -> Offense` (fluent/consuming
setter). Every existing in-tree call site uses `Offense::new` (verified:
`murphy-cli` constructs only via `Offense::new`; struct-literal construction
occurs only inside `murphy-core`), so no existing call site is broken.

Adding a `pub` field to a fully-`pub` struct **is** a source-breaking change
for any out-of-crate caller that builds the struct by literal. To make the
"contract fields can grow without a Rust source break" claim mechanically true
rather than aspirational, `Offense` carries `#[non_exhaustive]`. This forbids
struct-literal construction (and exhaustive destructuring) from outside
`murphy-core`, so the stable Rust surface is exactly `Offense::new` +
`Offense::with_autocorrect`. In-crate literals (cops, tests) are unaffected by
`#[non_exhaustive]`. Phase 4+ may add further contract fields without a Rust
source break for downstream crates. The JSON wire shape (§ above) and these two
constructors are the only stable surfaces; out-of-crate struct-literal
construction is explicitly unsupported.

## Rust types added (Phase 4 Task 1)

```rust
// crates/murphy-core/src/offense.rs

pub struct Edit {
    pub range: Range,
    pub replacement: String,
}
// derive: Debug, Clone, PartialEq, Eq, Serialize, Deserialize

pub struct Autocorrect {
    pub edits: Vec<Edit>,
}
// derive: Debug, Clone, PartialEq, Eq, Serialize, Deserialize

// Added to Offense:
#[serde(skip_serializing_if = "Option::is_none", default)]
pub autocorrect: Option<Autocorrect>,

// Added to impl Offense:
pub fn with_autocorrect(mut self, ac: Autocorrect) -> Offense { ... }
```

## Regression guard

`crates/murphy-cli/tests/snapshots/sample_project.json` MUST remain
**byte-identical** after this task. The sample project has no cops that emit a
fix, so every offense has `autocorrect: None`, so the key is absent, so the
snapshot is unchanged. `integration_snapshot` and `parallel_determinism` tests
verify this. Any diff in that file means the serde attribute is not working
correctly and is a hard failure.

## Scope (explicitly out)

- `mruby::sdk::FixEdit` and the captured-fix seam are **not touched** by this
  task. Their modification is Phase 4 Task 2.
- No autocorrect is applied to source in this task. Application logic (reparse
  loop, idempotency, etc.) is Phase 4.
- No changes to exit codes, CLI output format, or any other contract.
- No changes to `crates/murphy-cli` in this task.

## Phase 4 Task 2 update (murphy-hwe.2)

`mruby::sdk::FixEdit` and `CopRun.fixes` were removed in Phase 4 Task 2.
The ADR 0009 field-disjointness soundness argument now covers only the
`ctx ↔ sink` pair (no second `UnsafeCell`); the prior `sink`/`fixes` single-writer
clause is no longer applicable and has been removed from the `CopRun` doc.

## Phase 4 Task 5 update (murphy-hwe.5)

`run_to_fixpoint` (in `crates/murphy-core/src/autocorrect.rs`) implements the
reparse-and-rerun loop with max-iteration cutoff and oscillation detection.

Oscillation semantics (APIN1): `FixpointOutcome::corrected` = the re-visited
state at cycle detection (`next`, which was found already in `seen`), NOT the
previous round's output.  Rationale: re-feeding this value to `run_to_fixpoint`
immediately re-detects the oscillation and is therefore stable (weak idempotency).
