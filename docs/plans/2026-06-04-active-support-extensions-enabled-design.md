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
| Wiring | **mirror `AllCops.TargetRailsVersion`** (CxRaw field + `cx` method + config parse + dispatch populate) | Established precedent; low risk. |
| ABI | **bump `MURPHY_PLUGIN_ABI_VERSION` 4 → 5** | Adding a tail field to `CxRaw` changes its layout/size; old cdylibs must be rejected (same reason as the v1→v2 token-kind bump). |
| Cop scope | **`Style/SymbolProc` only** | It is the demonstrated over-flagger. The other four under-flag (no false positives) and each needs distinct AS-shape work — tracked as follow-ups. |
| AllCops-layer generality | **one key (ASE) only** | First instance of a loaded pack overriding a global AllCops scalar. Generalising to arbitrary AllCops keys is YAGNI for v1. |

## 3. Core wiring (mirrors TargetRailsVersion)

1. **ABI / `CxRaw`** (`crates/murphy-plugin-api/src/abi.rs`): add a tail field
   `active_support_extensions_enabled: bool` after `target_rails_version`.
   Bump `MURPHY_PLUGIN_ABI_VERSION` 4 → 5. Update the `offset_of!` / size
   assertions and the `abi_version_is_four` test (→ five).
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

## 4. Rails override layer (std owns the key, rails overrides the value)

Today `MurphyConfig::load_with_defaults(root, std_yaml)` takes a single
bundled-defaults layer (`murphy-std`'s `BUNDLED_DEFAULTS_YAML`). Generalise to
merge **enabled packs' layers** in order:

```
std (false)  <  rails pack layer (true, only when the rails pack is enabled)  <  user .murphy.yml
```

- The rails pack exposes a minimal `BUNDLED_DEFAULTS_YAML` `pub const`
  (`crates/murphy-rails`, via `include_str!`) containing only:
  ```yaml
  AllCops:
    ActiveSupportExtensionsEnabled: true
  ```
- `murphy-cli` resolves which packs are enabled (Mastodon's `.murphy.yml`
  requests `murphy-rails`), then merges the enabled packs' `BUNDLED_DEFAULTS_YAML`
  over the std layer before applying the user config. `murphy-cli` already
  depends on `murphy-rails` (Cargo.toml), so the const is reachable at compile
  time; the merge is gated on runtime enablement.
- `DefaultCopsData` layer-merge: for the ASE scalar, a later layer with
  `Some(value)` overrides an earlier layer; `None` leaves the prior value.

**Pollution note.** The rails pack supplies a *value* for the std-owned key; it
does not define a new global key. This is RuboCop's exact model (core defines
`ActiveSupportExtensionsEnabled`, `rubocop-rails` overrides to true) and the
user `.murphy.yml` always wins last, so resolution stays predictable.

## 5. Testing

- **config** (`config.rs`): parse `AllCops.ActiveSupportExtensionsEnabled`
  true/false/absent; layer-merge std(false)+rails(true)→true; user override
  wins both directions.
- **ABI** (`abi.rs`): `MURPHY_PLUGIN_ABI_VERSION == 5`; updated `CxRaw`
  offset/size assertions.
- **Cx** (`cx.rs`): `active_support_extensions_enabled()` returns the CxRaw
  value; `with_active_support_extensions_enabled` test-support hook works.
- **SymbolProc**: existing `flags_lambda_arrow` / `corrects_lambda_arrow` /
  `flags_proc_block` / `flags_proc_new_block` stay green (no flag set → default
  false → flagged). New (flag enabled): `->`, `lambda { }`, `proc { }`,
  `Proc.new { }` produce **no offense**; a regular block / numblock / itblock is
  still flagged when enabled (exemption is lambda/proc-only).
- **dynamic pack e2e** (`crates/murphy-cli/tests/rails_pack_e2e.rs`): rebuild
  rails/rspec packs against ABI 5; existing e2e stays green. If cheap, add one
  CLI integration asserting the rails pack enables ASE so SymbolProc exempts a
  `lambda` block.

## 6. Out of scope (follow-ups)

- The four under-flagging cops (`hash_except`, `hash_slice`, `array_intersect`,
  `redundant_filter_chain`) gaining their AS-gated shapes (`in?`, `exclude?`,
  `present?`, `blank?`, `many?`). The flag is now readable via
  `cx.active_support_extensions_enabled()`; each is a separate per-cop gap-fill.
- Generalising the pack override layer to arbitrary AllCops keys beyond ASE.
- `murphy migrate` mapping `AllCops.ActiveSupportExtensionsEnabled` pass-through
  (only needed if a user sets it explicitly; the rails-pack layer covers the
  rubocop-rails-implied default).
