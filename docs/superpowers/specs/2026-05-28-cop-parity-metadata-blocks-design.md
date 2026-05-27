# 2026-05-28 Cop parity metadata blocks

## Goal

Murphy needs a reliable way to answer "which cops have RuboCop parity, which
are partial, and which are stubs?" without re-reading scattered comments,
searching beads issues by hand, or relying on memory.

The source of truth should live next to each cop implementation so AI agents
and human reviewers see it while reading the code. It must also be structured
enough for tests and debug commands to parse later.

## Decision

Add a machine-readable fenced block to each cop file's top module doc comment.
The block is named `murphy-parity`.

Example:

```rust
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/HashSyntax
//! upstream_version_checked: 1.86.2
//! version_added: "0.9"
//! safe: true
//! supports_autocorrect: true
//! status: partial
//! gap_issues:
//!   - murphy-90zo
//! notes: >
//!   EnforcedShorthandSyntax and target Ruby version behavior are not yet
//!   covered.
//! ```
```

The block is documentation first and metadata second. It should be easy to read
in the cop file, easy to find with `rg "murphy-parity"`, and easy to parse with
a small Rust or script-based checker.

## Schema

Required fields:

- `status`: one of `stub`, `partial`, `verified`, `custom`
- `notes`: short human-readable explanation

Required for RuboCop-derived cops:

- `upstream`: `rubocop`, `rubocop-rails`, or `rubocop-rspec`
- `upstream_cop`: canonical RuboCop cop name
- `upstream_version_checked`: version used for the parity audit
- `gap_issues`: list of beads issue ids, empty only for `verified`

Optional fields:

- `version_added`: RuboCop `VersionAdded` value when known
- `version_changed`: list of RuboCop `VersionChanged` values when known
- `safe`: RuboCop `Safe` value when known
- `supports_autocorrect`: RuboCop autocorrect support when known

`custom` cops, such as `Murphy/NoReceiverPuts`, may omit upstream fields and
use `notes` to explain why they have no RuboCop ancestor.

## Status Semantics

- `stub`: registered for config/listing compatibility but intentionally inert.
- `partial`: implemented and dispatched, but known RuboCop parity gaps remain.
- `verified`: audited against the named upstream version with no known parity
  gaps. `gap_issues` must be empty.
- `custom`: Murphy-specific cop with no upstream parity target.

`verified` is intentionally strict. Small known limitations should keep the cop
at `partial`, even if the implementation is good enough for most users.

## Validation

Add a checker in the implementation phase that:

1. Finds every registered cop file and requires a `murphy-parity` block.
2. Parses the block as YAML.
3. Validates required fields and status-specific invariants.
4. Fails if `verified` has non-empty `gap_issues`.
5. Fails if `partial` has no `gap_issues` and no explanatory `notes`.
6. Allows Rails arena-migration stubs to remain `stub`.

The checker does not need live beads integration in v1. It should treat issue
ids as strings so validation is deterministic and offline.

## CLI Exposure

The first implementation should preserve the current default table output of
`murphy cops list`.

Later, `murphy cops list --format=json` can include:

```json
{
  "name": "Style/HashSyntax",
  "status": "enabled",
  "parity": {
    "status": "partial",
    "upstream": "rubocop",
    "upstream_cop": "Style/HashSyntax",
    "upstream_version_checked": "1.86.2",
    "gap_issues": ["murphy-90zo"]
  }
}
```

A future `--verbose` table mode may add a `PARITY` column. This is a debug and
project-management aid only; lint behavior, dispatch, autocorrect, config
semantics, and plugin ABI must not depend on parity metadata.

## Non-Goals

- Do not add parity fields to `#[cop(...)]`.
- Do not extend plugin ABI for parity metadata.
- Do not make parity affect lint results.
- Do not require live network or live beads DB access during tests.
- Do not solve all existing cop gaps in this work.

## Migration Plan

Start with active built-in and plugin cops:

- `murphy-std` active cops
- `murphy-rspec` active cops
- `murphy-rails` real arena-dispatch cops
- `murphy-example-pack` demo cops, marked `custom`

Rails no-op stubs can be handled in bulk after the checker supports generated
or centralized stub metadata. Until then, the checker may explicitly scope
itself to real cop files under `src/cops/**`.

## Open Follow-Ups

- Decide whether to parse parity blocks in Rust tests or with a small script.
- Decide exact CLI flag shape for verbose parity display.
- Backfill `version_added`, `safe`, and `supports_autocorrect` from upstream
  docs over time; these fields are useful but not required for the first pass.
