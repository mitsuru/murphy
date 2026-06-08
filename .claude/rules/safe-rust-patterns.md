# Avoid unnecessary heap allocations and unsafe string manipulation

Two recurring patterns in review feedback: wasteful allocations during AST
traversal, and panics / data corruption from manual byte-level string
operations.

## Allocations

### Don't call `.to_vec()` on `cx.list()` / `cx.children()` slices

These already return `&[NodeId]`. The slice supports `.is_empty()`, `.len()`,
`.iter()`, and indexing — no need to heap-allocate a copy.

```rust
// Good
let children = cx.list(list);
for &child in children { /* ... */ }
if children.len() > 1 { /* ... */ }

// Avoid
let children = cx.list(list).to_vec();
```

### Use single-pass iteration instead of multiple `.contains()` calls

Each `.contains()` call iterates the entire string. For multi-condition checks,
use `chars().any()` for a single pass:

```rust
// Good — single pass
fn needs_percent_i(name: &str) -> bool {
    name.chars().any(|c| matches!(c, '\\' | '\t' | '\n' | '\r' | '\x0C' | '#'))
}

// Avoid — up to 6 passes
fn needs_percent_i(name: &str) -> bool {
    name.contains('\\') || name.contains('\t') || name.contains('\n')
        || name.contains('\r') || name.contains('\x0C') || name.contains('#')
}
```

### Avoid temporary `Vec` allocations during AST traversal

Use iterators with early-return instead of collecting into intermediate `Vec`s:

```rust
// Good — iterate directly, no intermediate allocation
let has_named = seqs.iter().any(|s| s.style != FmtStyle::Unannotated);
let unannotated_count = seqs.iter().filter(|s| s.style == FmtStyle::Unannotated).count();

// Avoid — allocates temporary Vec
let (named, unannotated): (Vec<_>, Vec<_>) = seqs.iter()
    .partition(|s| s.style != FmtStyle::Unannotated);
```

### Don't clone `cx.raw_source()` when `&str` suffices

`cx.raw_source()` returns `&str` with the context's lifetime. Only call
`.to_owned()` / `.to_string()` if you need to hold the string past the borrow.

```rust
// Good — keep as &str
let lhs_src = cx.raw_source(cx.range(lhs));
let rhs_src = cx.raw_source(cx.range(rhs));

// Avoid — unnecessary clone
let lhs_src = cx.raw_source(cx.range(lhs)).to_string();
```

## String safety

### Use `strip_prefix` / `strip_suffix` instead of manual byte slicing

Manual slicing like `&src[1..src.len()-1]` panics on empty strings, short
strings, or boundaries inside multi-byte characters.

```rust
// Good — safe and idiomatic
let Some(content) = src.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) else {
    return;
};

let Some(inner) = src.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) else {
    return false;
};

// Avoid — panics if src.len() < 2 or multi-byte boundary
let content = &src[1..src.len() - 1];
```

### Use `.chars().count()` instead of `.len()` for column position

`.len()` counts raw bytes. For visible width or column calculation where
multi-byte UTF-8 may appear, use `.chars().count()`:

```rust
// Good
fn def_column(node: NodeId, cx: &Cx<'_>) -> usize {
    let node_start = cx.range(node).start as usize;
    let src = cx.source();
    let line_start = src[..node_start].rfind('\n').map_or(0, |pos| pos + 1);
    src[line_start..node_start].chars().count()
}

// Avoid — wrong result with multi-byte characters
src[line_start..node_start].len()
```

### Use `.pop()` instead of slice-based truncation

```rust
// Good
let mut prefix = zip_src.to_owned();
prefix.pop(); // safe: handles empty string gracefully with no-op
let replacement = format!("{prefix}[]).to_h");

// Avoid — panics on empty or multi-byte boundary
let prefix = &zip_src[..zip_src.len() - 1];
```

### Use `char::encode_utf8` for single-character equality checks

Avoid manual byte-length and prefix checks that can get confused by multi-byte
characters:

```rust
// Good
if trimmed == close.encode_utf8(&mut [0; 4]) { /* ... */ }

// Avoid — manual byte/prefix checks
if trimmed.len() == close.len_utf8() && trimmed.starts_with(close) { /* ... */ }
```

## Fast-path before allocation

Check for trivial cases before allocating or entering complex logic:

```rust
// Good — fast-path: no opening delimiters → no complex content possible
fn complex_content(name: &str) -> bool {
    if name.contains(' ') {
        return true;
    }
    if name.contains('[') || name.contains('(') {
        let stripped = strip_balanced_delimiter_pairs(name); // allocates
        stripped.contains('[') || stripped.contains(']')
            || stripped.contains('(') || stripped.contains(')')
    } else {
        name.contains(']') || name.contains(')')  // no allocation needed
    }
}
```

## See also

- `crates/murphy-std/src/cops/style/symbol_array.rs` — `complex_content`
  fast-path, `needs_percent_i` single-pass, `encode_utf8` usage.
- `crates/murphy-std/src/cops/style/empty_method.rs` — `def_column` with
  `.chars().count()`.
- `crates/murphy-std/src/cops/lint/interpolation_check.rs` — `strip_prefix` /
  `strip_suffix` replacing manual slicing.
- `crates/murphy-std/src/cops/lint/useless_setter_call.rs` — avoiding `.to_vec()`
  on `cx.list()` slices.
