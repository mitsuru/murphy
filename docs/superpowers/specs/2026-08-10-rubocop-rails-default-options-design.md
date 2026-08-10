# RuboCop Rails Default Options Design

## Problem

`crates/murphy-rails/config/default.yml` mirrors several non-Rails cop
overrides from `rubocop-rails`, but some `murphy-std` cops still ignore those
options. In particular, `Lint/SafeNavigationChain`,
`Lint/RedundantSafeNavigation`, and `Style/InvertibleUnlessCondition` use
hardcoded allowlists/maps while the Rails pack supplies additional entries.
`Lint/NumberConversion` has typed options already, but does not expose the
`AllowedPatterns` option present in the bundled configuration. Four related
cops already consume typed options and need regression coverage rather than a
second implementation.

## Goal

Make the Rails pack's cop-level defaults effective without removing each cop's
RuboCop-compatible built-in behavior. A configured list/map is additive to the
hardcoded core defaults for the cops whose upstream semantics include both
sets; an explicitly configured map entry overrides the built-in value for the
same key.

## Scope

In scope:

- `Lint/SafeNavigationChain`: decode `AllowedMethods` and union it with the
  built-in nil-safe method list.
- `Lint/RedundantSafeNavigation`: decode `AllowedMethods` and union it with
  the built-in nil-safe predicate list.
- `Lint/NumberConversion`: add `AllowedPatterns` decoding and apply regex
  matching to the receiver method name, while preserving the existing
  `AllowedMethods` and `AllowedClasses` behavior.
- `Style/InvertibleUnlessCondition`: decode the `InverseMethods` object,
  normalize YAML symbol-style keys such as `:present?`, and merge configured
  entries with the built-in inverse map.
- Regression tests for the four already option-aware cops:
  `Style/CollectionCompact`, `Style/SymbolProc`, `Style/AndOr`, and the
  existing `Lint/NumberConversion` behavior.
- Parity comments/tests needed to describe the newly supported options.

Out of scope:

- Changes to `crates/murphy-rails/config/default.yml`.
- Broader RuboCop parity gaps such as data-flow inference or unsupported AST
  shapes.
- Changes to the plugin ABI or `MURPHY_PLUGIN_ABI_VERSION`.
- A generic map type in the `CopOptions` derive macro; only this cop needs a
  map-shaped option and can use the existing manual `CopOptions` pattern.

## Design

### Safe-navigation cops

Each cop gets a typed options struct with a PascalCase `AllowedMethods` field.
The runtime predicate receives both the method name and decoded options:

```text
allowed(method) = BUILT_IN_METHODS.contains(method)
             || options.allowed_methods.contains(method)
```

This preserves methods such as `nil?`, `to_i`, and `respond_to?` when a Rails
pack layer supplies a narrower `AllowedMethods` list, while allowing Rails'
`presence_in`/`presence` entries to take effect.

### Number conversion

Keep the current `AllowedMethods` and `AllowedClasses` fields for compatibility
with existing unit tests and callers. Add `AllowedPatterns: Vec<String>` and
call `cx.matches_any_pattern` alongside the exact `AllowedMethods` check for a
receiver method. The pattern list is empty by default, so existing behavior is
unchanged when no pattern is configured.

### Invertible unless

Use a manually implemented `CopOptions` type because the current derive macro
supports scalar and string-list fields but not maps. The type stores
`BTreeMap<String, String>` and decodes the `InverseMethods` JSON object. Keys
and values with a leading `:` are normalized to their method names. Runtime
lookup builds the effective map as:

```text
effective = built_in_inverse_methods
effective.extend(configured_inverse_methods)
```

Thus the Rails entries for `present?`/`blank?` and `include?`/`exclude?` are
available, and a user-configured value wins for a duplicate key. The existing
autocorrect and message-generation paths use this effective map.

### Existing option-aware cops

`Style/CollectionCompact`, `Style/SymbolProc`, and `Style/AndOr` already use
`cx.options_or_default` with correctly named fields. Their current production
implementation remains unchanged; tests will exercise the Rails default
values (`params`, `mail`, `respond_to`, and `conditionals`) so future changes
cannot silently disconnect the pack configuration.

## Testing

Tests follow red-green-refactor:

1. Add failing tests for configured `AllowedMethods`, `AllowedPatterns`, and
   `InverseMethods`, including preservation of built-in defaults when a custom
   list/map is present.
2. Run the focused `murphy-std` tests and confirm the failures are caused by
   ignored options.
3. Implement the smallest option decoders/runtime unions needed to make those
   tests pass.
4. Run the focused cop tests, then `cargo test -p murphy-std --lib`, workspace
   formatting, and clippy checks appropriate to the changed crates.

The existing option JSON is the test harness boundary; no new config loader or
ABI surface is introduced.

## Risks and decisions

- A user-provided safe-navigation allowlist remains additive to the intrinsic
  nil-safe methods, matching the cop's `nil_methods` model and preventing a
  pack layer from reintroducing false positives.
- `InverseMethods` map decoding is deliberately local rather than widening the
  derive macro/ABI, keeping this task small and avoiding an ABI version change.
- Unknown option keys continue to be ignored by the existing derive decoder;
  the new `AllowedPatterns` field makes the relevant NumberConversion key
  observable and testable.
