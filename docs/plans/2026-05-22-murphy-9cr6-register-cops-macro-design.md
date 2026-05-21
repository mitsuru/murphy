# murphy-9cr.6 — `register_cops!` proc macro Design

**Status**: design accepted 2026-05-22, ready for implementation
**Issue**: murphy-9cr.6 (parent epic: murphy-9cr)
**Related**: ADR 0033 (plugin ABI v1 option metadata), ADR 0034 (no synthesized dispatch), ADR 0035 (to be authored as part of this work)

## Background

murphy-9cr.3 introduced `murphy-plugin-api` with a method-based `Cop`
trait (`fn name(&self) -> &'static str`). To let plugin authors write
`register_cops!(MyCop1, MyCop2);` and have a proc macro emit a static
`[MurphyPluginCopV1; N]` table, the macro needs cop metadata at const
evaluation time. Method-based traits cannot be evaluated in const
context (Rust 2024 still does not allow `fn` in traits to be `const`).

The macro also needs to know whether each cop registers a `run_file` /
`run_node` / `run_call` callback. Stable Rust lacks specialization, so
"does `C` implement `FileCop`?" cannot be answered inside a const
expression by trait reflection alone.

This design fixes the trait shape so the macro can do its job entirely
through const associated items, with callback presence carried by
`Option<MurphyRun*>` constants on `Cop` itself.

## Scope

1. Reshape `murphy_plugin_api::Cop` from method-based to const-based
   metadata.
2. Add separate callback traits `FileCop` / `NodeCop` / `CallCop`
   (super-trait `Cop`, static methods, no `&self`).
3. Add `RUN_FILE` / `RUN_NODE` / `RUN_CALL` const fn-pointer fields on
   `Cop`, defaulting to `None`.
4. Provide generic thunks (`run_file_thunk<C: FileCop>` etc.) in
   `murphy-plugin-api` so cops can set
   `const RUN_FILE = Some(run_file_thunk::<Self>);`.
5. New crate `murphy-plugin-macros` (`proc-macro = true`) exposing
   `register_cops!`.
6. `register_cops!(C1, C2, …)` expands to a `murphy_plugin_register`
   `extern "C"` function with a static cops table built from each
   `C::*` const.
7. Minimal stubs for safe `FileContext` / `NodeContext` /
   `CallContext` / `Emitter` (raw pointer wrappers; field-level
   accessors deferred to murphy-9cr.8).
8. Compile-time uniqueness check on cop names via const fn assert.
9. trybuild ui tests (3 pass, 2 fail).
10. ADR 0035 to record the const-based `Cop` decision.

## Non-scope

- `#[derive(CopOptions)]` proc macro — **murphy-9cr.7**.
- `#[murphy::cop]` + `#[on_node]` attribute macro — **murphy-9cr.8**.
- Full safe-API surface on `FileContext` / `NodeContext` / `Emitter`
  (accessors, autocorrect emission, etc.) — incremental, tied to .8.
- Migrating `murphy-rails` (138 cops) / `murphy-example-pack` (3 cops)
  to `register_cops!` — bundled with .8 once attribute macros remove
  the remaining hand-written boilerplate.
- Hot-reload, multi-plugin orchestration, plugin-pack manifest — .10.

## Trait Architecture

```rust
pub trait Cop: Send + Sync + 'static {
    type Options: CopOptions;
    const NAME: &'static str;
    const DESCRIPTION: &'static str = "";
    const DEFAULT_SEVERITY: Option<Severity> = None;
    const DEFAULT_ENABLED: Option<bool> = None;

    // Callback fn pointers — default None. Cops that opt in to a
    // callback set their const to Some(run_*_thunk::<Self>).
    const RUN_FILE: Option<MurphyRunFile> = None;
    const RUN_NODE: Option<MurphyRunNodeDispatch> = None;
    const RUN_CALL: Option<MurphyRunCallDispatch> = None;
}

pub trait FileCop: Cop {
    fn run_file(ctx: &FileContext<'_>, emit: &mut Emitter);
}
pub trait NodeCop: Cop {
    fn run_node(node: &Node<'_>, ctx: &NodeContext<'_>, emit: &mut Emitter);
}
pub trait CallCop: Cop {
    fn run_call(ctx: &CallContext<'_>, emit: &mut Emitter);
}
```

Notes:

- All callbacks are `fn`, not `fn(&self, …)`. Murphy cops are
  stateless by design; the typed AST inputs supply everything needed.
- Setting `RUN_FILE = Some(run_file_thunk::<Self>)` without an
  accompanying `impl FileCop` is a compile error: the thunk's `where
  C: FileCop` bound rejects it.
- The boilerplate of repeating `Some(run_file_thunk::<Self>)` per cop
  is removed by `#[murphy::cop]` in murphy-9cr.8; in this issue it is
  hand-written.

## Macro Expansion

Input:

```rust
register_cops!(NoTabs, NoSpaces);
```

Expansion sketch:

```rust
const _: () = {
    use ::murphy_plugin_api as __api;

    const _: () = __api::__internal::assert_unique_cop_names::<2>([
        <NoTabs as __api::Cop>::NAME,
        <NoSpaces as __api::Cop>::NAME,
    ]);

    static OPTIONS_0: &[__api::MurphyCopOptionV1] =
        <<NoTabs as __api::Cop>::Options as __api::CopOptions>::schema();
    static OPTIONS_1: &[__api::MurphyCopOptionV1] =
        <<NoSpaces as __api::Cop>::Options as __api::CopOptions>::schema();

    static COPS: [__api::MurphyPluginCopV1; 2] = [
        __api::__internal::build_cop::<NoTabs>(OPTIONS_0),
        __api::__internal::build_cop::<NoSpaces>(OPTIONS_1),
    ];

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn murphy_plugin_register(
        out: *mut __api::MurphyPluginV1,
    ) -> i32 {
        if out.is_null() { return 1; }
        unsafe {
            *out = __api::MurphyPluginV1 {
                size: ::core::mem::size_of::<__api::MurphyPluginV1>(),
                cops_ptr: COPS.as_ptr(),
                cops_len: COPS.len(),
                call_dispatch_ptr: ::core::ptr::null(),
                call_dispatch_len: 0,
                run_call_dispatch: None,
                node_dispatch_ptr: ::core::ptr::null(),
                node_dispatch_len: 0,
                run_node_dispatch: None,
            };
        }
        0
    }
};
```

`build_cop::<C>` is the const fn that reads `C::NAME` / `DESCRIPTION` /
`DEFAULT_SEVERITY` / `DEFAULT_ENABLED` / `RUN_FILE` and packs them into
a `MurphyPluginCopV1`. `assert_unique_cop_names` const-panics with a
diagnostic if any two `NAME`s match, surfacing as a compile error.

## Callback Detection & Thunks

The const-fn-pointer pattern moves "does C have a `run_file`?" from
trait reflection to value inspection. The macro reads
`<C as Cop>::RUN_FILE` and stores it verbatim — `None` becomes a null
ABI fn pointer, `Some(thunk)` becomes the thunk address.

Thunks live in `murphy_plugin_api` as monomorphizable
`extern "C"` functions:

```rust
pub unsafe extern "C" fn run_file_thunk<C: FileCop>(
    ctx: *const MurphyFileContext,
    emit: MurphyEmitOffense,
    sink: *mut c_void,
) -> i32 {
    if ctx.is_null() { return 1; }
    let safe_ctx = FileContext::from_raw(unsafe { &*ctx });
    let mut emitter = Emitter::from_raw(emit, sink);
    C::run_file(&safe_ctx, &mut emitter);
    0
}
```

Each cop type instantiates its own monomorphized copy of the thunk —
that is the address stored in `MurphyPluginCopV1.run_file`. The thunk
owns the unsafe/safe boundary; cop authors never write `extern "C"` or
touch raw pointers.

Open follow-ups on the safe wrappers (`FileContext` / `Emitter` etc.):
this issue ships them as minimal stubs (constructor + access to the
underlying `MurphyFileContext` fields). murphy-9cr.8 will round out the
API surface as the attribute macro grows.

## Test Strategy

- **trybuild ui tests** in `crates/murphy-plugin-macros/tests/ui/`:
  - `pass_single_cop.rs` — single cop, expansion compiles.
  - `pass_multiple_cops.rs` — two cops.
  - `pass_cop_with_options.rs` — `Options` is a non-`NoOptions` type
    implementing `CopOptions`.
  - `fail_non_cop_type.rs` — `register_cops!(NotACop);` where
    `NotACop` does not implement `Cop`; expects the standard
    "unsatisfied trait bound" diagnostic.
  - `fail_duplicate_name.rs` — two cops share `const NAME`; expects a
    const-eval panic surfaced as a compile error pointing at
    `assert_unique_cop_names`.
- **plugin-api unit tests**:
  - `build_cop` packs `DEFAULT_SEVERITY` / `DEFAULT_ENABLED` into the
    expected `u8` sentinels.
  - `build_cop` carries `RUN_FILE = None` through unchanged.
  - `assert_unique_cop_names` panics with a recognisable message on
    duplicates (smoke test via `#[should_panic]` wrapping a runtime
    call; the compile-time use lives in trybuild).
- **doctest**: `register_cops!` usage example on the macro's doc
  comment; runs as part of `cargo test`.
- **deferred**: migrating `murphy-rails` / `murphy-example-pack` to
  `register_cops!`, and an end-to-end `cargo build` + dynamic load
  integration test. Both wait on murphy-9cr.8.

## Implementation Order

1. Rewrite `Cop` trait in `murphy-plugin-api` (const-based + RUN_*
   consts).
2. Add `FileCop` / `NodeCop` / `CallCop` callback traits.
3. Add minimal `FileContext` / `NodeContext` / `CallContext` /
   `Emitter` stubs.
4. Add `run_file_thunk` / `run_node_thunk` / `run_call_thunk` generics.
5. Add `__internal::build_cop` and `__internal::assert_unique_cop_names`
   const fns.
6. Update existing plugin-api doctest and unit tests for the new
   trait shape.
7. Create `crates/murphy-plugin-macros` crate (`proc-macro = true`,
   syn / quote / proc-macro2 dependencies, minimal features).
8. Implement `register_cops!` parser and expansion.
9. Add trybuild ui tests (3 pass, 2 fail).
10. Run `cargo fmt --check`, `cargo clippy --workspace --all-targets
    -- -D warnings`, `cargo test --workspace`.
11. Author ADR 0035.
12. Commit (one commit per logical group is fine; rebase down before
    push if needed).

## Risks

- The thunk pointer is `unsafe extern "C" fn` with a generic
  parameter. Each cop type triggers monomorphization; with 138 cops in
  `murphy-rails`, this is ~138 mono copies per thunk. Compile-time
  cost is observable but bounded (function body is tiny). If it ever
  becomes a hotspot, the thunk body can be `#[inline(never)]` with a
  small core that takes `fn(...)` as a parameter, but YAGNI for now.
- `FileContext::from_raw(&*ctx)` lifetimes are tricky around the
  callback boundary. We document the invariant that the raw pointer
  outlives the safe wrapper for the duration of the callback and
  enforce it with a `'a` lifetime parameter on `FileContext<'a>`.
- `assert_unique_cop_names` produces a compile error pointing at the
  const block, not at the duplicate names. We accept the slightly
  indirect diagnostic for v1; a custom `compile_error!` with span
  picking is possible if users complain.

## Open Follow-ups

- murphy-9cr.7 — `#[derive(CopOptions)]` (consumes the same trait
  layout).
- murphy-9cr.8 — `#[murphy::cop]` + `#[on_node]` (removes the
  `Some(run_file_thunk::<Self>)` boilerplate, populates the
  call/node-dispatch arrays in the `MurphyPluginV1` build).
- murphy-9cr.10 — distribution UX, template repo, load diagnostics.
- murphy-9cr.12 — `safe_autocorrect: bool` on `Cop` (extends the
  const list once the ABI grows).
