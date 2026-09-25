# CI / PR integration (`--since`, reusable workflow, SARIF)

`murphy lint --since <ref>` lints only the files changed since `<ref>` —
the diff-driven runner (Pronto/Danger replacement) for fast PR checks.
Pair it with the reusable workflow (`.github/workflows/murphy.yml`) to
upload SARIF to GitHub Code Scanning.

## `murphy lint --since <ref>`

```bash
# Local: what changed vs main (staged + unstaged + untracked)?
murphy lint --since origin/main

# Before pushing a branch:
murphy lint --since origin/main --format github

# Full SARIF for upload:
murphy lint --format sarif --since origin/main > murphy.sarif
```

Semantics:

- Base: `git merge-base HEAD <ref>` when it succeeds — the diff is against
  the fork point (PR semantics), not the moving base tip. Falls back to
  `<ref>` directly (shallow clones, tags).
- Changed set: worktree (staged and unstaged) versus the base, plus
  untracked files. Deleted files never appear; renames do.
- Restriction happens up front: unchanged files are never parsed or linted,
  so `--since` is both faster and quieter than a full run.
- Intersects with explicit paths and discovery roots: naming an unchanged
  file skips it. Combine freely with `--baseline` (file restriction, then
  offense filtering) and any `--format` (all see the diff subset).
- Exit codes are unchanged (`0` clean / `1` offenses / `2` setup /
  `3` internal). A non-git directory or an unknown ref exits `2` with a
  hint — never silently lints everything or nothing.

CI checkouts need the base ref locally. With `actions/checkout`, set
`fetch-depth: 0` (the reusable workflow below does this for you).
Without full history the `<ref>` fallback still works as long as the ref
itself is fetched.

## Reusable workflow

`.github/workflows/murphy.yml` (`workflow_call`) installs murphy, lints
the diff as SARIF, uploads to Code Scanning, re-runs for PR annotations,
and gates the job. Call it from any repository:

```yaml
# .github/workflows/murphy-lint.yml in the caller repo
name: murphy-lint
on:
  pull_request:
  push:
    branches: [main]

permissions:
  contents: read
  security-events: write   # required for the SARIF upload

jobs:
  murphy:
    uses: mitsuru/murphy/.github/workflows/murphy.yml@main
    permissions:
      contents: read
      security-events: write
    with:
      since: origin/main
```

Inputs:

| Input | Default | Meaning |
|---|---|---|
| `since` | `origin/main` | Git ref for `--since` (merge-base semantics). |
| `paths` | `.` | Space-separated lint paths, intersected with the diff. |
| `murphy-ref` | `main` | Murphy version to install (git ref of `mitsuru/murphy`). Pin to a tag/SHA for stable CI. |
| `upload-sarif` | `true` | Upload `murphy.sarif` to Code Scanning (`security-events: write` needed). |
| `annotations` | `true` | Re-run with `--format github` so offenses show inline on the PR. |

The job fails iff the SARIF run found offenses (exit `1`); setup errors
(bad ref, bad config) fail as exit `2` with an `::error::` pointer.
The upload runs even on a dirty diff (`if: always()`), so findings reach
Code Scanning before the job goes red.

## SARIF and Code Scanning

`--format sarif` emits SARIF 2.1.0 (`runs[0].tool.driver.name: murphy`,
deduped `rules`, one `result` per offense with `ruleId`/`level`/`message`
and a `region` for located offenses; see `docs/guides/output-formats.md`).
Upload it with `github/codeql-action/upload-sarif` (the reusable workflow
uses `category: murphy`):

```yaml
- run: murphy lint --format sarif --since origin/main > murphy.sarif
- uses: github/codeql-action/upload-sarif@v3
  with:
    sarif_path: murphy.sarif
    category: murphy
```

Findings appear under the repository's **Security → Code scanning** tab.
The offense JSON shape is unchanged (ADR 0006): `--since` restricts
*files*, never output fields.

## Annotations and step summary

- `murphy lint --format github --since <ref>` prints one `::warning` /
  `::error` workflow command per offense — inline PR annotations with no
  upload and no extra permissions.
- `murphy lint --format markdown --since <ref> >> "$GITHUB_STEP_SUMMARY"`
  appends the human-review report to the job summary.

## Fork-PR note

`GITHUB_TOKEN` on `pull_request` from a fork cannot write code-scanning
results, so the upload step fails there. Either trigger via
`pull_request_target`, or set `upload-sarif: false` for forks and keep
the annotations (which work everywhere).

## Dogfood

Murphy lints itself through the same reusable workflow
(`.github/workflows/murphy-lint.yml` → `./.github/workflows/murphy.yml`,
`since: origin/main`). On `main` pushes the diff is empty and the run is
a green no-op; on PRs only changed Ruby files are linted. Note: test
fixtures under `crates/*/tests/fixtures/` carry *intentional* offenses —
touching one flags it, which is the desired signal to check intent.
