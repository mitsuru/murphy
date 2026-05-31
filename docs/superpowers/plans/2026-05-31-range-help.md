# RangeHelp Source Range Helpers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add RuboCop `RangeHelp`-style range expansion helpers to `murphy-plugin-api::Cx`.

**Architecture:** Keep the feature in `crates/murphy-plugin-api/src/cx.rs` because `Cx` has source, comment, and token access. Model RuboCop's behavior over byte ranges, clamping all results to the source buffer.

**Tech Stack:** Rust 2024, `murphy-plugin-api`, `murphy-translate` dev dependency, Cargo unit tests.

---

### Task 1: Add RangeHelp API and tests

**Files:**
- Modify: `crates/murphy-plugin-api/src/cx.rs`
- Modify: `crates/murphy-plugin-api/src/lib.rs`

- [ ] **Step 1: Write failing tests**

Add unit tests in `crates/murphy-plugin-api/src/cx.rs` for `range_by_whole_lines`, `range_with_surrounding_space`, `range_with_comments`, and heredoc whole-line behavior.

- [ ] **Step 2: Verify tests fail**

Run: `CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo test -p murphy-plugin-api range_help -- --nocapture`

Expected: compilation fails because the new methods and types do not exist.

- [ ] **Step 3: Implement minimal API**

Add `RangeSide`, `SpaceRangeOptions`, and `Cx` methods. Implement line and whitespace expansion by scanning `self.source().as_bytes()` around the input byte range. Implement comment expansion using `self.comments()` and own-line comment checks.

- [ ] **Step 4: Verify tests pass**

Run: `CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo test -p murphy-plugin-api range_help -- --nocapture`

Expected: tests pass.

- [ ] **Step 5: Run package tests**

Run: `CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo test -p murphy-plugin-api`

Expected: all `murphy-plugin-api` tests pass.
