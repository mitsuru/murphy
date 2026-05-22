# murphy-9cr.7 — `#[derive(CopOptions)]` Design

**Status**: design accepted 2026-05-22, ready for implementation
**Issue**: murphy-9cr.7 (parent epic: murphy-9cr)
**Related**: ADR 0033 (plugin ABI v1 option metadata), ADR 0035 (const-based Cop trait), ADR 0036 (to be authored as part of this work)

## Background

murphy-9cr.6 settled `CopOptions` as a trait carrying a `const SCHEMA:
&'static [MurphyCopOptionV1]`. Authors implementing it by hand must
hand-maintain that slice and a JSON decoder, which is exactly the
boilerplate `#[derive(CopOptions)]` exists to remove.

The issue description originally specified a hand-written JSON parser
(no serde). During brainstorming we reversed that: `serde_json`
becomes a production dependency of `murphy-plugin-api`. Native Rust
plugins already pull serde-shaped crates routinely; the binary-size
cost is minor; and mruby user cops never touch `murphy-plugin-api`, so
their lightweight-runtime story is unaffected. ADR 0036 records the
reversal.

## Scope

1. `murphy-plugin-api`:
   - Add `serde_json` (+ transitive `serde`) as a production
     dependency.
   - New `ConfigError` type (parse error / non-object / type
     mismatch / enum violation / missing required field).
   - Add `fn from_config_json(bytes: &[u8]) -> Result<Self,
     ConfigError>` to the `CopOptions` trait, with a default impl
     (`Ok(Self::default())`) so `NoOptions` and option-less cops need
     nothing.
2. `murphy-plugin-macros`:
   - `#[derive(CopOptions)]` proc macro generating `impl Default` and
     `impl CopOptions` (the `SCHEMA` const plus `from_config_json`).
   - `#[option(...)]` field attribute parsing.
3. trybuild ui tests (3 pass, 2 fail) + a behavioural integration
   test.
4. ADR 0036.

## Non-scope

- `f64`, nested-struct, and map-typed option fields. Supported field
  types are `bool` / `i64` / `String` / `Vec<String>` and the
  `Option<...>` of the first three.
- Migrating `murphy-rails` / `murphy-example-pack` to
  `#[derive(CopOptions)]` — bundled with murphy-9cr.8.
- The validation gate that diffs user config against `SCHEMA` —
  murphy-9cr.9 (it reuses `ConfigError` and `from_config_json`).
- Deserialising into `serde::Deserialize`-derived structs. The macro
  decodes through `serde_json::Value` field-by-field so `#[option]`
  metadata and decoding stay in one place.

## Supported Field Types

| Rust type | JSON shape | absent in config |
|---|---|---|
| `bool` | boolean | needs `#[option(default = …)]`, else `MissingRequired` |
| `i64` | integer | same |
| `String` | string | same |
| `Vec<String>` | array of string | same |
| `Option<bool/i64/String>` | matching scalar, or `null` | `None` |

Anything else is a compile error from the derive macro
(`fail_derive_unsupported_type`).

## Attribute Grammar

All field-level configuration goes through a single `#[option(...)]`
attribute. We do **not** use a bare `#[default]` (it collides with the
`#[derive(Default)]` enum-variant attribute and reads ambiguously).

```rust
#[derive(CopOptions)]
struct LineLengthOptions {
    #[option(default = 80, description = "Maximum line width")]
    max: i64,

    #[option(default = "indented", enum_values = ["indented", "aligned"])]
    style: String,

    #[option(default = ["id"], description = "Names always allowed")]
    allowed: Vec<String>,

    #[option(deprecated = "use max")]
    max_chars: Option<i64>,
}
```

Recognised `#[option(...)]` keys:

- `default = <literal>` — bool / integer / string / string-list
  (`[...]`). Drives both the generated `Default` impl and the
  `default_json` schema field.
- `description = "..."` — schema `description`.
- `enum_values = ["a", "b"]` — allowed values for a `String` field;
  also emitted as `enum_values_json`. A `default` outside the set is a
  compile error.
- `deprecated` (bare) — deprecated, no replacement hint.
- `deprecated = "use other"` — deprecated with a replacement key
  (schema `replacement`).
- `reason = "..."` — deprecation reason (schema `reason`, the second
  axis from ADR 0033).

`enum_values` on a non-`String` field is a compile error.

## Generated Code

For each `#[derive(CopOptions)]` struct the macro emits two impls.

`impl Default` reflects every `#[option(default = …)]`; fields without
a default fall back to `Option::None` (for `Option<_>` fields) or are
**required** — their absence from config is a `MissingRequired` error
at decode time, and `Default::default()` uses the type's own
`Default` so the struct still constructs.

`impl CopOptions` provides:

- `const SCHEMA: &'static [MurphyCopOptionV1]` — one entry per field.
  `name` / `ty` / `description` / `replacement` / `reason` wrap
  `&'static str` literals through
  `murphy_plugin_api::__internal::str_to_slice`. `default_json` and
  `enum_values_json` are JSON text the macro assembles at compile time
  (`80` → `"80"`, `"indented"` → `"\"indented\""`, `["id"]` →
  `"[\"id\"]"`).
- `fn from_config_json` — `serde_json::from_slice` into a
  `serde_json::Value`, require a top-level object, then per field:
  - present → convert via `as_bool` / `as_i64` / `as_str` /
    `as_array`; type mismatch → `ConfigError::TypeMismatch`.
  - `String` with `enum_values` → reject out-of-set values with
    `ConfigError::EnumViolation`.
  - absent and `Option<_>` → `None`.
  - absent with `#[option(default)]` → the default literal.
  - absent, non-`Option`, no default → `ConfigError::MissingRequired`.

`ty` schema strings: `bool` → `"bool"`, `i64` → `"int"`, `String` →
`"string"` (or `"enum"` when `enum_values` is present), `Vec<String>`
→ `"string_list"`. `Option<T>` carries the inner type's `ty`.

## ConfigError

```rust
pub struct ConfigError { kind: ConfigErrorKind }

enum ConfigErrorKind {
    Parse(String),                                   // serde_json syntax error
    NotAnObject,                                     // top-level value not an object
    TypeMismatch { field: String, expected: &'static str },
    EnumViolation { field: String, value: String },
    MissingRequired { field: String },
}
```

Public, `Debug + Display + std::error::Error`. murphy-9cr.9's
validation gate consumes the same type so config diagnostics share one
vocabulary.

## Test Strategy

trybuild ui tests (added to `crates/murphy-plugin-macros/tests/ui/`):

- `pass_derive_basic.rs` — one field of each supported scalar / list
  type.
- `pass_derive_attrs.rs` — `default` / `description` / `enum_values`
  / `deprecated` / `reason` together.
- `pass_derive_optional.rs` — `Option<_>` fields.
- `fail_derive_unsupported_type.rs` — `f64` field; macro emits a
  compile error.
- `fail_derive_default_outside_enum.rs` — `default` not in
  `enum_values`; compile error.

Behavioural integration test
(`crates/murphy-plugin-macros/tests/derive_behavior.rs`): exercises a
derived struct at runtime — `Default` reflects `#[option(default)]`,
`from_config_json` handles valid input, type mismatch, enum violation,
missing required, and `Option`-absent / `null` paths; `SCHEMA`
contents are asserted.

plugin-api unit tests cover the `ConfigError` constructors and
`NoOptions::from_config_json` returning `Ok(NoOptions)`.

## Implementation Order

1. plugin-api: add `serde_json`; define `ConfigError`; extend
   `CopOptions` with `from_config_json` (defaulted).
2. plugin-api: update existing tests for the new trait shape.
3. murphy-plugin-macros: implement `#[derive(CopOptions)]` —
   `#[option(...)]` parsing, field-type recognition, `Default` +
   `CopOptions` generation.
4. Add the five trybuild ui fixtures.
5. Add `derive_behavior.rs`.
6. `cargo fmt --check`, `cargo clippy --workspace --all-targets --
   -D warnings`, `cargo test --workspace`.
7. Author ADR 0036.

## Risks

- **`serde_json::Value` allocation per decode.** `from_config_json`
  builds a full `Value` tree before field extraction. Config blobs
  are tiny (one `[cops.rules."X"]` table), so this is negligible; if a
  hot path ever appears, a streaming decoder can replace it behind the
  same trait method.
- **Compile-time JSON assembly for `default_json`.** The macro must
  emit syntactically valid JSON. Limiting `default` literals to
  bool / integer / string / string-list keeps the encoder small and
  total; exotic literals are rejected at parse time.
- **`enum_values` validation is `String`-only.** Integer or boolean
  enums are not expressible. No real cop needs them today; revisit if
  one does.

## Open Follow-ups

- murphy-9cr.8 — `#[murphy::cop]` ties an `Options` type to a cop and
  removes the remaining `RUN_*` boilerplate; migrates the in-tree
  packs.
- murphy-9cr.9 — validation gate diffs user config against `SCHEMA`,
  reusing `ConfigError`.
