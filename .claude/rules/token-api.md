# Token API cheat-sheet for cop authors

Covers everything needed to write token-based cops without reading `cx.rs`
(6 000+ lines). This is the stable public surface; all types come from
`murphy_plugin_api`.

## Import

```rust
use murphy_plugin_api::{Cx, Range, SourceToken, SourceTokenKind};
```

---

## `SourceTokenKind` variants (complete list)

| Variant | Token(s) | Notes |
|---|---|---|
| `LeftParen` | `(` | Argument list open. NOT `#{` or `\->(`|
| `RightParen` | `)` | |
| `LeftBrace` | `{` | Hash literal or brace block. NOT `#{` (interpolation) or `\-> {` (lambda begin) — those are `Other` |
| `RightBrace` | `}` | Hash literal, brace block, or lambda body. String interpolation `}` is `Other` |
| `Comma` | `,` | |
| `HeredocStart` | `<<~RUBY` etc. | Token covers up to end of label (not `\n`) |
| `HeredocEnd` | `RUBY` terminator | Token covers just the label, not its newline |
| `Comment` | `# …` | |
| `Newline` | `\n` | Significant newline |
| `IgnoredNewline` | `\n` | Continuation newline (after `\`, inside `[]`, etc.) |
| `Other` | everything else | Keywords (`do`, `end`, `if`, …), operators, identifiers, string delimiters, etc. |

**Key rule:** `do` keyword = `SourceTokenKind::Other` with source text `b"do"`.

---

## Core methods on `Cx<'_>`

### `cx.sorted_tokens() -> &[SourceToken]`

Returns all tokens in source order (sorted by `range.start`). Each token is:

```rust
pub struct SourceToken {
    pub range: Range,   // byte offsets in cx.source()
    pub kind:  SourceTokenKind,
}
```

Tokens are **non-overlapping** and **sorted monotonically** by `range.start`,
so `partition_point` / `binary_search_by_key` gives O(log N) positional lookup.

### `cx.token_before(offset: u32) -> Option<SourceToken>`

Last token whose `range.end <= offset`. Useful to find what ends just before a
position.

### `cx.token_after(offset: u32) -> Option<SourceToken>`

First token whose `range.start >= offset`. Useful to find what starts at or
after a position.

### `cx.tokens_in(range: Range) -> &[SourceToken]`

Slice of all tokens **fully contained** within `range`
(`range.start <= tok.start` and `tok.end <= range.end`).

---

## Common patterns

### Find the first block opener (`do` or `{`) after a method name

Used to trim an offense range to exclude the block body (e.g. for
`RSpec/MultipleExpectations` `example_call_range`):

```rust
fn block_opener_start(method_name_end: u32, node_end: u32, cx: &Cx<'_>) -> Option<u32> {
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < method_name_end);
    toks[idx..]
        .iter()
        .take_while(|t| t.range.start < node_end)
        .find(|t| {
            t.kind == SourceTokenKind::LeftBrace
                || (t.kind == SourceTokenKind::Other
                    && &source[t.range.start as usize..t.range.end as usize] == b"do")
        })
        .map(|t| t.range.start)
}
```

Usage for `example_call_range`:

```rust
// Offense range ends just before the block opener, trimming `do`/`{` and
// any block args or multiline comments between the method name and the block.
let offense_end = block_opener_start(cx.node(call).loc.name.end, cx.range(call).end, cx)
    .unwrap_or(cx.node(call).loc.name.end);
let offense_range = Range { start: cx.range(call).start, end: offense_end };
```

### Find first `LeftParen` after a receiver (implicit call / dot-position)

```rust
let toks = cx.sorted_tokens();
let idx = toks.partition_point(|t| t.range.start < receiver_end);
let paren = toks[idx..]
    .iter()
    .take_while(|t| t.range.start < node_end)
    .find(|t| t.kind == SourceTokenKind::LeftParen);
```

See `crates/murphy-std/src/cops/layout/dot_position.rs` for full context.

### Collect heredoc body ranges (FIFO, stack-based)

```rust
fn heredoc_body_ranges(cx: &Cx<'_>) -> Vec<(u32, u32)> {
    let source = cx.source().as_bytes();
    let mut starts: Vec<u32> = Vec::new();  // stack of (HeredocStart.end + 1)
    let mut ranges: Vec<(u32, u32)> = Vec::new();

    for tok in cx.sorted_tokens() {
        match tok.kind {
            SourceTokenKind::HeredocStart => {
                starts.push(tok.range.end + 1); // +1 skips the opener's \n
            }
            SourceTokenKind::HeredocEnd => {
                if let Some(body_start) = starts.pop() {
                    // end = start of terminator line (handles squiggly indent)
                    let line_start = line_start_before(source, tok.range.start);
                    ranges.push((body_start, line_start));
                }
            }
            _ => {}
        }
    }
    ranges
}
```

See `crates/murphy-std/src/cops/layout/trailing_whitespace.rs` for
`terminator_line_start` and the full implementation.

### Check if a byte offset is inside any heredoc body

```rust
fn in_heredoc_body(offset: u32, ranges: &[(u32, u32)]) -> bool {
    ranges.iter().any(|&(start, end)| offset >= start && offset < end)
}
```

---

## What `Other` covers (partial list)

Keywords: `do`, `end`, `if`, `unless`, `while`, `until`, `case`, `when`,
`then`, `begin`, `rescue`, `ensure`, `return`, `yield`, `self`, `super`,
`nil`, `true`, `false`, `def`, `class`, `module`, `and`, `or`, `not`, …

Operators: `+`, `-`, `*`, `/`, `%`, `**`, `==`, `!=`, `<`, `>`, `<=`, `>=`,
`<=>`, `&&`, `||`, `!`, `~`, `&`, `|`, `^`, `<<`, `>>`, `=`, `+=`, `?`, `:`,
`..`, `...`, `->`, `=>`, `::`, `.`, `&.`, …

String/regexp delimiters, `[`, `]`, `{` (lambda begin / string interpolation
begin), `}` (string interpolation end), `|` (block param pipes), etc.

To match a specific keyword/operator, check the source bytes:
```rust
cx.raw_source(tok.range) == "do"
// or
&source[tok.range.start as usize..tok.range.end as usize] == b"do"
```

---

## See also

- `crates/murphy-std/src/cops/layout/dot_position.rs` — binary-search lookup,
  implicit call shape
- `crates/murphy-std/src/cops/layout/trailing_whitespace.rs` — heredoc body
  range collection (FIFO stack)
- `crates/murphy-plugin-api/src/cx.rs` — full method docs (authoritative)
- `crates/murphy-ast/src/node.rs:124` — `SourceTokenKind` enum definition
