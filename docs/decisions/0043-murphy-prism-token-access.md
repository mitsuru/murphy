# ADR 0043 — murphy-prism fork for Prism token access

- Date: 2026-05-24
- Status: Accepted
- Issue: `murphy-4pdx`
- Implements: `murphy-55k8`
- Feeds: `murphy-ji60` (`Ast::sorted_tokens()`), `murphy-gsdm`
  (`Layout/SpaceInsideParens` token rewrite)
- Related: ADR 0001 (`ruby-prism` binding selection), ADR 0037 (arena AST),
  ADR 0038 (single-surface plugin ABI)

## Context

Murphy needs a RuboCop-like `processed_source.sorted_tokens` surface for layout
cops. `Layout/SpaceInsideParens` is the first concrete case: RuboCop implements
the default `no_space` style by walking adjacent source tokens, not by scanning
AST node ranges. That distinction matters for cases such as:

```ruby
f( # comment
  1)
```

RuboCop sees the token after `(` as a comment and skips the offense. An
AST-range-only implementation tends to see raw whitespace after `(` and can
false-positive.

Murphy currently depends on upstream `ruby-prism = "=1.9.0"` through the Rust
wrapper crate. That wrapper exposes parse results, nodes, comments, diagnostics,
and locations, but it does **not** expose the lex token stream. The underlying
`ruby-prism-sys` bindings do expose Prism's C lexer callback mechanism
(`pm_lex_callback_t`, `pm_token_t`), which can collect tokens during `pm_parse`.

Upstream context checked during `murphy-55k8`:

- `ruby/prism#4027` tracked missing Rust APIs and listed token support
  (`Token`, `lex()`, `parse_lex()`) as lower priority.
- That issue was closed with maintainer guidance that the Rust crate should not
  blindly mirror the full Ruby API; consumers should present specific use cases
  for additional API.
- No open upstream PR was found that adds a Rust token API equivalent to
  `Prism.lex` / `Prism.parse_lex`.

## Decision

Maintain a small publishable crate named **`murphy-prism`** as a fork of
upstream `ruby-prism`, starting from version 1.9.0.

The fork adds only the API Murphy needs for linter/formatter infrastructure:

```rust
pub fn parse_with_tokens(source: &[u8]) -> ParseResultWithTokens<'_>;

pub struct ParseResultWithTokens<'pr> {
    // exposes parse result + source-order tokens
}

pub struct Token {
    pub fn type_(&self) -> pm_token_type_t;
    pub fn start_offset(&self) -> usize;
    pub fn end_offset(&self) -> usize;
}
```

`parse_with_tokens` installs Prism's lex callback before `pm_parse`, collects
`pm_token_t { type_, start, end }`, converts pointers to byte offsets while the
source buffer and parser are live, clears the callback after parsing, and returns
the ordinary parse result plus the collected tokens from the same parse.

This is a narrow fork, not a broad compatibility layer. We do **not** attempt to
port `Prism::Translation::Parser33` / `Parser34`, and we do not mirror Ruby's
full `Prism.lex` / `parse_lex` API unless a Murphy use case requires it.

## Import / update discipline

To keep upstream tracking reviewable, every upstream import or refresh must be
split into two conceptual commits:

1. **Vendor upstream unchanged.** Copy the upstream `ruby-prism` crate contents
   exactly as imported.
2. **Apply Murphy delta.** Rename crate metadata to `murphy-prism`, remove
   crates.io extraction metadata that is not useful in-tree, add the README, and
   apply the narrow token API patch.

The initial branch follows this shape:

- `e8ce0e2` — `Vendor ruby-prism 1.9.0`
- `af31c0a` — `Add murphy-prism token API`

When upstream updates, repeat the same pattern so the second commit remains the
only Murphy-specific diff to inspect.

## Alternatives considered

### Keep using upstream `ruby-prism` and parse twice

Murphy could keep `ruby_prism::parse(source)` for AST/comments/diagnostics and
run a second `ruby-prism-sys` parse only to collect tokens.

Rejected. It minimizes wrapper changes but doubles parse work in the hot path
and adds another Prism ownership path to maintain.

### Use `[patch.crates-io]` with a local patched `ruby-prism`

Rejected for publishable crates. A local `[patch.crates-io]` can make the
workspace compile, but crates.io consumers would resolve the unpatched upstream
`ruby-prism` crate and lose any Murphy-only API used by published Murphy crates.

### Build `sorted_tokens` from AST ranges

Rejected as the general mechanism. It is cheap in big-O terms, but ASTs do not
faithfully encode whitespace, comments, ignored newlines, heredoc token order,
and parser-internal token distinctions. Reconstructing those details drifts
toward writing a second Ruby lexer.

### Depend directly on `ruby-prism-sys` in Murphy

Deferred / rejected for this layer. Direct `sys` use would be publishable, but
it would spread Prism parser lifetime and unsafe callback handling into Murphy's
core crates. A small wrapper crate localizes that unsafe boundary and preserves a
`ruby-prism`-like surface for the rest of Murphy.

### Wait for upstream

Deferred. Upstream may accept a specific Rust token API later. Murphy should
continue with `murphy-prism` now because `sorted_tokens` blocks RuboCop-aligned
layout cops.

## Consequences

### Positive

- Tokens come from Prism's own lexer during the same parse that produces the AST.
- `Ast::sorted_tokens()` can be built from source-order parser tokens instead of
  AST-range inference.
- Layout cops can match RuboCop's adjacent-token logic for comments, newlines,
  and heredoc-adjacent cases.
- The unsafe callback handling is isolated inside `murphy-prism`.
- The crate is publishable on crates.io without relying on local patch
  overrides.

### Negative

- Murphy now owns a fork and must periodically refresh it from upstream.
- Security and parser bug fixes in upstream `ruby-prism` require explicit
  `murphy-prism` updates.
- The crate name differs from upstream, so future migration back to upstream
  requires dependency renaming.

## ABI and versioning

This ADR does **not** bump `MURPHY_PLUGIN_ABI_VERSION`.

The immediate `murphy-prism` API is not itself the plugin ABI. Follow-up work
(`murphy-ji60`) may add token slices to arena/raw-parts/plugin-facing surfaces,
but the native plugin ABI is still evolving under numeric version `1`; struct
changes alone do not justify changing the numeric ABI version without explicit
approval.

## Retirement path

If upstream `ruby-prism` accepts an equivalent Rust token API, migrate Murphy
back to upstream by:

1. Replacing `murphy-prism` dependencies with upstream `ruby-prism`.
2. Adapting `murphy-translate` token harvesting to the upstream API.
3. Removing `crates/murphy-prism` after all consumers are switched.
4. Closing this ADR with a superseding ADR or an addendum that records the
   upstream version that made the fork unnecessary.
