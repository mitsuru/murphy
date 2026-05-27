# Autocorrect: prefer surgical edits over whole-node interpolation

When writing a cop that emits an autocorrect, prefer **two non-overlapping `cx.emit_edit` calls** over reading the source with `raw_source` + `format!` and replacing the whole node range.

## Why

- The receiver and argument source pass through byte-for-byte — no whitespace, parenthesisation, or quoting drift.
- The autocorrect ships as the **minimal diff**, which is easier to read in PR review and friendlier to downstream diff tooling.
- No string allocation for the replacement payload.

## Pattern

For `!x.include?(y)` → `x.exclude?(y)`:

```rust
// Edit 1: delete the negation prefix (`!` or `not `, plus any whitespace).
// The outer range starts at the negation token; the inner range starts
// at the first byte of `include?`'s receiver — everything in between
// is the prefix.
let negation_prefix = Range {
    start: cx.range(outer).start,
    end: cx.range(inner).start,
};
cx.emit_edit(negation_prefix, "");

// Edit 2: rename the selector. `loc.name` is the parser-gem-style
// selector range, so this is exactly the bytes of `include?`.
cx.emit_edit(cx.node(inner).loc.name, "exclude?");
```

The two edits never overlap, so the test harness's edit-application phase applies them cleanly and the result reaches fixpoint on the next pass.

## Anti-pattern

```rust
// Avoid: walks the AST to extract receiver/arg source, format!s a new
// string, and replaces the whole outer range. Functional, but loses
// the original spacing/style and allocates a payload for what is
// usually a 1-byte deletion + 1-byte selector rename.
let replacement = format!(
    "{}.exclude?({})",
    cx.raw_source(cx.range(inner_receiver)),
    cx.raw_source(cx.range(arg)),
);
cx.emit_edit(cx.range(outer), &replacement);
```

## When to fall back to whole-node interpolation

The surgical form requires `loc.name` to be set on the relevant node (true for `Send` selectors). For rewrites that fundamentally rearrange the structure — e.g. swapping argument positions, or moving the receiver to a different position — whole-node interpolation is the cleaner choice. The rule of thumb: **if the rewrite is "delete X" + "rename Y", use two edits; if it's "shuffle the AST", use interpolation**.

## See also

- `crates/murphy-rails/src/cops/rails/negate_include.rs` — canonical example of the two-edit form.
- `crates/murphy-std/src/cops/lint/deprecated_class_methods.rs` — mixes selector renames with whole-node replacements depending on the shape; useful precedent.
