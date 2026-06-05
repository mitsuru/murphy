# AllCops.ActiveSupportExtensionsEnabled config infrastructure — design

Issue: `murphy-pfcb`
Scope: `crates/murphy-plugin-api`, `crates/murphy-core`, `crates/murphy-cli`,
`crates/murphy-std`, `crates/murphy-rails`
Status: design approved 2026-06-04

## 1. Why

RuboCop's `AllCops.ActiveSupportExtensionsEnabled` (default `false`) tells
cops whether ActiveSupport extensions are available. `rubocop-rails` ships a
`config/default.yml` that sets `AllCops.ActiveSupportExtensionsEnabled: true`
under the global `AllCops:` section; loading it via `require:`/`plugins:`
merges that value into the global config, so core `Style/*` cops see it.

`Style/SymbolProc` reads it: when enabled it **exempts** `lambda`/`proc`/
`Proc.new` blocks from the `&:sym` rewrite:

```ruby
if active_support_extensions_enabled?
  return if proc_node?(dispatch_node)
  return if LAMBDA_OR_PROC.include?(dispatch_node.method_name)
end
```

Murphy does not expose this AllCops key to cops, so `Style/SymbolProc` always
takes the `false` path and **flags `lambda`/`proc` blocks unconditionally**.
On Mastodon (which loads the rails pack, i.e. `rubocop-rails` is effectively
enabled with `ActiveSupportExtensionsEnabled: true`), all 11 `Style/SymbolProc`
offenses are `lambda`/`proc` — **100% false positives relative to the project's
own RuboCop config**.

Four other std cops (`hash_except`, `hash_slice`, `array_intersect`,
`redundant_filter_chain`) also reference this flag, but in the opposite
direction (they would *add* AS-specific shapes when enabled). They under-flag,
not over-flag, so they are out of scope here (see §6).

## 2. Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Key ownership | **`murphy-std`** owns `AllCops.ActiveSupportExtensionsEnabled` (already in its bundled `default.yml`, `false`) | It is a core/global concept, not rails-specific. The rails pack overrides the *value*, not the key. Avoids a plugin "owning" a global key. |
| Default-true driver | **rails pack contributes an override layer** | Mirrors RuboCop (`rubocop-rails` default.yml sets it true). Loading the rails pack → `true` automatically; plain-Ruby projects stay `false`. No manual config, no `migrate` dependency. |
| Pack-config delivery | **embedded `default.yml` exposed as a data symbol; core collects + merges** | The rails pack is dlopen-only (`murphy-cli` dev-dep; `PACK_COPS` is one global linkme slice, so static-linking it would double-register cops). So the pack embeds its `default.yml` via `include_str!` and exposes it as a `#[no_mangle] static RawSlice` (pure data, no behavior callback, no `register_cops!` macro change, no `PluginRegistration`/ABI-version change). The loader reads the symbol when present; core collects every loaded pack's yaml and merges (std < pack < user). This is RuboCop's "each extension ships `config/default.yml`, host merges" model. ELF/Mach-O have no Windows-`.rsrc`-style resource API; a data symbol is the portable equivalent. (Custom-section + `object`-crate parsing was considered — only worth it to read config *without* dlopen, which we don't need.) |
| Wiring | **mirror `AllCops.TargetRailsVersion`** (CxRaw field + `cx` method + config parse + dispatch populate) | Established precedent; low risk. |
| ABI | **keep `MURPHY_PLUGIN_ABI_VERSION` at 4 (lockstep tail-append)** | Project policy: tail-appended `CxRaw` fields do *not* bump the numeric ABI (host/plugins are rebuilt together from one tree). `target_rails_version`, `file_path`, `var_model` all followed this. The new `bool` lands at offset 242 inside existing padding, so `size_of::<CxRaw>()` stays 248 — no layout change at all. |
| Cop scope | **`Style/SymbolProc` only** | It is the demonstrated over-flagger. The other four under-flag (no false positives) and each needs distinct AS-shape work — tracked as follow-ups. |
| AllCops-layer generality | **one key (ASE) only** | First instance of a loaded pack overriding a global AllCops scalar. Generalising to arbitrary AllCops keys is YAGNI for v1. |

## 3. Core wiring (mirrors TargetRailsVersion)

1. **ABI / `CxRaw`** (`crates/murphy-plugin-api/src/abi.rs`): tail-append
   `active_support_extensions_enabled: bool` after `target_rails_version`
   (u16 @ 240). The bool lands at offset 242, inside existing tail padding, so
   `size_of::<CxRaw>()` stays 248. **Do NOT bump `MURPHY_PLUGIN_ABI_VERSION`**
   — follow the established lockstep policy (see the `target_rails_version` /
   `file_path` precedent). Add `assert_eq!(offset_of!(CxRaw,
   active_support_extensions_enabled), 242)`, keep `size_of == 248`, keep the
   `abi_version_is_four` test, and add a one-line policy comment next to the
   field like the existing ones.
2. **`Cx` method** (`crates/murphy-plugin-api/src/cx.rs`):
   ```rust
   /// Configured `AllCops.ActiveSupportExtensionsEnabled` (default false).
   pub fn active_support_extensions_enabled(&self) -> bool {
       self.raw.active_support_extensions_enabled
   }
   ```
3. **Config parse** (`crates/murphy-core/src/config.rs`): `DefaultCopsData`
   gains `allcops_active_support_extensions_enabled: Option<bool>`, parsed in
   the `AllCops` arm next to `Include`/`Exclude` (`config.rs:183`). `MurphyConfig`
   resolves the effective bool (default `false`).
4. **dispatch populate** (`crates/murphy-core/src/dispatch.rs` +
   `crates/murphy-cli/src/main.rs`): thread the resolved bool into `CxRaw`
   construction next to `target_rails_version` (the dispatch entry point is
   already `run_cops_with_options_and_target_rails_version`; extend its
   signature or add the field to the config struct it already receives).
5. **`Style/SymbolProc`** (`crates/murphy-std/src/cops/style/symbol_proc.rs`):
   early in `check_any_block`, after `block_method` is known:
   ```rust
   if cx.active_support_extensions_enabled()
       && is_lambda_or_proc_dispatch(node, block_method, cx)
   {
       return;
   }
   ```
   `is_lambda_or_proc_dispatch` returns true for a `->` lambda literal
   (`cx.is_lambda_literal(node)`), or `block_method` ∈ `{"lambda", "proc"}`
   (covers `lambda { }` / `proc { }`), or `Proc.new { }` (receiver `Proc`,
   selector `new`). Mirrors RuboCop's
   `proc_node?(dispatch) || LAMBDA_OR_PROC.include?(method_name)`.
6. **test-support** (`crates/murphy-plugin-api/src/test_support.rs`): add
   `with_active_support_extensions_enabled(bool)` to the tester builder,
   threaded to the `Cx` construction exactly like `with_target_rails_version`.

## 4. Pack-config delivery: embedded `default.yml` data symbol (core collects + merges)

The rails pack cannot be referenced statically from `murphy-cli` (it is a
`[dev-dependencies]` dlopen pack — "never `use`d from Rust" — and `PACK_COPS`
is one global `linkme` distributed-slice, so linking the rlib would register
rails cops twice). So the pack delivers its `default.yml` as **embedded data
read at load time**, and core does the collecting + merging.

**Rails pack** (`crates/murphy-rails/src/lib.rs`, no macro change):
```rust
/// default.yml embedded in the .so (resource).
pub const BUNDLED_DEFAULTS_YAML: &str = include_str!("../config/default.yml");

/// Pure data symbol the host reads after dlopen. Not a behavior callback.
/// `RawSlice::from_str` is already `const`; `RawSlice` is `Sync`.
#[no_mangle]
pub static MURPHY_PLUGIN_DEFAULT_CONFIG: RawSlice = RawSlice::from_str(BUNDLED_DEFAULTS_YAML);
```
`crates/murphy-rails/config/default.yml`:
```yaml
AllCops:
  ActiveSupportExtensionsEnabled: true
```

**Loader** (`crates/murphy-core/src/plugin_loader.rs`): after resolving the
required `murphy_plugin_register`, optionally resolve the data symbol
`MURPHY_PLUGIN_DEFAULT_CONFIG` (`library.get::<*const RawSlice>(b"…")`). If
present, copy the bytes into an owned `String` and store on `LoadedPluginPack`
(`Option<String> default_config_yaml`). Absent → `None` (rspec/example-pack
define no symbol → no contribution). Copy to owned so it outlives borrows (the
backing bytes live in the `.so`, kept alive by the held `Library`, but owning a
`String` is simplest/safest).

**Core collects + merges** (`crates/murphy-core/src/config.rs` +
`registry.rs`): the registry exposes loaded packs' `default_config_yaml`
contributions. Config resolution merges `DefaultCopsData` layers in order:
```
std (false)  <  each loaded pack's default.yml (rails: true)  <  user .murphy.yml
```
For the ASE scalar a later layer with `Some(value)` overrides; `None` leaves the
prior value. The user value (parsed in §3) wins last. Because pack loading
happens after the initial config load, the merge is applied **after** the
registry is built, re-resolving `config.active_support_extensions_enabled`
before dispatch.

**Pollution note.** The pack supplies a *value* for the std-owned key through a
host-defined channel (the data symbol); it does not define a new global key or
imperatively mutate host state. Config is one global namespace by design
(RuboCop parity — `inherit_from`, extension default.yml, and user config all
merge into it); the safety property is deterministic layering with the user as
final authority, which holds here.

## 5. Testing

- **config** (`config.rs`): parse `AllCops.ActiveSupportExtensionsEnabled`
  true/false/absent; layer-merge std(false)+pack(true)→true; user override
  wins both directions.
- **loader** (`plugin_loader.rs`): a pack exporting
  `MURPHY_PLUGIN_DEFAULT_CONFIG` surfaces its yaml on `LoadedPluginPack`; a pack
  without the symbol yields `None` (no error).
- **ABI** (`abi.rs`): `MURPHY_PLUGIN_ABI_VERSION` stays 4; new
  `offset_of!(CxRaw, active_support_extensions_enabled) == 242`; `size_of ==
  248` unchanged.
- **Cx** (`cx.rs`): `active_support_extensions_enabled()` returns the CxRaw
  value; `with_active_support_extensions_enabled` test-support hook works.
- **SymbolProc**: existing `flags_lambda_arrow` / `corrects_lambda_arrow` /
  `flags_proc_block` / `flags_proc_new_block` stay green (no flag set → default
  false → flagged). New (flag enabled): `->`, `lambda { }`, `proc { }`,
  `Proc.new { }` produce **no offense**; a regular block / numblock / itblock is
  still flagged when enabled (exemption is lambda/proc-only).
- **dynamic pack e2e** (`crates/murphy-cli/tests/rails_pack_e2e.rs`): packs are
  recompiled from the same tree (lockstep, ABI stays 4); existing e2e stays
  green. If cheap, add one CLI integration asserting the rails pack enables ASE
  so SymbolProc exempts a `lambda` block.

## 6. Out of scope (follow-ups)

- The four under-flagging cops (`hash_except`, `hash_slice`, `array_intersect`,
  `redundant_filter_chain`) gaining their AS-gated shapes (`in?`, `exclude?`,
  `present?`, `blank?`, `many?`). The flag is now readable via
  `cx.active_support_extensions_enabled()`; each is a separate per-cop gap-fill.
- Generalising the pack override layer to arbitrary AllCops keys beyond ASE.
- `murphy migrate` mapping `AllCops.ActiveSupportExtensionsEnabled` pass-through
  (only needed if a user sets it explicitly; the rails-pack layer covers the
  rubocop-rails-implied default).
