# Magic Comments Infrastructure Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expose structured shebang, `frozen_string_literal`, and `encoding` metadata to native cops through the existing single-surface AST API.

**Architecture:** Add a small source-level side table to `murphy-ast`, fill it during Prism translation, and expose read-only helpers on `Cx`. Keep the existing `CxRaw` layout and comment table unchanged, and do not bump `MURPHY_PLUGIN_ABI_VERSION` without approval.

**Tech Stack:** Rust workspace crates `murphy-ast`, `murphy-translate`, `murphy-plugin-api`, and `murphy-core`; tests via `cargo test`.

---

### Task 1: AST Side Table

**Files:**
- Modify: `crates/murphy-ast/src/node.rs`
- Modify: `crates/murphy-ast/src/builder.rs`
- Modify: `crates/murphy-ast/src/ast.rs`

- [ ] **Step 1: Write tests for side-table storage**

Add an `AstBuilder` unit test that constructs `MagicComment` entries for shebang, frozen string literal, and encoding, then asserts `Ast::magic_comments()` and `Ast::raw_parts().magic_comments` return them in source order.

- [ ] **Step 2: Run failing test**

Run: `cargo test -p murphy-ast magic_comments`
Expected: FAIL because the types and methods do not exist.

- [ ] **Step 3: Add minimal side table**

Add `MagicCommentKind`, `MagicComment`, builder storage, `add_magic_comment`, `Ast::magic_comments`, and `AstRawParts::magic_comments`.

- [ ] **Step 4: Run passing test**

Run: `cargo test -p murphy-ast magic_comments`
Expected: PASS.

### Task 2: Translation

**Files:**
- Modify: `crates/murphy-translate/src/lib.rs`
- Test: existing translator tests in the same crate

- [ ] **Step 1: Write translator tests**

Add tests that parse sources containing `#!/usr/bin/env ruby`, `# frozen_string_literal: true`, `# frozen_string_literal: false`, and `# encoding: utf-8`, then assert structured entries are produced.

- [ ] **Step 2: Run failing test**

Run: `cargo test -p murphy-translate magic_comments`
Expected: FAIL because translation does not fill the side table.

- [ ] **Step 3: Populate side table**

Use Prism comments/magic-comments where available and source-line scanning for shebang. Store byte ranges and parsed values; preserve source order.

- [ ] **Step 4: Run passing test**

Run: `cargo test -p murphy-translate magic_comments`
Expected: PASS.

### Task 3: Plugin API Exposure

**Files:**
- Modify: `crates/murphy-plugin-api/src/cx.rs`
- Modify: `crates/murphy-plugin-api/src/lib.rs`

- [ ] **Step 1: Write API tests**

Add `Cx` tests for `magic_comments()`, `shebang()`, `frozen_string_literal_comment()`, and `encoding_comment()` using parsed Ruby sources.

- [ ] **Step 2: Run failing test**

Run: `cargo test -p murphy-plugin-api magic_comments`
Expected: FAIL because the API does not exist.

- [ ] **Step 3: Add helpers without changing `CxRaw`**

Parse structured magic comments from the existing source/comment table inside `Cx`, add safe `Cx` accessors, and keep `CxRaw` size/offset tests unchanged. Do not change `MURPHY_PLUGIN_ABI_VERSION`.

- [ ] **Step 4: Run focused tests**

Run: `cargo test -p murphy-plugin-api magic_comments && cargo test -p murphy-core dispatch::tests::dispatch_stamps_cop_name_into_cx_raw_per_cop`
Expected: PASS.

### Task 4: Verification

**Files:**
- No new files beyond implementation files

- [ ] **Step 1: Format code**

Run: `cargo +nightly fmt --check`
Expected: PASS.

- [ ] **Step 2: Build workspace**

Run: `cargo build`
Expected: PASS.

- [ ] **Step 3: Run workspace tests**

Run: `cargo test --workspace`
Expected: PASS.
