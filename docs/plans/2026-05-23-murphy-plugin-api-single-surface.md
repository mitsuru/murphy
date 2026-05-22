# murphy-plugin-api Single-Surface Redesign — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Rewrite `murphy-plugin-api` as the single, arena-direct-read surface every cop uses to read the AST (ADR 0038), replacing the pre-reboot spike ABI.

**Architecture:** A cop callback receives a `Cx<'a>` that reads an immutable arena (`murphy-ast`) directly — traversal and `NodeKind` matching are pure memory reads. The only host-state operation is emitting offenses, so the `#[repr(C)]` `FnTable` carries just `emit_offense` / `emit_edit`. Metadata lives on the const-based `Cop` trait; dispatch lives on `NodeCop`. `FileCop` / `CallCop` / `run_file` are deleted.

**Tech Stack:** Rust 2024, `murphy-ast` (arena AST, zero-dep), `serde_json` (config decode). No `murphy-core` dependency (the dep graph inverts: murphy-9cr.22 makes `murphy-core` depend on this crate).

## Issue

beads `murphy-9cr.20` (epic `murphy-9cr`). Design is in the issue's `design` field; this plan implements it. ADR 0038 (`docs/decisions/0038-single-surface-plugin-abi.md`) is the contract.

## Build environment

All work happens in the worktree `.worktrees/murphy-9cr-20-plugin-api`. **Every `cargo` command must set `CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target`** — a fresh worktree cannot rebuild `mruby3-sys` (no `ruby`/`rake`), so it reuses the main checkout's cached artifacts.

- Per-crate commands (`cargo test -p murphy-plugin-api`, `-p murphy-ast`) do not pull `mruby3-sys` and are the fast inner loop.
- The full `--workspace` gate (Task 10) does, and relies on the cached `libmruby.a`.

## Scope boundaries

Out of scope (downstream issues): `register_cops!` / `#[derive(CopOptions)]` / `#[on_node]` macros (.21, .8); the `.so` `dlopen` loader and dispatch-to-arena swap (.22); deleting the old `Murphy*` structs from `murphy-core` (.22); config validation gate (.9). This crate cannot be exercised by real dispatch until .22 — tests use `murphy-ast::AstBuilder`-built fixtures and `#[repr(C)]` layout assertions.

## Module layout (end state)

```
crates/murphy-plugin-api/src/
  lib.rs          crate docs + the public re-export surface
  config_error.rs ConfigError / ConfigErrorKind          (KEPT, unchanged)
  severity.rs     Severity + ABI wire encoding
  abi.rs          all #[repr(C)] boundary types + layout asserts
  options.rs      CopOptions trait + NoOptions
  cop.rs          Cop trait (metadata-only)
  node_cop.rs     NodeKindTag + NodeCop trait (dispatch)
  cx.rs           Cx<'a> — the direct-read context
```

---

### Task 1: Re-point the crate — deps, legacy removal, macros-test neutralization

Scaffolding (not a red→green TDD cycle): the crate is reduced to a minimal compiling shell so later tasks build the new surface onto it.

**Files:**
- Modify: `crates/murphy-plugin-api/Cargo.toml`
- Modify: `crates/murphy-plugin-api/src/lib.rs` (gut to a shell)
- Delete: `crates/murphy-plugin-api/src/kinds.rs`
- Modify: `crates/murphy-ast/src/ast.rs:25` (`collect_children` visibility)
- Modify: `crates/murphy-ast/src/lib.rs` (re-export `collect_children`)
- Modify: `crates/murphy-plugin-macros/tests/derive_behavior.rs` (neutralize)
- Modify: `crates/murphy-plugin-macros/tests/trybuild.rs` (neutralize)
- Create: `crates/murphy-ast/tests/collect_children_pub.rs` (re-export check)

**Step 1: Re-point `Cargo.toml`.** Replace the `[dependencies]` block:

```toml
[dependencies]
murphy-ast = { path = "../murphy-ast" }
# JSON decoding for `CopOptions::from_config_json` (ADR 0036).
serde_json = "1"
```

Delete the `[dev-dependencies]` block entirely (`ruby-prism` was only used by the deleted `kinds` coverage test; the new tests need no extra dev-deps).

**Step 2: Promote `collect_children` to `pub`.** In `crates/murphy-ast/src/ast.rs`, change `pub(crate) fn collect_children` to `pub fn collect_children`. In `crates/murphy-ast/src/lib.rs`, add `collect_children` to the `pub use ast::{...}` list. Update the `collect_children` doc comment's "Single source of truth" sentence to also mention `murphy-plugin-api`'s `Cx::children`.

**Step 3: Gut `lib.rs`.** Replace the entire file with the shell:

```rust
//! Safe, plugin-author-facing surface over the Murphy single-surface
//! plugin ABI (ADR 0038). A cop reads the arena AST through [`ConfigError`]
//! and the types added by later tasks of murphy-9cr.20.

mod config_error;

pub use config_error::{ConfigError, ConfigErrorKind};
```

Delete `src/kinds.rs`.

**Step 4: Neutralize the macros test suite.** `murphy-plugin-macros` depends on `murphy-plugin-api` only as a dev-dependency; its `src/` does not import this crate and still compiles. Only its tests consume the (now-deleted) old surface. Replace `crates/murphy-plugin-macros/tests/derive_behavior.rs` and `crates/murphy-plugin-macros/tests/trybuild.rs` each with a single placeholder so `cargo test --workspace` / `cargo clippy --workspace --all-targets` stay green:

```rust
//! Neutralized for the plugin-reboot (murphy-9cr.20). The macro crate is
//! rewritten against the new single-surface ABI in murphy-9cr.21, which
//! restores a real test suite. `src/` (the macros) is untouched here.

#[test]
fn placeholder_until_murphy_9cr_21() {}
```

Leave `crates/murphy-plugin-macros/tests/ui/` and `src/` untouched — neutralizing `trybuild.rs` already stops the `ui/` cases from running. Keep the `murphy-plugin-api` dev-dependency in the macros `Cargo.toml` (.21 needs it again).

**Step 5: Re-export check.** Create `crates/murphy-ast/tests/collect_children_pub.rs`:

```rust
//! `collect_children` must be reachable as a public `murphy-ast` item so
//! `murphy-plugin-api`'s `Cx::children` can delegate to it (murphy-9cr.20).

use murphy_ast::{collect_children, AstBuilder, NodeKind, Range};

#[test]
fn collect_children_is_public_and_enumerates_children() {
    let mut b = AstBuilder::new("x", "x".into());
    let leaf = b.push(NodeKind::Nil, Range { start: 0, end: 1 });
    let root = b.push(
        NodeKind::Return(murphy_ast::OptNodeId::some(leaf)),
        Range { start: 0, end: 1 },
    );
    let ast = b.finish(root);
    let mut out = Vec::new();
    collect_children(ast.kind(root), &[], &mut out);
    assert_eq!(out, vec![leaf]);
}
```

Adjust the `AstBuilder` calls to the crate's actual constructor signature (check `crates/murphy-ast/src/builder.rs` — the call shape there is authoritative; do not guess).

**Step 6: Verify.**

Run: `CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo test -p murphy-ast -p murphy-plugin-api`
Expected: PASS (murphy-ast gains 1 test; murphy-plugin-api compiles as the shell with its `config_error` tests intact).

Run: `CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo test --workspace`
Expected: PASS — the macros placeholder keeps the workspace green.

**Step 7: Commit.**

```bash
git add -A
git commit -m "refactor(murphy-plugin-api): re-point to arena, gut legacy spike surface"
```

---

### Task 2: `Severity` + ABI wire encoding

**Files:**
- Create: `crates/murphy-plugin-api/src/severity.rs`
- Modify: `crates/murphy-plugin-api/src/lib.rs` (add `mod severity;` + re-export)

**Step 1: Write the failing test.** Append to `severity.rs` (created with the module):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_wire_round_trips_each_variant() {
        for value in [None, Some(Severity::Warning), Some(Severity::Error)] {
            assert_eq!(Severity::from_wire(Severity::to_wire(value)), value);
        }
        assert_eq!(Severity::to_wire(None), SEVERITY_UNSET);
        assert_eq!(Severity::from_wire(SEVERITY_UNSET), None);
    }

    #[test]
    fn tristate_wire_round_trips_each_variant() {
        for value in [None, Some(false), Some(true)] {
            assert_eq!(tristate_from_wire(tristate_to_wire(value)), value);
        }
        assert_eq!(tristate_to_wire(None), TRISTATE_UNSET);
    }

    #[test]
    fn unknown_wire_bytes_decode_to_none() {
        assert_eq!(Severity::from_wire(7), None);
        assert_eq!(tristate_from_wire(7), None);
    }
}
```

**Step 2: Run to verify it fails.**
Run: `CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo test -p murphy-plugin-api`
Expected: FAIL — `severity` module / `Severity` not found.

**Step 3: Implement.** Prepend to `severity.rs`:

```rust
//! Plugin-facing offense severity and its ABI wire encoding.

/// How serious an offense is, as declared by a cop.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// A non-fatal style / correctness concern.
    Warning = 0,
    /// A serious problem.
    Error = 1,
}

/// Wire byte for "severity not specified" — the host keeps its default.
pub const SEVERITY_UNSET: u8 = 255;
/// Wire byte for "enablement not specified".
pub const TRISTATE_UNSET: u8 = 255;

impl Severity {
    /// Encode an optional severity to its ABI wire byte.
    pub const fn to_wire(value: Option<Severity>) -> u8 {
        match value {
            Some(Severity::Warning) => 0,
            Some(Severity::Error) => 1,
            None => SEVERITY_UNSET,
        }
    }

    /// Decode an ABI wire byte. `SEVERITY_UNSET` and any unknown byte → `None`.
    pub const fn from_wire(byte: u8) -> Option<Severity> {
        match byte {
            0 => Some(Severity::Warning),
            1 => Some(Severity::Error),
            _ => None,
        }
    }
}

/// Encode an optional bool (a cop's default-enabled) to its wire byte.
pub const fn tristate_to_wire(value: Option<bool>) -> u8 {
    match value {
        Some(false) => 0,
        Some(true) => 1,
        None => TRISTATE_UNSET,
    }
}

/// Decode a tristate wire byte. `TRISTATE_UNSET`/unknown → `None`.
pub const fn tristate_from_wire(byte: u8) -> Option<bool> {
    match byte {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}
```

Add to `lib.rs`: `mod severity;` and `pub use severity::{Severity, SEVERITY_UNSET, TRISTATE_UNSET, tristate_from_wire, tristate_to_wire};`.

**Step 4: Run to verify it passes.**
Run: `CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo test -p murphy-plugin-api`
Expected: PASS.

**Step 5: Commit.**

```bash
git add -A
git commit -m "feat(murphy-plugin-api): add Severity + ABI wire encoding"
```

---

### Task 3: `RawSlice`, `OptionSpec`, `CopOptions`, `NoOptions`

**Files:**
- Create: `crates/murphy-plugin-api/src/abi.rs` (`RawSlice`, `OptionSpec`)
- Create: `crates/murphy-plugin-api/src/options.rs` (`CopOptions`, `NoOptions`)
- Modify: `crates/murphy-plugin-api/src/lib.rs`

**Step 1: Write the failing tests.** In `abi.rs` `#[cfg(test)]`:

```rust
#[test]
fn raw_slice_from_str_round_trips() {
    let s = RawSlice::from_str("send");
    assert_eq!(unsafe { s.as_bytes() }, b"send");
    assert_eq!(unsafe { RawSlice::EMPTY.as_bytes() }, b"");
}

#[test]
fn option_spec_is_repr_c_seven_slices() {
    use std::mem::{offset_of, size_of};
    assert_eq!(size_of::<OptionSpec>(), 7 * size_of::<RawSlice>());
    assert_eq!(offset_of!(OptionSpec, name), 0);
    assert_eq!(offset_of!(OptionSpec, reason), 6 * size_of::<RawSlice>());
}
```

In `options.rs` `#[cfg(test)]`:

```rust
#[test]
fn no_options_has_empty_schema_and_ignores_input() {
    assert!(<NoOptions as CopOptions>::SCHEMA.is_empty());
    assert!(<NoOptions as CopOptions>::from_config_json(b"not json").is_ok());
}
```

**Step 2: Run to verify failure.**
Run: `CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo test -p murphy-plugin-api`
Expected: FAIL — `abi` / `options` modules absent.

**Step 3: Implement `abi.rs`** (prepend before the test module):

```rust
//! `#[repr(C)]` types that cross the plugin ABI boundary (ADR 0038).
//!
//! Every struct here has a frozen layout: the `#[cfg(test)]` `offset_of!`
//! assertions are the freeze guard. New fields append at the end only.

/// The ABI's borrowed-slice primitive: a `#[repr(C)]` pointer+length pair.
///
/// `len == 0` is valid with any `ptr` (including null); accessors check
/// `len` before dereferencing.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RawSlice {
    /// Start pointer. Meaningful only when `len > 0`.
    pub ptr: *const u8,
    /// Byte length.
    pub len: usize,
}

// Safety: a RawSlice is an immutable, non-owning view. The pointee's
// validity and thread-safety are the host's responsibility under the
// ADR 0038 safety contract (the arena is immutable during dispatch).
unsafe impl Sync for RawSlice {}
unsafe impl Send for RawSlice {}

impl RawSlice {
    /// The empty slice.
    pub const EMPTY: RawSlice = RawSlice { ptr: std::ptr::null(), len: 0 };

    /// Borrow a `&'static str`.
    pub const fn from_str(s: &'static str) -> RawSlice {
        RawSlice { ptr: s.as_ptr(), len: s.len() }
    }

    /// Reconstruct the byte slice.
    ///
    /// # Safety
    /// When `len > 0`, `ptr` must point to `len` initialized bytes valid
    /// for `'a`.
    pub unsafe fn as_bytes<'a>(self) -> &'a [u8] {
        if self.len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
        }
    }
}

/// `#[repr(C)]` schema entry for one cop option. Re-implements the
/// option-metadata struct (murphy-9cr.2 concept) for the single-surface
/// ABI. The validation gate (murphy-9cr.9) reads `CopOptions::SCHEMA`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OptionSpec {
    /// Option key in `[cops.rules."Name"]`.
    pub name: RawSlice,
    /// Wire type: `"bool"` / `"int"` / `"string"` / `"string_list"`.
    pub ty: RawSlice,
    /// Default value, JSON-encoded. `EMPTY` when the option is required.
    pub default_json: RawSlice,
    /// One-line human description.
    pub description: RawSlice,
    /// Allowed values for an enum `string` (JSON array); `EMPTY` if free.
    pub enum_values_json: RawSlice,
    /// Suggested replacement when this option is deprecated.
    pub replacement: RawSlice,
    /// Why the option exists / its deprecation reason.
    pub reason: RawSlice,
}

unsafe impl Sync for OptionSpec {}
```

**Step 4: Implement `options.rs`:**

```rust
//! The `CopOptions` trait: a cop's typed view of its config table.

use crate::abi::OptionSpec;
use crate::config_error::ConfigError;

/// A cop's option struct, backing its `[cops.rules."Name"]` table.
///
/// `Default` lets the runtime hand a cop an `Options` value even with no
/// user config. `SCHEMA` is an associated `const` so it is readable from
/// `static` / `const fn` contexts (what `register_cops!` — murphy-9cr.21
/// — needs). `#[derive(CopOptions)]` (murphy-9cr.21) overrides
/// `from_config_json` with field-by-field decoding.
pub trait CopOptions: Default + Sized + 'static {
    /// Static schema, one entry per option. Empty for [`NoOptions`].
    const SCHEMA: &'static [OptionSpec] = &[];

    /// Decode an `Options` value from the cop's config table (a JSON
    /// object). The default ignores the input and returns [`Default`],
    /// correct for cops that take no configuration.
    fn from_config_json(_bytes: &[u8]) -> Result<Self, ConfigError> {
        Ok(Self::default())
    }
}

/// Marker for cops that declare no options.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoOptions;

impl CopOptions for NoOptions {}
```

Add to `lib.rs`: `mod abi;`, `mod options;`, and `pub use abi::{OptionSpec, RawSlice};`, `pub use options::{CopOptions, NoOptions};`.

**Step 5: Run to verify passing.**
Run: `CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo test -p murphy-plugin-api`
Expected: PASS.

**Step 6: Commit.**

```bash
git add -A
git commit -m "feat(murphy-plugin-api): add RawSlice, OptionSpec, CopOptions"
```

---

### Task 4: `Cop` trait (metadata-only)

**Files:**
- Create: `crates/murphy-plugin-api/src/cop.rs`
- Modify: `crates/murphy-plugin-api/src/lib.rs`

**Step 1: Write the failing test.** In `cop.rs` `#[cfg(test)]`:

```rust
#[test]
fn cop_metadata_consts_are_readable() {
    struct Stub;
    impl Cop for Stub {
        type Options = crate::NoOptions;
        const NAME: &'static str = "Plugin/Stub";
        const DEFAULT_SEVERITY: Option<crate::Severity> = Some(crate::Severity::Warning);
    }
    assert_eq!(<Stub as Cop>::NAME, "Plugin/Stub");
    assert_eq!(<Stub as Cop>::DESCRIPTION, ""); // default
    assert_eq!(<Stub as Cop>::DEFAULT_SEVERITY, Some(crate::Severity::Warning));
    assert_eq!(<Stub as Cop>::DEFAULT_ENABLED, None); // default
}
```

**Step 2: Run to verify failure.**
Run: `CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo test -p murphy-plugin-api`
Expected: FAIL — `cop` module / `Cop` absent.

**Step 3: Implement `cop.rs`:**

```rust
//! The `Cop` trait — a cop's compile-time metadata.

use crate::options::CopOptions;
use crate::severity::Severity;

/// A cop, as authored against the plugin API: **metadata only**.
///
/// Every field is an associated `const` so `register_cops!`
/// (murphy-9cr.21) can assemble the static registration table at
/// const-eval time. Runtime dispatch lives on [`NodeCop`](crate::NodeCop).
/// This continues the const-based, stateless-cop design of ADR 0035.
pub trait Cop: Send + Sync + 'static {
    /// Option struct backing this cop's config table. [`NoOptions`] for
    /// cops with no configuration beyond `enabled` / `severity`.
    ///
    /// [`NoOptions`]: crate::NoOptions
    type Options: CopOptions;

    /// The cop identifier, e.g. `"Plugin/MyCop"`. Must match the name in
    /// `murphy.toml` and offense JSON.
    const NAME: &'static str;

    /// One-line human-readable description. Empty by default.
    const DESCRIPTION: &'static str = "";

    /// Default severity when the user does not override it. `None` leaves
    /// Murphy's built-in fallback.
    const DEFAULT_SEVERITY: Option<Severity> = None;

    /// Default enablement. `None` keeps Murphy's built-in default.
    const DEFAULT_ENABLED: Option<bool> = None;
}
```

Add to `lib.rs`: `mod cop;` and `pub use cop::Cop;`.

**Step 4: Run to verify passing.**
Run: `CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo test -p murphy-plugin-api`
Expected: PASS.

**Step 5: Commit.**

```bash
git add -A
git commit -m "feat(murphy-plugin-api): add metadata-only Cop trait"
```

---

### Task 5: `RawOffense`, `RawEdit`, `FnTable`, `CxRaw`

**Files:**
- Modify: `crates/murphy-plugin-api/src/abi.rs` (append the four types + layout asserts)
- Modify: `crates/murphy-plugin-api/src/lib.rs`

**Step 1: Write the failing tests.** Add to `abi.rs` `#[cfg(test)]`:

```rust
#[test]
fn fn_table_field_offsets_are_frozen() {
    use std::mem::offset_of;
    // Two function pointers; reordering them must fail this test.
    assert_eq!(offset_of!(FnTable, emit_offense), 0);
    assert_eq!(offset_of!(FnTable, emit_edit), size_of::<usize>());
}

#[test]
fn raw_offense_field_offsets_are_frozen() {
    use std::mem::offset_of;
    assert_eq!(offset_of!(RawOffense, cop_name), 0);
    assert_eq!(offset_of!(RawOffense, message), size_of::<RawSlice>());
    assert_eq!(offset_of!(RawOffense, range), 2 * size_of::<RawSlice>());
}

#[test]
fn cx_raw_first_and_last_field_offsets_are_frozen() {
    use std::mem::offset_of;
    assert_eq!(offset_of!(CxRaw, nodes), 0);
    // `sink` is the last field; this pins the struct's field tail.
    assert!(offset_of!(CxRaw, sink) + size_of::<*mut std::ffi::c_void>()
        <= size_of::<CxRaw>());
}
```

(`use std::mem::size_of;` at the test module top if not already imported.)

**Step 2: Run to verify failure.**
Run: `CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo test -p murphy-plugin-api`
Expected: FAIL — types absent.

**Step 3: Implement.** Append to `abi.rs` (after `OptionSpec`, before the test module). Add `use std::ffi::c_void;` and `use murphy_ast::{AstNode, Comment, NodeId, Range};` at the file top:

```rust
/// `#[repr(C)]` offense payload passed to [`FnTable::emit_offense`].
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RawOffense {
    /// Reporting cop's `NAME`.
    pub cop_name: RawSlice,
    /// Human-readable offense message.
    pub message: RawSlice,
    /// Source byte range of the offense.
    pub range: Range,
    /// Severity wire byte (see [`Severity::to_wire`](crate::Severity::to_wire));
    /// `SEVERITY_UNSET` defers to the host default.
    pub severity: u8,
}

/// `#[repr(C)]` autocorrect edit passed to [`FnTable::emit_edit`].
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RawEdit {
    /// Source byte range the edit replaces.
    pub range: Range,
    /// Replacement text.
    pub replacement: RawSlice,
}

/// `#[repr(C)]` table of host operations a cop cannot perform by direct
/// memory read — i.e. writing into the host's offense sink.
///
/// Everything else a cop needs (traversal, `NodeKind` matching, interner
/// resolution, comments, source text) is a pure read of the immutable
/// arena and lives on [`Cx`](crate::Cx) directly, off the ABI's hot path.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FnTable {
    /// Record one offense into `sink`.
    pub emit_offense: unsafe extern "C" fn(*mut c_void, *const RawOffense),
    /// Record one autocorrect edit into `sink`.
    pub emit_edit: unsafe extern "C" fn(*mut c_void, *const RawEdit),
}

unsafe impl Sync for FnTable {}

/// `#[repr(C)]` bundle the host passes per dispatch call. [`Cx<'a>`] is
/// the safe wrapper built from a borrowed `&CxRaw`.
///
/// [`Cx<'a>`]: crate::Cx
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CxRaw {
    /// Arena node array.
    pub nodes: *const AstNode,
    pub nodes_len: usize,
    /// `node_lists` side table (variable-length children).
    pub lists: *const NodeId,
    pub lists_len: usize,
    /// Interner blob.
    pub interner_blob: *const u8,
    pub interner_blob_len: usize,
    /// Interner per-entry offsets.
    pub interner_offsets: *const Range,
    pub interner_offsets_len: usize,
    /// Source comments.
    pub comments: *const Comment,
    pub comments_len: usize,
    /// Source text (UTF-8).
    pub source: *const u8,
    pub source_len: usize,
    /// Arena root node.
    pub root: NodeId,
    /// Reporting cop's `NAME`, stamped into every emitted `RawOffense`.
    pub cop_name: RawSlice,
    /// Host operation table.
    pub fns: *const FnTable,
    /// Opaque host offense sink, passed back to `fns` callbacks.
    pub sink: *mut c_void,
}
```

Add to `lib.rs`: `pub use abi::{CxRaw, FnTable, OptionSpec, RawEdit, RawOffense, RawSlice};` (extend the existing `abi` re-export).

**Step 4: Run to verify passing.**
Run: `CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo test -p murphy-plugin-api`
Expected: PASS.

**Step 5: Commit.**

```bash
git add -A
git commit -m "feat(murphy-plugin-api): add FnTable, CxRaw, RawOffense/RawEdit"
```

---

### Task 6: `Cx<'a>` — direct-read accessors

`Cx` is the safe wrapper over `&CxRaw`. All read accessors are pure memory reads; traversal delegates to `murphy_ast::collect_children`.

**Performance note:** `children` / `descendants` materialize a `Vec<NodeId>` per call because `collect_children` writes into a `Vec`. This is one allocation per traversal step on the future dispatch hot path — accepted for v1. An allocation-free children iterator, if ever needed, would be added here.

**Files:**
- Create: `crates/murphy-plugin-api/src/cx.rs`
- Modify: `crates/murphy-plugin-api/src/lib.rs`

**Step 1: Write the failing tests.** In `cx.rs` `#[cfg(test)]`. The helper builds a real `Ast`, then a `CxRaw` pointing into it, then a `Cx`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use murphy_ast::{Ast, AstBuilder, NodeKind, OptNodeId, Range};

    /// Build `return nil` and the matching CxRaw. Returns the owned `Ast`
    /// (kept alive by the caller) and the root id.
    fn fixture() -> (Ast, murphy_ast::NodeId) {
        let mut b = AstBuilder::new("return nil", "t.rb".into());
        let nil = b.push(NodeKind::Nil, Range { start: 7, end: 10 });
        let root = b.push(NodeKind::Return(OptNodeId::some(nil)), Range { start: 0, end: 10 });
        (b.finish(root), root)
    }

    // A FnTable is required to construct CxRaw; reads never call it.
    unsafe extern "C" fn noop_offense(_: *mut std::ffi::c_void, _: *const RawOffense) {}
    unsafe extern "C" fn noop_edit(_: *mut std::ffi::c_void, _: *const RawEdit) {}

    fn cx_raw_for<'a>(ast: &'a Ast, fns: &'a FnTable) -> CxRaw {
        // Build a CxRaw from the Ast's parts. murphy-ast exposes the
        // accessors used here; for the raw slices, use the crate's
        // serialization/þview surface. If murphy-ast does not expose the
        // backing slices, this test helper constructs them via the public
        // accessors node-by-node — see Step 3 note.
        unimplemented!("filled in Step 3")
    }

    #[test]
    fn accessors_match_the_underlying_ast() {
        let (ast, root) = fixture();
        let fns = FnTable { emit_offense: noop_offense, emit_edit: noop_edit };
        let raw = cx_raw_for(&ast, &fns);
        let cx = unsafe { Cx::from_raw(&raw) };

        assert_eq!(cx.root(), root);
        assert_eq!(*cx.kind(root), *ast.kind(root));
        assert_eq!(cx.range(root), ast.range(root));
        assert_eq!(cx.parent(root), ast.parent(root));
        assert_eq!(cx.children(root), ast.children(root).collect::<Vec<_>>());
        let desc: Vec<_> = cx.descendants(root);
        assert_eq!(desc, ast.descendants(root).collect::<Vec<_>>());
    }
}
```

**Implementation reality for `cx_raw_for`:** `Cx` needs raw pointers into the arena's `nodes` / `node_lists` / interner / comments / source. `murphy-ast`'s `Ast` keeps those `pub(crate)`. To build a `CxRaw` in a test (and, later, in the murphy-9cr.22 host) you need access to those slices. **Before writing Task 6, check `crates/murphy-ast/src/`** for an existing slice/pointer accessor (e.g. an `as_raw`/`parts` method, or `serialize` internals). If none exists, add a minimal, additive `murphy-ast` accessor — e.g. `Ast::raw_parts(&self) -> AstRawParts` returning the borrowed slices — in this task, with its own murphy-ast test. This is the one place Task 6 may extend `murphy-ast`; keep it additive and slice-borrowing (no new owned types, no `NodeKind` change). Document the choice in the commit message.

**Step 2: Run to verify failure.**
Run: `CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo test -p murphy-plugin-api`
Expected: FAIL — `Cx` absent.

**Step 3: Implement `cx.rs`** (read surface only — emit is Task 7):

```rust
//! `Cx<'a>` — the single surface through which a cop reads the AST.

use std::marker::PhantomData;

use murphy_ast::{collect_children, AstNode, Comment, NodeId, NodeKind, OptNodeId, Range};

use crate::abi::CxRaw;

/// Borrowed, direct-read view of the arena for one dispatch call.
///
/// Traversal and `NodeKind` matching are pure memory reads — zero FFI
/// (ADR 0038). The lifetime `'a` forbids retaining any part past the
/// call; the arena is immutable and host-owned for the call's duration.
#[derive(Clone, Copy)]
pub struct Cx<'a> {
    raw: &'a CxRaw,
    _marker: PhantomData<&'a murphy_ast::Ast>,
}

/// Reconstruct a slice from a `#[repr(C)]` pointer+length pair.
///
/// # Safety
/// `len == 0` → empty; otherwise `ptr..ptr+len` must be valid for `'a`.
unsafe fn slice<'a, T>(ptr: *const T, len: usize) -> &'a [T] {
    if len == 0 { &[] } else { unsafe { std::slice::from_raw_parts(ptr, len) } }
}

impl<'a> Cx<'a> {
    /// Wrap a raw context.
    ///
    /// # Safety
    /// Every pointer/length pair in `raw` must describe live, immutable
    /// data valid for `'a`, and `raw.fns` must be non-null. The host
    /// upholds this for one dispatch call (ADR 0038 safety contract).
    pub unsafe fn from_raw(raw: &'a CxRaw) -> Cx<'a> {
        Cx { raw, _marker: PhantomData }
    }

    fn nodes(&self) -> &'a [AstNode] {
        unsafe { slice(self.raw.nodes, self.raw.nodes_len) }
    }

    fn lists(&self) -> &'a [NodeId] {
        unsafe { slice(self.raw.lists, self.raw.lists_len) }
    }

    /// The arena root node.
    pub fn root(&self) -> NodeId {
        self.raw.root
    }

    /// The node at `id`.
    pub fn node(&self, id: NodeId) -> &'a AstNode {
        &self.nodes()[id.0 as usize]
    }

    /// The kind of the node at `id`.
    pub fn kind(&self, id: NodeId) -> &'a NodeKind {
        &self.nodes()[id.0 as usize].kind
    }

    /// The source range of the node at `id`.
    pub fn range(&self, id: NodeId) -> Range {
        self.nodes()[id.0 as usize].range
    }

    /// The parent of `id`; `OptNodeId::NONE` for the root.
    pub fn parent(&self, id: NodeId) -> OptNodeId {
        self.nodes()[id.0 as usize].parent
    }

    /// Direct children of `id`, in source order. Allocates (see the
    /// plan's Task 6 performance note).
    pub fn children(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        collect_children(self.kind(id), self.lists(), &mut out);
        out
    }

    /// Ancestors of `id`, nearest first, up to and including the root.
    pub fn ancestors(&self, id: NodeId) -> impl Iterator<Item = NodeId> + 'a {
        let nodes = self.nodes();
        let mut current = nodes[id.0 as usize].parent;
        std::iter::from_fn(move || {
            let next = current.get()?;
            current = nodes[next.0 as usize].parent;
            Some(next)
        })
    }

    /// All descendants of `id` in DFS pre-order, excluding `id`. Allocates.
    pub fn descendants(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = self.children(id);
        stack.reverse();
        while let Some(n) = stack.pop() {
            out.push(n);
            let mut kids = self.children(n);
            kids.reverse();
            stack.extend(kids);
        }
        out
    }

    /// Resolve an interner index (`Symbol` / `StringId`) to its string.
    fn resolve(&self, index: u32) -> &'a str {
        let offsets: &[Range] =
            unsafe { slice(self.raw.interner_offsets, self.raw.interner_offsets_len) };
        let blob: &[u8] =
            unsafe { slice(self.raw.interner_blob, self.raw.interner_blob_len) };
        let r = offsets[index as usize];
        std::str::from_utf8(&blob[r.start as usize..r.end as usize])
            .expect("interner blob holds valid UTF-8")
    }

    /// The string behind an interned `Symbol`.
    pub fn symbol_str(&self, sym: murphy_ast::Symbol) -> &'a str {
        self.resolve(sym.0)
    }

    /// The contents behind an interned string-literal `StringId`.
    pub fn string_str(&self, id: murphy_ast::StringId) -> &'a str {
        self.resolve(id.0)
    }

    /// The file's comments, in source order.
    pub fn comments(&self) -> &'a [Comment] {
        unsafe { slice(self.raw.comments, self.raw.comments_len) }
    }

    /// The source text covered by `range`.
    pub fn raw_source(&self, range: Range) -> &'a str {
        let src: &[u8] = unsafe { slice(self.raw.source, self.raw.source_len) };
        std::str::from_utf8(&src[range.start as usize..range.end as usize])
            .expect("source is valid UTF-8")
    }
}
```

Add to `lib.rs`: `mod cx;` and `pub use cx::Cx;`.

**Step 4: Run to verify passing.**
Run: `CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo test -p murphy-plugin-api`
Expected: PASS.

**Step 5: Commit.**

```bash
git add -A
git commit -m "feat(murphy-plugin-api): add Cx direct-read accessors"
```

---

### Task 7: `Cx` — offense / edit emission

**Files:**
- Modify: `crates/murphy-plugin-api/src/cx.rs` (add emit methods + tests)

**Step 1: Write the failing test.** Add to `cx.rs` `#[cfg(test)]`. A fake `FnTable` records calls into a sink:

```rust
use std::cell::RefCell;

struct Sink {
    offenses: Vec<(String, String, Range, u8)>,
    edits: Vec<(Range, String)>,
}

unsafe extern "C" fn record_offense(sink: *mut std::ffi::c_void, o: *const RawOffense) {
    let sink = unsafe { &*(sink as *const RefCell<Sink>) };
    let o = unsafe { &*o };
    sink.borrow_mut().offenses.push((
        String::from_utf8(unsafe { o.cop_name.as_bytes() }.to_vec()).unwrap(),
        String::from_utf8(unsafe { o.message.as_bytes() }.to_vec()).unwrap(),
        o.range,
        o.severity,
    ));
}

unsafe extern "C" fn record_edit(sink: *mut std::ffi::c_void, e: *const RawEdit) {
    let sink = unsafe { &*(sink as *const RefCell<Sink>) };
    let e = unsafe { &*e };
    sink.borrow_mut().edits.push((
        e.range,
        String::from_utf8(unsafe { e.replacement.as_bytes() }.to_vec()).unwrap(),
    ));
}

#[test]
fn emit_forwards_offense_and_edit_to_the_fn_table() {
    let (ast, root) = fixture();
    let fns = FnTable { emit_offense: record_offense, emit_edit: record_edit };
    let sink = RefCell::new(Sink { offenses: Vec::new(), edits: Vec::new() });

    let mut raw = cx_raw_for(&ast, &fns);
    raw.cop_name = RawSlice::from_str("Plugin/Demo");
    raw.sink = &sink as *const _ as *mut std::ffi::c_void;
    let cx = unsafe { Cx::from_raw(&raw) };

    cx.emit_offense(cx.range(root), "bad return", Some(crate::Severity::Error));
    cx.emit_edit(Range { start: 7, end: 10 }, "false");

    let s = sink.borrow();
    assert_eq!(s.offenses.len(), 1);
    assert_eq!(s.offenses[0].0, "Plugin/Demo");
    assert_eq!(s.offenses[0].1, "bad return");
    assert_eq!(s.offenses[0].3, crate::Severity::to_wire(Some(crate::Severity::Error)));
    assert_eq!(s.edits, vec![(Range { start: 7, end: 10 }, "false".to_string())]);
}
```

**Step 2: Run to verify failure.**
Run: `CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo test -p murphy-plugin-api`
Expected: FAIL — `emit_offense` / `emit_edit` absent.

**Step 3: Implement.** Add to `impl<'a> Cx<'a>` in `cx.rs`:

```rust
    /// Record an offense. `cop_name` is stamped from the `CxRaw` the host
    /// built for the running cop.
    pub fn emit_offense(&self, range: Range, message: &str, severity: Option<crate::Severity>) {
        let offense = crate::RawOffense {
            cop_name: self.raw.cop_name,
            message: crate::RawSlice { ptr: message.as_ptr(), len: message.len() },
            range,
            severity: crate::Severity::to_wire(severity),
        };
        // Safety: `fns` is non-null and `sink` valid per `from_raw`'s
        // contract; the message slice outlives this synchronous call.
        let fns = unsafe { &*self.raw.fns };
        unsafe { (fns.emit_offense)(self.raw.sink, &offense) };
    }

    /// Record an autocorrect edit. Offense↔edit correlation is the host's
    /// (murphy-9cr.22) concern.
    pub fn emit_edit(&self, range: Range, replacement: &str) {
        let edit = crate::RawEdit {
            range,
            replacement: crate::RawSlice {
                ptr: replacement.as_ptr(),
                len: replacement.len(),
            },
        };
        // Safety: see `emit_offense`.
        let fns = unsafe { &*self.raw.fns };
        unsafe { (fns.emit_edit)(self.raw.sink, &edit) };
    }
```

**Step 4: Run to verify passing.**
Run: `CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo test -p murphy-plugin-api`
Expected: PASS.

**Step 5: Commit.**

```bash
git add -A
git commit -m "feat(murphy-plugin-api): add Cx offense/edit emission"
```

---

### Task 8: `NodeKindTag` + `NodeCop` trait

`NodeCop` is the single dispatch trait. Its `KINDS` declares target node kinds via `NodeKindTag` — a `u8`-discriminant newtype.

**Conflict-avoidance note:** murphy-9cr.17 (in parallel) adds an identically-shaped `NodeKindTag(pub u8)` to `murphy-ast`. To avoid a `murphy-ast` merge conflict, **murphy-9cr.20 defines its own `NodeKindTag` inside `murphy-plugin-api`**, not in `murphy-ast`. They are the same shape, so the dedup (re-export `murphy_ast::NodeKindTag`) is trivial once .17 lands — that dedup is filed as a follow-up issue in Task 10.

**Files:**
- Create: `crates/murphy-plugin-api/src/node_cop.rs`
- Modify: `crates/murphy-plugin-api/src/lib.rs`

**Step 1: Write the failing tests.** In `node_cop.rs` `#[cfg(test)]`:

```rust
#[test]
fn node_kind_tag_reads_the_discriminant() {
    use murphy_ast::{NodeKind, Symbol};
    // `#[repr(C, u8)]` discriminants are NodeKind declaration order.
    assert_eq!(NodeKindTag::of(&NodeKind::Error).0, 0);
    assert_eq!(NodeKindTag::of(&NodeKind::Nil).0, 1);
    assert_eq!(NodeKindTag::of(&NodeKind::Lvar(Symbol(0))).0, 9);
}

#[test]
fn node_cop_declares_kinds_and_a_check_fn() {
    use murphy_ast::NodeId;
    struct Stub;
    impl crate::Cop for Stub {
        type Options = crate::NoOptions;
        const NAME: &'static str = "Plugin/Stub";
    }
    impl NodeCop for Stub {
        // NodeKindTag(1) == NodeKind::Nil.
        const KINDS: &'static [NodeKindTag] = &[NodeKindTag(1)];
        fn check(&self, _node: NodeId, _cx: &crate::Cx<'_>) {}
    }
    assert_eq!(<Stub as NodeCop>::KINDS, &[NodeKindTag(1)]);
}
```

**Step 2: Run to verify failure.**
Run: `CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo test -p murphy-plugin-api`
Expected: FAIL — `node_cop` module absent.

**Step 3: Implement `node_cop.rs`:**

```rust
//! `NodeKindTag` and the `NodeCop` dispatch trait.

use murphy_ast::{NodeId, NodeKind};

use crate::cop::Cop;
use crate::cx::Cx;

/// The `u8` discriminant of a [`NodeKind`] variant — its payload-free
/// projection, used to declare a [`NodeCop`]'s dispatch targets.
///
/// The discriminant is `NodeKind`'s `#[repr(C, u8)]` declaration order,
/// frozen by ADR 0037. (murphy-ast grows an identical `NodeKindTag` in
/// murphy-9cr.17; this crate keeps its own copy to stay mergeable in
/// parallel — see the plan's Task 8 note.)
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeKindTag(pub u8);

impl NodeKindTag {
    /// The tag of a node kind.
    pub fn of(kind: &NodeKind) -> NodeKindTag {
        // Safety: `NodeKind` is `#[repr(C, u8)]`, so its first byte is the
        // discriminant (ADR 0037 — frozen layout).
        NodeKindTag(unsafe { *(kind as *const NodeKind as *const u8) })
    }
}

/// The dispatch trait: a cop subscribes to node kinds and is called once
/// per matching node.
///
/// Merges the spike's `NodeCop` and `CallCop` (a call cop is just a
/// `NodeCop` on `Send`); `FileCop` / `run_file` are deleted (ADR 0038).
pub trait NodeCop: Cop {
    /// Node kinds this cop is dispatched on. `#[on_node]` (murphy-9cr.8)
    /// generates this; until then it is written by hand.
    const KINDS: &'static [NodeKindTag];

    /// Inspect one matched node. Stateless: everything the callback needs
    /// is `node` and `cx`.
    fn check(&self, node: NodeId, cx: &Cx<'_>);
}
```

Add to `lib.rs`: `mod node_cop;` and `pub use node_cop::{NodeCop, NodeKindTag};`.

**Step 4: Run to verify passing.**
Run: `CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo test -p murphy-plugin-api`
Expected: PASS.

**Step 5: Commit.**

```bash
git add -A
git commit -m "feat(murphy-plugin-api): add NodeKindTag and NodeCop dispatch trait"
```

---

### Task 9: `.so` ABI boundary — `PluginCopV1`, `PluginRegistration`, entry point

The structs and the entry-point signature crossing the `.so` boundary. The `register_cops!` macro that *fills* them is murphy-9cr.21; the `dlopen` loader that *reads* them is murphy-9cr.22.

**Files:**
- Modify: `crates/murphy-plugin-api/src/abi.rs` (append + layout asserts)
- Modify: `crates/murphy-plugin-api/src/lib.rs`

**Step 1: Write the failing tests.** Add to `abi.rs` `#[cfg(test)]`:

```rust
#[test]
fn abi_version_is_one() {
    assert_eq!(MURPHY_PLUGIN_ABI_VERSION, 1);
}

#[test]
fn plugin_cop_v1_field_offsets_are_frozen() {
    use std::mem::offset_of;
    assert_eq!(offset_of!(PluginCopV1, size), 0);
    assert_eq!(offset_of!(PluginCopV1, name), size_of::<usize>());
    // `dispatch` is the last field.
    assert!(offset_of!(PluginCopV1, dispatch) + size_of::<DispatchFn>()
        <= size_of::<PluginCopV1>());
}

#[test]
fn plugin_registration_field_offsets_are_frozen() {
    use std::mem::offset_of;
    assert_eq!(offset_of!(PluginRegistration, abi_version), 0);
}
```

**Step 2: Run to verify failure.**
Run: `CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo test -p murphy-plugin-api`
Expected: FAIL — types absent.

**Step 3: Implement.** Append to `abi.rs` (before the test module). Add `use crate::node_cop::NodeKindTag;` at the file top:

```rust
/// The plugin ABI version. A fresh v1 (ADR 0038-8): the pre-reboot ABI
/// was never frozen, so this is a new ABI starting at 1, not a bump.
pub const MURPHY_PLUGIN_ABI_VERSION: u32 = 1;

/// The dispatch entry for one cop: invoked once per matching node.
///
/// The thunk (generated by `register_cops!`, murphy-9cr.21) wraps a
/// `NodeCop::check`. It must not unwind across the boundary (ADR 0038
/// safety contract) and returns `0` on success, non-zero on a trapped
/// panic.
pub type DispatchFn = unsafe extern "C" fn(node: NodeId, cx: *const CxRaw) -> i32;

/// `#[repr(C)]` registration descriptor for one cop.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PluginCopV1 {
    /// `size_of::<PluginCopV1>()` — the loader rejects a mismatch.
    pub size: usize,
    /// Cop `NAME`.
    pub name: RawSlice,
    /// Cop `DESCRIPTION`.
    pub description: RawSlice,
    /// Default severity wire byte.
    pub default_severity: u8,
    /// Default enablement tristate byte.
    pub default_enabled: u8,
    /// `CopOptions::SCHEMA`.
    pub options_ptr: *const OptionSpec,
    pub options_len: usize,
    /// `NodeCop::KINDS`.
    pub kinds_ptr: *const NodeKindTag,
    pub kinds_len: usize,
    /// Per-node dispatch entry.
    pub dispatch: DispatchFn,
}

unsafe impl Sync for PluginCopV1 {}

/// `#[repr(C)]` table the plugin's single entry point fills in.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PluginRegistration {
    /// Must equal [`MURPHY_PLUGIN_ABI_VERSION`]; the loader rejects a mismatch.
    pub abi_version: u32,
    /// The plugin's cop table.
    pub cops_ptr: *const PluginCopV1,
    pub cops_len: usize,
}

/// The one symbol a plugin `.so` exports, generated by `register_cops!`
/// (murphy-9cr.21). The loader calls it to obtain the cop table; it
/// returns `0` on success.
pub type MurphyPluginRegister = unsafe extern "C" fn(*mut PluginRegistration) -> i32;
```

Extend the `abi` re-export in `lib.rs`:
`pub use abi::{CxRaw, DispatchFn, FnTable, MurphyPluginRegister, MURPHY_PLUGIN_ABI_VERSION, OptionSpec, PluginCopV1, PluginRegistration, RawEdit, RawOffense, RawSlice};`

**Step 4: Run to verify passing.**
Run: `CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo test -p murphy-plugin-api`
Expected: PASS.

**Step 5: Commit.**

```bash
git add -A
git commit -m "feat(murphy-plugin-api): add .so registration ABI structs"
```

---

### Task 10: Public surface, crate docs, full workspace gate

**Files:**
- Modify: `crates/murphy-plugin-api/src/lib.rs` (final crate doc; verify the re-export surface)

**Step 1: Finalize `lib.rs`.** Confirm the module list and re-exports cover the whole public surface, and write the crate-level doc:

```rust
//! Safe, plugin-author-facing surface over the Murphy single-surface
//! plugin ABI (ADR 0038).
//!
//! Every cop — built-in or external `.so` — reads the AST through this
//! one crate. A callback receives a [`Cx`], a direct-read view of an
//! immutable [`murphy-ast`](murphy_ast) arena: traversal and `NodeKind`
//! matching are pure memory reads. [`Cop`] carries compile-time metadata;
//! [`NodeCop`] carries dispatch. The `#[repr(C)]` types in [`abi`] cross
//! the `.so` boundary.
//!
//! `register_cops!` / `#[derive(CopOptions)]` / `#[on_node]` live in
//! `murphy-plugin-macros` (murphy-9cr.21 / .8) and consume this surface.

mod abi;
mod config_error;
mod cop;
mod cx;
mod node_cop;
mod options;
mod severity;

pub use abi::{
    CxRaw, DispatchFn, FnTable, MurphyPluginRegister, MURPHY_PLUGIN_ABI_VERSION, OptionSpec,
    PluginCopV1, PluginRegistration, RawEdit, RawOffense, RawSlice,
};
pub use config_error::{ConfigError, ConfigErrorKind};
pub use cop::Cop;
pub use cx::Cx;
pub use node_cop::{NodeCop, NodeKindTag};
pub use options::{CopOptions, NoOptions};
pub use severity::{tristate_from_wire, tristate_to_wire, Severity, SEVERITY_UNSET, TRISTATE_UNSET};
```

(`mod abi` may be declared `pub mod abi` if the crate doc links `[abi]` — otherwise reword the doc to not link a private module. Decide based on whether the `abi` types are better browsed as a module; the re-exports above already flatten them to the crate root.)

**Step 2: Run the full quality gate.**

```bash
CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo test --workspace
CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo clippy --workspace --all-targets -- -D warnings
CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo fmt --check
CARGO_TARGET_DIR=/home/ubuntu/projects/murphy/target cargo build --workspace
```

Expected: all PASS. Fix any clippy / fmt findings before committing.

**Step 3: Commit.**

```bash
git add -A
git commit -m "feat(murphy-plugin-api): finalize single-surface public API"
```

**Step 4: File the follow-up.** After the gate is green, file the dedup follow-up:

```bash
bd create --title="murphy-plugin-api: NodeKindTag を murphy_ast::NodeKindTag へ統合" \
  --description="murphy-9cr.20 は murphy-9cr.17 との murphy-ast 衝突回避のため plugin-api 内に独自 NodeKindTag(pub u8) を定義した。murphy-9cr.17 マージ後、plugin-api は murphy_ast::NodeKindTag を re-export し独自定義を削除する。" \
  --type=task --priority=3
```

---

## Acceptance

The issue's `acceptance` field is the checklist. In short: the new surface (`Cx` / `Cop` / `NodeCop` / `FnTable` / the `abi` types / `Severity` / `CopOptions`) is exported; the legacy surface is gone; `murphy-plugin-api` depends on `murphy-ast`, not `murphy-core`; `collect_children` is `pub`; the macros test suite is neutralized; `MURPHY_PLUGIN_ABI_VERSION == 1`; and `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`, `cargo build --workspace` all pass.
