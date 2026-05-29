# RuboCop `def_node_matcher` Pattern Compatibility — Gap Survey

beads: `murphy-xvjv` (epic) / `murphy-707j` (this survey).
Date: 2026-05-29.
Test harness: `crates/murphy-pattern/tests/rubocop_pattern_compat.rs`.

## Goal

Quantify and categorise the work needed to land RuboCop `def_node_matcher`
strings — `def_node_matcher :foo, '<pattern>'` — into Murphy's `def_node_matcher!`
verbatim, with no syntactic massaging required from the cop author.

## Approach

A dedicated integration test (`rubocop_pattern_compat::def_node_matcher_compat_survey`)
runs `murphy_pattern::compile()` against 56 representative patterns split into
three buckets, prints a categorised summary to stderr, and only fails if the
self-baseline regresses.

- **MurphyBaseline (4 cases)** — patterns lifted from `crates/murphy-std/src/cops/**`.
  Production regression guard.
- **RuboCopCanon (21 cases)** — patterns transcribed from major RuboCop /
  RuboCop-RSpec / RuboCop-Rails cops (`Style/Lambda`, `Style/SafeNavigation`,
  `Lint/UselessAssignment`, `Style/RescueModifier`, `Style/CaseLikeIf`,
  `Style/RedundantReturn`, `Style/StringConcatenation`, `Style/Encoding`,
  `Style/IfUnlessModifier`, `Style/ClassAndModuleChildren`,
  `Style/EmptyMethod`, `Lint/CaseEquality`, `Style/EachWithObject`,
  `Style/NilComparison`, `Style/NonNilCheck`, etc.). The paste-and-run target.
- **GapProbe (31 cases)** — minimal one-line patterns that touch a single
  suspected missing node kind. Designed to fail today and make the failure
  mode explicit.

## Result

```
total cases : 56
compile ok  : 25
compile err : 31

By category:
  MurphyBaseline   ok=  4   fail=  0
  RuboCopCanon     ok= 21   fail=  0    ← target audience: 100% green today
  GapProbe         ok=  0   fail= 31
```

**Headline**: every RuboCopCanon pattern compiles today. The "paste a
def_node_matcher pattern into `def_node_matcher!`" goal is **already met for the
mainstream RuboCop cop catalogue**. What remains is the long tail of
specialised node kinds.

## Failure inventory

All 31 GapProbe failures share one error bucket — `unknown node type 'xxx'`
(one is a related `unexpected character` for `back_ref :$~`'s `:$` symbol).
This means the parser is fine; the only thing missing is a `NodeKindTag`
variant the resolver can look up. Subject-side support (extending
`murphy-ast::NodeKind` + the `murphy-translate` AST builder) is a separate,
larger piece of work and is **not** required to make `compile()` succeed.

### HIGH-priority gaps (frequent in mainstream RuboCop cops)

| Node kind | Why it matters |
|---|---|
| `for` | classic `for x in y` loop; appears in `Style/For`, `Style/ForLoop` |
| `lambda` | short `-> { ... }` form; `Style/Lambda` and friends |
| `defs` | singleton method (`def self.foo`); `Style/ClassMethodsDefinitions` |
| `index` / `indexasgn` | modern split of `foo[i]` and `foo[i] = x` |
| `kwbegin` | `begin..end` keyword form; `Style/RedundantBegin` |
| `cbase` | top-level const root (`::Foo`); namespace-aware cops |
| `regopt` | regex options node; any regex-introspection cop |
| `rational` / `complex` | `1r` / `1i` numeric literals |
| `not` | `not foo` keyword (separate from `!foo`); `Style/Not` |
| `retry` / `redo` | rare but exposed by `Style/EmptyElse` etc. |
| `numblock` | numbered-param block (`{ _1 + _2 }`); `Style/NumberedParameters*` |
| `procarg0` | single-arg proc; `Style/SingleLineMethods` etc. |
| `forward_args` / `forwarded_args` | `...` forwarding (Ruby 2.7+) |

### MID-priority gaps (Ruby 3 pattern matching)

| Node kind | Why it matters |
|---|---|
| `case_match` | `case ... in ...` |
| `in_pattern` | `in <pattern>` arm |
| `array_pattern` | `in [a, b, c]` |
| `hash_pattern` | `in {a:, b:}` |
| `match_var` | pattern-binding variable |
| `itblock` | `it { ... }` block (Ruby 3.4) — used in RSpec-adjacent cops |

### LOW-priority gaps (niche / legacy)

| Node kind | Why it's low |
|---|---|
| `alias` / `undef` | only `Style/Alias` etc. — rare overall |
| `preexe` / `postexe` | `BEGIN { }` / `END { }`; almost never linted |
| `back_ref` / `nth_ref` | `$~`, `$&`, `$1`, `$2`; `Style/SpecialGlobalVars` only |
| `shadowarg` | lambda shadow var (`; x`); cosmetic |
| `kwnilarg` / `blocknilarg` | `**nil`, `&nil`; rarely referenced |

## What "fixing" a row means

Two layers of work, separable:

1. **`NodeKindTag` extension** (`crates/murphy-ast/src/node.rs` + the resolver in
   `crates/murphy-pattern/src/parser.rs`). Adds the tag so the pattern compiles.
   Required for the paste-and-run goal.
2. **`NodeKind` + `murphy-translate` AST builder extension**. Lets actual Ruby
   source produce the new kind in the subject AST so matches can succeed at
   runtime. Required for the cop to fire on real code.

Layer 1 closes the parse-time gap surveyed here. Layer 2 closes the runtime
gap. The survey deliberately measures only layer 1 because layer 2 is much
larger work and is best driven by an actual cop port, not a synthetic probe.

## Recommended next steps

1. Land a HIGH-priority `NodeKindTag` extension PR that adds the 13 HIGH tags
   above (parser-only — no subject-side wiring yet) and bumps every RuboCop
   cop into "parses verbatim" territory.
2. File MID + LOW as follow-up issues against `murphy-xvjv`, lower priority.
3. Pick a real RuboCop cop that uses one of the HIGH tags (e.g. `Style/Lambda`
   uses `(lambda)`) and port it end-to-end; this exercises layer 2 in a
   focused, value-producing way instead of as bulk infrastructure work.

The survey test itself stays in the repo as a regression guard: if a future
`NodeKindTag` extension breaks a MurphyBaseline pattern, the test fails.
