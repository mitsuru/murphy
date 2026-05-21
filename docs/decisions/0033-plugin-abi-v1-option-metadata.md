# ADR 0033: Plugin ABI v1 Option Metadata Extension

**Status**: Accepted (2026-05-22)
**Issue**: murphy-9cr.2 (parent epic: murphy-9cr)
**Related**: ADR 0011 (Severity ordering), ADR 0015 (config schema), ADR 0031 (native plugin pack ABI), ADR 0032 (plugin ABI option schema fixed to JSON regardless of user-facing config syntax)

## Context

The native plugin ABI exposed by `crates/murphy-core/src/plugin.rs` lets plugins declare cops via `MurphyPluginCopV1 { size, name, run_file }`. Before this work, plugins could not declare:

- A human-readable cop description (needed for `murphy plugins list` and future LSP hover).
- A default severity (warning / error / unset).
- A default enabled flag (true / false / unset).
- A per-option schema, which is required to drive the validation gate (murphy-9cr.9) and the `#[derive(CopOptions)]` proc macro (murphy-9cr.7).

The ABI surface is still pre-freeze (Phase 6 just completed). The strict-equality `size` check at `plugin.rs:846` already gates ABI compatibility and was set up so that any future plugin must be rebuilt against the new headers — there is no expectation of running a plugin compiled before this ADR. The single precedent for v1 ABI extension is commit `42b96cd` (Add RuboCop node dispatch hooks) which appended fields to `MurphyPluginV1` under the same operating model.

## Decision

1. **Append four fields to `MurphyPluginCopV1` (ABI v1, no version bump)**:
   - `description: MurphySlice` (free-form, may be empty).
   - `default_severity: u8` (0 = warning, 1 = error, 255 = unset).
   - `default_enabled: u8` (0 = false, 1 = true, 255 = unset).
   - `options_ptr: *const MurphyCopOptionV1` plus `options_len: usize` (paired `ptr+len`; null only when `len == 0`).

2. **Introduce `MurphyCopOptionV1`** for per-option schema entries with seven fields, all `MurphySlice` (`name`, `ty`, `default_json`, `description`, `enum_values_json`, `replacement`, `reason`). The struct has *no* `size` field — it follows the same layout family as `MurphyCallDispatchV1` / `MurphyNodeDispatchV1`.

3. **Retain strict-equality `size` check**. Plugins compiled before this ABI extension are rejected with the existing size mismatch error. This is the same forward-compat policy as ADR 0031 in spirit and matches commit `42b96cd`. We accept the one-time breakage because ABI is pre-freeze and all in-tree plugins (`murphy-rails`, `murphy-example-pack`) live in the same workspace.

4. **Sentinel encoding**:
   - `default_severity == 255` → "plugin did not declare a default severity"; loader records `None`.
   - `default_enabled == 255` → "plugin did not declare a default enabled"; loader records `None`.
   - Any `MurphySlice` with `len == 0` → "absent". For text fields (`description`, `replacement`, `reason`) this is "no value". For `default_json` this is "no default at all" — a Required option. The literal JSON token `null` distinguishes "default is explicitly null" from "no default".

5. **Compile-time guard for severity wire encoding**. Continuing ADR 0011's anchor pattern, a `const _: () = assert!(…)` in `plugin.rs` verifies that `Severity::Warning as u8 == MURPHY_SEVERITY_WARNING` and `Severity::Error as u8 == MURPHY_SEVERITY_ERROR`. If anyone reorders `Severity` (already forbidden by ADR 0011), this build also fails, providing redundant protection at the ABI boundary.

6. **Deprecation is implicit, not a separate field**. An option is "deprecated" iff `replacement` or `reason` is non-empty. This matches the 2-axis (alternative + reason) form used by RuboCop's `changed_parameters` rule and avoids an extra `bool deprecated` field that would either need a sentinel or risk drifting from the text fields. Severity of the deprecation warning is fixed to `warn` (see murphy-9cr.9 spec).

7. **JSON, not TOML/YAML, on the ABI boundary**. Per ADR 0032, `default_json` and `enum_values_json` carry JSON-encoded text regardless of the user-facing config syntax. Loader does not parse JSON at this layer; that responsibility belongs to murphy-9cr.9.

8. **Owned intermediate representation in core**. `PluginFileCop` stores the decoded data as `description: Vec<u8>`, `default_severity: Option<Severity>`, `default_enabled: Option<bool>`, and `options: Vec<CopOptionMetadata>` (a new struct that owns each option's bytes). Loader copies out of plugin-static buffers so downstream consumers (murphy-9cr.9 validation gate, future `murphy plugins list`) can hold references without depending on the plugin's `'static` lifetime.

## Consequences

- Existing plugins must be rebuilt — done in-tree for `murphy-rails` (138 cops) and `murphy-example-pack` (3 cops) via the `cop_v1` / `cop_v1_dispatch_only` constructors added to `murphy-core`. These constructors are temporary scaffolding until the `register_cops!` proc macro (murphy-9cr.6) subsumes them.
- The loader now decodes per-cop metadata on load; cost is one allocation per cop plus seven small allocations per option, paid once per `LoadedPluginPack` lifetime.
- ABI v1 grows by ~56 bytes per `MurphyPluginCopV1` on x86_64. Layout drift is caught by `tests/plugin_abi_exports.rs` (sizeof and field offset assertions).
- The validation gate (murphy-9cr.9) and `#[derive(CopOptions)]` (murphy-9cr.7) can now be built against this surface without further ABI changes (modulo the planned `safe_autocorrect: bool` from murphy-9cr.12).
- ADR 0032 governs how schema text is encoded. ADR 0031 (native plugin pack ABI) still describes the overall packaging; this ADR extends only the per-cop record.

## Alternatives Considered

- **Bump to `MurphyPluginCopV2`** (parallel struct, keep v1). Rejected: ABI is pre-freeze, no external plugin needs v1 stability, and v2 doubles maintenance.
- **Relax `size` check to `>=` for forward compatibility**. Rejected for this issue: it would let plugins built against a stale view of the struct silently miss new fields the loader expects (e.g., the validation gate could read defaults that were never set). May be reconsidered post-freeze if we add an ABI versioning hop.
- **Boolean `deprecated` field plus `MurphySlice replacement`**. Rejected: redundant — the implicit "deprecated when replacement/reason non-empty" rule is already required by RuboCop-style messages and avoids representing "deprecated but no hint" twice.
- **JSON pre-parsed at load time** (store `serde_json::Value`). Rejected: loader stays at the bytes layer; murphy-9cr.9 handles parsing alongside the actual config diff, where parse-error semantics (exit 2 vs warn) live.

## Implementation

Implemented in murphy-9cr.2. See:

- `crates/murphy-core/src/plugin.rs` — struct definitions, `cop_v1` / `cop_v1_dispatch_only` constructors, `decode_cop_metadata` helper, loader integration, `PluginFileCop` accessors.
- `crates/murphy-core/src/lib.rs` — re-exports of new types and constants.
- `crates/murphy-core/tests/plugin_abi_exports.rs` — ABI surface coverage.
- 10 new unit tests in `plugin::tests` (sentinel acceptance, severity / tristate decoding, all six reject paths, round-trip of every option field).
- `crates/murphy-rails/src/lib.rs`, `crates/murphy-example-pack/src/lib.rs` — migrated to `cop_v1` / `cop_v1_dispatch_only`.

Design notes: `docs/plans/2026-05-22-murphy-9cr2-abi-extension-design.md`.

## Follow-up Issues

- murphy-9cr.6 — `register_cops!` proc macro (subsumes `cop_v1` / `cop_v1_dispatch_only`).
- murphy-9cr.7 — `#[derive(CopOptions)]` proc macro (populates `MurphyCopOptionV1` entries).
- murphy-9cr.9 — option schema validation gate (consumes `PluginFileCop::options`).
- murphy-9cr.12 — `SafeAutoCorrect` (planned next ABI extension, follows the same strict-equality policy).
