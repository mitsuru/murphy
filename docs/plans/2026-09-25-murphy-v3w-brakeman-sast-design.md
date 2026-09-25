# Brakeman-compatible SAST pack on Murphy (murphy-v3w)

Date: 2026-09-25
Status: brainstorm (pre-ADR, docs/design only — no code, no ABI change)
Issue: murphy-v3w (post-fmw D-axis, Phase 8 A6a dependent)
Ref: docs/plans/2026-05-20-murphy-post-fmw-roadmap-design.md §7

## 1. Goal

Ship a Brakeman-equivalent Rails security scanner as a pluggable cop pack
(`murphy-sast` / `murphy-brakeman`) on the Phase 8 A6a pack mechanism —
not in Murphy core. Two policies are realistic and combined:

- **α (native re-implementation):** SAST cops written natively in Rust
  against `murphy-plugin-api`, reusing the A6a distribution contract
  (cop ID namespace, versioning per C4, `[[plugins]]` loading).
- **γ (Brakeman output compat):** emit Brakeman-compatible warning codes
  and JSON so existing CI/tooling consuming `brakeman -f json` keeps working.

Rejected (roadmap decision, recorded here for traceability):

- **β (mruby wrap of Brakeman itself):** rejected. Brakeman needs full app
  context (routes, initializers, ERB, Gemfile) plus unrestricted `require`;
  the Phase 7 restricted mruby sandbox (allowlist, deadline, per-cop
  isolation) cannot host it, and version drift would pin Murphy to
  Brakeman internals.
- **δ (side-by-side parallel run):** rejected. No integration value —
  users can already run both tools; a Murphy pack must add single-config,
  single-baseline, SARIF-unified value.

## 2. Background: what Brakeman provides

Brakeman is a Rails-only SAST scanner. The subset Murphy must cover:

| Category | Examples |
|----------|----------|
| Injection | SQLi, command injection, mass assignment |
| XSS | unescaped output in views, `html_safe` / `raw` misuse |
| Unsafe code | `eval`, unsafe reflection (`send(params…)`), deserialization (`YAML.load`, `Marshal.load`) |
| App config | dangerous `attr_accessible`, open redirect, file access, weak crypto/hashing |

Brakeman grades each warning with **confidence** (High / Medium / Weak),
orthogonal to severity, and filters with `-m <level>` / `-x <checks>`.
Its JSON output carries `warning_code`, `confidence`, `file`, `line`,
`message`, and `link`. γ-compat means preserving these fields.

## 3. Required Murphy base extensions (all preconditions, not this issue)

The pack is impossible on per-file cops alone. Four base extensions are
required, each a separate future issue/ADR:

1. **Cross-file analysis (A2 promotion).** Constant resolution, call/route
   graph (route → controller → model → view), unused-path pruning.
   Today Murphy is per-file; SAST needs at minimum controller↔model
   symbol resolution. A2 is currently backlog — SAST is its first hard
   consumer, so SAST scope must pull a minimal A2 slice (project symbol
   table) forward.
2. **Taint-tracking primitive.** `source → sanitizer → sink` declarations
   exposed via `murphy-plugin-api` (or a `murphy-taint` helper crate packs
   depend on). Staged: (a) intra-file dataflow on top of ADR 0044
   `VarSemanticModel`; (b) inter-procedural summaries (per-method
   taint signatures); (c) cross-file propagation via the A2 symbol table.
   Cops declare sources/sinks; the engine computes reachability. Never
   per-cop ad-hoc dataflow.
3. **Rails-aware project model.** A host-side service (routes, controller/
   model/view inventory, ERB surface, config flags, Gemfile.lock) built
   once per run and exposed read-only to packs through `Cx`. Packs must
   NOT re-scan the project themselves (duplicated I/O, cache-key skew
   with A5 persistent cache).
4. **Confidence level (severity extension).** Current `Severity` is
   `Warning | Error` (wire `u8`, `255` = unset). SAST needs
   `Confidence: High | Medium | Weak` as a separate orthogonal field on
   the offense (Brakeman semantics), with `--min-confidence` filtering
   and JSON passthrough. Must be additive: new optional field, new wire
   byte, old consumers ignore it. No existing severity/ABI behavior changes.

## 4. Pack sketch

- **Crate:** `murphy-sast` (name TBD at ADR time; `murphy-brakeman` reserved
  as alias if γ-compat is exact). Lives under `crates/`, distributed as an
  A6a pack, versioned per C4.
- **Cop IDs:** `SAST/<Check>` namespace (e.g. `SAST/SqlInjection`,
  `SAST/CrossSiteScripting`, `SAST/CommandInjection`,
  `SAST/UnsafeDeserialization`, `SAST/MassAssignment`,
  `SAST/OpenRedirect`). Each cop maps to one Brakeman `warning_code`
  (mapping table shipped in pack docs).
- **MVP check subset (staged):** P0 — SQLi, command injection, `eval`/
  unsafe reflection, unsafe deserialization (all mostly intra-file +
  taint stage (a)). P1 — XSS incl. ERB surface (needs project model +
  view inventory). P2 — mass assignment, open redirect, config checks
  (needs cross-file + route graph).
- **Config:** standard cop options (`Enabled`, `Severity`) plus
  `MinConfidence` global and per-cop `Exclude`/baseline entries in
  `murphy.toml` / `.murphy-baseline.toml` (B3). No Brakeman `config/`
  file import in MVP (later γ enhancement, explicitly out of scope).
- **γ output:** `--format brakeman-json` (or a `brakeman` formatter under
  the B7 formatter extension) emitting Brakeman's JSON fields
  (`warning_code`, `confidence`, `file`, `line`, `message`, `link`).
  SARIF mapping reuses the B5/B7 SARIF path with `confidence` as a
  `result.properties` extension. Default Murphy JSON (ADR 0006) is
  unchanged; confidence appears as an additive optional field.

## 5. Staging & dependencies

```text
Phase 8 A6a pack mechanism ── prerequisite (this design assumes it)
  ├─ A2 slice: project symbol table ──→ taint stage (b/c), P1/P2 checks
  ├─ ADR 0044 VarSemanticModel ──→ taint stage (a), P0 checks (already accepted)
  ├─ Rails project model service ──→ P1 (views/ERB), P2 (routes/config)
  ├─ Confidence field (additive) ──→ γ output, --min-confidence
  └─ B7 formatter extension ──→ brakeman-json formatter
```

Suggested order: confidence ADR (small, additive) → taint stage (a) +
P0 pack MVP → project-model spike → P1 → A2-slice + P2. Each stage is
independently shippable as a pack version bump (C4).

## 6. Non-goals (this brainstorm)

- ERB/HAML parsing itself (pack consumes the project model's view surface;
  parser choice is the project-model issue's decision).
- Auto-fix for SAST offenses (detection only; A4 smart-autocorrect
  explicitly excludes security rewrites).
- Full Brakeman check parity (document the shipped subset; parity is
  a versioned pack roadmap, not a gate).
- bifurcating `Severity` into confidence (kept orthogonal by design).

## 7. Open questions (for the ADR session)

1. Taint engine placement: `murphy-plugin-api` primitive vs separate
   `murphy-taint` crate — who owns inter-procedural summaries?
2. Project model caching: does the A5 persistent-cache key cover the
   project model snapshot (routes/config mtime set)?
3. `warning_code` stability contract: mirror Brakeman's numeric codes
   verbatim, or namespaced (`murphy-001`) with a mapping table?
4. ERB handling: lint generated Ruby, or a view-surface abstraction?
5. Confidence default threshold (`Medium`?) and its interaction with
   `Severity` in SARIF/GitHub code-scanning upload (B5).

## 8. Next actions

1. Phase 8 A6a gate first — no SAST code before an external pack loads e2e.
2. Open follow-up issues: taint-primitive ADR, project-model spike,
   confidence-field ADR, pack MVP (P0 subset).
3. Decide `warning_code` contract (Q3) before P0 ships — it is the γ
   compatibility promise and hard to change later.
