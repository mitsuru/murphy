# ADR 0035: const-based `Cop` trait and `RUN_*` fn-pointer pattern

**Status**: Accepted (2026-05-22)
**Issue**: murphy-9cr.6 (parent epic: murphy-9cr)
**Related**: ADR 0011 (Severity ordering), ADR 0031 (native plugin pack ABI), ADR 0033 (plugin ABI v1 option metadata), ADR 0034 (no synthesized dispatch)

## Context

murphy-9cr.3 first shipped `murphy_plugin_api::Cop` as a method-based
trait (`fn name(&self) -> &'static str`, etc.) sketched after the
existing `murphy_core::Cop`. The next macro work (murphy-9cr.6's
`register_cops!`) needs to assemble a `static [MurphyPluginCopV1; N]`
table from each cop type, at const evaluation time. Method-based
traits cannot meet that requirement on stable Rust:

- Trait methods are not (yet) `const fn`. There is no way to read
  `<C as Cop>::name(&instance)` from a `static` initializer.
- Stable Rust has no `specialization`, so a generic helper cannot ask
  "does `C` implement `FileCop`?" inside a const expression.
- Calling `<X as CopOptions>::schema()` (a non-const method) from a
  `static` slice produces `E0015` ("cannot call non-const associated
  function in statics").

`register_cops!(C1, C2)` must therefore see everything it needs about
each cop as **associated constants** rather than methods, and it must
have a way to inspect callback presence without specialization.

## Decision

The `murphy_plugin_api::Cop` trait shape is:

```rust
pub trait Cop: Send + Sync + 'static {
    type Options: CopOptions;
    const NAME: &'static str;
    const DESCRIPTION: &'static str = "";
    const DEFAULT_SEVERITY: Option<Severity> = None;
    const DEFAULT_ENABLED: Option<bool> = None;

    // Callback fn pointers — default None. A cop opts in to a
    // callback by implementing the matching trait and setting the
    // const to Some(run_*_thunk::<Self>).
    const RUN_FILE: Option<MurphyRunFile> = None;
    const RUN_NODE: Option<MurphyRunNodeDispatch> = None;
    const RUN_CALL: Option<MurphyRunCallDispatch> = None;
}

pub trait FileCop: Cop {
    fn run_file(ctx: &FileContext<'_>, emit: &mut Emitter<'_>);
}
pub trait NodeCop: Cop {
    fn run_node(ctx: &NodeContext<'_>, emit: &mut Emitter<'_>);
}
pub trait CallCop: Cop {
    fn run_call(ctx: &CallContext<'_>, emit: &mut Emitter<'_>);
}
```

`CopOptions` likewise exposes its schema as an associated const so
`register_cops!` can wire it into the static cop table without a
non-const method call:

```rust
pub trait CopOptions: Default + Sized + 'static {
    const SCHEMA: &'static [MurphyCopOptionV1] = &[];
}
```

Callbacks are **static methods** (no `&self`). Murphy cops are
stateless by design: the typed AST context plus the `Emitter` are
everything a callback receives.

Generic thunks
(`run_file_thunk::<C: FileCop>` and siblings) live in
`murphy-plugin-api`. Plugin authors set
`const RUN_FILE = Some(run_file_thunk::<Self>)`, which monomorphizes
into a per-cop `extern "C" fn`. `register_cops!` reads the const value
verbatim — `None` becomes a null ABI fn pointer, `Some(thunk)` becomes
the thunk's address.

A doc-hidden `__internal` module on `murphy-plugin-api` exposes
`build_cop::<C>()` (a `const fn` that reads every `Cop` associated
const and packs a `MurphyPluginCopV1`) and
`assert_unique_cop_names::<const N: usize>([&'static str; N])` (a
`const fn` that const-panics on duplicate NAMEs). The macro emits a
`const _: () = { … };` block around both helpers, so violations
surface as compile errors pointing at that block.

## Reasons

1. **Stable Rust const-eval mechanics.** A `static [MurphyPluginCopV1; N] = [build_cop::<C1>(), build_cop::<C2>(), …];` is the simplest expansion that produces a valid plugin table, and it requires every input to live in `const` land.
2. **No specialization required.** "Does C have a `run_file`?" is answered by the const value `<C as Cop>::RUN_FILE`. Authors who never set it stay at `None`; authors who set it must also implement `FileCop` (the thunk has a `where C: FileCop` bound that rejects mismatched pairs at compile time).
3. **Stateless cop philosophy stays explicit.** Callbacks take no `self`. There is no per-call instance allocation, no `Default` requirement for *cops* (only for `Options`), and no temptation to lean on hidden mutable state.
4. **Macro stays thin.** With every read of cop metadata expressed through `const`, `register_cops!` is essentially a list-to-array transformation plus one `const _: () = ...;` assertion. Future macros (`#[murphy::cop]`) can lean on the same surface.
5. **Boilerplate is contained.** The one true boilerplate cost is repeating `const RUN_FILE: ... = Some(run_file_thunk::<Self>);` per callback. `#[murphy::cop]` in murphy-9cr.8 will fill this in automatically; in the meantime, hand-written plugins write the line once per cop.

## Alternatives Considered

- **Box::leak per-cop instance at register time.** Builds a `Box<MyCop>`, leaks it for `'static`, and calls `&self` methods through the leaked pointer. Rejected: leaks compound (every nested `'static` field — `MurphyCopOptionV1` slices, for instance — needs its own leak), requires giving up `static [MurphyPluginCopV1; N]`, and adds runtime allocation cost on plugin load.
- **Autoref specialization hack** (`(&Wrap::<C>).method()` resolves to a more specific blanket when available). Rejected: method calls are not `const fn` on stable Rust; the hack only works in expression position at runtime, not inside a `static` initializer.
- **`register_cops!(MyCop as FileCop, Other as (FileCop, NodeCop))`** — let the input list spell out which callbacks each cop carries. Rejected: it abandons the type-list ergonomics promised by `register_cops!(MyCop1, MyCop2)`, makes `#[murphy::cop]` generation more brittle (the attribute macro would have to coordinate naming with the function-like macro), and pushes specialization-shaped problems out of the language into the macro.
- **Method-based Cop with a separate `static_metadata` constructor const.** Rejected: doubles the surface that needs to stay in sync — every change to Cop metadata would touch both the methods and the static constructor. Pure-const wins by collapsing the two.

## Consequences

- **Breaking change to murphy-9cr.3.** The `Cop` and `CopOptions` traits ship with `fn name(&self)` / `fn schema()` in murphy-9cr.3; murphy-9cr.6 reshapes them to associated consts. Nothing in the workspace depends on the .3 shape yet (murphy-rails and murphy-example-pack go through `cop_v1` / `cop_v1_dispatch_only` from murphy-core, not this trait), so the change has no in-tree consumers to migrate.
- **Per-cop boilerplate for `RUN_*`.** Hand-written plugins repeat one line per callback (`const RUN_FILE: Option<MurphyRunFile> = Some(run_file_thunk::<Self>);`). The attribute macro in murphy-9cr.8 removes this; the boilerplate is the price of staying on stable Rust without specialization.
- **`#[derive(CopOptions)]` (murphy-9cr.7) emits an associated const, not a method.** The trait shape settled here is what the derive must produce.
- **Generic thunks monomorphize per cop.** With ~140 cops in `murphy-rails`, three thunks each, you get up to ~420 monomorphized copies — each ~20 lines of generated code, tiny. If this ever shows up in compile profiling, we can switch to a non-generic core fn taking a `fn(...)` parameter, but YAGNI for v1.
- **Const-eval diagnostics for duplicate NAMEs.** The duplicate-name compile error points at the `register_cops!` invocation rather than at the specific cops, by design. We accept the slightly indirect message in exchange for keeping the assertion fully `const`.

## Implementation status

Implemented in murphy-9cr.6. See:

- `crates/murphy-plugin-api/src/lib.rs` — rewritten `Cop` /
  `CopOptions` traits, `FileCop` / `NodeCop` / `CallCop`, safe context
  / emitter stubs, three `run_*_thunk` generics,
  `__internal::{build_cop, assert_unique_cop_names, …}`.
- `crates/murphy-plugin-macros/` — new proc-macro crate exposing
  `register_cops!`.
- `crates/murphy-plugin-macros/tests/ui/` — 3 pass + 2 fail trybuild
  fixtures (single cop, multiple cops, cop with non-empty options;
  non-`Cop` rejection, duplicate-name rejection).

## Follow-up issues

- murphy-9cr.7 — `#[derive(CopOptions)]` emits the const `SCHEMA` slice.
- murphy-9cr.8 — `#[murphy::cop]` + `#[on_node]` auto-fills `RUN_*` consts and populates the call/node-dispatch arrays.
- murphy-9cr.10 — distribution UX, template repo, load diagnostics.
- murphy-9cr.12 — extends `Cop` with `const SAFE_AUTOCORRECT: bool = true;` once the underlying ABI grows.
