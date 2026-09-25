# RuboCop Hook Dispatch Mapping

Murphy models RuboCop `on_<node_type>` hooks with restricted Prism node-kind
dispatch. The core visitor walks the Prism tree once and dispatches only cops
that registered interest in the current node kind.

## Lifecycle Hooks

RuboCop lifecycle hooks map to Murphy phases as follows:

- `on_new_investigation`: `Cop::inspect_file` for existing native Rust cops, or a future structured file-metadata hook for plugin packs.
- `on_investigation_end`: `Cop::after_file`, called after AST traversal for per-file aggregation.
- `on_other_file`: not applicable to Murphy's per-file Ruby parser path unless non-Ruby file analysis is added.

## Node Hooks

RuboCop traversal hooks such as `on_class`, `on_def`, `on_hash`, `on_str`, and
`on_send` map to Murphy node dispatch by node kind:

- `on_class` -> `class`
- `on_module` -> `module`
- `on_def` -> `def` (filter with `Cx::is_plain_def` to exclude singleton defs)
- `on_defs` -> `def` + `defs`, filtered with `Cx::is_defs` (all singleton defs fold into `Def` with a receiver; see derived table below)
- `on_send` -> `send` (use `methods = [...]` for `restrict_on_send`; pattern group `call` = `send` + `csend`)
- `on_csend` -> `csend` (precise; filter helper `Cx::is_csend` is an alias of `is_safe_navigation`)
- `on_block` -> `block`
- `on_hash` -> `hash`
- `on_pair` -> `assoc`
- `on_str` -> `string`
- `on_sym` -> `symbol`
- `on_int` -> `integer`
- `on_float` -> `float`
- `on_array` -> `array`
- `on_if` -> `if`
- `on_case` -> `case`
- `on_return` -> `return`

Murphy exposes Prism names as byte strings through `NodeDispatchRestriction` and
`MurphyNodeDispatchV1.node_kind`. The complete supported set is the complete
`ruby_prism::Node` enum surface, normalized by removing the `Node` suffix and
converting CamelCase to snake_case. Examples include `constant_read`,
`local_variable_write`, `keyword_hash`, `regular_expression`, `rescue`,
`while`, `until`, and `yield`.

RuboCop hook names are accepted as aliases. For example, `on_str` maps to
`string`, `on_sym` maps to `symbol`, and the pattern group `call` expands to
`send` + `csend` (native `#[on_node]` dispatch keeps `send` / `csend`
separate for precision).

## Derived hook exact semantics (murphy-6ln)

Seven RuboCop traversal hooks need folding notes because Prism collapses the
parser-gem spelling or shares one structural node. The canonical metadata is
`murphy_ast::DERIVED_HOOK_COMPATIBILITY`; each row names the structural
`#[on_node(kind = "...")]` subscription(s) and the `Cx` helper that recovers
exact semantics without reimplementing the check:

| RuboCop hook | Subscribe to | Filter with | Notes |
|---|---|---|---|
| `on_if_guard` | `kind = "if_guard"` | `Cx::is_if_guard` | distinct `IfGuard` for `in <pat> if <cond>`; precise via kind |
| `on_unless_guard` | `kind = "unless_guard"` | `Cx::is_unless_guard` | distinct `UnlessGuard` for `in <pat> unless <cond>`; precise via kind |
| `on_while_post` | `kind = "while"` | `Cx::is_while_post` | folds into `While` with `post: true` (`begin...end while`) |
| `on_until_post` | `kind = "until"` | `Cx::is_until_post` | folds into `Until` with `post: true` (`begin...end until`) |
| `on_empty_else` | `kind = "case_match"` | `Cx::has_empty_else` | parser-gem `empty_else` only for `case/in` empty else; token scan distinguishes absent `else` from empty `else` (`else nil` has a body, not empty) |
| `on_csend` | `kind = "csend"` | `Cx::is_csend` | distinct `Csend` for `&.`; precise via kind |
| `on_defs` | `kind = "def"` + `kind = "defs"` | `Cx::is_defs` | all singleton defs (`def self.foo`, `def obj.foo`, `def Foo.bar`) fold into `Def` with a receiver; `Defs` is parser-only |

Example (post-condition loop port):

```rust
#[on_node(kind = "while")]
fn check_while(&self, node: NodeId, cx: &Cx<'_>) {
    if !cx.is_while_post(node) {
        return; // RuboCop `on_while_post` fires only here
    }
    // ... derived-case logic
}
```

## Plugin ABI

Native packs can register `MurphyNodeDispatchV1` entries. Murphy calls
`run_node_dispatch` with `MurphyNodeContext`, which includes file/source/config,
the node kind, dispatch ID, and byte range. Pointers in the context remain valid
only for the callback duration.

`MURPHY_PLUGIN_ABI_VERSION` remains `1`; do not renumber it without explicit
approval.

## File-Stage Policy

Use node dispatch or shared file metadata for AST-expressible cop semantics.
Keep `run_file` for raw text/layout/prelude work only, such as whitespace or
single shared magic-comment/prelude parsing.
