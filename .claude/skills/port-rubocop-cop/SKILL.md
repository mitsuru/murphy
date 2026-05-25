---
name: port-rubocop-cop
description: This skill should be used when the user asks to "port a RuboCop cop", "move RuboCop's X cop to Murphy", "add an RSpec/Rails/Style/Layout cop to murphy-*", "RuboCop の cop を移植", "X cop を移植して", or otherwise wants to translate a RuboCop (or RuboCop-RSpec / RuboCop-Rails) rule into a Murphy plugin cop using the single-surface ABI from `docs/guides/plugin-cop-infrastructure.md`. Runs the full porting workflow end-to-end: read the RuboCop source, implement against the guide (target pack, `#[on_node]` dispatch including `methods = […]` and `node_pattern!`, `#[derive(CopOptions)]`, `register_cops!`, `test::<T>()` tester-builder tests covering offense / correction / no-correction shapes), analyse the gap to the RuboCop original, escalate gaps the guide cannot cover (AST mismatches, missing hooks, ABI extensions), iterate through `roborev-refine` review until passing, then open a PR and merge after CI is green.
---

# Porting a RuboCop cop to Murphy

This skill drives the full workflow for translating one RuboCop rule into a
Murphy plugin cop authored against the single-surface plugin ABI documented
in `docs/guides/plugin-cop-infrastructure.md` (the "infra guide"). It is the
master procedure; the infra guide is the API reference it points back into.

Treat the infra guide as load-bearing — re-read the relevant section instead
of guessing at the API surface. Hand-copying type names from older PRs is a
common source of build failures because the macro surface has churned.

## When to use this skill

Trigger on requests that name a RuboCop (or RuboCop-RSpec / RuboCop-Rails)
cop and ask to bring it into Murphy. Typical phrasings:

- "Port `RSpec/EmptyExampleGroup` to murphy-rspec."
- "Add `Lint/UselessAssignment` as a Murphy std cop."
- "`Style/RedundantReturn` を Murphy に移植して — autocorrect 込みで。"
- "Translate this rubocop-rails rule into murphy-rails."

If the request is "write a brand-new cop with no RuboCop ancestor", most of
the same steps apply — Phase 1 (read the RuboCop source) is optional;
start from Phase 2.

## Workflow

Each cop port goes through six phases. Do not skip them: the gap analysis
and review loop catch the kinds of regressions that block merge later.

1. **Phase 1 — read the RuboCop source.** Capture the exact spec the port
   must mirror.
2. **Phase 2 — implement to the guide.** Pick the target pack, decide the
   dispatch shape, scaffold the file, register it, write tests, verify
   the build.
3. **Phase 3 — analyse the gap to RuboCop.** Diff the implementation
   against the RuboCop original; surface every shape / option /
   autocorrect / test case the port does not cover.
4. **Phase 4 — escalate gaps the guide cannot cover.** Anything that
   needs an ABI extension, a new `Cx` method, a new dispatch kind, or a
   `node_pattern!` grammar extension goes to the user, not into a
   silent workaround.
5. **Phase 5 — pass `roborev-refine` review.** Iterate fix → re-review
   until the daemon reports passing or hits its iteration cap.
6. **Phase 6 — open the PR, then merge after CI passes.** Push from the
   worktree, open a PR with summary and test plan, wait for CI green,
   merge with the repo's merge-commit convention.

Each phase below points to the canonical reference in the infra guide or
the in-tree example cop that demonstrates the pattern.

## Phase 1: read the RuboCop source

Before writing any Rust, read the RuboCop (or rubocop-rspec /
rubocop-rails) implementation that the port should mirror, plus its spec
file. Capture the following facts — Phase 3 (gap analysis) will diff the
implementation against this list, so keep it explicit:

- **Dispatch hooks.** Which `on_<kind>` / `on_send` / `on_block` / … the
  cop subscribes to. Note `on_csend` separately — RuboCop treats
  safe-navigation `a&.b` as a distinct hook.
- **`RESTRICT_ON_SEND`.** The method-name allow-list (translates
  directly into Murphy's `methods = [...]`).
- **`def_node_matcher` / `def_node_search` patterns.** These translate
  to `node_pattern!` — log every one verbatim so the v1 grammar
  subset can be compared against (`references/node-pattern.md`).
- **`cop_config` keys, defaults, and types.** Each will become a field
  on the `#[derive(CopOptions)]` struct. Note `SupportedStyles` /
  enum-like keys — Murphy v1 renders these as `String`.
- **Autocorrect.** Whether the cop ships `autocorrect` /
  `extend AutoCorrector`, and whether RuboCop marks it `Safe: false`.
  The Murphy port should match RuboCop's safety stance.
- **`Include` / `Exclude` patterns.** RuboCop's `RSpec/*` cops fire only
  on `*_spec.rb`; Rails cops on `app/**/*.rb`. Murphy has no per-cop
  file-pattern gating yet (Phase 4 candidate if the cop strictly needs
  it).
- **Offense message text.** Murphy aims for parity unless the RuboCop
  text is misleading.
- **Spec coverage.** Walk the cop's `spec_helper`-style RSpec file and
  list every example — positive flag, negative case, false-positive
  guard, autocorrect input/output. Phase 2.5 (tests) will mirror this
  list.

For greenfield cops (no RuboCop ancestor), Phase 1 reduces to writing a
short spec by hand: matched shapes, option keys, autocorrect stance,
test cases.

## Phase 2: implement to the guide

The implementation phase has six sub-steps. Each points to the infra
guide section and to an in-tree example cop.

### Phase 2.1: identify the target pack and the Murphy cop name

Two facts to nail down before scaffolding:

1. **The target Murphy pack.** Map the RuboCop namespace to a Murphy
   crate:
   - `Murphy/*`, `Lint/*`, `Style/*`, `Layout/*` → `crates/murphy-std/`
     (static pack, linked into `murphy-cli`).
   - `RSpec/*` → `crates/murphy-rspec/` (dynamic pack).
   - `Rails/*` → `crates/murphy-rails/` (dynamic pack).
   - A new third-party namespace → a new dynamic pack scaffolded off
     `crates/murphy-example-pack/` — see infra guide §9.
2. **The Murphy cop name.** Always `"Pack/CopName"`, matching the
   directory layout and `murphy.toml` key. The name passed to
   `#[cop(name = …)]`, the file path under `src/cops/<namespace>/`, and
   user-facing config must all agree.

If a similar cop already exists in the target pack, read it first — pack
conventions (helpers, `is_example_call`-style gates, doc-comment style)
should be reused rather than reinvented.

### Phase 2.2: decide the dispatch shape

This is the single most important design choice. The infra guide §3
lists all valid forms.

- **One node kind, no method-name filter** — use
  `#[on_node(kind = "block")]` (or `class`, `def`, `hash`, `if`, …). See
  `RSpec/MultipleExpectations` (`crates/murphy-rspec/src/cops/rspec/multiple_expectations.rs`).
- **`Send` with a known method allow-list** — use
  `#[on_node(kind = "send", methods = ["describe", "context"])]`. The
  host pre-filters by method symbol so the cop body never runs for
  unrelated `Send` nodes. See `RSpec/DescribeClass`. The
  `methods = […]` shape is mandatory whenever the allow-list is a fixed
  literal set — it is faster *and* clearer than re-checking the symbol
  inside the body. **Never use `methods = []`** (rejected at parse time
  as a typo) and **never combine `methods` with a non-`send` kind**
  (also rejected).
- **`Send` with a runtime-computed method name** — use bare
  `#[on_node(kind = "send")]` and check `cx.symbol_str(method)` inside
  the body. Only reach for this when the predicate genuinely cannot be
  expressed as a static literal set (configuration-dependent, dynamic).
- **Whole-file scan / raw-source cop** — use `#[on_new_investigation]`.
  Mirrors RuboCop's `on_new_investigation` and is dispatched once per
  file with `node == cx.root()`. Use this for layout / trailing-whitespace
  style cops that work on source text rather than a specific node kind.
- **Shape-based matching across multiple kinds** — declare a
  `node_pattern!(...)` matcher at module top level and call it from the
  dispatch body. See `references/node-pattern.md` for the v1 grammar
  subset, or infra guide §3 ("Reusable matchers").

When mapping RuboCop's `on_<kind>` names to Murphy kind strings, use
`docs/guides/rubocop-hook-dispatch.md`. RuboCop hook aliases (`on_send`,
`on_str`, …) are accepted by the dispatcher but the *kind string* passed
to `#[on_node(kind = …)]` is the Murphy-canonical form (`"send"`,
`"str"`, etc., per the infra guide's RSpec / std examples).

### Phase 2.3: scaffold the cop file

For an existing pack, add one file per cop under
`crates/<pack>/src/cops/<namespace>/<cop_snake_case>.rs` and re-export it
from `cops/<namespace>/mod.rs`. The file layout (top-of-file doc comment,
options struct, `#[cop]` impl block, helpers, `#[cfg(test)] mod tests`)
should mirror an existing peer cop in the pack — see
`crates/murphy-rspec/src/cops/rspec/describe_class.rs` for the canonical
shape.

The minimum skeleton, with no options:

```rust
//! `Pack/CopName` — one-line summary of what the cop policies.
//!
//! ## Matched shapes
//! …
//! ## Why this shape
//! …
//! ## Autocorrect (or "## No autocorrect" with the reason)
//! …

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct CopName;

#[cop(
    name = "Pack/CopName",
    description = "Human-readable one-liner.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl CopName {
    #[on_node(kind = "send", methods = ["foo"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, args, .. } = *cx.kind(node) else {
            return;
        };
        // …gate, then emit
        cx.emit_offense(cx.range(node), "Message here.", None);
    }
}
```

Cops with options follow `references/options.md` (and the
`ExampleLengthOptions` example shipped in `murphy-rspec`).

Cops with autocorrect add `cx.emit_edit(range, replacement)` calls
alongside `emit_offense`; see `references/autocorrect.md`.

### Phase 2.4: register the cop with the pack

Add the cop to the pack's `register_cops!` invocation in
`crates/<pack>/src/lib.rs`. The macro mode (`static` vs `dynamic`) is
already set by the existing call — just add the new struct to the list,
in source order.

Static pack example (`murphy-std`):

```rust
murphy_plugin_api::register_cops!(
    mode = static,
    NoReceiverPuts,
    UnreachableCode,
    StringLiterals,
    TrailingWhitespace,
    SpaceInsideParens,
    CopName,            // ← new entry; trailing comma matches house style
);
```

Dynamic pack example (`murphy-rspec`):

```rust
murphy_plugin_api::register_cops!(
    mode = dynamic,
    DescribeClass,
    ExampleLength,
    MultipleExpectations,
    CopName             // ← new entry; murphy-rspec omits the trailing comma
);
```

Trailing-comma style differs between packs — `murphy-std` keeps a
trailing comma; `murphy-rspec` does not. Match what the existing call
in `crates/<pack>/src/lib.rs` already does rather than imposing a new
convention.

Do not add a new `pub use` or re-export — the macro takes the type
directly, and the cop struct is reached via the use line at the top of
`lib.rs` (`use crate::cops::<namespace>::<file>::CopName;`).

### Phase 2.5: write tests against the test-support harness

Every cop ships its tests in the same file, inside `#[cfg(test)] mod
tests` and using `murphy_plugin_api::test_support`. The pack must
have

```toml
[dev-dependencies]
murphy-plugin-api = { path = "../murphy-plugin-api", features = ["test-support"] }
```

in its `Cargo.toml` (already true for `murphy-std`, `murphy-rspec`,
`murphy-rails`, `murphy-example-pack`).

The new preferred API is the tester builder — Cop is a generic
parameter, options are typed, and one tester drives many expectations
through a method chain:

```rust
use murphy_plugin_api::test_support::{indoc, test};

#[test]
fn flags_and_corrects_equals_equals() {
    test::<MyCop>()
        .expect_offense(indoc! {r#"
            x==0
             ^^ Surrounding space missing for operator `==`.
        "#})
        .expect_correction(
            indoc! {r#"
                a+b
                 ^ Surrounding space missing for operator `+`.
            "#},
            "a + b\n",
        );
}
```

For cops with non-default options, prefix the chain with
`with_options(&T::Options)` — the struct comes in by reference, no JSON
is constructed at the callsite:

```rust
test::<MyCop>()
    .with_options(&MyOpts { foo: true, ..Default::default() })
    .expect_offense(indoc! {r#"
        …
    "#});
```

Pick the right method:

- **`expect_offense(annotated)`** — positive case. Caret annotations
  describe the expected range and message; multiple annotation lines
  can stack under one source line. See `references/testing.md` for
  the full grammar and the multi-line caveat.
- **`expect_no_offenses(src)`** — negative case.
- **`expect_correction(annotated, after)`** — mandatory for shapes
  the cop autocorrects via `cx.emit_edit`. The annotated source uses
  the same caret grammar; `after` is the exact source expected after
  applying every emitted edit. Pins offense set *and* corrected
  output in one assertion. See
  `crates/murphy-std/src/cops/layout/space_inside_parens.rs` for the
  canonical in-tree usage.
- **`expect_no_corrections(src)`** — for shapes the cop deliberately
  reports without emitting an edit (unsafe rewrite, judgement
  required), and for clean inputs that should produce no edits. Only
  constrains the edit set — pair with `expect_offense` /
  `expect_no_offenses` when the offense set also matters.
- **`run_cop` escape hatch** — for tests the caret grammar can't
  express cleanly (multi-line emit ranges, parameterised loops,
  asserting `cop_name` / `severity`). Returns `Vec<CapturedOffense>`.
  See `RSpec/MultipleExpectations`' tests for the whole-block emit
  pattern. For raw edits, use `run_cop_with_edits` (returns
  `CapturedRun { offenses, edits }`); for non-default options use
  `run_cop_with_options` / `run_cop_with_options_and_edits`.

Each `expect_*` method is `#[track_caller]` and returns `&Self`, so a
single tester can carry many expectations through the same options.
When tests do not share setup, one-tester-per-`#[test]` is also fine.

The legacy `expect_*!` macros (`expect_offense!` etc.) are not exported.
Use `test::<Cop>()` for every expectation, including single-call tests.

Wrap every Ruby fixture string in `indoc!` (re-exported from the
harness) so the source can stay indented in the test body without
affecting caret column math.

Mirror the Phase 1 spec list — every case the RuboCop spec covers
should have a Murphy test (positive, negative, false-positive guards,
syntactic variants, alias methods, option boundaries).

### Phase 2.6: verify the build and the boundary

Run, in order:

```bash
cargo test -p <pack>                            # the cop's tests
cargo test -p <pack> --test dep_boundary        # single-surface invariant
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

The `dep_boundary.rs` test will fail if the new cop pulled in
`murphy-core`, `murphy-ast`, `murphy-translate`, etc., directly — the
only allowed runtime Murphy dependency is `murphy-plugin-api`. If it
fails, do not edit `ALLOWED_MURPHY_RUNTIME_DEPS`; instead rewrite the
cop to use only `murphy-plugin-api` re-exports. Widening the boundary
requires an ADR (infra guide §2 and §9).

For dynamic packs, also run the end-to-end test if one exists (e.g.
`cargo test -p murphy-cli --test rspec_pack_e2e`) to confirm the
cdylib loads correctly and the host's plugin loader sees the new cop.

## Phase 3: analyse the gap to the RuboCop original

After Phase 2.6 is green, do not yet open a PR. Diff the implementation
against the Phase 1 spec list and produce a short gap report. The report
drives Phase 4 (escalation), and any "tolerated" gaps get a note in the
cop file's top-doc comment so users running against a RuboCop baseline
are not surprised.

Walk each axis explicitly:

- **Shape coverage.** For every `on_<kind>` and `def_node_matcher` the
  RuboCop cop used: is the equivalent Murphy dispatch in place? Common
  misses: `on_csend` (safe-navigation `a&.b`) silently not handled
  because the port wrote `kind = "send"` only; `on_defs` (singleton
  defs) not added alongside `on_def`.
- **Option coverage.** Every `cop_config` key from Phase 1 should have a
  matching field in the `CopOptions` struct, with the same default and
  a compatible type. Options dropped on purpose get a Known v1
  limitation note.
- **Autocorrect parity.** If RuboCop autocorrects shape X, does Murphy
  also emit an `emit_edit` for X — and is the replacement equivalent?
  If the Murphy port is report-only on a shape RuboCop fixes, the
  reason ("unsafe rewrite", "not in v1 scope", …) belongs in the
  doc-comment.
- **Test parity.** Every example from the RuboCop spec file should
  resolve to a Murphy test. List the ones that don't, with one of:
  ported / tolerated v1 gap / Phase 4 blocker.
- **File-pattern gating.** RSpec / Rails cops will fire outside their
  intended file set. The standard mitigation is the "Known v1
  limitation" note in the doc-comment plus the
  `[cops.rules."Pack/CopName"] enabled = false` opt-out — see
  `RSpec/DescribeClass` for the canonical wording.
- **Offense message.** Compare verbatim against RuboCop. If the Murphy
  text deliberately diverges, note why.
- **Severity / default-enabled.** Match RuboCop unless there is a
  stated reason to differ.

Output of Phase 3 is one of:

- **No gaps.** Continue to Phase 5.
- **Gaps closable with more Phase 2 work.** Return to the relevant
  Phase 2.x and finish, then re-run Phase 3.
- **Gaps the guide / ABI cannot cover.** Go to Phase 4.

## Phase 4: escalate gaps the guide cannot cover

Some gaps cannot be closed by writing more Rust against the existing
ABI. Do **not** invent a workaround that reaches past the single-surface
boundary or papers over the gap silently. Escalate to the user with a
short report.

Escalation candidates:

- **AST mismatch.** RuboCop walks a node kind Murphy does not expose
  yet, or the kind exists but the destructured shape lacks a field the
  cop needs (e.g. `Block { call, body, args }` is missing a numblock
  variant the RuboCop cop relies on).
- **Missing hook.** RuboCop uses a hook (`on_lvar`, `on_for`,
  `on_op_asgn`, …) that maps to a Murphy kind which is not yet wired
  for dispatch, or for which substituting a nearby kind would change
  observable semantics.
- **`node_pattern!` v1 limit.** The pattern uses named captures,
  predicate calls (`#some_method?`), or non-trailing repetition — see
  `references/node-pattern.md` for what v1 *can* express.
- **Option type unsupported.** The RuboCop config key is a nested hash
  or enum whose Murphy mapping is not obvious. `i64` / `bool` /
  `String` / `Vec<String>` cover most cases.
- **Missing `Cx` method.** The cop needs lexer tokens, comment
  attachment, or some other facility not on `Cx`.
- **Single-surface boundary.** The honest implementation would need a
  direct dep on `murphy-core` / `murphy-ast` / `murphy-translate`.
  Widening `ALLOWED_MURPHY_RUNTIME_DEPS` requires an ADR.

Escalation format — bring this to the user in one short report:

1. **What the gap is.** Cop name + the RuboCop shape / option / test
   that triggered it.
2. **Workaround attempted.** What you tried inside the existing ABI and
   why it does not work, or why an alternative hook changes semantics.
3. **Blocker vs degradable.** Whether the cop ships with a documented
   v1 limitation, or whether shipping at all needs the ABI extension.
4. **Suggested extension.** ADR, new `Cx` method, new dispatch kind,
   `node_pattern!` grammar item — name the surface to extend.

If the user agrees to degrade and ship with the limitation, document
it in the cop file's top-doc comment and continue to Phase 5. If the
user agrees to extend the ABI, file a bd issue for the extension and
pause the port until that work lands.

## Phase 5: pass `roborev-refine` review

Run `roborev-refine` against the worktree branch. The skill (installed
in this plugin set) drives a review → fix → re-review loop via the
roborev daemon and stops when the daemon reports passing or hits the
iteration cap.

To start the loop in this session, invoke the `roborev-refine` skill —
the user typing `/roborev-refine` does the same thing. The skill takes
care of finding open reviews, applying inline fixes, re-requesting
review, and looping. Stay in the same worktree.

Outcomes:

- **Passing.** Continue to Phase 6.
- **Iteration cap hit.** Treat as a Phase 4 escalation — bring the
  daemon's last unresolved findings to the user. Do not force-merge a
  cop that the reviewer has open objections on.

## Phase 6: open the PR, then merge after CI passes

Ship the work:

1. **Push the worktree branch** if it is not already on origin:
   `git push -u origin <branch>`.
2. **Open a PR** with `gh pr create`. Title: `feat(<pack>-<cop-id>): port Pack/CopName from RuboCop`
   (or `feat(murphy-std-…)` etc., matching the commit-message convention
   visible in `git log --oneline main`). Body: short summary (1–3
   bullets — what the cop does, dispatch shape, autocorrect stance) and
   a test plan checklist (the Phase 2.5 / Phase 3 outcomes).
3. **Wait for CI.** Use `gh pr checks --watch` or poll periodically. Do
   not merge until every required check is green. If a check fails,
   triage the failure: a `dep_boundary` regression goes back to Phase
   2.6; a clippy / fmt nit can be fixed inline.
4. **Merge.** This repository uses merge commits (see
   `git log --oneline main` — `Merge pull request #N from …`). Run
   `gh pr merge --merge --delete-branch` once CI is green. Squash and
   rebase merges are allowed by the repo config but the project
   convention is merge commits; do not switch styles without asking.
5. **Clean up.** Leave the worktree (`ExitWorktree`) so the next port
   starts from a clean main.

Confirm with the user before merging if any of the following applies:

- The PR has open human review comments.
- Phase 4 produced documented v1 limitations the user has not seen.
- The branch contains commits unrelated to the cop port.

Otherwise, the user's "port this cop" request implicitly authorises the
merge — proceed once CI is green.

## Common pitfalls

1. **Counting `Send`s without the empty-receiver gate.** RuboCop matchers
   that look at `expect(...)` mean *bare* `expect`, not `foo.expect(...)`.
   Inside the cop body, always check `receiver == OptNodeId::NONE`
   before treating a `Send` as a DSL call. See
   `RSpec/MultipleExpectations`' `is_bare_expect_call` helper.
2. **Synthesising autocorrects from string literals.** A cop that flags
   `describe "Foo"` cannot autocorrect to `describe Foo` — the class may
   not exist. Reach for `emit_edit` only when the replacement is
   provably safe; otherwise document "no autocorrect" with the reason
   in the file's top-doc comment, the same way `RSpec/DescribeClass`
   does.
3. **`methods = []` or stale method-name checks.** The `methods = [...]`
   attribute is the *primary* dispatch filter; the
   `if cx.symbol_str(method) != "describe" { return; }` line inside
   the body is redundant when the filter is present (and leaving it in
   is a warning sign that the port was done mechanically rather than
   against the current macro surface).
4. **`#[on_node]` on the wrong kind.** A cop that mirrors RuboCop's
   `on_block` does *not* use `kind = "send"` for the call — it uses
   `kind = "block"` and destructures the block to reach the call. The
   infra guide §3 examples are authoritative.
5. **Missing per-file-name gating for spec/Rails-only rules.** Murphy
   has no per-cop `Include` glob support yet (infra guide §3 note on
   `RSpec/DescribeClass`). RSpec / Rails cops will fire outside their
   intended file set. Document this in the cop's doc-comment as a
   known v1 limitation and tell users to disable via
   `[cops.rules."Pack/CopName"] enabled = false` if it hurts them.
6. **Re-using state across calls.** Cops are stateless unit structs.
   Anything that needs cross-node accounting (counts, sets, …) must be
   accumulated *inside one dispatch call* by walking `cx.descendants(id)`
   or `cx.children(id)`. Do not stash state on the struct — the macro's
   const-metadata shape does not allow it.
7. **Skipping Phase 3.** A passing test suite does not mean the cop
   ported faithfully. The gap analysis is what catches `on_csend`-class
   misses, dropped options, and message divergence — all of which
   reviewers will catch later if you do not.
8. **Quiet workarounds for Phase 4 gaps.** Reaching into `murphy-ast` /
   `murphy-translate` to dodge a missing `Cx` method, or hand-rolling
   a parser to substitute for an unsupported `node_pattern!` predicate,
   bypasses the single-surface boundary and the dep-boundary test will
   fail. Escalate instead.

## Additional resources

### Reference files

- **`references/api-surface.md`** — quick map of every
  `murphy_plugin_api::` import the cop file is likely to pull, with
  short notes per type.
- **`references/dispatch-and-cx.md`** — extended cheat-sheet for the
  `#[on_node]` / `#[on_new_investigation]` attributes and every method
  on `Cx<'_>`.
- **`references/node-pattern.md`** — `node_pattern!` v1 grammar subset
  and when to reach for it instead of hand-written destructuring.
- **`references/options.md`** — `#[derive(CopOptions)]` + `#[option(...)]`
  reference, including the current v1 limitation that options are read
  from `OptionStruct::default()` inside the dispatch (murphy-9cr.9 will
  wire live overrides through `Cx`).
- **`references/autocorrect.md`** — when to ship `emit_edit`, idempotency
  expectations, and the offense/edit pairing the host uses.
- **`references/testing.md`** — `test::<T>()` tester-builder API, the
  caret annotation grammar (multi-annotation-per-line included), and
  the `run_cop` escape hatch. Legacy `expect_*!` macros are not
  exported.

### Example cop files in-tree

The canonical reference cops to read while porting:

- `crates/murphy-rspec/src/cops/rspec/describe_class.rs` — `Send` cop
  with method-name filter, multiple receiver shapes, no options, no
  autocorrect.
- `crates/murphy-rspec/src/cops/rspec/multiple_expectations.rs` — block
  cop with options, descendant counting, `run_cop` test pattern for
  multi-line ranges.
- `crates/murphy-rspec/src/cops/rspec/example_length.rs` — block cop
  with options and raw-source line counting (`cx.raw_source`).
- `crates/murphy-std/src/cops/style/string_literals.rs` — `Str` cop with a
  string-enum option (`preferred_quote = single | double`) and a
  conditional `emit_edit` gated on a safety predicate. Closest peer
  when porting `Style/*` cops with autocorrect.
- `crates/murphy-std/src/cops/layout/space_inside_parens.rs` — pure
  autocorrect cop with the canonical `expect_correction` and
  `expect_no_corrections` usage; closest peer when porting `Layout/*`
  cops.
- `crates/murphy-std/src/cops/layout/space_around_operators.rs` —
  canonical reference for the tester-builder API end-to-end: typed
  `with_options` against a 3-key `CopOptions` struct, chained
  `expect_offense` / `expect_correction` / `expect_no_offenses`
  calls, and multi-annotation-per-source-line tests for the
  operator-run shapes.

### Companion docs

- **`docs/guides/plugin-cop-infrastructure.md`** — the API reference
  this skill is a procedure for. Re-read the relevant section instead
  of guessing.
- **`docs/guides/rubocop-hook-dispatch.md`** — RuboCop `on_<kind>` ⇄
  Murphy kind-string mapping table.
- **`docs/guides/third-party-cop-sandbox.md`** — security posture
  (v1 ships no sandbox; cops are trusted code).
