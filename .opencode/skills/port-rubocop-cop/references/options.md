# `#[derive(CopOptions)]` — option struct reference

Authoritative source: infra guide §7 and ADR 0036
(`docs/plans/2026-05-22-murphy-9cr7-derive-copoptions-design.md`).

## When to use options

Mirror the RuboCop original:

- RuboCop cop has no `cop_config` keys → use `options = NoOptions` in
  `#[cop(...)]` and skip the derive.
- RuboCop cop has `cop_config: { 'Max' => 5, 'IgnoredPatterns' => [...] }`
  → declare a struct with one field per key and derive `CopOptions`.

## Declaring an options struct

```rust
use murphy_plugin_api::CopOptions;

#[derive(CopOptions)]
pub struct ExampleLengthOptions {
    #[option(
        default = 5,
        description = "Maximum number of lines in an example body."
    )]
    pub max: i64,
}
```

Then in the `#[cop]` block:

```rust
#[cop(
    name = "RSpec/ExampleLength",
    options = ExampleLengthOptions,
    // …
)]
impl ExampleLength { /* … */ }
```

## `#[option(...)]` field-level arguments

| Argument | Required | Notes |
|---|---|---|
| `default = …` | **yes** | Literal default. Drives `Default::default()` and is the value that fires today (see "v1 access" below). |
| `description = "…"` | optional | One-line description surfaced in `murphy cops list` and used for schema docs. |

The macro lowers each `#[option(...)]` field into the schema metadata
emitted by `#[derive(CopOptions)]` so the host can validate
`[cops.rules."Pack/CopName"]` tables at config-load time.

## Supported field types

The v1 derive supports the JSON-shaped field types RuboCop options
normally take. Maps to TOML / JSON cleanly:

| Rust type | TOML / RuboCop shape |
|---|---|
| `i64` | Integer (`Max = 5`). |
| `bool` | Boolean (`Enabled = false`). |
| `String` | String (`Style = "single_quotes"`). |
| `Vec<String>` | Array of strings (`IgnoredPatterns = ["spec/**"]`). |

Other shapes (nested structs, enums) need an ADR before adding them.
For a port, if the RuboCop original uses an enum-style key, render it as
a `String` with a documented set of accepted values and check it inside
the cop.

## Reading options at runtime — v1 limitation

Runtime option access via `Cx` is murphy-9cr.9 and is **not yet wired**.
Until it lands, every cop reads `OptionStruct::default()` inside the
dispatch method:

```rust
fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
    // …gate…
    let opts = ExampleLengthOptions::default();
    if line_count <= opts.max as usize {
        return;
    }
    // …
}
```

That means **`murphy.toml` overrides for option values do not affect
runtime behaviour today** — the value compiled into `default = …` is
what fires. The schema *is* still validated against user config so the
override path is forward-compatible. Document this in the cop's
top-doc comment (see `ExampleLengthOptions` / `MultipleExpectationsOptions`
for the canonical wording).

When murphy-9cr.9 lands, the same code will switch to reading through
`Cx` without changing the public surface — the schema metadata and the
struct layout are already what the future loader expects.

## Default values & RuboCop parity

Match the RuboCop default exactly when porting unless there is a stated
reason to diverge. `Max` defaults are the most common case:

- `RSpec/ExampleLength` → `Max = 5`.
- `RSpec/MultipleExpectations` → `Max = 1`.
- `Metrics/MethodLength` → `Max = 10`.

If the RuboCop default has changed across versions, prefer the most
recent stable RuboCop release and note the version in the file's
doc-comment.
