# ADR 0004 — v1 trust & security posture

- Date: 2026-05-19
- Status: Accepted
- Spike: `murphy-npa` (Phase 0, Spike 0.4 — decision only, no code)
- Related: ADR 0002 (mruby bridge), ADR 0003 (cop deadlines)
- Feeds: Phase 0 Gate; Phase 7 (third-party cop sandbox)

## Context

Design §2 locks: **"サンドボックス: v1 なし（自分で置いた信頼 `.rb` 前提）。
第三者配布 cop の sandbox は将来課題"**. User cops run in-process via embedded
mruby (ADR 0002) with the host's full privileges. This ADR makes the trust
boundary, the accepted residual risk, and its v1 mitigations explicit so the
decision is deliberate and documented rather than implicit.

## Decision

**v1 ships with NO sandbox for user cops.** A cop is arbitrary Ruby executed
in-process with the privileges of the `murphy` process: it can read/write the
filesystem, open network connections, spawn processes, and read environment
(secrets, tokens) — exactly what the host user can do.

The trust model is explicit: **a `.rb` in `cops/` is trusted code, equivalent
to running a script the user wrote or vendored deliberately.** Murphy treats
adding a cop as equivalent to adding a Rakefile task or a git hook.

What v1 isolation **does** and **does not** give (do not conflate):

- **Does** (ADR 0002/0003): per-cop isolated `mrb_state`; a crashing or runaway
  cop degrades to an `error offense` for that cop×file and the run continues.
  This is **fault isolation**, not a **security boundary**.
- **Does NOT:** restrict what a cop's Ruby may *do*. No seccomp, no filesystem/
  network jail, no syscall filtering, no resource cap beyond the wall-clock
  deadline (ADR 0003). The deadline bounds *time*, not *capability* — a cop can
  exfiltrate a secret well within 300 ms.

## Accepted residual risk

The load-bearing risk to state plainly: **a cop pulled in transitively — a
third-party / OSS-dependency-provided cop, or one copied from the internet —
runs unsandboxed in CI and on developer machines.** Linters routinely run in CI
with repo write access and often with registry/cloud credentials in the
environment. A malicious or compromised cop is, under v1, arbitrary code
execution in that context.

This risk is **accepted for v1** on the explicit precondition that all cops are
first-party / deliberately vendored and reviewed — the same trust users already
extend to build scripts and git hooks. It is **not** acceptable as a permanent
posture once third-party cop distribution exists.

## v1 mitigations (documentation/process, not enforcement)

1. **Loud trust contract in docs:** "cops are trusted code; only add cops you
   would run as a script. Murphy does not sandbox them in v1."
2. **No implicit cop discovery from untrusted locations:** v1 loads cops only
   from the project's own configured `cops/` path — never auto-fetched, never
   from a dependency's directory tree.
3. **Surface what loaded:** `murphy --debug` lists every cop file path executed,
   so an unexpected cop is visible in CI logs.

These reduce *accidental* exposure; none is a security control against a
*malicious* cop. That is deferred deliberately.

## Deferred to Phase 7 (hard prerequisite for third-party cop distribution)

A real sandbox for untrusted cops: candidates noted for later evaluation —
seccomp-bpf syscall filtering, a capability-restricted mruby build (disable
`Kernel#system`, `File`, socket APIs), Landlock/namespaces, or WASM-compiled
cops. **Third-party cop distribution MUST NOT ship before this exists.** This
links to the Phase 7 sandbox item (design §8) and reinforces ADR 0003's Phase 7
note (a custom mruby build is already on the Phase 7 path).

## Consequences

- A one-line pointer is added to `CLAUDE.md` "What This Project Is" so the
  trust posture is visible to anyone working on the project.
- Phase 3 (mruby cop path) implements mitigation 2 (cops only from the
  configured path) and 3 (`--debug` lists cop paths). It must **not** add
  features that auto-load cops from dependencies.
- No code in this spike; the decision itself is the deliverable.
