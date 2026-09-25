# ADR 0047 — Persistent lint-result cache (A5 incremental analysis)

- Date: 2026-09-25
- Status: Accepted
- Issue: `murphy-fmw.1.1` (A5: persistent cache + incremental analysis)
- Parent: `murphy-fmw.1` (Phase 8 epic)
- Integrates: `murphy-fvh` (Post-Phase-2 persistent on-disk `source_digest` cache)
- Depends on: ADR 0040 (arena binary cache format v1), ADR 0039 (translation cost gate)
- Related: ADR 0006 (frozen JSON contract — unchanged), ADR 0038 (single-surface plugin ABI)

## Context

ADR 0040 gave Murphy a persistent *AST* cache (`$XDG_CACHE_HOME/murphy/v1`,
`content_hash(source)` + version key). The second consecutive `murphy lint`
still re-ran every cop, so the Phase 8 gate #3 (2nd run 5x+ faster) could
not pass on parse savings alone. `murphy-fvh` (filed by Phase 2) asked for
a persistent on-disk `source_digest` cache enabling cross-run skip of
unchanged files, including a cache dir, invalidation strategy, and CLI
cache flags. The post-fmw roadmap redefined that scope as A5
(`murphy-fmw.1.1`): `source_digest` persistence **plus** AST/analysis-result
caching as the foundation for B1 watch/daemon.

## Decision

### 1. Two caches, one root

- `Cache` (existing, ADR 0040): arena AST bytes, `$root/<aa>/<hex>.ast`.
- `ResultCache` (new, `murphy-cache/src/results.rs`): per-file lint
  *results* — the post-inline-filter `Vec<Offense>` serialized as JSON —
  at `$root/results/<aa>/<hex>.json`.

Both live under `$XDG_CACHE_HOME/murphy/v1` (else `$HOME/.cache/murphy/v1`),
both are best-effort (any failure is a silent miss, never an error), both
honour `--no-cache` and `MURPHY_NO_CACHE`.

### 2. Cache key mixes cop pack version + config (stale-cache avoidance)

```
ast_key    = sha256(content_hash || version_key)
result_key = sha256(content_hash || result_version_key)
version_key        = sha256(murphy_version || target_triple || layer_version)
result_version_key = sha256(version_key || lint_fingerprint)
lint_fingerprint   = sha256(murphy-core version || plugin ABI
                     || sorted dispatch cop names + per-cop options JSON
                     + severity || pack names || AllCops context
                     + target versions + extensions flag || full rule maps)
```

`content_hash` is `sha256(source)` (`source_digest`). Different cop sets
or configs therefore miss by construction and never read stale offenses.
The roadmap's design question ("`source_digest` alone vs mixing cop pack
version + config") is decided: **mix them**.

### 3. Shareability guard

The result cache is keyed per path (content + path + fingerprint), not
purely by content: identical content at different paths — which can
yield different offenses under per-cop `Include`/`Exclude` scopes,
including pack-bundled defaults — never shares an entry. In-run
content grouping still lints once per content group and fans out (with
each miss path's entry populated separately). The result cache is
enabled whenever the run is deterministic per path:

- no mruby user cops (sources outside the fingerprint),
- no `--fix` intermediate pass (fixpoint sources must not poison finals),
- no `--profile` / `--debug` (timing paths always run cops).

Per-cop path scopes need no gating (per-path keys are scope-safe).
Otherwise the run falls back to the AST cache. Same output, slower.

### 4. Known limitation (documented, RuboCop-parity policy)

A same-name dynamic `.so` pack rebuilt with different cop logic but an
unchanged version string is **not** detected (names, not bytes, feed the
key). The remedy is `murphy cache clean` — the same policy RuboCop uses
for implementation changes without a version bump. Hashing `.so` bytes
at load time is a future improvement; the key shape already supports it
(extra fingerprint bytes).

### 5. CLI contract

- `murphy lint --no-cache`: disables both caches (existing flag, widened).
- `MURPHY_NO_CACHE=1`: disables both (existing env, widened).
- `murphy cache stat`: prints root, AST/result entry counts, total bytes.
- `murphy cache clean`: removes the whole `$root` tree.

### 6. What is cached

Pre-aggregate, pre-B4-enrichment `lint_source` output (post-inline-filter
offenses, including the single `Murphy/Syntax` offense for broken files).
Aggregation, B4 enrichment, baseline filtering, and formatting still run
every invocation, so the ADR 0006 JSON contract is byte-identical whether
the run hits or misses.

## Consequences

- Phase 8 gate #3 passes: 50 large Ruby files (744 KB), release build,
  1st run 0.59 s → 2nd run 0.10 s (**5.8x**). Small-file corpora are
  startup-bound and show less; the gate corpus must be dispatch-bound.
- `murphy-fvh` scope (persistent `source_digest` cache + dir +
  invalidation + CLI flags) is fully covered and closed as integrated.
- B1 watch/daemon is unblocked: the result cache is the cross-run skip
  primitive a file watcher needs.
- Orphaned entries (old config keys) accumulate until `cache clean`;
  no TTL/size cap in v1 (same as ADR 0040).

## Alternatives considered

- AST-only + faster dispatch: rejected — dispatch dominates, parse
  savings alone cannot reach 5x.
- SQLite result store: rejected — file-per-entry needs no new
  dependencies, matches the AST layout, and is trivially inspectable.
- Path-keyed results: rejected — content-keyed shares identical files
  and survives renames; path scopes are handled by disabling sharing.
