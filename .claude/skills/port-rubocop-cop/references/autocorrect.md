# Autocorrect — `emit_edit` reference

Authoritative source: infra guide §5. Project-level invariant lives in
the root `CLAUDE.md` under "Testing Philosophy": **autocorrect must
remain idempotent**.

## When to ship autocorrect

Ship an `emit_edit` only when the replacement is **provably safe** —
the corrected source parses, has the same semantics, and passes the
cop on the next run.

Safe by construction:

- Whitespace cleanups (`Layout/TrailingWhitespace`,
  `Layout/EmptyLineAfterMagicComment`).
- Quote style normalisation (`Style/StringLiterals` —
  `"foo"` ↔ `'foo'` when the string has no escapes or interpolation).
- `return` removal (`Style/RedundantReturn` for the last expression in
  a method body that already evaluates to the return value).

Refuse to autocorrect when:

- The replacement requires looking up something not in source —
  resolving a class name from a string literal
  (`describe "Foo"` → `describe Foo`) needs the class to exist.
- The fix is a refactor with judgement attached — splitting an
  example, extracting a method, renaming an identifier across callers.
- The original is ambiguous and a "fix" would silently change
  semantics.

Mirror what the RuboCop original ships. If RuboCop marks the cop as
"unsafe autocorrect", do not ship it from Murphy without an explicit
note in the file's doc-comment justifying the call.

## Calling `emit_edit`

```rust
cx.emit_edit(cx.range(node), "replacement_text");
```

The `Range` argument is the span of source the replacement overwrites
(usually the offense range itself, but can be narrower — e.g.
overwriting just the closing quote of a string).

The string argument is plain bytes inserted in place. The cop is
responsible for matching whitespace / indentation / line endings — the
host writes the bytes verbatim.

## Offense + edit pairing

A cop emitting edits should also emit the offense the edit fixes.
The host pairs them by range overlap when applying `murphy lint --fix`
(infra guide §5).

```rust
cx.emit_offense(cx.range(node), "Use single quotes for strings without interpolation.", None);
cx.emit_edit(cx.range(node), "'value'");
```

A bare `emit_edit` with no matching offense is a bug — the host has no
way to surface to the user that the file was rewritten.

## Idempotency

The fix-loop runs `lint --fix` until no further edits apply. A cop
whose edit re-introduces something the cop itself would flag will
infinite-loop the fix-point (and `cargo run -p murphy-cli -- lint --fix --debug`
will surface it).

Plugin-api's `test_support` currently exposes `expect_offense!`,
`expect_no_offenses!`, `indoc!`, and `run_cop` only — there is no
ready-made `apply_fix` helper. To check idempotency of an edit at
test time, apply the edit by hand on top of `run_cop`'s output and
re-run the cop, asserting the second pass returns no offenses:

```rust
let offenses = run_cop::<MyCop>(src);
let edit = offenses[0]
    .clone()
    /* the captured edit list lives next to `CapturedOffense`; if
       a pack needs this routinely, add an `apply_fix` helper to its
       local `tests/test_support_ext.rs` rather than per-cop. */;
let fixed_once = apply_edit_locally(src, edit);
assert!(run_cop::<MyCop>(&fixed_once).is_empty(), "cop fired on its own output");
```

The end-to-end fix-point invariant is also asserted at the CLI level
in `crates/murphy-cli/tests/cli.rs` — adding a cop that breaks
idempotency will surface there even without a per-cop unit test.

## Idiomatic patterns

### Range narrowing

When only part of the offense range needs rewriting (e.g. only the
trailing whitespace, not the whole line), build a narrower `Range`:

```rust
let whole = cx.range(node);
let trailing = Range {
    start: whole.start + content_len as u32,
    end: whole.end,
};
cx.emit_edit(trailing, "");
```

### Computing replacements from source

Pull the original text via `cx.raw_source(range)` when the replacement
is a small transform of the original (quote style, casing), not a
fresh literal. That keeps the rewrite respecting any escapes the user
already had:

```rust
let original = cx.raw_source(cx.range(node));
let rewritten = swap_quotes(original);
cx.emit_edit(cx.range(node), &rewritten);
```
