# ADR 0001 — prism Rust binding selection

- Date: 2026-05-19
- Status: Accepted
- Spike: `murphy-9fc` (Phase 0, Spike 0.1)
- Supersedes: none
- Feeds: P1 Task 3 (parse adapter), Phase 3 native-primitive IDL

## Context

Murphy parses each target file with prism **once** and exposes the shared
immutable AST to both the native cop engine and (later) embedded mruby. Offenses
are keyed on byte offsets `{start_offset, end_offset}` (design §5), so the
binding must give faithful byte-level locations, not just line/column. We must
choose how Rust talks to prism. Candidates:

1. **`ruby-prism` crate** — official Rust bindings published by ruby/prism,
   wrapping `ruby-prism-sys` (the generated C FFI). `vendored` feature builds
   the prism C source via a `cc` build script.
2. **Raw FFI to a system `libprism`** — link an externally installed prism.
3. **Vendor prism C ourselves** + hand-roll bindings.

Selection criteria (from the spike): node coverage, byte-offset/location
fidelity, zero-copy source access, and **build burden on us, never on cop
authors** (a cop author drops a `.rb` into `cops/`; they must not need a C
toolchain — design §2).

## Decision

**Use the official `ruby-prism` crate (locked at v1.9.0) with its `vendored`
feature.** Murphy's Rust core links prism statically via the crate's build
script. The PoC at `spikes/prism_poc/` proves viability.

## Evidence (from `spikes/prism_poc`)

Parsed `"puts \"hi\"\nlogger.info(x)\nobj.foo\n"` and walked it via the crate's
`Visit` trait (`visit_call_node`). Verified against a hand-counted snippet:

- `puts` → `message_loc` byte range **[0, 4)** = `"puts"`, `receiver()` is
  `None`. Exactly the range/predicate a `NoReceiverPuts` cop needs.
- `logger.info(x)` → `info` message range `[17, 21)` = `"info"`,
  `receiver().is_some()` = true.
- All node and message byte ranges matched the hand-checked offsets; the PoC's
  assertions pass (`ALL BYTE-RANGE ASSERTIONS PASSED`).
- **Error tolerance:** parsing `"def (\n"` returned a partial tree **plus** a
  structured error list, each with a byte `location()` and message — **no
  panic**. This directly satisfies design §6 ("syntax-error file → 1 offense,
  skip cops, continue") and P1 Task 8.
- **Ruby-semantics note for cop authors / IDL:** bare identifiers (`logger`,
  `obj`, `x`) also parse as `CallNode`s with no receiver. The native-primitive
  IDL (Phase 3) and native cops must distinguish "method call" intent via
  `receiver()` + arguments/`message_loc`, not by node type alone. Recorded here
  so it is not rediscovered later.

Build: compiled cleanly with the project toolchain (mise-pinned Rust 1.95.0,
system `cc` 13.3). `ruby-prism-sys` built prism C via `cc` with no extra setup.

## Rejected alternatives

- **Raw FFI to system `libprism`** — rejected. Forces every Murphy build/CI host
  to install a matching prism, and version skew between the C lib and our
  expected node schema is a silent-corruption risk on the load-bearing offsets.
- **Vendor prism C + hand-rolled bindings** — rejected. Re-implements exactly
  what `ruby-prism-sys` already generates and maintains in lockstep with prism
  releases; pure maintenance cost with no upside for v1.

## Consequences

- **Build burden lands on us, not cop authors** — exactly the required split.
  Cop authors never see a C toolchain; the prism C build is internal to our
  Cargo build.
- **Version pin:** `ruby-prism = "=1.9.0"` initially. Prism node schema changes
  ride crate upgrades; treat a `ruby-prism` bump as a potential AST-contract
  change and re-run the offset assertions before accepting.
- **P1 Task 3** (`crates/murphy-core/src/parse.rs`) wraps `ruby_prism::parse`,
  returning our `Ast` on success and a structured `ParseError { message, range }`
  built from `result.errors()` on failure — never panicking.
- **Zero-copy:** locations are byte offsets into the original source buffer; the
  parse adapter must keep the source alive for the AST/visitor lifetime
  (`'pr`). Carry this constraint into the Task 3 ownership design.
- **Offsets are BYTES, not chars.** Verified with a multibyte snippet
  (`"# あいさつ\nputs \"こんにちは\"\n"`): prism returns byte offsets into
  `source.as_bytes()`; the `puts` selector resolved to bytes `[15, 19)` and the
  offsets were valid UTF-8 char boundaries, so `&src[a..b]` recovered `"puts"`.
  Consequence for all of Murphy: any code slicing source by an offense range
  (output, autocorrect, P1 Task 9 snapshots) **must** index by byte and treat
  offsets as `u8` positions. Slicing a `&str` at a non-boundary panics — native
  cops and the mruby native primitives must never assume char indices. Offense
  `Range` stays `{start_offset, end_offset}` in bytes (design §5 unchanged).
- **Version pin discipline:** the spike's `Cargo.toml` uses `ruby-prism =
  "1.9.0"` (Cargo-semantics `^1.9.0`). When P1 Task 3 lands this in
  `crates/murphy-core`, pin exactly (`=1.9.0`) so a prism node-schema change
  cannot ride in silently on the load-bearing offsets; bumping is a deliberate,
  re-verified step.
- The PoC under `spikes/prism_poc/` is throwaway and is **not** promoted into
  `crates/`; only this ADR and its conclusions are load-bearing.
