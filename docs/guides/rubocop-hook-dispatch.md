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
- `on_def` -> `def`
- `on_defs` -> `singleton_class` plus `def`/method metadata as needed
- `on_send` -> existing call dispatch for method-name-restricted cops, or node kind `call` for unrestricted call-node cops
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
`string`, `on_sym` maps to `symbol`, `on_send` and `on_csend` map to `call`,
and assignment hooks such as `on_and_asgn`/`on_or_asgn` expand to the matching
Prism `*_and_write` / `*_or_write` node kinds. Derived hooks whose exact parser
semantics depend on AST attributes, such as `on_if_guard`, `on_until_post`, and
`on_empty_else`, currently dispatch on their closest Prism structural node kind;
cop implementations should inspect the node details if they need to distinguish
the derived case.

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
