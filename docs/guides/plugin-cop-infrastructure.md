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
imports stay short:

```rust
use murphy_plugin_api::{
    Cop, Cx, NodeCop, NodeKindTag,        // traits + dispatch
    NodeId, NodeKind, OptNodeId, Range,   // AST types (re-exported from murphy-ast)
    Symbol, StringId, AstNode, Comment,
    CopOptions, NoOptions,                // options trait
    Severity,                             // offense severity
    cop, on_node, on_new_investigation,   // attribute macros
    register_cops, node_pattern,          // function-like macros
    CopOptions as _DeriveCopOptions,      // #[derive(CopOptions)]
};
```

Anything reachable only through `murphy-core`, `murphy-ast`,
`murphy-translate`, `murphy-pattern` etc. is **off limits** at runtime —
the dep-boundary test will fail the build.

## 3. The `Cop` trait — compile-time metadata

```rust
pub trait Cop: Send + Sync + 'static {
    type Options: CopOptions;
    const NAME: &'static str;
    const DESCRIPTION: &'static str = "";
    const DEFAULT_SEVERITY: Option<Severity> = None;
    const DEFAULT_ENABLED: Option<bool> = None;
}
```

Every field is an associated `const` so `register_cops!` can assemble
the registration table at const-eval time (ADR 0035). Runtime dispatch
lives on `NodeCop` (see §4). A `Cop` is a stateless unit struct in
practice — the `#[cop]` attribute (see §5) generates the impl from
declarative metadata.

## 4. The `NodeCop` trait — dispatch

```rust
pub trait NodeCop: Cop {
    const KINDS: &'static [NodeKindTag];
    fn kinds(&self) -> &[NodeKindTag] { Self::KINDS }
    fn check(&self, node: NodeId, cx: &Cx<'_>);
}
```

`NodeKindTag` is the `u8` discriminant of a `NodeKind` variant
(`#[repr(C, u8)]` declaration order, ADR 0037). The host dispatches
`check` once per matching node.

Two dispatch modes:

- **Per-kind** — `KINDS = &[NodeKindTag::of(&NodeKind::Send(..)), …]`.
  `check` is called once for every node whose tag is in the list.
- **File-visit** — `KINDS = &[]`. `check` is called exactly once per
  file with `node == cx.root()`. Used by whole-file scanners like
  `Layout/TrailingWhitespace` whose root may be any of `Begin`, `Nil`,
  `Send`, `Def`, `Class`, … (a fixed kind subscription would not work).

The `kinds()` method exists so dynamic in-process cops (mruby-backed)
can compute their subscription at runtime without changing the ABI.

See `crates/murphy-plugin-api/src/node_cop.rs` and ADR 0034.

## 5. Authoring a cop with `#[cop]` and `#[on_node]`

The recommended shape is the declarative `#[cop]` attribute. Example
from `murphy-rspec`:

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
        // … emit_offense via cx
    }
}
```

What `#[cop]` generates:

- `impl Cop for DescribeClass` populated from the attribute arguments
  (`NAME`, `DESCRIPTION`, `DEFAULT_SEVERITY`, `DEFAULT_ENABLED`, `Options`).
- `impl NodeCop for DescribeClass` with:
  - `KINDS` collected from every `#[on_node(kind = "…")]` in the impl
    block (multiple `#[on_node]` methods supported).
  - A `check` body that matches the runtime tag and dispatches to the
    matching `check_*` method.

`#[on_new_investigation]` declares the file-visit form
(`KINDS = &[]`). It is the natural mapping for RuboCop's
`on_new_investigation` lifecycle hook. See
`docs/guides/rubocop-hook-dispatch.md` for the full RuboCop hook ⇄
Murphy kind table.

The `node_pattern!` macro (ADR 0033 / murphy-9cr.18) provides a Prism
shape-matching DSL for cops that need to test deeper than one kind at a
time; the parser is in `crates/murphy-pattern`.

## 6. Reading the AST: `Cx`

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

## 7. Emitting offenses and edits

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

## 8. Severity

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

## 9. Options: `CopOptions` and `#[derive(CopOptions)]`

```rust
pub trait CopOptions: Default + Sized + 'static {
    const SCHEMA: &'static [OptionSpec] = &[];
    fn from_config_json(_bytes: &[u8]) -> Result<Self, ConfigError>;
}
```

Cops with no configuration use `NoOptions` (a unit struct that
implements `CopOptions` with an empty schema).

Cops with options derive the impl with `#[derive(CopOptions)]`
(ADR 0036, murphy-9cr.7):

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

What `#[derive(CopOptions)]` generates:

- `impl Default` returning the field defaults.
- `const SCHEMA: &[OptionSpec]` — one entry per `#[option]` field,
  used by the host to validate `murphy.toml` `[cops.rules."Name"]`
  tables at config-load time.
- `fn from_config_json` — field-by-field decoding via `serde_json`
  (the only third-party dep `murphy-plugin-api` carries; the runtime
  ABI itself does not pull serde).

The `Options` associated type on `Cop` is set to the options struct
(`options = ExampleLengthOptions` in `#[cop(...)]`).

Runtime option access via `Cx` is murphy-9cr.9 (not yet wired). v1 cops
read `OptionStruct::default()` inside `check`; live overrides land with
that ticket.

## 10. Registering a pack: `register_cops!`

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

## 11. Pack project layout

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

## 12. Loading a pack via `murphy.toml`

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

## 13. Testing cops: the `test-support` feature

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

## 14. References

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
