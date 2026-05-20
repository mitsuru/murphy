# ADR 0022 — Phase 7.3 LSP contract and diagnostic mapping

- Date: 2026-05-20
- Status: Accepted
- Issue: `murphy-bn3.3`
- Parent: `0020-phase-6-gate-review`
- Scope: `crates/murphy-cli`

## Decision

`murphy lsp` will be a dedicated stdio JSON-RPC command that exposes diagnostics
from the existing `murphy-core` offense pipeline with a narrow, explicit protocol
surface. The first milestone is deterministic publish and lifecycle handling,
with quick-fix exposed as a future-guarded envelope.

## Contract and scope

- Command: `murphy lsp`
- Transport: stdin/stdout JSON-RPC over stdio, UTF-8.
- Version negotiation:
  - Server claims support for `initialize`/`initialized`/`shutdown`/`exit`.
  - Server capability advertisement starts conservative.
- Implemented method set (MVP):
  - `initialize`
  - `initialized`
  - `shutdown`
  - `exit`
  - `textDocument/didOpen`
  - `textDocument/didChange`
  - `textDocument/didClose`
  - `textDocument/publishDiagnostics` (server push)
  - `textDocument/codeAction` (optional but disabled-empty until payload is validated)

## Behavioral constraints

- Additive API only: this contract must not change existing `murphy lint` output
  shape or timing profile.
- Keep core watchdog behavior unchanged:
  `COP_DEADLINE` and existing isolated-run semantics in `murphy-core` remain the
  authoritative execution boundary.
- On unsupported methods the server returns an explicit JSON-RPC error
  (`MethodNotFound`), not silent no-op success.

## Offense-to-diagnostic mapping (phase 1)

- `Offense.range` maps to `Diagnostic.range` (0-based line/char conversion based on
  original UTF-8 byte offsets).
- `Diagnostic.message` uses `offense.message`.
- `Diagnostic.code` uses `offense.cop_name`.
- Severity mapping:
  - `severity: error` -> `DiagnosticSeverity::Error`
  - `severity: warning` -> `DiagnosticSeverity::Warning`
  - `severity: convention` / `refactor` / other -> `DiagnosticSeverity::Information`
- `Diagnostic.source` is `Some("murphy")`.
- `RelatedInformation` is initially omitted.

## Document lifecycle and publish policy

- `textDocument/didOpen`: lint request is triggered for the opened file path and
  diagnostics are published for that URI.
- `textDocument/didChange`: currently use full-text re-lint of the known URI.
- `textDocument/didClose`: clear diagnostics for that URI immediately.
- If lint errors from core include syntax/parsing errors, map as normal offenses and
  publish them (no separate transport-level error). Core failures are surfaced via
  JSON-RPC error responses on request-like operations.

## Path and URI policy

- Input file handling is delegated to `std::path`-based open/change payloads.
- URI resolution is `file://` only for phase 1; non-file URIs return `InvalidParams`.
- Empty/virtual documents are supported only if the client sends concrete source
  text in open/change events; URI remains authoritative for caching and dedupe.

## Diagnostics shape tests

- Add integration test `crates/murphy-cli/tests/lsp_smoke.rs` (or equivalent) to
  assert:
  - initialize+shutdown handshake completes,
  - open/change triggers at least one diagnostic for a sample offense fixture,
  - diagnostic carries expected `source`, `code`, and `message` prefix semantics.
- Add a focused test for `didClose` clearing diagnostics (either by notification
  count or last message snapshot).

## Quick-fix path (explicitly deferred in v1)

- `codeAction` will be registered but return `[]` when mapping is unavailable.
- No auto-constructed `TextEdit` payload is emitted until range and replacement
  mapping is stable across UTF-8 boundaries and line-ending variants.

## Open items before implementation

- Confirm client support assumptions for diagnostic version target (3.16 vs 3.17)
  in test fixtures.
- Finalize mapping of `Range` offsets into UTF-16 character positions if the first
  transport test indicates a mismatch with client expectations.
