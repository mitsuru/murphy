# Murphy BN3 Sandbox Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish `murphy-bn3.1` by turning the third-party cop sandbox milestone into accepted ADRs, PoC evidence, docs, and a final implementation re-plan.

**Architecture:** This phase stays design/PoC-first. The MVP sandbox is capability-restricted mruby, with package-scoped require/cache policy and OS sandboxing deferred as defense-in-depth.

**Tech Stack:** Rust workspace, embedded mruby via `mruby3-sys`, custom mruby runtime seam, Markdown ADRs, beads (`bd`) for persistent task tracking.

---

## File Structure

- Create `docs/decisions/0024-third-party-cop-capability-policy.md` for `murphy-bn3.1.2`.
- Create `docs/decisions/0025-restricted-mruby-runtime-poc.md` for `murphy-bn3.1.3`.
- Create `docs/decisions/0026-restricted-require-allowlist-poc.md` for `murphy-bn3.1.4`.
- Create `docs/decisions/0027-package-cache-isolation-poc.md` for `murphy-bn3.1.8`.
- Create `docs/decisions/0028-sandbox-failure-integration-poc.md` for `murphy-bn3.1.5`.
- Create `docs/guides/third-party-cop-sandbox.md` for `murphy-bn3.1.6`.
- Create `docs/decisions/0029-phase-7-sandbox-gate-review.md` for `murphy-bn3.1.7`.

## Task 1: Capability Policy ADR (`murphy-bn3.1.2`)

**Files:**
- Create: `docs/decisions/0024-third-party-cop-capability-policy.md`
- Read: `docs/decisions/0004-trust-model.md`
- Read: `docs/decisions/0023-third-party-cop-sandbox-threat-model.md`

- [ ] **Step 1: Claim the issue**

Run: `bd update murphy-bn3.1.2 --claim`

Expected: issue becomes in progress.

- [ ] **Step 2: Write ADR 0024**

Create `docs/decisions/0024-third-party-cop-capability-policy.md` with these sections:

```markdown
# ADR 0024 — Third-party cop capability policy

- Date: 2026-05-20
- Status: Accepted
- Issue: `murphy-bn3.1.2`
- Related: ADR 0004, ADR 0021, ADR 0023
- Feeds: `murphy-bn3.1` Phase 7 third-party cop sandbox

## Decision

The MVP third-party cop sandbox uses a deny-by-default Ruby capability policy.
Third-party cops may use only Murphy's cop DSL, AST read primitives,
offense/autocorrect reporting primitives, package-root require, and allowlisted
pure Ruby stdlib supplied by Murphy.

Host capabilities are denied: `File`, `Dir`, `IO`, `Socket`, `Process`, `ENV`,
`Kernel#system`, backticks, `exec`, `spawn`, `Open3`, unrestricted `load`,
absolute-path require, package-root escapes, non-allowlisted stdlib, native
extensions, FFI, and `dlopen`.

## Require policy

`require` and `require_relative` are allowed only through Murphy's resolver.
Resolution has two namespaces:

- `murphy_stdlib`: immutable, Murphy-managed pure Ruby stdlib modules.
- `package`: files under the canonical package root.

Allowlisted stdlib names resolve before package-local bare names. A package-local
`json.rb` cannot shadow Murphy's allowlisted `json`. Package-local files should
use explicit relative require when they intend package resolution.

## Initial allowlist

The first allowlist is intentionally small: `set` and `json` only if Murphy ships
pure Ruby implementations that do not expose filesystem, process, network, or
native extension access. If Murphy cannot ship a safe pure Ruby implementation,
the name remains denied.

## Denied behavior contract

Denied capability use raises a Ruby exception that the existing mruby cop runner
maps to one isolated error offense. It must not abort the host process or affect
other cops.

## Future capabilities

Any new host-facing capability must be named, package-scoped, testable, and
documented. Example: bundled read-only package assets may be added later as a
specific package-asset API, not by re-enabling unrestricted `File.read`.
```

- [ ] **Step 3: Verify ADR has no placeholders or whitespace errors**

Run: `grep -nE 'TBD|TODO|FIXME|\?\?\?' docs/decisions/0024-third-party-cop-capability-policy.md || true`

Expected: no matches.

Run: `git diff --check`

Expected: no output.

- [ ] **Step 4: Commit and close issue**

Run:

```bash
bd close murphy-bn3.1.2 --reason="Added ADR 0024 capability whitelist and denylist policy."
git add docs/decisions/0024-third-party-cop-capability-policy.md
git commit -m "docs: define third-party cop capability policy"
```

Expected: commit succeeds.

## Task 2: Restricted mruby Runtime PoC ADR (`murphy-bn3.1.3`)

**Files:**
- Create: `docs/decisions/0025-restricted-mruby-runtime-poc.md`
- Read: `crates/murphy-core/src/mruby/build.rs`

- [ ] **Step 1: Claim issue**

Run: `bd update murphy-bn3.1.3 --claim`

Expected: issue becomes in progress.

- [ ] **Step 2: Write PoC evidence ADR**

Create `docs/decisions/0025-restricted-mruby-runtime-poc.md` documenting:

```markdown
# ADR 0025 — Restricted mruby runtime PoC result

- Date: 2026-05-20
- Status: Accepted
- Issue: `murphy-bn3.1.3`
- Related: ADR 0021, ADR 0023, ADR 0024

## Decision

Murphy will implement the restricted runtime by extending the custom mruby build
path, not by filtering after loading unrestricted Ruby APIs.

## PoC criteria

The implementation PoC must prove these expressions fail inside the restricted
runtime: `File.read`, `Dir.entries`, `IO.read`, `ENV.to_h`, `system`, backticks,
`Process.pid`, `Socket.tcp`, `Open3.capture3`, `load`, and native extension
loading. The same runtime must still run a minimal Murphy cop that uses the DSL,
reads node data, reports an offense, and registers an autocorrect edit.

## Expected mechanism

Build-time configuration removes or never loads dangerous mruby gems/classes.
Runtime boot validates that denied constants/methods are absent before executing
third-party code. Validation failure is a setup error for the custom runtime, not
a silently weakened sandbox.

## Evidence required before full implementation

Tests must run with `--features murphy-core/mruby-custom-build` and a test
runtime path. The test fixture must attempt each denied expression and assert the
failure is surfaced as an isolated cop error, while a legitimate cop still
returns deterministic offenses.
```

- [ ] **Step 3: Verify doc**

Run: `grep -nE 'TBD|TODO|FIXME|\?\?\?' docs/decisions/0025-restricted-mruby-runtime-poc.md || true`

Expected: no matches.

Run: `git diff --check`

Expected: no output.

- [ ] **Step 4: Commit and close issue**

Run:

```bash
bd close murphy-bn3.1.3 --reason="Captured restricted mruby runtime PoC criteria and mechanism in ADR 0025."
git add docs/decisions/0025-restricted-mruby-runtime-poc.md
git commit -m "docs: capture restricted mruby runtime poc"
```

Expected: commit succeeds.

## Task 3: Require Allowlist PoC ADR (`murphy-bn3.1.4`)

**Files:**
- Create: `docs/decisions/0026-restricted-require-allowlist-poc.md`

- [ ] **Step 1: Claim issue**

Run: `bd update murphy-bn3.1.4 --claim`

Expected: issue becomes in progress.

- [ ] **Step 2: Write ADR 0026**

Create `docs/decisions/0026-restricted-require-allowlist-poc.md` with:

```markdown
# ADR 0026 — Restricted require allowlist PoC result

- Date: 2026-05-20
- Status: Accepted
- Issue: `murphy-bn3.1.4`
- Related: ADR 0023, ADR 0024

## Decision

Third-party cop `require` uses Murphy's resolver rather than mruby's ambient load
path. The resolver accepts package-root files and Murphy-managed allowlisted pure
Ruby stdlib only.

## Resolution rules

1. Canonicalize the package root once before execution.
2. Reject absolute require paths from third-party code.
3. Resolve allowlisted stdlib names from Murphy's immutable stdlib root.
4. Resolve relative package requires only if the canonical result remains under
   the package root.
5. Reject `load` for third-party packages.
6. Reject native extensions and files outside `.rb` source form.

## PoC cases

- `require_relative "lib/helper"` succeeds for a package-local helper.
- `require "set"` succeeds only when `set` is shipped in Murphy's pure stdlib
  allowlist.
- `require "/tmp/evil"`, `require_relative "../evil"`, and symlink escapes fail.
- `require "socket"`, `require "open3"`, and native extension require fail.
- Package-local `json.rb` does not shadow allowlisted stdlib `json`.
```

- [ ] **Step 3: Verify doc**

Run: `grep -nE 'TBD|TODO|FIXME|\?\?\?' docs/decisions/0026-restricted-require-allowlist-poc.md || true`

Expected: no matches.

Run: `git diff --check`

Expected: no output.

- [ ] **Step 4: Commit and close issue**

Run:

```bash
bd close murphy-bn3.1.4 --reason="Captured restricted require allowlist rules and PoC cases in ADR 0026."
git add docs/decisions/0026-restricted-require-allowlist-poc.md
git commit -m "docs: define restricted require allowlist poc"
```

Expected: commit succeeds.

## Task 4: Cache Isolation PoC ADR (`murphy-bn3.1.8`)

**Files:**
- Create: `docs/decisions/0027-package-cache-isolation-poc.md`

- [ ] **Step 1: Claim issue**

Run: `bd update murphy-bn3.1.8 --claim`

Expected: issue becomes in progress.

- [ ] **Step 2: Write ADR 0027**

Create `docs/decisions/0027-package-cache-isolation-poc.md` with:

```markdown
# ADR 0027 — Package cache isolation PoC result

- Date: 2026-05-20
- Status: Accepted
- Issue: `murphy-bn3.1.8`
- Related: ADR 0023, ADR 0024, ADR 0026

## Decision

All third-party cop package caches are scoped by package identity and package
fingerprint. A cache hit may reuse work only for the same package, same content,
same sandbox policy version, and same stdlib allowlist version.

## Fingerprint inputs

The package fingerprint is a content hash over canonical package-root-relative
paths and bytes for `.rb` files, package manifest/config, and vendored helpers.
mtime is ignored. Symlink targets are canonicalized; targets outside the package
root are rejected instead of fingerprinted.

## Cache key

Every package-local cache key must include:

- `package_id`
- `package_fingerprint`
- `sandbox_policy_version`
- `stdlib_allowlist_version`

## PoC cases

- A one-byte change in package A changes package A's fingerprint.
- A package A change does not invalidate package B.
- `$LOAD_PATH`, `$LOADED_FEATURES`, constants, and monkey patches from package A
  are not visible to package B.
- Package-local `json.rb` cannot shadow Murphy's allowlisted stdlib `json`.
- Symlink and canonicalization escapes are rejected before cache insertion.
```

- [ ] **Step 3: Verify doc**

Run: `grep -nE 'TBD|TODO|FIXME|\?\?\?' docs/decisions/0027-package-cache-isolation-poc.md || true`

Expected: no matches.

Run: `git diff --check`

Expected: no output.

- [ ] **Step 4: Commit and close issue**

Run:

```bash
bd close murphy-bn3.1.8 --reason="Captured package fingerprint and cache isolation PoC requirements in ADR 0027."
git add docs/decisions/0027-package-cache-isolation-poc.md
git commit -m "docs: define package cache isolation poc"
```

Expected: commit succeeds.

## Task 5: Failure Integration PoC ADR (`murphy-bn3.1.5`)

**Files:**
- Create: `docs/decisions/0028-sandbox-failure-integration-poc.md`

- [ ] **Step 1: Claim issue**

Run: `bd update murphy-bn3.1.5 --claim`

Expected: issue becomes in progress.

- [ ] **Step 2: Write ADR 0028**

Create `docs/decisions/0028-sandbox-failure-integration-poc.md` with:

```markdown
# ADR 0028 — Sandbox failure integration PoC result

- Date: 2026-05-20
- Status: Accepted
- Issue: `murphy-bn3.1.5`
- Related: ADR 0003, ADR 0011, ADR 0023, ADR 0024

## Decision

Sandbox denial is treated like a cop exception: one isolated error offense for
that cop and file, then execution continues for other cops and files.

## Error shape

The error offense uses the offending cop name, severity `error`, and a message
prefix `Sandbox violation:` followed by the denied capability name. The JSON
contract remains the existing offense shape; no new top-level fields are added.

## Determinism

Sandbox denials participate in the existing aggregate ordering and severity
precedence. If a denied cop races with timeout at an exact deadline boundary,
ADR 0003's accepted boundary race applies; normal headroom cases must be stable.

## PoC cases

- Denied `File.read` becomes one error offense.
- Denied `require "socket"` becomes one error offense.
- A sibling well-behaved cop still runs on the same file.
- A denied cop in one file does not poison later files.
- Output order remains byte-identical across repeated runs with the same inputs.
```

- [ ] **Step 3: Verify doc**

Run: `grep -nE 'TBD|TODO|FIXME|\?\?\?' docs/decisions/0028-sandbox-failure-integration-poc.md || true`

Expected: no matches.

Run: `git diff --check`

Expected: no output.

- [ ] **Step 4: Commit and close issue**

Run:

```bash
bd close murphy-bn3.1.5 --reason="Captured sandbox denial error-offense integration contract in ADR 0028."
git add docs/decisions/0028-sandbox-failure-integration-poc.md
git commit -m "docs: define sandbox failure integration poc"
```

Expected: commit succeeds.

## Task 6: User and Developer Docs (`murphy-bn3.1.6`)

**Files:**
- Create: `docs/guides/third-party-cop-sandbox.md`

- [ ] **Step 1: Claim issue**

Run: `bd update murphy-bn3.1.6 --claim`

Expected: issue becomes in progress.

- [ ] **Step 2: Write guide**

Create `docs/guides/third-party-cop-sandbox.md` with:

```markdown
# Third-party cop sandbox

Murphy's planned third-party cop sandbox is a capability-restricted mruby
runtime. Third-party cops are not allowed to access host filesystem,
environment, network, process execution, native extensions, or arbitrary code
loading.

## Allowed

- Murphy cop DSL.
- Murphy AST reader methods.
- Murphy offense and autocorrect reporting methods.
- Package-local Ruby helpers under the package root.
- Murphy-managed allowlisted pure Ruby stdlib modules.

## Denied

- `File`, `Dir`, `IO`, `Socket`, `Process`, `ENV`.
- `system`, backticks, `exec`, `spawn`, `Open3`.
- `load`, absolute-path require, package-root escapes.
- Non-allowlisted stdlib, native extensions, FFI, `dlopen`.

## Require

Use package-local relative require for package helpers. Bare stdlib names resolve
only when Murphy ships a safe pure Ruby implementation in the allowlist. Package
files cannot shadow Murphy-managed stdlib modules.

## Cache invalidation

Murphy fingerprints third-party cop package content. If any package Ruby file,
manifest, config, or vendored helper changes, package-scoped require and sandbox
caches are invalidated for that package only.

## OS sandboxing

seccomp, Landlock, namespaces, and separate worker processes are reserved as
future defense-in-depth. They are not the MVP sandbox mechanism.
```

- [ ] **Step 3: Verify docs**

Run: `grep -nE 'TBD|TODO|FIXME|\?\?\?' docs/guides/third-party-cop-sandbox.md || true`

Expected: no matches.

Run: `git diff --check`

Expected: no output.

- [ ] **Step 4: Commit and close issue**

Run:

```bash
bd close murphy-bn3.1.6 --reason="Documented third-party cop sandbox contract for users and developers."
git add docs/guides/third-party-cop-sandbox.md
git commit -m "docs: document third-party cop sandbox contract"
```

Expected: commit succeeds.

## Task 7: Phase 7 Sandbox Gate (`murphy-bn3.1.7`)

**Files:**
- Create: `docs/decisions/0029-phase-7-sandbox-gate-review.md`

- [ ] **Step 1: Claim issue**

Run: `bd update murphy-bn3.1.7 --claim`

Expected: issue becomes in progress.

- [ ] **Step 2: Write gate review**

Create `docs/decisions/0029-phase-7-sandbox-gate-review.md` with:

```markdown
# ADR 0029 — Phase 7 sandbox gate review

- Date: 2026-05-20
- Status: Accepted
- Issue: `murphy-bn3.1.7`
- Parent: ADR 0023

## Gate result

The Phase 7 sandbox milestone is ready to be re-planned as implementation work.
The accepted MVP sandbox is capability-restricted mruby with package-scoped
require, package fingerprint cache invalidation, and isolated error-offense
handling for denied capabilities.

## Implementation tasks to create next

- Build restricted mruby runtime profile.
- Add runtime boot self-check for denied constants and methods.
- Implement Murphy-managed require resolver.
- Implement package fingerprinting and package-scoped cache keys.
- Map sandbox denials to isolated error offenses.
- Add feature-gated integration tests for denied APIs, require policy, cache
  isolation, and deterministic output.

## Third-party distribution gate

Third-party cop distribution remains blocked until the implementation tasks pass
with tests proving denied APIs, restricted require, cache invalidation, and
failure integration.
```

- [ ] **Step 3: Verify all docs and issue state**

Run: `grep -R -nE 'TBD|TODO|FIXME|\?\?\?' docs/decisions/0024-third-party-cop-capability-policy.md docs/decisions/0025-restricted-mruby-runtime-poc.md docs/decisions/0026-restricted-require-allowlist-poc.md docs/decisions/0027-package-cache-isolation-poc.md docs/decisions/0028-sandbox-failure-integration-poc.md docs/guides/third-party-cop-sandbox.md docs/decisions/0029-phase-7-sandbox-gate-review.md || true`

Expected: no matches.

Run: `git diff --check`

Expected: no output.

- [ ] **Step 4: Commit and close issue**

Run:

```bash
bd close murphy-bn3.1.7 --reason="Added Phase 7 sandbox gate review and implementation re-plan."
git add docs/decisions/0029-phase-7-sandbox-gate-review.md
git commit -m "docs: add phase 7 sandbox gate review"
```

Expected: commit succeeds.

## Task 8: Close Parent and Push

**Files:**
- No new files.

- [ ] **Step 1: Verify parent children are closed**

Run: `bd show murphy-bn3.1`

Expected: all child issues `murphy-bn3.1.1` through `.1.8` show closed.

- [ ] **Step 2: Close parent**

Run: `bd close murphy-bn3.1 --reason="Split sandbox milestone into accepted threat model, capability policy, PoC evidence, docs, and gate review."`

Expected: parent issue closes.

- [ ] **Step 3: Final verification**

Run: `cargo test -p murphy-core -- --nocapture`

Expected: all tests pass.

Run: `git status -sb`

Expected: branch has only committed changes or shows ahead of origin.

- [ ] **Step 4: Push code and beads**

Run:

```bash
git pull --rebase origin main
bd dolt push
git push -u origin bn3-sandbox
```

Expected: branch and beads updates are pushed.
