# murphy-9cr.2 — Plugin ABI Extension Design

**Status**: design accepted 2026-05-22, ready for TDD implementation
**Issue**: murphy-9cr.2 (parent epic: murphy-9cr)
**Related ADR**: 0033 (to be authored as part of this work), 0032 (plugin-9cr.11, JSON schema fixed)

## Background

The current plugin ABI (`crates/murphy-core/src/plugin.rs:97-132`) exposes only `MurphyPluginCopV1 { size, name, run_file }`. Plugin authors cannot declare cop description, default severity, default enablement, or per-option schema metadata. Without those, the core cannot run option-schema validation (murphy-9cr.9), produce a `murphy plugins list` diagnostic, or back the proc-macro DX (`#[derive(CopOptions)]` in murphy-9cr.7).

ABI/config surface is still pre-freeze (Phase 6 complete). Existing precedent for ABI extension is murphy-aki / commit `42b96cd` (Add RuboCop node dispatch hooks) which appends fields to ABI v1 structs and relies on plugin authors rebuilding against the new headers; the `size` check at `plugin.rs:846` is strict equality. We continue the same operating model.

## Scope

1. New struct `MurphyCopOptionV1` (option schema entry, no `size` field; same layout family as `MurphyCallDispatchV1` / `MurphyNodeDispatchV1`).
2. Extend `MurphyPluginCopV1` with `description`, `default_severity`, `default_enabled`, `options_ptr`, `options_len` (appended after existing fields; preserves `size`-based discrimination).
3. Loader validation in `PluginPack::load` (`plugin.rs:840`+) for the new fields.
4. Core-side intermediate representation: new struct `CopOptionMetadata` (owned), stored on `PluginFileCop`.
5. Test coverage in `tests/plugin_abi_exports.rs` and a new integration test fixture per reject case.
6. ADR 0033 — record the v1 extension policy (strict equality, plugin rebuild required, sentinel encodings).

## Non-scope

- `SafeAutoCorrect` / `AutoCorrect` cop self-declaration → **murphy-9cr.12** (split out, depends on this issue).
- Proc-macro emission of metadata (`#[derive(Cop)]` / `#[derive(CopOptions)]`) → **murphy-9cr.6 / .7**.
- JSON parsing of `default_json` / `enum_values_json` and schema-vs-config diffing → **murphy-9cr.9** (validation gate).
- Cop trait public API additions on `crates/murphy-core/src/cop.rs` → deferred; this issue only adds storage on `PluginFileCop`.
- Forward-compat (`size >=` relaxation) and ABI v2 split-out — Phase 6 freeze hasn't happened, strict equality is acceptable.
- RuboCop-style multi-alternative, `minimum_ruby_version` gating, contextual autocorrect.

## ABI Struct Definitions

### `MurphyCopOptionV1` (new)

```rust
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MurphyCopOptionV1 {
    pub name: MurphySlice,            // non-empty required
    pub ty: MurphySlice,              // non-empty required: "bool" | "int" | "string" | "string_list" | "enum"
    pub default_json: MurphySlice,    // empty = no default (required option); "null" literal = default is JSON null
    pub description: MurphySlice,     // free-form, empty allowed
    pub enum_values_json: MurphySlice,// JSON array text, empty = no enum constraint
    pub replacement: MurphySlice,     // RuboCop `alternative` analog, empty = no rename hint
    pub reason: MurphySlice,          // RuboCop `reason` analog, empty = no reason text
}
unsafe impl Sync for MurphyCopOptionV1 {}
```

- No `size` field. Layout aligned with `MurphyCallDispatchV1` / `MurphyNodeDispatchV1`.
- "Deprecated" is implicit: `replacement` or `reason` non-empty ⇒ option treated as deprecated by the validation gate.
- Default severity for deprecated-option warning is fixed to `warn` (murphy-9cr.9 spec).
- All `MurphySlice` "len == 0" means "absent" (consistent with the rest of the ABI).

### `MurphyPluginCopV1` (extended)

```rust
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MurphyPluginCopV1 {
    pub size: usize,                                // existing
    pub name: MurphySlice,                          // existing
    pub run_file: Option<MurphyRunFile>,            // existing
    // appended in v1:
    pub description: MurphySlice,                   // empty allowed
    pub default_severity: u8,                       // 0=Warning, 1=Error, 255=unset
    pub default_enabled: u8,                        // 0=false, 1=true, 255=unset
    pub options_ptr: *const MurphyCopOptionV1,      // null only when options_len == 0
    pub options_len: usize,
}
unsafe impl Sync for MurphyPluginCopV1 {}  // existing impl already covers raw pointer
```

ADR 0011-style compile-time guard for the severity wire encoding:

```rust
const _: () = assert!(
    Severity::Warning as u8 == 0 && Severity::Error as u8 == 1,
    "ABI v1 default_severity wire encoding pinned to Severity declaration order"
);
```

`sizeof::<MurphyPluginCopV1>()` grows by `description (16) + default_severity (1) + default_enabled (1) + padding + options_ptr (8) + options_len (8)`; total ≈ +56 bytes on x86_64. Exact value asserted in `tests/plugin_abi_exports.rs`.

### Sentinel summary

| Field | Sentinel | Meaning |
|---|---|---|
| `default_severity` | `255` | unset (cop made no declaration) |
| `default_enabled` | `255` | unset |
| any `MurphySlice` | `len == 0` | absent |

## Loader Validation (`PluginPack::load`)

Added between existing cop iteration loop (`plugin.rs:845-859`) and downstream dispatch wiring:

- `cop.size != sizeof::<MurphyPluginCopV1>()` → reject (strict equality, unchanged).
- `cop.description`: UTF-8 check only; empty allowed.
- `cop.default_severity ∈ {0, 1, 255}` else config error (exit 2).
- `cop.default_enabled ∈ {0, 1, 255}` else config error.
- `cop.options_len > 0 && cop.options_ptr.is_null()` → reject.
- For each `MurphyCopOptionV1` in the slice:
  - `name`: non-empty + UTF-8 required.
  - `ty`: non-empty + UTF-8 required. (The set of legal `ty` strings is not enforced here; validation gate handles unknown types.)
  - `default_json` / `enum_values_json`: empty or non-empty; **no JSON parse at load time** (responsibility belongs to murphy-9cr.9).
  - `description` / `replacement` / `reason`: UTF-8 check only; empty allowed.

Reject messages follow existing patterns (`"invalid plugin cop size: got X, expected Y"` style).

## Core Intermediate Representation

```rust
pub struct CopOptionMetadata {
    pub name: Vec<u8>,
    pub ty: Vec<u8>,
    pub default_json: Vec<u8>,
    pub description: Vec<u8>,
    pub enum_values_json: Vec<u8>,
    pub replacement: Vec<u8>,
    pub reason: Vec<u8>,
}

pub struct PluginFileCop {
    // existing fields …
    description: Vec<u8>,
    default_severity: Option<Severity>,    // u8 255 → None, else Some(…)
    default_enabled: Option<bool>,         // u8 255 → None, else Some(…)
    options: Vec<CopOptionMetadata>,
    // existing fields continue …
}
```

Minimum public API surface for downstream issues:

```rust
impl PluginFileCop {
    pub fn description(&self) -> &[u8] { &self.description }
    pub fn default_severity(&self) -> Option<Severity> { self.default_severity }
    pub fn default_enabled(&self) -> Option<bool> { self.default_enabled }
    pub fn options(&self) -> &[CopOptionMetadata] { &self.options }
}
```

No additions to the `Cop` trait (`crates/murphy-core/src/cop.rs`) in this issue; that surface belongs with the proc-macro work in murphy-9cr.6 / .7.

## Test Strategy (TDD)

Unit and integration tests added before implementation:

1. `tests/plugin_abi_exports.rs`
   - `MurphyCopOptionV1` symbol export assertion + `sizeof` constant.
   - `MurphyPluginCopV1` sizeof matches new expected value.
   - Field-offset asserts for new fields (catches accidental reordering on platforms with different padding).

2. Integration tests against synthetic plugin fixtures (use the same fixture infrastructure as existing native plugin tests):
   - Healthy plugin with `description / default_severity / default_enabled / options[N]` declared → loader records expected values.
   - `options_len > 0 && options_ptr == null` → reject.
   - `default_severity = 2` (out of {0,1,255}) → reject.
   - `default_enabled = 99` → reject.
   - `option.name` empty → reject.
   - `option.ty` empty → reject.
   - `default_severity = 255 && default_enabled = 255 && options_len = 0` → load succeeds and `PluginFileCop` accessors return `None` / empty.
   - Old-size plugin (size matches Phase 6 V1, missing new fields) → rejected with the existing size mismatch error (documents the strict-equality breakage policy).

3. Existing tests audited — any helper that constructs `MurphyPluginCopV1` is updated to populate the new fields with sentinels so behaviour is unchanged.

## Implementation Order

1. Add failing assertions to `tests/plugin_abi_exports.rs` (new field offsets, struct sizes).
2. Add failing integration tests (all reject cases listed above).
3. Introduce `MurphyCopOptionV1` and extend `MurphyPluginCopV1` (preserves `unsafe impl Sync`).
4. Add severity-discriminant `const _: () = assert!(…)`.
5. Extend loader validation in `PluginPack::load`.
6. Extend `PluginFileCop` and add accessors. Update all existing constructors / helpers.
7. Run `cargo test --workspace`, `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`.
8. Author `docs/decisions/0033-plugin-abi-v1-option-metadata.md` (strict-equality v1 extension policy, sentinel encodings, JSON schema rationale cross-references 0032).
9. Commit per logical step; squash if necessary for the PR.

## Risks

- **Size assertion is platform-dependent.** Asserting an exact byte count in the test would fail on 32-bit targets. Mitigation: use `std::mem::size_of` directly in the assertion (compare against the new computed value rather than a literal).
- **Strict equality is a one-shot break.** All existing native plugin fixtures must be rebuilt against the new headers in the same PR.
- **Sentinel `255` collides with future Severity variants.** ADR 0011 already forbids reordering; ADR 0033 will record the additional rule "Severity must not have ≥255 variants" (effectively no risk in practice; documented for completeness).

## Open Follow-ups

- **murphy-9cr.12**: SafeAutoCorrect cop self-declaration + CLI `--unsafe-autocorrect` (depends on this issue).
- **murphy-9cr.6 / .7**: proc-macro emission of metadata (`register_cops!`, `#[derive(CopOptions)]`).
- **murphy-9cr.9**: option-schema validation gate (consumes `PluginFileCop::options`).
- **murphy-9cr.11**: ADR 0032 (JSON schema fixed) — cross-reference from ADR 0033.
