# Phase 6 Diff-Quality Watch

`scripts/diff/phase6_rubocop_diff.sh` compares Murphy output and RuboCop auto-corrected output over the Phase 6 corpus.

## Purpose

This script is a **quality watch**, not a hard compatibility gate.

- Murphy runs `murphy lint --fix` on a copied corpus.
- RuboCop runs `rubocop -a` on a parallel copy.
- The output diff is shown for inspection, and teams decide whether remaining gaps
  are acceptable ADR-1 behaviour or should be logged as follow-up tasks.

## Behavior

- Requires `rubocop` in PATH.
- Assumes the phase 6 corpus exists (defaults to
  `crates/murphy-cli/tests/fixtures/phase6_project`).
- Returns `0` on completion; it does not fail CI on mismatch.
- Prints the raw `diff -ru` output so reviewers can inspect
  deterministic, repeatable differences.

## Running

```bash
scripts/diff/phase6_rubocop_diff.sh
```

or with a custom corpus:

```bash
scripts/diff/phase6_rubocop_diff.sh /path/to/corpus
```

## Follow-ups

When differences are meaningful and not intentionally out-of-scope for ADR 0018,
create a beads issue with a clear reproduction and link it as a follow-up to
`murphy-7rg.5`.
