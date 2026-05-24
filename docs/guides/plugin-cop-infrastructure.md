# Plugin Cop Infrastructure

How a cop is authored, registered, loaded, configured, and tested against
Murphy's single-surface plugin ABI.

This guide is a reference for cop authors. Rationale and trade-offs live
in the ADRs cited at the bottom of each section.

## 1. What a cop is

A **cop** is a stateless inspector that reads an immutable Ruby AST and
emits zero or more **offenses** (and optional **edits** for autocorrect).
A **plugin pack** is a Rust crate that registers one or more cops with
the host through `murphy-plugin-api`.

Two pack flavours:

- **Static pack** — a `[lib]` crate linked directly into `murphy-cli`.
  Used for the built-in `murphy-std` pack.
- **Dynamic pack** — a `cdylib` loaded at runtime via `dlopen` from the
  user's `murphy.toml` `[[plugins]]` list. Used for `murphy-rspec`,
  `murphy-rails`, `murphy-example-pack`, and all third-party packs.

Both flavours speak the same `#[repr(C)]` ABI (ADR 0038, ADR 0031) and
both use the same author-facing surface.

## 2. Single-surface API: `murphy-plugin-api`

Every cop — built-in, bundled, or third-party — reaches the host
**only** through `murphy-plugin-api`. This is the "single-surface" rule
(ADR 0038, design §5 of
`docs/plans/2026-05-22-plugin-reboot-design.md`). A pack's runtime
`[dependencies]` allow-list is exactly `{murphy-plugin-api}` and the
invariant is asserted by a `tests/dep_boundary.rs` in every pack
(`crates/murphy-std/tests/dep_boundary.rs` is the template; rspec, rails,
and example-pack each carry their own copy).

The crate re-exports everything a pack needs at one path so the pack
imports stay short. A typical cop file pulls:

```rust
use murphy_plugin_api::{
    Cx,                                   // AST read context
    NodeId, NodeKind, OptNodeId, Range,   // AST types (re-exported from murphy-ast)
    Symbol,                               // interned identifiers
    NoOptions,                            // marker type for cops with no config
    Severity,                             // offense severity
    cop, on_node, on_new_investigation,   // dispatch attributes
};
```

A pack's `lib.rs` also pulls `register_cops` (§8). Cops with options
pull `CopOptions` for `#[derive(CopOptions)]` (§7). The `node_pattern!`
macro (§3) is available for deeper shape matching. Anything reachable
only through `murphy-core`, `murphy-ast`, `murphy-translate`,
`murphy-pattern` etc. is **off limits** at runtime — the dep-boundary
test will fail the build.

## 3. Writing a cop with `#[cop]`

A cop is a stateless unit struct annotated with `#[cop(...)]` on its
`impl` block. Inside the block, one or more **dispatch methods**
declare which node kinds the cop subscribes to. This is the only
authoring shape this guide documents — the underlying `Cop` and
`NodeCop` traits are implementation detail the macro fills in.

Example from `murphy-rspec`:

```rust
use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

#[derive(Default)]
pub struct DescribeClass;

#[cop(
    name = "RSpec/DescribeClass",
    description = "The first argument to `describe` should be the class or module under test, not a string.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl DescribeClass {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, method, args } = *cx.kind(node) else { return };
        if cx.symbol_str(method) != "describe" {
            return;
        }
        // … cx.emit_offense(...)
    }
}
```

### `#[cop(...)]` attribute arguments

| Argument | Required | Notes |
|---|---|---|
| `name` | yes | The cop identifier (`"Pack/CopName"`), must match the name in `murphy.toml` and offense JSON. |
| `description` | optional | One-line human-readable summary. |
| `default_severity` | optional | `"warning"` or `"error"`. Omit to leave the host default. |
| `default_enabled` | optional | `true` / `false`. Omit to leave the host default. |
| `options` | yes | Options struct type — `NoOptions` for cops with no config, or your own `#[derive(CopOptions)]` struct (see §8). |

### Dispatch attributes

Two attributes mark dispatch methods inside the `impl` block:

- **`#[on_node(kind = "…")]`** — per-kind dispatch. The method is
  called once for every AST node whose kind matches. The string is the
  RuboCop hook name (`"send"`, `"block"`, `"class"`, …); the full
  mapping lives in `docs/guides/rubocop-hook-dispatch.md`. Multiple
  `#[on_node]` methods are allowed — the macro collects every kind
  into the cop's subscription set and dispatches to the matching
  method by node tag.

- **`#[on_new_investigation]`** — file-visit dispatch. The method is
  called exactly once per file with `node == cx.root()`. Used by
  whole-file scanners (raw-source cops like `Layout/TrailingWhitespace`
  whose root kind can be any of `Begin`, `Nil`, `Send`, `Def`, …).
  Mirrors RuboCop's `on_new_investigation` lifecycle hook.

Each dispatch method takes `&self, node: NodeId, cx: &Cx<'_>` (see
§4 for what `Cx` exposes). The body emits offenses via `cx.emit_offense`
and edits via `cx.emit_edit` (§5).

### Deeper pattern matching: `node_pattern!`

For cops that need to test more than one kind at a time (e.g. "a
`Send` whose receiver is a `Const` named `RSpec`"), the
`node_pattern!` macro provides a Prism shape-matching DSL
(ADR 0033 / murphy-9cr.18). It works inside a dispatch method body and
returns a `bool` / option binding. The parser lives in
`crates/murphy-pattern`.

### Reference: what `#[cop]` generates

The macro expands to `impl Cop for T` (compile-time metadata) and
`impl NodeCop for T` (runtime dispatch). Both traits are in
`crates/murphy-plugin-api/src/{cop,node_cop}.rs`. The const-eval shape
(ADR 0035) lets `register_cops!` assemble the registration table at
build time; the `NodeKindTag` set comes from each `#[on_node]` kind
string. The traits are not part of the documented authoring API — hand
implementations exist only for the mruby-backed dynamic proxy
(`MrubyCopProxy`) that computes its subscription at runtime.

## 4. Reading the AST: `Cx`

`Cx<'a>` is a borrowed, direct-read view of the arena. **Traversal and
`NodeKind` matching are pure memory reads — zero FFI** (ADR 0038). The
lifetime `'a` forbids retaining any part past the `check` call.

Key methods on `Cx`:

| Method | Returns | Notes |
|---|---|---|
| `root()` | `NodeId` | The file's root node. |
| `node(id)` | `&'a AstNode` | The packed node header. |
| `kind(id)` | `&'a NodeKind` | The typed payload — match on this. |
| `range(id)` | `Range` | Byte range `[start, end)` into source. |
| `parent(id)` | `OptNodeId` | `OptNodeId::NONE` at the root. |
| `list(NodeList)` | `&'a [NodeId]` | Dereference a variable-length child list. |
| `children(id)` | `Vec<NodeId>` | Direct children, in source order. |
| `ancestors(id)` | `impl Iterator<Item = NodeId>` | Walks up to the root. |
| `descendants(id)` | `Vec<NodeId>` | Subtree pre-order, including `id`. |
| `symbol_str(Symbol)` | `&'a str` | Interned identifier text. |
| `string_str(StringId)` | `&'a str` | Interned string literal text. |
| `comments()` | `&'a [Comment]` | All file comments, source order. |
| `raw_source(Range)` | `&'a str` | Bytes in source covered by `Range`. |
| `source()` | `&'a str` | Whole file source. |
| `emit_offense(Range, &str, Option<Severity>)` | `()` | See §7. |
| `emit_edit(Range, &str)` | `()` | Autocorrect replacement. |

Source is UTF-8 (panics otherwise). `Range::{start, end}` are `u32`
byte offsets; converting to/from char/column positions is the cop's
responsibility (see `test_support`'s `char_indices()` translation for
the canonical pattern).

## 5. Emitting offenses and edits

```rust
cx.emit_offense(
    cx.range(node),
    "Use snake_case",
    None,                      // None → host applies the cop's DEFAULT_SEVERITY
);

cx.emit_edit(
    cx.range(node),
    "snake_case_name",
);
```

`emit_offense`'s severity argument is `Option<Severity>`. `None` lets
the host's default chain (cop default → murphy.toml override → built-in
warning) apply. Passing `Some(Severity::Error)` overrides at the
emission site and is rare — use it only for cops that distinguish
severity per match site.

`emit_edit` is a separate FFI callback (`FnTable::emit_edit`). An
offense without an edit is a report-only cop. A cop emitting edits
should also emit the offense the edit fixes (the host pairs them by
range overlap).

## 6. Severity

```rust
#[repr(u8)]
pub enum Severity { Warning = 0, Error = 1 }
```

The ABI wire encoding uses one byte:

- `0` / `1` → `Warning` / `Error`.
- `SEVERITY_UNSET = 255` → "no override; host default applies".

`Severity::to_wire(Option<Severity>) -> u8` and
`Severity::from_wire(u8) -> Option<Severity>` are the canonical
adapters. The same tristate-byte pattern (`TRISTATE_UNSET = 255`)
encodes `DEFAULT_ENABLED`.

See `crates/murphy-plugin-api/src/severity.rs`.

## 7. Options: `#[derive(CopOptions)]`

Cops with no configuration pass `options = NoOptions` to `#[cop]`
(see §3) — `NoOptions` is a unit type with an empty schema.

Cops with options declare an options struct and derive the schema:

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

Then pass it through in `#[cop]`:

```rust
#[cop(
    name = "RSpec/ExampleLength",
    options = ExampleLengthOptions,
    // …
)]
impl ExampleLength { /* … */ }
```

`#[option(...)]` arguments on each field:

| Argument | Notes |
|---|---|
| `default = …` | Required. Literal default for `Default::default()`. |
| `description = "…"` | Optional. Surfaced in `murphy cops list` output. |

What `#[derive(CopOptions)]` generates (ADR 0036, murphy-9cr.7) is an
implementation of the `CopOptions` trait — schema metadata for the
host to validate `murphy.toml` `[cops.rules."Name"]` tables at
config-load time, plus a field-by-field `from_config_json` decoder. The
trait itself lives in `crates/murphy-plugin-api/src/options.rs` and is
not part of the documented authoring surface.

Runtime option access via `Cx` is murphy-9cr.9 (not yet wired). v1 cops
read `OptionStruct::default()` inside the dispatch method; live
overrides land with that ticket.

## 8. Registering a pack: `register_cops!`

`register_cops!` (ADR 0038, murphy-9cr.6) emits the registration table
the host loads. One macro, two modes.

### Static pack

For packs linked directly into `murphy-cli` (currently only
`murphy-std`):

```rust
murphy_plugin_api::register_cops!(
    mode = static,
    NoReceiverPuts,
    UnreachableCode,
    StringLiterals,
    TrailingWhitespace,
);
```

This generates a `pub const REGISTRATION: PluginRegistration` the host
links against directly. No FFI at the pack boundary.

### Dynamic pack

For packs shipped as cdylibs (rspec, rails, example-pack, third party):

```rust
murphy_plugin_api::register_cops!(
    mode = dynamic,
    DescribeClass,
    ExampleLength,
    MultipleExpectations,
);
```

This expands to:

```rust
#[unsafe(no_mangle)]
pub unsafe extern "C" fn murphy_plugin_register() -> *const PluginRegistration { … }
```

plus the same `REGISTRATION` const referenced through that entry point.
The host calls `dlsym("murphy_plugin_register")` after `dlopen`ing the
cdylib (see §12).

Both modes assemble the same `PluginRegistration` struct
(`abi.rs`); the only difference is whether it ships via Rust linkage or
through an FFI entry point.

## 9. Pack project layout

A typical dynamic pack:

```text
crates/my-pack/
  Cargo.toml             # crate-type = ["cdylib", "rlib"]
  src/
    lib.rs               # register_cops!(mode = dynamic, …)
    cops/
      mod.rs
      my_namespace/
        mod.rs
        my_cop.rs        # one cop per file
  tests/
    dep_boundary.rs      # asserts runtime murphy deps = {murphy-plugin-api}
```

`crate-type = ["cdylib", "rlib"]`: the `cdylib` is what `dlopen` loads
in production; the `rlib` is so murphy-cli's `[dev-dependencies]` can
resolve the package for in-process e2e tests
(`crates/murphy-cli/tests/rspec_pack_e2e.rs` does this for the RSpec
pack). Production never `use`s a dynamic pack from Rust.

The `tests/dep_boundary.rs` invariant (see §2) catches accidental
direct imports of `murphy-core`, `murphy-ast`, etc. — copy
`crates/murphy-rspec/tests/dep_boundary.rs` and update the package
name. New entries to `ALLOWED_MURPHY_RUNTIME_DEPS` are intentional
API-surface expansions that need an ADR.

## 10. Loading a pack via `murphy.toml`

User-facing config (`murphy.toml`, ADR 0041):

```toml
[[plugins]]
name = "murphy-rspec"

[[plugins]]
name = "my-pack"
path = "./vendor/my-pack/libmy_pack.so"
```

The shorthand `plugins = ["murphy-rspec"]` is equivalent to a `name`
entry and resolves through the search order in ADR 0042
(high → low):

1. **Same-array `Detailed { name, path }`** — a sibling
   `{ name = "…", path = "…" }` entry in the same array wins regardless
   of array order (dedup pre-pass).
2. **`MURPHY_PLUGIN_PATH` env** — `std::env::split_paths` semantics
   (`:` on Unix, `;` on Windows).
3. **Project-local** `<project_root>/.murphy/plugins/`.
4. **User-local** `dirs::data_dir()/murphy/plugins/` (Linux
   `$XDG_DATA_HOME/...`, macOS `~/Library/Application Support/...`).

Filename convention in every search directory is `lib<sanitized>.{so,dylib}`
where `<sanitized>` replaces `-` with `_` — matching Cargo's cdylib
naming (`murphy-rails` ⇒ `libmurphy_rails.so`). Names go through path
validation (1..=64 chars, `[A-Za-z0-9_.-]`, no `..`) to block path
traversal.

A `[[plugins]] name = "…" path = "…"` entry (the **Detailed** form)
skips the search and uses the explicit `path` (project-root relative).
Per-cop `enabled = false` / `severity = "warning"` overrides live under
`[cops.rules."Pack/CopName"]` (ADR 0015 / murphy-9cr.2).

The host calls `dlopen` → `dlsym("murphy_plugin_register")` → reads the
returned `PluginRegistration`, validates `MURPHY_PLUGIN_ABI_VERSION`,
and registers each cop's metadata with the dispatch table.

## 11. Testing cops: the `test-support` feature

`murphy-plugin-api` exposes a parser-driven test harness gated by the
`test-support` cargo feature. Any pack can enable it in
`[dev-dependencies]` and write inline `#[cfg(test)] mod tests`
against its cops:

```toml
[dev-dependencies]
murphy-plugin-api = { path = "../murphy-plugin-api", features = ["test-support"] }
```

With the feature off, `murphy-translate` (the runtime parser) is not
pulled in and the test_support module is `#[cfg]`-gated out — the
production cdylib stays parser-free.

### `run_cop`

The low-level harness — parse `src` as Ruby, run `T::check` once per
matching node (or once at root for `KINDS = &[]`), return captured
offenses:

```rust
use murphy_plugin_api::test_support::run_cop;

let offenses = run_cop::<MyCop>("def foo; end\n");
assert_eq!(offenses.len(), 1);
assert_eq!(offenses[0].message, "…");
```

`CapturedOffense` carries `cop_name`, `message`, `range`, and
`severity: Option<Severity>` (`None` when the cop didn't override —
the host's default chain applies in production).

### `expect_offense!` / `expect_no_offenses!`

Higher-level assertion macros (murphy-ac6) with rubocop-style caret
annotations. Each annotation line attaches to the source line
immediately above; caret column is the **char index** of the source
line (multibyte safe via `char_indices()`).

```rust
use murphy_plugin_api::test_support::{expect_offense, expect_no_offenses, indoc};

#[test]
fn flags_two_expects() {
    expect_offense!(
        MultipleExpectations,
        indoc! {r#"
            it "x" do expect(a).to eq(1); expect(b).to eq(2) end
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Example has too many expectations (2/1)
        "#}
    );
}

#[test]
fn does_not_flag_single_expect() {
    expect_no_offenses!(
        MultipleExpectations,
        indoc! {r#"
            it "works" do
              expect(a).to eq(1)
            end
        "#}
    );
}
```

Rules:

- One annotation line per source line in MVP (parser already absorbs
  multiple; comparator/renderer support is murphy-swo).
- Empty annotation message (carets only, no text after) ⇒ range-only
  match. Useful for cops with dynamic message text like `(6/5)`.
- `expect_no_offenses!` rejects caret-bearing input as a typo guard
  for `expect_offense!`; the inverse guard applies the other way.
- Failure renders both expected and actual sides as caret-annotated
  source for a diff-style panic message.
- Multi-line ranges emit carets only on the first line and append
  `(+ N more chars)` to the message — block-level cops (whole `it … end`)
  are awkward to annotate with carets; `run_cop` + manual `assert_eq!`
  is the escape hatch.

Design: `docs/plans/2026-05-24-expect-offense-macro-design.md`.
Implementation: `crates/murphy-plugin-api/src/test_support.rs`.

### `indoc!`

Re-exported from `indoc` (also feature-gated). Strips the common
ASCII-whitespace prefix from each line of a `r#"…"#` literal at
compile time so Ruby fixtures can stay indented inside the Rust test
without affecting parse output or caret column math.

## 12. References

- ADR 0031 — Native plugin pack ABI (first cut).
- ADR 0033 — Plugin ABI v1 option metadata.
- ADR 0034 — No synthesised node dispatch.
- ADR 0035 — Const-based `Cop` trait.
- ADR 0036 — `#[derive(CopOptions)]` via serde-json.
- ADR 0037 — Arena, parser-shaped typed AST (`NodeKind` layout).
- ADR 0038 — Single-surface plugin ABI.
- ADR 0041 — `[[plugins]]` loading schema.
- ADR 0042 — Plugin name resolution / search path.

Design docs (`docs/plans/`):

- `2026-05-22-plugin-reboot-design.md` — the §5 single-surface design.
- `2026-05-22-murphy-9cr2-abi-extension-design.md`.
- `2026-05-22-murphy-9cr6-register-cops-macro-design.md`.
- `2026-05-22-murphy-9cr7-derive-copoptions-design.md`.
- `2026-05-23-murphy-9cr8-cop-attribute.md`.
- `2026-05-23-murphy-plugin-api-single-surface.md`.
- `2026-05-24-murphy-9cr10-1-dynamic-plugin-pack-mvp.md`.
- `2026-05-24-expect-offense-macro-design.md`.

Companion guides:

- `docs/guides/rubocop-hook-dispatch.md` — full RuboCop hook ⇄ Murphy
  kind table.
- `docs/guides/third-party-cop-sandbox.md` — security posture for
  third-party packs (v1 ships no sandbox — packs are trusted code; see
  ADR 0023 / ADR 0024).
