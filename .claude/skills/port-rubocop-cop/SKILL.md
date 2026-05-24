---
name: port-rubocop-cop
description: This skill should be used when the user asks to "port a RuboCop cop", "move RuboCop's X cop to Murphy", "add an RSpec/Rails/Style/Layout cop to murphy-*", "RuboCop の cop を移植", "X cop を移植して", or otherwise wants to translate a RuboCop (or RuboCop-RSpec / RuboCop-Rails) rule into a Murphy plugin cop using the single-surface ABI from `docs/guides/plugin-cop-infrastructure.md`. Covers selecting the target pack, scaffolding the cop file, picking the right `#[on_node]` dispatch (including `methods = […]` and `node_pattern!`), wiring options via `#[derive(CopOptions)]`, registering with `register_cops!`, and writing `expect_offense!` / `expect_no_offenses!` / `run_cop` tests.
---

# Porting a RuboCop cop to Murphy

This skill walks through translating one RuboCop rule into a Murphy plugin cop
authored against the single-surface plugin ABI documented in
`docs/guides/plugin-cop-infrastructure.md` (the "infra guide"). It is the
master procedure; the infra guide is the API reference it points back into.

Treat the infra guide as load-bearing — re-read the relevant section instead
of guessing at the API surface. Hand-copying type names from older PRs is a
common source of build failures because the macro surface has churned.

## When to use this skill

Trigger on requests that name a RuboCop (or RuboCop-RSpec / RuboCop-Rails)
cop and ask to bring it into Murphy. Typical phrasings:

- "Port `RSpec/EmptyExampleGroup` to murphy-rspec."
- "Add `Lint/UselessAssignment` as a Murphy std cop."
- "`Style/RedundantReturn` を Murphy に移植して — autocorrect 込みで。"
- "Translate this rubocop-rails rule into murphy-rails."

If the request is "write a brand-new cop with no RuboCop ancestor", most of
the same steps apply — start from §3 below; the source-reading step in §2 is
optional.

## End-to-end procedure

Follow these phases in order. Each phase points to the canonical reference
in the infra guide or the in-tree example cop that demonstrates the pattern.

### 1. Identify the source rule and target pack

Establish three facts before writing code:

1. **The cop's RuboCop identifier and source.** Read the RuboCop (or
   rubocop-rspec / rubocop-rails) implementation that the port should
   mirror. Note: which `on_<kind>` hooks it uses, what `RESTRICT_ON_SEND`
   it sets, what options it accepts, whether it autocorrects, and any
   `def_node_matcher` patterns.
2. **Murphy's target pack.** Map the RuboCop namespace to a Murphy crate:
   - `Murphy/*`, `Lint/*`, `Style/*`, `Layout/*` → `crates/murphy-std/`
     (static pack, linked into `murphy-cli`).
   - `RSpec/*` → `crates/murphy-rspec/` (dynamic pack).
   - `Rails/*` → `crates/murphy-rails/` (dynamic pack).
   - A new third-party namespace → a new dynamic pack scaffolded off
     `crates/murphy-example-pack/` — see infra guide §9.
3. **The Murphy cop name.** Always `"Pack/CopName"`, matching the
   directory layout and `murphy.toml` key. The name passed to
   `#[cop(name = …)]`, the file path under `src/cops/<namespace>/`, and
   user-facing config must all agree.

If a similar cop already exists in the target pack, read it first — pack
conventions (helpers, `is_example_call`-style gates, doc-comment style)
should be reused rather than reinvented.

### 2. Decide the dispatch shape

This is the single most important design choice. The infra guide §3 lists
all valid forms.

- **One node kind, no method-name filter** — use
  `#[on_node(kind = "block")]` (or `class`, `def`, `hash`, `if`, …). See
  `RSpec/MultipleExpectations` (`crates/murphy-rspec/src/cops/rspec/multiple_expectations.rs`).
- **`Send` with a known method allow-list** — use
  `#[on_node(kind = "send", methods = ["describe", "context"])]`. The
  host pre-filters by method symbol so the cop body never runs for
  unrelated `Send` nodes. See `RSpec/DescribeClass`. The
  `methods = […]` shape is mandatory whenever the allow-list is a fixed
  literal set — it is faster *and* clearer than re-checking the symbol
  inside the body. **Never use `methods = []`** (rejected at parse time
  as a typo) and **never combine `methods` with a non-`send` kind**
  (also rejected).
- **`Send` with a runtime-computed method name** — use bare
  `#[on_node(kind = "send")]` and check `cx.symbol_str(method)` inside
  the body. Only reach for this when the predicate genuinely cannot be
  expressed as a static literal set (configuration-dependent, dynamic).
- **Whole-file scan / raw-source cop** — use `#[on_new_investigation]`.
  Mirrors RuboCop's `on_new_investigation` and is dispatched once per
  file with `node == cx.root()`. Use this for layout / trailing-whitespace
  style cops that work on source text rather than a specific node kind.
- **Shape-based matching across multiple kinds** — declare a
  `node_pattern!(...)` matcher at module top level and call it from the
  dispatch body. See `references/node-pattern.md` for the v1 grammar
  subset, or infra guide §3 ("Reusable matchers").

When mapping RuboCop's `on_<kind>` names to Murphy kind strings, use
`docs/guides/rubocop-hook-dispatch.md`. RuboCop hook aliases (`on_send`,
`on_str`, …) are accepted by the dispatcher but the *kind string* passed
to `#[on_node(kind = …)]` is the Murphy-canonical form (`"send"`,
`"str"`, etc., per the infra guide's RSpec / std examples).

### 3. Scaffold the cop file

For an existing pack, add one file per cop under
`crates/<pack>/src/cops/<namespace>/<cop_snake_case>.rs` and re-export it
from `cops/<namespace>/mod.rs`. The file layout (top-of-file doc comment,
options struct, `#[cop]` impl block, helpers, `#[cfg(test)] mod tests`)
should mirror an existing peer cop in the pack.

The minimum skeleton, with no options:

```rust
//! `Pack/CopName` — one-line summary of what the cop policies.
//!
//! ## Matched shapes
//! …
//! ## Why this shape
//! …
//! ## Autocorrect (or "## No autocorrect" with the reason)
//! …

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct CopName;

#[cop(
    name = "Pack/CopName",
    description = "Human-readable one-liner.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl CopName {
    #[on_node(kind = "send", methods = ["foo"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, args, .. } = *cx.kind(node) else {
            return;
        };
        // …gate, then emit
        cx.emit_offense(cx.range(node), "Message here.", None);
    }
}
```

Cops with options follow `references/options.md` (and the
`ExampleLengthOptions` example shipped in `murphy-rspec`).

Cops with autocorrect add `cx.emit_edit(range, replacement)` calls
alongside `emit_offense`; see `references/autocorrect.md`.

### 4. Register the cop with the pack

Add the cop to the pack's `register_cops!` invocation in
`crates/<pack>/src/lib.rs`. The macro mode (`static` vs `dynamic`) is
already set by the existing call — just add the new struct to the list,
in source order.

Static pack example (`murphy-std`):

```rust
murphy_plugin_api::register_cops!(
    mode = static,
    NoReceiverPuts,
    UnreachableCode,
    StringLiterals,
    CopName             // ← new entry (no trailing comma — house style)
);
```

Dynamic pack example (`murphy-rspec`):

```rust
murphy_plugin_api::register_cops!(
    mode = dynamic,
    DescribeClass,
    ExampleLength,
    MultipleExpectations,
    CopName             // ← new entry (no trailing comma — house style)
);
```

Do not add a new `pub use` or re-export — the macro takes the type
directly, and the cop struct is reached via `cops::<namespace>::CopName`
in scope at the top of `lib.rs`.

### 5. Write tests against the test-support harness

Every cop ships its tests in the same file, inside `#[cfg(test)] mod
tests` and using the `murphy_plugin_api::test_support` macros. The pack
must have

```toml
[dev-dependencies]
murphy-plugin-api = { path = "../murphy-plugin-api", features = ["test-support"] }
```

in its `Cargo.toml` (already true for `murphy-std`, `murphy-rspec`,
`murphy-rails`, `murphy-example-pack`).

Pick the right macro:

- **`expect_offense!`** — the default for positive cases whose emit
  range fits on one source line. Caret annotations describe the
  expected range and message. See `references/testing.md` for the
  annotation grammar and the multi-line caveat.
- **`expect_no_offenses!`** — the default for negative cases.
- **`run_cop`** — escape hatch for multi-line emit ranges (block-level
  cops, layout cops), parameterised loops, or assertions on
  `cop_name` / `severity`. Returns `Vec<CapturedOffense>`. See the
  `RSpec/MultipleExpectations` test file for the canonical pattern of
  using `run_cop` for whole-block emits.

Wrap every Ruby fixture string in `indoc!` (re-exported from the
harness) so the source can stay indented in the test body without
affecting caret column math.

Aim for tests that cover, at minimum: the positive flag, the canonical
negative case, the false-positive guards the cop deliberately includes
(e.g. "ignores explicit receiver", "ignores empty body"), and any
syntactic variants RuboCop's original test suite covers (`do…end` vs
`{…}`, single- vs multi-line, alias methods).

### 6. Verify the build and the boundary

Run, in order:

```bash
cargo test -p <pack>                            # the cop's tests
cargo test -p <pack> --test dep_boundary        # single-surface invariant
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

The `dep_boundary.rs` test will fail if the new cop pulled in
`murphy-core`, `murphy-ast`, `murphy-translate`, etc., directly — the
only allowed runtime Murphy dependency is `murphy-plugin-api`. If it
fails, do not edit `ALLOWED_MURPHY_RUNTIME_DEPS`; instead rewrite the
cop to use only `murphy-plugin-api` re-exports. Widening the boundary
requires an ADR (infra guide §2 and §9).

For dynamic packs, also run the end-to-end test if one exists (e.g.
`cargo test -p murphy-cli --test rspec_pack_e2e`) to confirm the
cdylib loads correctly and the host's plugin loader sees the new cop.

## Common pitfalls

1. **Counting `Send`s without the empty-receiver gate.** RuboCop matchers
   that look at `expect(...)` mean *bare* `expect`, not `foo.expect(...)`.
   Inside the cop body, always check `receiver == OptNodeId::NONE`
   before treating a `Send` as a DSL call. See
   `RSpec/MultipleExpectations`' `is_bare_expect_call` helper.
2. **Synthesising autocorrects from string literals.** A cop that flags
   `describe "Foo"` cannot autocorrect to `describe Foo` — the class may
   not exist. Reach for `emit_edit` only when the replacement is
   provably safe; otherwise document "no autocorrect" with the reason
   in the file's top-doc comment, the same way `RSpec/DescribeClass`
   does.
3. **`methods = []` or stale method-name checks.** The `methods = [...]`
   attribute is the *primary* dispatch filter; the
   `if cx.symbol_str(method) != "describe" { return; }` line inside
   the body is redundant when the filter is present (and leaving it in
   is a warning sign that the port was done mechanically rather than
   against the current macro surface).
4. **`#[on_node]` on the wrong kind.** A cop that mirrors RuboCop's
   `on_block` does *not* use `kind = "send"` for the call — it uses
   `kind = "block"` and destructures the block to reach the call. The
   infra guide §3 examples are authoritative.
5. **Missing per-file-name gating for spec/Rails-only rules.** Murphy
   has no per-cop `Include` glob support yet (infra guide §3 note on
   `RSpec/DescribeClass`). RSpec / Rails cops will fire outside their
   intended file set. Document this in the cop's doc-comment as a
   known v1 limitation and tell users to disable via
   `[cops.rules."Pack/CopName"] enabled = false` if it hurts them.
6. **Re-using state across calls.** Cops are stateless unit structs.
   Anything that needs cross-node accounting (counts, sets, …) must be
   accumulated *inside one dispatch call* by walking `cx.descendants(id)`
   or `cx.children(id)`. Do not stash state on the struct — the macro's
   const-metadata shape does not allow it.

## Additional resources

### Reference files

- **`references/api-surface.md`** — quick map of every
  `murphy_plugin_api::` import the cop file is likely to pull, with
  short notes per type.
- **`references/dispatch-and-cx.md`** — extended cheat-sheet for the
  `#[on_node]` / `#[on_new_investigation]` attributes and every method
  on `Cx<'_>`.
- **`references/node-pattern.md`** — `node_pattern!` v1 grammar subset
  and when to reach for it instead of hand-written destructuring.
- **`references/options.md`** — `#[derive(CopOptions)]` + `#[option(...)]`
  reference, including the current v1 limitation that options are read
  from `OptionStruct::default()` inside the dispatch (murphy-9cr.9 will
  wire live overrides through `Cx`).
- **`references/autocorrect.md`** — when to ship `emit_edit`, idempotency
  expectations, and the offense/edit pairing the host uses.
- **`references/testing.md`** — full `expect_offense!` annotation
  grammar, the multi-line caret caveat, and the `run_cop` escape hatch.

### Example cop files in-tree

The canonical reference cops to read while porting:

- `crates/murphy-rspec/src/cops/rspec/describe_class.rs` — `Send` cop
  with method-name filter, multiple receiver shapes, no options, no
  autocorrect.
- `crates/murphy-rspec/src/cops/rspec/multiple_expectations.rs` — block
  cop with options, descendant counting, `run_cop` test pattern for
  multi-line ranges.
- `crates/murphy-rspec/src/cops/rspec/example_length.rs` — block cop
  with options and raw-source line counting (`cx.raw_source`).
- `crates/murphy-std/src/cops/` — every cop with autocorrect lives
  here; pick the closest peer when porting `Style/*` / `Lint/*` cops.

### Companion docs

- **`docs/guides/plugin-cop-infrastructure.md`** — the API reference
  this skill is a procedure for. Re-read the relevant section instead
  of guessing.
- **`docs/guides/rubocop-hook-dispatch.md`** — RuboCop `on_<kind>` ⇄
  Murphy kind-string mapping table.
- **`docs/guides/third-party-cop-sandbox.md`** — security posture
  (v1 ships no sandbox; cops are trusted code).
