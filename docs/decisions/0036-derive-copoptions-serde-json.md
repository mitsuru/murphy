# ADR 0036: `#[derive(CopOptions)]` uses serde_json and a unified `#[option(...)]` attribute

**Status**: Accepted (2026-05-22)
**Issue**: murphy-9cr.7 (parent epic: murphy-9cr)
**Related**: ADR 0032 (plugin ABI option schema fixed to JSON), ADR 0033 (plugin ABI v1 option metadata), ADR 0035 (const-based Cop trait)

## Context

murphy-9cr.7 implements `#[derive(CopOptions)]`, the macro that turns
a plain options struct into the `SCHEMA` const and JSON decoder that
`CopOptions` requires. Two design points needed deciding, and one
reverses the original issue text.

1. **JSON decoding.** The murphy-9cr.7 issue description specified a
   hand-written JSON parser, explicitly avoiding serde. During
   brainstorming we reconsidered: a correct hand-written JSON parser
   (string escapes, `\uXXXX`, number edge cases) is ~200 lines of
   error-prone code, duplicating a solved problem.

2. **Field-attribute grammar.** The issue sketched two attributes:
   `#[default = expr]` and `#[option(...)]`. `#[default]` collides
   with the `#[derive(Default)]` enum-variant attribute and reads
   ambiguously on a struct field.

## Decision

### serde_json as a production dependency of `murphy-plugin-api`

`murphy-plugin-api` takes `serde_json` (and transitively `serde`) as a
production dependency. `CopOptions` gains:

```rust
fn from_config_json(bytes: &[u8]) -> Result<Self, ConfigError>;
```

with a default implementation (`Ok(Self::default())`) so `NoOptions`
and option-less cops need nothing. `#[derive(CopOptions)]` overrides
it with field-by-field decoding through `serde_json::Value`.

The macro decodes through `serde_json::Value` — it does **not** emit
`#[derive(serde::Deserialize)]`. Keeping the decode explicit means
`#[option(...)]` metadata, the `SCHEMA` const, and the decoder all
live in one generated `impl` and cannot drift.

`murphy-plugin-macros` also takes `serde_json` as a build-time
dependency, used only to encode `#[option(default = …)]` literals and
`enum_values` into the JSON strings embedded in `SCHEMA`. Proc-macro
crates run at compile time, so this never reaches a plugin binary.

A new public `ConfigError` type (`Parse` / `NotAnObject` /
`TypeMismatch` / `EnumViolation` / `MissingRequired`) is the decode
error. The validation gate (murphy-9cr.9) reuses it.

### Unified `#[option(...)]` attribute

All field configuration goes through one `#[option(...)]` attribute.
There is no bare `#[default]`. Recognised keys:

- `default = <literal>` — bool / integer / string / string-list.
- `description = "..."`.
- `enum_values = ["a", "b"]` — `String` fields only.
- `deprecated` (bare) or `deprecated = "replacement_key"`.
- `reason = "..."`.

Supported field types: `bool`, `i64`, `String`, `Vec<String>`, and
`Option<bool|i64|String>`. The schema `ty` wire strings are `"bool"` /
`"int"` / `"string"` / `"string_list"`, or `"enum"` when
`enum_values` is set. Anything else is a compile error from the
derive macro.

## Reasons

1. **serde_json is the right scope of dependency.** ADR 0032 already
   fixed the ABI option representation to JSON; decoding it with the
   ecosystem-standard JSON crate is consistent, not a new axis. Native
   Rust plugins routinely depend on serde-shaped crates; the binary
   cost is minor. mruby user cops never link `murphy-plugin-api` — they
   go through the embedded-mruby path — so their lightweight-runtime
   story is untouched. The "no serde" line in the issue predated this
   reconciliation; this ADR supersedes it.
2. **A hand-written parser is pure liability here.** It would need to
   be as correct as serde_json to be safe, with none of serde_json's
   fuzzing and battle-testing. Writing it would trade ~200 lines of
   risk for zero capability gain.
3. **Decode through `Value`, not `Deserialize`.** A `serde::Deserialize`
   derive would split option metadata across two derives (`#[option]`
   for schema, `#[serde]` for decoding) and invite them to disagree.
   One generated `impl` that reads `Value` field-by-field keeps schema
   and decoder honest.
4. **One attribute, no `#[default]`.** Folding everything into
   `#[option(...)]` sidesteps the `#[derive(Default)]` name clash and
   gives every option-related setting one obvious home.

## Alternatives Considered

- **Hand-written JSON parser (issue's original text).** Rejected for the liability reason above. Kept the door open only as far as: if a future constraint bans serde from `murphy-plugin-api`, the `from_config_json` method boundary lets a parser swap in without touching the macro.
- **`#[derive(serde::Deserialize)]` for decoding.** Rejected: dual-derive drift between `#[option]` and `#[serde]`, and serde's attribute surface would leak into the cop-author-facing API.
- **Separate `#[default = expr]` attribute (issue's original text).** Rejected: `#[default]` collides with `#[derive(Default)]` and the split offers no clarity over a unified `#[option(default = …)]`.
- **Streaming JSON decode.** Rejected as premature: config blobs are one tiny `[cops.rules."X"]` table; a full `Value` tree per decode is negligible. The trait-method boundary leaves room to switch later.
- **`f64` / nested-struct / map fields.** Out of scope; no real cop needs them. Adding them later is backwards-compatible (new field types, same trait).

## Consequences

- `murphy-plugin-api`'s dependency tree gains `serde` + `serde_json` + their small transitive set (`itoa`, `ryu`, `memchr`). Acceptable for a plugin-facing crate.
- `CopOptions` from murphy-9cr.6 grows one defaulted method; no breakage for `NoOptions` or hand-written impls (the default covers them).
- `#[derive(CopOptions)]` generates two impls — `Default` (honouring `#[option(default)]`) and `CopOptions`. A field with no `default` and no `Option<_>` wrapper is **required**: absent from config it produces `ConfigError::MissingRequired`, while `Default::default()` still constructs it via the type's own `Default`.
- The validation gate (murphy-9cr.9) inherits `ConfigError` and `from_config_json` as its decode layer.
- The issue's "no serde" instruction is formally superseded here.

## Implementation status

Implemented in murphy-9cr.7. See:

- `crates/murphy-plugin-api/src/config_error.rs` — `ConfigError`.
- `crates/murphy-plugin-api/src/lib.rs` — `serde_json` dependency,
  `CopOptions::from_config_json` defaulted method.
- `crates/murphy-plugin-macros/src/cop_options.rs` —
  `#[derive(CopOptions)]`.
- `crates/murphy-plugin-macros/tests/ui/` — 3 pass + 2 fail trybuild
  fixtures.
- `crates/murphy-plugin-macros/tests/derive_behavior.rs` — 10
  runtime behaviour tests.

Design notes:
`docs/plans/2026-05-22-murphy-9cr7-derive-copoptions-design.md`.

## Follow-up issues

- murphy-9cr.8 — `#[murphy::cop]` ties an `Options` type to a cop.
- murphy-9cr.9 — validation gate diffs user config against `SCHEMA`, reusing `ConfigError`.
