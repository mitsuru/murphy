# Murphy AST S-expression CLI Design

Date: 2026-05-20
Status: approved design draft
Scope: `murphy-j1p` (`A6b.1`), precursor to `murphy-ary`

## Context

RuboCop core cop migration needs a way to inspect how Murphy sees Ruby code before
we finalize native plugin node handles, matcher descriptors, and any future
NodePattern compiler. JSON is useful for machines, but Rubyists are more likely
to reason in Parser/RuboCop-style S-expressions and NodePattern shapes.

Murphy already parses Ruby through Prism and exposes byte ranges internally. The
missing tool is a small CLI surface that turns a Ruby file or fragment into a
stable, Ruby-facing AST representation.

## Goal

Add `murphy ast --format sexp` as a developer tool. It should print a
Parser/RuboCop-inspired S-expression for Ruby source read from a file or stdin.
The first purpose is fixture authoring and matcher design, not a perfect Parser
gem compatibility layer.

## Non-Goals

- No promise of exact Parser gem AST compatibility.
- No JSON output in the first pass.
- No Rust-ish fixture DSL in the first pass.
- No NodePattern evaluation in this CLI.
- No autocorrect, linting, or cop execution.

## CLI Shape

Supported invocation:

```bash
murphy ast --format sexp path/to/file.rb
printf 'x == nil' | murphy ast --format sexp -
```

Rules:

- `ast` is a new top-level subcommand.
- `--format sexp` is required for the first implementation, so future formats can
  be added without changing semantics.
- Exactly one path argument is accepted.
- `-` means read UTF-8 source from stdin.
- Output goes to stdout only on success.
- Diagnostics go to stderr.

## Output Format

Output is Ruby S-expression text using `s(...)` calls:

```ruby
s(:send,
  s(:lvar, :x),
  :==,
  s(:nil))
```

Formatting is stable and deterministic:

- one expression per input
- two-space indentation
- symbols for node kinds and method names when they are valid Ruby symbol names
- quoted strings for string literal values
- no byte ranges in the default `sexp` format

Byte ranges are intentionally omitted from default `sexp` because the primary
audience is Rubyists comparing shape against RuboCop/Parser expectations. A later
`--with-ranges` flag or `--format sexp-ranges` can add byte ranges without
polluting the default shape.

## Initial Node Mapping

The initial mapping should cover forms needed for `Style/NilComparison` and early
RuboCop core matcher work:

- receiver-less local variable call: `x` -> `s(:lvar, :x)` when Prism represents
  it as a receiver-less call with no arguments
- method call: `obj.foo(1)` -> `s(:send, <receiver>, :foo, s(:int, 1))`
- receiver-less method call: `foo(1)` -> `s(:send, nil, :foo, s(:int, 1))`
- operator call: `x == nil` -> `s(:send, s(:lvar, :x), :==, s(:nil))`
- nil literal: `nil` -> `s(:nil)`
- booleans: `true` / `false` -> `s(:true)` / `s(:false)`
- integer literal: `1` -> `s(:int, 1)`
- string literal: `'x'` or `"x"` -> `s(:str, "x")`
- symbol literal: `:x` -> `s(:sym, :x)`
- array: `[a, nil]` -> `s(:array, <items...>)`
- hash: `{a: 1}` -> `s(:hash, <pairs...>)`
- if/unless/case/when/return/class/module/def/block are out of the first pass
  unless already needed by the test fixtures. If encountered before explicit
  support lands, they render as `s(:unknown, "PrismKind", ...)` with children
  preserved.

Unknown nodes must not crash dumping. They should render as:

```ruby
s(:unknown, "PrismKind", ...children)
```

This keeps the CLI useful while node coverage grows.

## Parser Compatibility Posture

The output is Parser/RuboCop-inspired, not guaranteed byte-for-byte equivalent to
Parser gem. The docs and tests should describe it as `sexp`, not `parser` or
`rubocop`, to avoid overpromising compatibility.

Where Murphy can cheaply match Parser conventions, it should. Where Prism shape
differs, Murphy should favor consistency and clear output over exact emulation.

## Error Handling

- Missing file or unreadable input: setup error, exit `2`, stderr diagnostic, no
  stdout.
- Bad usage or unknown format: setup error, exit `2`, stderr diagnostic, no
  stdout.
- Parse error: exit `1`, stderr diagnostic, no stdout. This differs from `lint`,
  where parse errors become `Murphy/Syntax` offenses, because `ast` is an
  inspection command and has no offense JSON contract.
- Broken stdout pipe exits `0`, matching existing CLI behavior.

## Testing

- Unit-test S-expression formatting helpers for symbol/string escaping.
- Unit-test AST-to-sexp conversion for:
  - `x == nil`
  - `nil == x`
  - `x != nil`
  - `obj.foo(1)`
  - `foo(1)`
  - string/symbol/integer/nil/true/false literals
- CLI-test file input and stdin input.
- CLI-test unknown format and missing file as exit `2` with empty stdout.
- CLI-test parse error as exit `1` with empty stdout.

## Success Criteria

- `printf 'x == nil' | murphy ast --format sexp -` prints a stable
  S-expression equivalent to `s(:send, s(:lvar, :x), :==, s(:nil))`.
- `printf 'nil == x' | murphy ast --format sexp -` prints the nil receiver shape.
- Output is stable enough to paste into matcher/proc-macro design discussions and
  test fixtures.
- Existing `murphy lint`, `migrate`, and `lsp` behavior remains unchanged.

## Follow-Ups

- Add `--with-ranges` or `--format sexp-ranges` for byte-offset debugging.
- Add `--format rust` for proc-macro fixture generation if Rust-side test needs
  outgrow sexp.
- Feed these fixtures into `murphy_node_pattern!` design and matcher descriptor
  tests.
