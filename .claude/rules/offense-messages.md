# Construct offense messages with `format!`, not `.replace()`

Use Rust's `format!("...{var}...")` macro instead of chaining `.replace()` or
`.replacen()` on template string constants.

## Why

- **Readability.** `format!("Use `{prefer}` instead of `{current}`.")` is
  immediately clear; `MSG.replace("{current}", &current).replace("{prefer}",
  &prefer)` requires reading the template constant to understand.
- **Compile-time checking.** Wrong placeholder names in `format!` are caught at
  compile time. Wrong `.replace()` tokens silently produce broken output.
- **No unnecessary constants.** Template strings used once are better inlined.

## Pattern

```rust
// Good
let current = cx.raw_source(offense_range);
let message = format!("Use '{prefer}' instead of '{current}'.");

let msg = format!("Favor `{}` over `{}`.", opts.enforced_style.as_str(), detected.as_str());

let msg = format!("Use `File.{method}`.");

let src = cx.raw_source(cx.range(expr));
let msg = format!("Move `{src}` out of the conditional.");
```

## Anti-pattern

```rust
// Avoid
const MSG: &str = "Use `{prefer}` instead of `{current}`.";
let msg = MSG.replace("{current}", &current).replace("{prefer}", &prefer);

// Avoid — single-use template constant
const MSG_TOUCH: &str = "Use `FileUtils.touch({filename})` instead of `File.open` in append mode with empty block.";
let msg = MSG_TOUCH.replace("{filename}", filename_src);

// Avoid — conceals the actual message
let msg = MSG_TEMPLATE.replacen("{method}", method, 1);
```

## When constants are appropriate

If the same static string (no placeholders) is used in multiple places,
a `const` / `static` is fine:

```rust
const MSG_TRAILING_CONDITIONAL: &str =
    "Do not use trailing conditionals in string interpolation.";
const MSG_TERNARY: &str =
    "Do not use ternary conditions in string interpolation.";
```

## See also

- `crates/murphy-std/src/cops/style/hash_conversion.rs` — uses `format!` for
  method-preference messages.
- `crates/murphy-std/src/cops/style/identical_conditional_branches.rs` — uses
  `format!` with embedded source snippets.
- `crates/murphy-std/src/cops/style/empty_string_inside_interpolation.rs` —
  static message constants for style-dependent messages (appropriate use-case).
