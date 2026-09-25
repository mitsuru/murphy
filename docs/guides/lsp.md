# LSP server (`murphy lsp`)

`murphy lsp` is a stdio JSON-RPC language server over the native lint
pipeline. Phase 7 (murphy-bn3.3, ADR 0022) shipped diagnostics publish and
document lifecycle; B2 (murphy-fmw.2.2) adds quick-fix code actions built
from the existing autocorrect edits plus range-selected re-lint.

## Start

```bash
murphy lsp
```

The server speaks LSP over stdin/stdout (`Content-Length` framing).
`initialize` advertises `textDocumentSync: 1` (full-text sync) and
`codeActionProvider: true`. Only `file://` URIs are accepted;
other schemes answer `InvalidParams` (`-32602`).

## Quick fix (`textDocument/codeAction`)

Each fixable offense — one whose `autocorrect.edits` are non-empty —
maps to one `quickfix` action:

```json
{
  "title": "Murphy: fix Layout/TrailingWhitespace",
  "kind": "quickfix",
  "diagnostics": [ { "range": {}, "code": "Layout/TrailingWhitespace" } ],
  "edit": { "changes": { "file:///app.rb": [
    { "range": { "start": {"line": 0, "character": 5},
                 "end": {"line": 0, "character": 7} },
      "newText": "" }
  ] } }
}
```

Behavior:

- The handler re-lints the current open-document text, so actions never
  go stale after edits.
- With non-empty `context.diagnostics`, only offenses matching a context
  diagnostic (same cop `code` + intersecting range) get actions.
- Otherwise every fixable offense intersecting the requested `range`
  gets an action. A cursor (empty range) parked on or at the edge of an
  offense still offers its fix.
- `context.only` without `quickfix` yields `[]` (B2 serves the
  `quickfix` kind only).
- Offenses without a fix or without a location never produce actions.

## Range-selected re-lint (`murphy/rangeDiagnostics`)

Custom request: re-lint the open document, return only the diagnostics
intersecting `params.range`:

```json
{ "jsonrpc": "2.0", "id": 2, "method": "murphy/rangeDiagnostics",
  "params": { "textDocument": {"uri": "file:///app.rb"},
              "range": { "start": {"line": 1, "character": 0},
                         "end": {"line": 2, "character": 0} } } }
```

The result is a diagnostics array (same shape as
`textDocument/publishDiagnostics`). A never-opened document answers
`InvalidParams` (`"document not open"`), distinct from a clean range
which answers `[]`.

## Editor setup

Neovim (`nvim-lspconfig` style):

```lua
vim.lsp.start({
  name = 'murphy',
  cmd = { 'murphy', 'lsp' },
  root_dir = vim.fs.root(0, { 'murphy.toml', '.git' }),
})
```

VS Code (`languageClient`): server command `murphy lsp`,
`documentSelector = [{ scheme = 'file', language = 'ruby' }]`.

## Known limits / follow-ups

- Positions count bytes per line, not UTF-16 code units: multibyte
  text can misalign columns in UTF-16 clients. Diagnostics and edits
  share the same conversion, so they stay mutually consistent.
- One action per offense; no combined "fix all in file" action yet.
- `murphy/rangeDiagnostics` is a Murphy-specific extension, not part
  of the LSP standard.
