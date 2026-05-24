# Sorted Tokens on the Murphy Arena AST

## Context

Issue `murphy-ji60` threads Prism token information from `murphy-prism` through
`murphy-translate` into `murphy-ast`, so native cops can use a RuboCop-style
token stream instead of ad hoc source-range scanning.

The immediate downstream user is `Layout/SpaceInsideParens`
(`murphy-gsdm`), but the design should establish the general surface expected
by RuboCop-compatible layout cops.

Recent work in `murphy-55k8` added `murphy-prism::parse_with_tokens()` and a
`Token` wrapper exposing Prism token type plus byte start/end offsets. The
arena AST already owns source text, comments, and raw backing slices exposed via
`Ast::raw_parts()` and `murphy_plugin_api::CxRaw`.

## Goals

- Expose `Ast::sorted_tokens()` as a source-ordered token stream.
- Expose the same token stream to native/plugin cops through
  `murphy_plugin_api::Cx::sorted_tokens()`.
- Keep token ranges byte-offset based and stable.
- Preserve the existing `Ast::comments()` comment list.
- Update cache/raw-parts consumers so they compile and round-trip tokens.
- Do not bump `MURPHY_PLUGIN_ABI_VERSION`.
- Add focused tests for parens, comments, newlines, ignored newlines, and
  heredoc-adjacent examples.

## Non-Goals

- Full RuboCop token object parity in this step.
- Replacing `Ast::comments()` with tokens.
- Expanding every Prism token into a first-class Murphy token kind.
- Bumping the native plugin ABI version.

## Architecture

Add compact token types to `murphy-ast`:

```rust
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceToken {
    pub range: Range,
    pub kind: SourceTokenKind,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceTokenKind {
    LeftParen,
    RightParen,
    Comment,
    Newline,
    IgnoredNewline,
    HeredocStart,
    HeredocEnd,
    Other,
}
```

`Ast` owns `Vec<SourceToken>` alongside `nodes`, `node_lists`, `comments`, and
`source`. `AstBuilder` gains an append method for source tokens, and
`Ast::sorted_tokens()` returns `&[SourceToken]`.

`AstRawParts` gains `sorted_tokens: &'a [SourceToken]`. The plugin ABI raw
context appends token pointer/length fields at the end of `CxRaw`, and
`Cx::sorted_tokens()` reconstructs a borrowed slice from those fields.

The public native cop surface should be ergonomic:

```rust
for token in cx.sorted_tokens() {
    match token.kind {
        SourceTokenKind::LeftParen => { /* layout logic */ }
        _ => {}
    }
}
```

## Data Flow

`murphy-translate::translate()` switches from `prism::parse(source.as_bytes())`
to `prism::parse_with_tokens(source.as_bytes())`.

The existing AST translation continues to operate on `result.parse().node()`.
The existing comment translation continues to operate on
`result.parse().comments()`. After or before node translation, the translator
copies `result.tokens()` into the arena builder in source order.

Prism token types map into Murphy's compact token kinds:

- `PM_TOKEN_PARENTHESIS_LEFT` and
  `PM_TOKEN_PARENTHESIS_LEFT_PARENTHESES` -> `LeftParen`
- `PM_TOKEN_PARENTHESIS_RIGHT` -> `RightParen`
- `PM_TOKEN_COMMENT` -> `Comment`
- `PM_TOKEN_NEWLINE` -> `Newline`
- `PM_TOKEN_IGNORED_NEWLINE` -> `IgnoredNewline`
- `PM_TOKEN_HEREDOC_START` -> `HeredocStart`
- `PM_TOKEN_HEREDOC_END` -> `HeredocEnd`
- all other Prism tokens -> `Other`

Ranges come from `Token::start_offset()` and `Token::end_offset()` and are
stored in Murphy `Range` as half-open byte offsets. This follows the existing
project contract that offsets are byte offsets into `source.as_bytes()`.

## Compatibility

`MURPHY_PLUGIN_ABI_VERSION` remains `1`. This project treats the native plugin
ABI as still evolving under version 1, and the issue explicitly forbids a bump
without user approval.

`CxRaw` changes by appending fields:

```rust
pub sorted_tokens: *const SourceToken,
pub sorted_tokens_len: usize,
```

All host and test builders of `CxRaw` must fill these fields from
`Ast::raw_parts()`. ABI layout tests update the offsets and final size. The
loader's existing struct-size checks continue to guard against divergent builds.

The cache binary format does change because serialized arenas must include
tokens. `murphy_ast::serialize::FORMAT_VERSION` should bump, and
`Ast::to_bytes()` / `Ast::from_bytes()` should write/read the token section.
This cache format version is distinct from the native plugin ABI version.

## Error Handling

Token collection borrows Prism internals only during parse. The translator
copies token type and offsets into owned arena data, so no Prism pointer or
borrowed token object escapes `murphy-translate`.

If a token offset does not fit in `u32`, the implementation should follow the
same offset-domain assumption as arena ranges. The core parser already rejects
oversized sources before normal arena parsing; translation-level conversions
may use checked conversion or debug assertions consistent with nearby range
conversion code.

Malformed serialized token data should fail during `Ast::from_bytes()` with the
same style of structured `SerError` used by existing arena validation. At
minimum, bad token-kind discriminants must be rejected.

## Testing

`murphy-prism` tests should continue proving that `parse_with_tokens()` returns
source-ordered tokens and includes heredoc tokens. Add or adjust focused cases
only if the wrapper contract is not already covered.

`murphy-translate` tests should assert that `translate()` fills
`ast.sorted_tokens()` for:

- parentheses in calls and grouping
- inline comments
- normal newlines
- ignored newlines
- heredoc-adjacent tokens

These tests should validate both token kind order and byte ranges, preferably
by checking `ast.raw_source(token.range)` for representative tokens.

`murphy-ast` tests should cover:

- `AstBuilder` stores source tokens
- `Ast::sorted_tokens()` returns the stored slice
- `Ast::raw_parts()` includes the same token slice
- serialization round-trips source tokens
- bad serialized token discriminants are rejected

`murphy-plugin-api` and `murphy-core` tests should cover:

- `Cx::sorted_tokens()` matches `Ast::sorted_tokens()`
- all `CxRaw` builders populate token pointer/length fields
- ABI layout offset/size assertions reflect the appended fields
- a native cop can compile against and read the token stream through `Cx`

## Implementation Notes

Keep `SourceTokenKind` intentionally small. Future cops can promote additional
Prism token types from `Other` when they have a concrete need.

Do not remove the comment list. RuboCop-compatible token scanning benefits from
comment tokens, while existing Murphy consumers still use `Ast::comments()` for
comment-specific behavior.

Do not change `MURPHY_PLUGIN_ABI_VERSION` during this work.
