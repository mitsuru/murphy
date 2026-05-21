# ADR 0034: No synthesized node dispatch (percent_literal / heredoc / interpolation)

**Status**: Accepted (2026-05-22)
**Issue**: murphy-9cr.4 (parent epic: murphy-9cr)
**Related**: ADR 0033 (plugin ABI v1 option metadata), murphy-9cr.5 (Tier 1 typed wrappers)

## Context

RuboCop exposes three "synthesized" cop hooks that are not direct Parser AST node kinds:

| Hook | RuboCop hit count (murphy-9cr.1) | Mechanism |
|---|---|---|
| `on_percent_literal` | 8 | `Cop::Util` walks `array` AST nodes whose `loc.begin` starts with `%w` / `%W` / `%i` / `%I` and re-dispatches as a synthetic event. |
| `on_heredoc` | 8 | Similar walk over `str` / `dstr` nodes whose `loc.begin` is a heredoc opening token. |
| `on_interpolation` | 6 | Walk over `begin` (Parser-side `EmbeddedStatementsNode`) when the parent is a `dstr` / `dsym` / `regexp`. |

Murphy parses with ruby-prism, which has no equivalent variant for any of these — `%w[...]` is just an `ArrayNode` whose `opening_loc` is `"%w["`, a heredoc is a `StringNode` (or `InterpolatedStringNode`) whose `opening_loc` is `"<<-FOO"` / `"<<~FOO"`, and interpolation is `EmbeddedStatementsNode` regardless of context. The three RuboCop hooks have no corresponding `kinds::*` constant and were deliberately excluded from the Tier 1 typed-wrapper set in murphy-9cr.1.

The remaining question for the plugin API is whether to recreate the synthetic dispatch in Murphy or expose the information differently.

## Decision

**Do not provide synthesized dispatch hooks.** All three cases are handled through accessors on the existing Tier 1 typed wrappers (murphy-9cr.5):

| Concern | Surface (Tier 1) |
|---|---|
| Heredoc string | `StringNode::is_heredoc()` (true iff `opening_loc()` starts with `<<`) plus `StringNode::heredoc_indent()` returning `HeredocIndent::{None, Dedent, Squiggly}`. |
| Heredoc with interpolation | `InterpolatedStringNode::is_heredoc()` / `InterpolatedStringNode::heredoc_indent()`, defined the same way. |
| Percent-literal array | `ArrayNode::percent_kind()` returning `Option<PercentKind>` (`{LowerW, UpperW, LowerI, UpperI}`), `None` for `[ ... ]`. |
| Interpolation context | No special accessor. Cops that need the parent kind use the generic node ancestor walk — out of scope for this ADR and resolved when the wrapper crate exposes navigation helpers (murphy-9cr.5 / `murphy-plugin-api`). |

Author idiom in `murphy-9cr.5` typed wrappers:

```rust
#[murphy::cop]
impl PercentWWithSpaces {
    #[on_node(kind = kinds::ARRAY_NODE)]
    fn check(&self, array: &ArrayNode, ctx: &mut Context<'_>) {
        let Some(kind) = array.percent_kind() else { return };
        // ... percent-array-specific logic
    }
}
```

## Reasons

1. **Murphy is not a RuboCop port.** Drop-in hook compatibility is not a goal of the plugin API; tightening the dispatcher surface is.
2. **All three signals live on a single `opening_loc` byte string** that ruby-prism already provides. There is no information-content gain from re-dispatching, only ergonomic re-shaping.
3. **Core dispatcher stays simple.** Adding synthesized hooks would require: filter logic in `crates/murphy-core/src/cop.rs`, an ABI/wire surface (which dispatched hook ID a cop registers under, e.g. `b"percent_literal"`), proc-macro recognition of `#[on_percent_literal]`, and an implicit traversal contract. Accessor-only keeps the table at one column.
4. **Tier 1 wrapper coverage already plans for the parent kinds** (`StringNode`, `InterpolatedStringNode`, `ArrayNode`) — see the parent epic's DESIGN field. The cost is one accessor per concern, not a new dispatch axis.
5. **The synthesized hooks were rare even in RuboCop** (8 + 8 + 6 = 22 implementations out of 817 AST dispatch hooks, ~2.7%). The cost / benefit of paying complexity for them is poor.

## Alternatives considered

- **Provide all three as synthesized hooks.** Rejected for the dispatcher-complexity and proc-macro-surface reasons above.
- **Half-and-half: accessor for heredoc/percent, synthesized hook only for interpolation.** Rejected: interpolation's "context dependence" is solved by the ancestor walk that Tier 1 wrappers will already need for other reasons, so adding a dedicated hook for it does not pay for the asymmetry of the API.
- **No accessor at all (force ancestor walk + opening_loc parsing in user code).** Rejected: surface too minimal, RuboCop-porting cost too high.

## Consequences

- **murphy-9cr.5 owns the accessor additions** (`StringNode::is_heredoc / heredoc_indent`, `InterpolatedStringNode::is_heredoc / heredoc_indent`, `ArrayNode::percent_kind`). The HeredocIndent / PercentKind enums live in `murphy-plugin-api` alongside the wrappers.
- **No core dispatcher changes** flow from this decision; the ABI surface stabilised in ADR 0033 covers everything needed.
- **Porting-guide implication**: when documenting RuboCop migration, point readers at the accessor idiom rather than at a synthesised `#[on_percent_literal]`.
- **Future reopen condition**: if a real Murphy plugin needs to filter many cops on the same `percent_kind` and the per-cop early-return cost becomes a perf hotspot, revisit whether the dispatcher itself should short-circuit. Not expected at v1.

## Implementation status

- Decision recorded; this ADR is the deliverable.
- Accessor implementations land with the Tier 1 wrappers in murphy-9cr.5.
- No code change in this issue.
