# Murphy BN3 Sandbox Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the Phase 7 third-party cop sandbox MVP described by ADR 0024-0028 and close the remaining `murphy-bn3` child issues.

**Architecture:** Keep the existing trusted `cops/` mruby path unchanged. Add a small sandbox boundary in `crates/murphy-core/src/mruby` that can boot-validate denied Ruby capabilities, resolve package-local requirements, compute package fingerprints, and map sandbox denials to the existing error-offense JSON shape.

**Tech Stack:** Rust 2024, `mruby3-sys`, existing Murphy mruby SDK, `sha2`, `tempfile`, beads.

---

## Tasks

- [x] Restricted runtime boot self-check for denied Ruby-visible APIs.
- [x] Murphy-managed require resolver with package-root and stdlib allowlist policy.
- [x] Package fingerprint and cache key policy versions.
- [x] Sandbox denial error offenses preserving the existing JSON shape.
- [x] Focused feature/integration tests for denial, require, cache, symlink escape, and deterministic sibling continuation behavior.

## Verification

- Run: `cargo test -p murphy-core mruby::sandbox -- --nocapture`
- Run: `cargo test --workspace`
- Run: `cargo fmt --check`
