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
    description = "Check that the first argument to the top-level describe is a constant.",
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

### Method-name filtering on `Send` dispatch

`#[on_node(kind = "send")]` fires for **every** `Send` node in the file
— `puts(x)`, `obj.foo`, `a + b`, `arr[i]`, all of it. The
`methods = [...]` argument restricts dispatch to a fixed allow-list of
method symbol names — Murphy's analogue of RuboCop's
`RESTRICT_ON_SEND = %i[describe context]` (murphy-34d, murphy-ip0):

```rust
#[on_node(kind = "send", methods = ["describe", "context"])]
fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
    // Reached only when the Send's method symbol is "describe" or "context".
    let NodeKind::Send { receiver, args, .. } = *cx.kind(node) else { return };
    // … real check
}
```

The macro lowers the array into the cop's `SEND_METHODS` associated
const, which `register_cops!` pushes into `PluginCopV1::send_methods_ptr`.
**The filter is applied by the host dispatcher** — for a `Send` whose
method symbol is not in the list, the cop's `dispatch` thunk is never
invoked, saving the FFI call + cop body wake-up. Cops that don't opt
in (`SEND_METHODS` defaults to empty) keep the historical "every
`Send` reaches the cop" contract.

Rules:

- `methods` is **only** valid on `kind = "send"`. The macro rejects it
  on any other kind at parse time (other node types don't have a
  single "method name" axis).
- `methods = []` is rejected — it silently disables the cop, which is
  almost always a typo.
- The same kind cannot dispatch to multiple methods (`#[cop]`-level
  duplicate-kind check), so combine multiple allow-lists into one
  attribute rather than declaring two competing
  `#[on_node(kind = "send", methods = […])]` instances.
- The `let-else` destructure inside the cop is still defensive — a
  future `NodeKind` aliasing accident would silently misreport without
  it, but the method-name check is gone.

Without `methods = [...]`, the dispatch fires for every `Send` and the
cop body uses the historical manual idiom:

```rust
#[on_node(kind = "send")]
fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Send { method, .. } = *cx.kind(node) else { return };
    if cx.symbol_str(method) != "describe" { return; }
    // …
}
```

Use this form when the method-name predicate is not a fixed literal
set (runtime-computed, configuration-dependent, etc.); reach for
`methods = [...]` whenever the allow-list is statically known.

### Reusable matchers: `node_pattern!`

For cops that need to test more than one kind at a time — RuboCop's
`def_node_matcher` use cases — the `node_pattern!` macro lowers a
RuboCop-subset S-expression pattern to a free function at compile
time (ADR 0033, murphy-9cr.17 / .18; the runtime IR lives in
`crates/murphy-pattern`).

```rust
use murphy_plugin_macros::node_pattern;

// Zero captures → bool. Tests shape only.
node_pattern!(is_bare_expect_call, "(send nil :expect _)");

// Captured atoms → Option<(Capture1, Capture2, ...)>.
node_pattern!(describe_first_arg, "(send {nil (const nil :RSpec)} :describe $_ ...)");

#[on_node(kind = "send")]
fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
    if !is_bare_expect_call(node, cx) {
        return;
    }
    // …
}
```

The generated function takes `(node: NodeId, cx: &Cx<'_>)` and is a
plain item at the module scope where the macro is invoked — invoke it
once per pattern at module top level, then call the matcher from any
cop's dispatch body. Pattern grammar (v1 RuboCop subset): atoms
(`42`, `:foo`, `true`, `nil`), node-kind heads (`int`, `send`, …),
wildcard `_`, alternation `{a b c}`, `...` rest, `$_` captures, `$...`
seq captures. Full grammar in `docs/plans/2026-05-22-murphy-9cr17-pattern-grammar.md`.

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

`murphy-plugin-api` exposes an in-process cop test harness gated by the
`test-support` cargo feature. Any pack can enable it in
`[dev-dependencies]` and write inline `#[cfg(test)] mod tests`
against its cops without rebuilding the host plumbing:

```toml
[dev-dependencies]
murphy-plugin-api = { path = "../murphy-plugin-api", features = ["test-support"] }
```

With the feature off, `murphy-translate` (the runtime parser) is not
pulled in and the `test_support` module is `#[cfg]`-gated out — the
production cdylib stays parser-free.

The preferred test-writing surface is the **tester builder**
(`test::<T>()`). Each cop sits behind the generic parameter, options
flow through `with_options(&T::Options)`, and one or more
`expect_*` methods chain off the same tester.

### `test::<T>()` — the tester builder

```rust
use murphy_plugin_api::test_support::{indoc, test};

#[test]
fn flags_and_corrects_equals_equals() {
    test::<SpaceAroundOperators>()
        .expect_offense(indoc! {r#"
            x==0
             ^^ Surrounding space missing for operator `==`.
        "#})
        .expect_correction(
            indoc! {r#"
                a+b
                 ^ Surrounding space missing for operator `+`.
            "#},
            "a + b\n",
        );
}
```

For cops with non-default options, prefix the chain with
`with_options(&T::Options)`. The struct comes in by reference;
`CopOptions::to_config_json` is called internally so test code never
constructs raw JSON.

```rust
test::<SpaceAroundOperators>()
    .with_options(&SpaceAroundOperatorsOptions {
        allow_for_alignment: false,
        ..Default::default()
    })
    .expect_offense(indoc! {r#"
        …
    "#});
```

Methods on `Tester<T>`:

- **`with_options(self, &T::Options) -> Self`** — set the typed
  options blob. Subsequent calls overwrite earlier values.
- **`expect_offense(&self, annotated: &str) -> &Self`** — assert the
  exact offense set against the caret-annotated source.
- **`expect_no_offenses(&self, src: &str) -> &Self`** — companion
  for the negative case.
- **`expect_correction(&self, annotated: &str, after: &str) -> &Self`**
  — assert offenses and the post-autocorrect source in one call.
- **`expect_no_corrections(&self, src: &str) -> &Self`** — assert
  the cop emits no autocorrect edits for `src` (offense set is
  unconstrained).

Each `expect_*` is `#[track_caller]` and returns `&Self`, so a single
tester carries multiple expectations through the same setup.

### Annotation grammar

Caret annotations describe each offense's range and message inline.
Each annotation line attaches to the **nearest preceding source
line**; multiple annotation lines can stack under one source line
when the cop fires several offenses on the same row. Caret column is
the **char index** of the source line (multibyte safe via
`char_indices()`).

```text
a+b-c*d
 ^ Surrounding space missing for operator `+`.
   ^ Surrounding space missing for operator `-`.
     ^ Surrounding space missing for operator `*`.
```

Rules:

- A line whose first non-whitespace char is `^` is an **annotation
  line**; everything else is a **source line**. Annotation lines are
  stripped before the source is parsed as Ruby.
- The caret column equals the char-index start of the offense in the
  source line above. Number of carets = char length of the range.
- Text after the last caret (whitespace-trimmed) is the expected
  message. **Exact-match** comparison. Omit the message (`^^^` only)
  to assert range only — useful for cops with dynamic message text
  like `(6/5)`.
- Matching is **exact-set**: any cop emission without a matching
  annotation fails the test, any annotation without a matching
  emission fails the test.
- On failure, the panic renders **both** expected and actual sides
  as caret-annotated source for a diff-style read.
- Multi-line ranges (offense spans past a newline) get carets on the
  first line only and `(+ N more chars)` is appended to the message.
  Block-level cops (whole `it … end`) are awkward to annotate with
  carets — use `run_cop` (below) for those until multi-line caret
  support lands.
- `expect_offense` panics with a "use `expect_no_offenses` instead"
  message if the input has no annotations (typo guard);
  `expect_no_offenses` panics with the symmetric message when the
  input contains carets.

### Single-line offense example

```rust
use murphy_plugin_api::test_support::{indoc, test};

#[test]
fn flags_two_expects_single_line_block() {
    test::<MultipleExpectations>().expect_offense(indoc! {r#"
            it "x" do expect(a).to eq(1); expect(b).to eq(2) end
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Example has too many expectations (2/1)
        "#});
}
```

### `indoc!`

Re-exported from the `indoc` crate (also feature-gated). Strips the
common ASCII-whitespace prefix from each line of a `r#"…"#` literal
at compile time so Ruby fixtures can stay indented inside the test
function without affecting parse output or caret column math. Use it
on every multi-line fixture string.

### Escape hatch: `run_cop`

For tests the caret grammar can't express cleanly — block-level
multi-line ranges, asserting `cop_name` / `severity`, parametrised
loops over many inputs — drop down to `run_cop`:

```rust
use murphy_plugin_api::test_support::run_cop;

let offenses = run_cop::<MyCop>("def foo; end\n");
assert_eq!(offenses.len(), 1);
assert_eq!(offenses[0].cop_name, "Plugin/MyCop");
```

It parses `src` as Ruby, runs the cop's dispatch (per-kind or
file-visit, mirroring host semantics), and returns
`Vec<CapturedOffense>`. `CapturedOffense` carries `cop_name`,
`message`, `range`, and `severity: Option<Severity>` (`None` when
the cop didn't override — the host's default chain applies in
production). The tester-builder methods are thin layers on top.

For autocorrect tests, `run_cop_with_edits` returns `CapturedRun`
with both `offenses` and `edits`. For non-default options under the
raw-JSON form, use `run_cop_with_options` /
`run_cop_with_options_and_edits` (the tester builder forwards
through these under the hood).

Design / implementation references:

- `crates/murphy-plugin-api/src/test_support.rs`

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
