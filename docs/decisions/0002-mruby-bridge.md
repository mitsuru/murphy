# ADR 0002 — embedded mruby + native AST handle bridge

- Date: 2026-05-19
- Status: Accepted
- Spike: `murphy-fp7` (Phase 0, Spike 0.2)
- Depends on: ADR 0001 (prism binding)
- Feeds: Phase 0 Gate; Phase 3 (mruby cop path + native-primitive IDL); flags Spike 0.3

## Context

User cops stay as `.rb` and run via **in-process embedded mruby** — no daemon,
no IPC, no serialization round-trip (design §2/§3). The crux risk: can an mruby
script walk the single shared prism AST by calling Rust natively, in-process,
without serializing the tree? Spike 0.2 had to prove the pattern *and* pick the
embedding crate.

## Decision

- **Crate: `mruby3-sys` 3.2.0** (low-level FFI to vendored mruby 3.x). Its build
  script compiles mruby C; **build burden is on us, never on cop authors**
  (same criterion as ADR 0001) — verified, ~13 s clean build with mise-pinned
  Rust 1.95.0 + system `cc`.
- **Handle model: opaque integer indices, not raw pointers in Ruby.** Rust owns
  an `AstContext` (handle table + derived data). A `*mut AstContext` is stashed
  in `mrb_state.ud`. mruby holds only `Integer` handles; native primitives
  resolve handle → live prism node and read it directly. This is *safer* than
  exposing `*const Node` into the VM (no dangling pointer survivable in Ruby;
  resolution is bounds-checked) and is the recommended Phase 3 IDL handle form.
- **"Fast core, scripted glue" confirmed:** native primitives are Rust module
  functions (`Murphy.node_name`, `Murphy.node_count`); a thin Ruby `Node` class
  wraps them. The traversal reads like Ruby; the work is native.

## Evidence (`spikes/mruby_poc`)

Parsed `"puts \"hi\"\nlogger.info(x)\nFoo.bar(1)\n"` with the ADR 0001 binding
(5 call nodes). An mruby `.rb` snippet — loaded and run **as-is** — did:

```ruby
class Node
  def initialize(h) = @h = h
  def name = Murphy.node_name(@h)
end
Murphy.node_count.times { |i| Murphy.report("visited:" + Node.new(i).name) }
```

Result: `["visited:puts","visited:info","visited:logger","visited:x","visited:bar"]`
— every prism node walked from Ruby through native calls only; **only integers
crossed the boundary**, the node data was read live in Rust. Assertions pass:
`ALL ASSERTIONS PASSED — mruby walked the live prism AST, no serialization`.

A **second, independent `mrb_state`** was opened, ran `1 + 1`, and closed while
the first was live — confirming the per-cop isolated-state precondition
(design §6) is mechanically available.

## Findings / gotchas (load-bearing for Phase 3 & Spike 0.3)

1. **`mruby3-sys` bindgen omits the `data.h` API and inline value helpers.**
   `mrb_data_object_alloc`, `mrb_data_get_ptr`, `RData` (opaque), and the
   inline boxers (`mrb_fixnum_value`, `mrb_obj_value`, …) are **not** in the
   generated bindings — but the symbols *do* exist in `libmruby.a` (verified via
   `nm`). Consequences: (a) the classic `DATA_PTR` object-wrapping pattern is
   not turn-key here; (b) the integer-handle-via-`ud` model we chose sidesteps
   this entirely; (c) where Phase 3 needs omitted symbols, declare a small
   `extern "C"` block or a C shim — do **not** switch crates for this alone.
   Integer values were produced by evaluating a Ruby literal
   (`mrb_load_string`) to avoid the missing inline boxers — fine for the spike;
   Phase 3 should add the extern decls instead.
2. **Lifetime: prism `ParseResult<'pr>` borrows the source.** A struct owning
   both source and `ParseResult` is self-referential and will not compile
   (hit during the spike, E0106). The architecture rule (now binding for P1
   Task 3 / Phase 3): the **Core owns the source buffer and the AST as
   siblings** for the file's processing scope, source outliving the AST; the
   mruby-facing context holds **handles + derived data only**, never the
   borrowed tree.
3. **Drop order is load-bearing and is UB if reversed:** `mrb_close()` MUST run
   before the AST/source is dropped. A GC finalizer or in-flight native call
   could otherwise deref freed AST. Phase 3 must encode this ordering in types
   (e.g. the cop runner owns the mrb_state in a field declared *after* the AST
   borrow, or closes explicitly before returning).
4. **Spike 0.3 forward-flag (do not solve here):** `mrb_state` in this
   `mruby3-sys` build does **not** expose `code_fetch_hook` / instruction-hook
   fields (default mruby build = no `MRB_USE_DEBUG_HOOK`). Spike 0.3's runaway-
   cop deadline therefore likely needs an OS-thread + watchdog/timeout, or a
   custom mruby build config enabling the hook. Recorded so 0.3 is not
   blindsided.

## Rejected alternatives

- **`minutus` 0.5.0** (macro bridge) — its wrappers lean toward *moving* Rust
  values into mruby; our handles reference a borrowed, Core-owned AST. Fighting
  that ownership model adds risk for no v1 benefit.
- **`mrusty` 1.0.0** (safe wrapper) — safe abstractions hide exactly the
  `ud`/raw-state access this design needs, and it targets an older mruby line.
- **`mrb` 0.1.2** — minimal/old; fewer guarantees than `mruby3-sys` with no
  upside.
- **CRuby embedding (magnus/rb-sys)** — already rejected in design §2 (GVL +
  startup cost negate the speed motivation); not re-litigated.

## Consequences

- **Native-primitive IDL (design §8) seed.** Proven this spike: `node_count`,
  `node_name`, plus a `report` sink. Phase 3 IDL candidates to design next
  (NOT decided here): `receiver` / `receiver_nil?`, `message_loc` → byte
  `Range`, node type tag, child iteration (`each_child`), source-slice by range.
  All must respect ADR 0001's **byte-offset** rule.
- Phase 3 cop runner: one `mrb_state` per cop (isolation), `ud` → per-run
  context, primitives defined at open, `.rb` loaded verbatim, state closed
  before AST teardown.
- Pin `mruby3-sys = "=3.2.0"` in `crates/` for the same schema-stability reason
  as ADR 0001; a bump re-verifies this PoC's assertions.
- `spikes/mruby_poc` is throwaway; only this ADR and its rules are load-bearing.
