# RangeHelp Source Range Helpers Design

## Goal

Add RuboCop `RangeHelp`-style source range helpers to Murphy's plugin authoring surface so layout cops can expand byte ranges by source lines, surrounding whitespace, and associated comments without duplicating raw-source logic.

## Design

Implement the helpers on `murphy_plugin_api::Cx`, because `Cx` already owns access to the source bytes, source tokens, and parser-provided comments. The implementation mirrors RuboCop's `RuboCop::Cop::RangeHelp`: it scans `processed_source.buffer.source` equivalents for whitespace and line boundaries, and uses parsed comments rather than string-searching for `#`.

## API

- `RangeSide`: `Left`, `Right`, `Both` controls expansion direction.
- `SpaceRangeOptions`: controls newline, general whitespace, and backslash-continuation expansion.
- `Cx::range_by_whole_lines(range, include_final_newline) -> Range` expands to whole source lines and clamps to file bounds.
- `Cx::range_with_surrounding_space(range, options) -> Range` expands through spaces/tabs and optional continuation/newline/whitespace bytes.
- `Cx::range_with_comments(node) -> Range` unions a node's source range with immediately preceding own-line comments from `Cx::comments()`.
- `Cx::range_with_comments_and_lines(node) -> Range` composes comments with whole-line expansion including the final newline.

## Scope

Heredoc token support is limited to preserving correct source order and range arithmetic over existing `HeredocStart`/`HeredocEnd` tokens. Precise RuboCop comment association for all AST shapes remains out of scope until Murphy has an `ast_with_comments` equivalent.

## Testing

Unit tests in `crates/murphy-plugin-api/src/cx.rs` cover whole-line expansion, whitespace expansion defaults and options, comment union, final newline inclusion, and heredoc line expansion using real translated Ruby source.
