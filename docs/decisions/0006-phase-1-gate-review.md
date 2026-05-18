# ADR 0006 — Phase 1 Gate review (walking skeleton complete; contract frozen)

- Date: 2026-05-19
- Status: Accepted — **GATE PASSED**
- Epic: `murphy-03u` (Phase 1 walking skeleton)
- Reviews: P1 Tasks 1–10 (`murphy-kwq, tdd, edp, 6i5, iah, e41, e1k, vwb, c5e, gv5`)
- Effect: freezes the Phase-1 contract; Phase 2+ build on it without renegotiation

## Verdict

**PASS.** The Phase 1 walking skeleton is implemented end-to-end and verified.
Every task went through implement → independent spec-compliance review →
independent code-quality review → fix loop, all on the project toolchain
(mise-pinned Rust 1.95.0). The contract Phase 2+ depends on is now frozen.

## End-to-end demo (binary, observed)

`murphy lint` on representative inputs:

- clean file → stdout `[]`, exit `0`
- dirty file (`puts "x"`) → one `Murphy/NoReceiverPuts` offense, exit `1`
- broken file (`def (`) → one `Murphy/Syntax` offense (verbatim prism message,
  byte range), cops skipped, exit `1`
- missing file → exit `2`
- multi-file → offenses aggregated and sorted by `(file, start_offset)`,
  clean files contribute nothing, exit `1`

Full suite: **20 tests, 0 failed** (`murphy-cli` cli 5 + integration_snapshot
1; `murphy-core` lib 13 + cop integration 1). `cargo fmt --check` exit 0;
`cargo clippy --all-targets -- -D warnings` clean.

## Frozen contract (Phase 2+ must not renegotiate)

- **Offense JSON shape** (design §5 subset, no `autocorrect` until Phase 4):
  `{file, cop_name, range:{start_offset,end_offset}, severity, message}`.
- **Offsets are BYTE offsets** (ADR 0001). The `multibyte.rb` snapshot fixture
  bakes in the byte offset (selector at byte 903, *not* char 893) and is a
  contract artifact — editing it requires re-blessing the snapshot.
- **Exit codes:** `0` none / `1` offenses / `2` config-or-cop-or-file-setup
  (bad usage, missing file) / `3` internal (panic guard). BrokenPipe → `0`.
- **`SYNTAX_COP_NAME = "Murphy/Syntax"`** is the stable syntax-error cop name
  (single shared const in `murphy-core`).
- **Determinism:** cross-file output ordering comes from `aggregate`'s stable
  sort, not arg order; the committed `sample_project.json` snapshot is the
  regression guard.
- **`lint_one_file`** is the per-file unit Phase 2's parallel engine + file
  discovery will wrap (clean extraction, no rework needed).

## Phase-2 items tracked (deferred deliberately, NOT blocking)

- `murphy-tdl` — add `rust-toolchain.toml` (cargo-native pin alongside mise).
- `murphy-nkq` — native cop suite hygiene (name-set scaling, re-export layout).
- `murphy-eu9` — aggregator tie-break robustness (extend sort key beyond
  `(file,start_offset)` before a 2nd native cop) + extra CLI contract guards
  (missing-file-in-list exit 2; clean-only `[]`/exit 0; stderr-empty on
  offense path).

All three are correctly Phase-2 scope; none weakens the frozen Phase-1
contract.

## Carried-forward UNRESOLVED (correct to pass open)

Phase 3 live-resolution of mruby handles (ADR 0002 Finding 4) and the Phase 7
custom-mruby build (ADR 0003/0004) remain unproven by design; Phase 1 does not
touch the mruby path, so the gate does not require them.
