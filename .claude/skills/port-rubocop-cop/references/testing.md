# Testing cops — `test::<T>()` tester builder (plus the legacy `expect_*!` macros)

Authoritative source: infra guide §11 plus
`crates/murphy-plugin-api/src/test_support.rs`.

## Setup

Every pack already has the `test-support` feature enabled in
`[dev-dependencies]`:

```toml
[dev-dependencies]
murphy-plugin-api = { path = "../murphy-plugin-api", features = ["test-support"] }
```

(`murphy-std`, `murphy-rspec`, `murphy-rails`, `murphy-example-pack` are
all set up; new packs must add this line.)

Inside the cop file, the new preferred entry point is `test::<T>()`:

```rust
#[cfg(test)]
mod tests {
    use super::MyCop;
    use murphy_plugin_api::test_support::{indoc, test};

    // tests…
}
```

The `test-support` cargo feature pulls `murphy-translate` (the parser)
into `murphy-plugin-api` at test time only — production cdylib builds
stay parser-free. `tests/dep_boundary.rs` checks runtime `murphy-*`
deps only, so the single-surface invariant still holds.

## `test::<T>()` — the tester builder

The cop type is a turbofish parameter; everything else hangs off the
returned `Tester<T>` via method chain:

```rust
test::<MyCop>()
    .with_options(&MyOpts { foo: true, ..Default::default() })
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
```

Each `expect_*` method is `#[track_caller]` and returns `&Self` so a
single tester can carry many expectations through the same options.

### `with_options(self, &T::Options) -> Self`

Threads typed options through the chain. The struct comes in by
reference; `to_config_json` is called internally so test code never
constructs JSON. Subsequent `with_options` calls overwrite earlier
values. Cops with `Options = NoOptions` skip the `with_options`
clause entirely.

### `expect_offense(&self, annotated: &str) -> &Self`

Pins the exact offense set. The annotated string carries caret
markers under each source line, one or more per line:

```text
a+b-c*d
 ^ Surrounding space missing for operator `+`.
   ^ Surrounding space missing for operator `-`.
     ^ Surrounding space missing for operator `*`.
```

Annotation grammar:

- A line whose first non-whitespace char is `^` is an annotation
  line; everything else is a source line. Annotation lines are
  stripped before parsing the source as Ruby.
- The caret column is the **char index** start of the offense in the
  source line above. Number of carets = char length of the range.
- Caret math is multibyte-safe via `char_indices()` — count chars,
  not bytes, when computing the caret column for fixtures with
  multibyte characters.
- Text after the last caret (trimmed) is the expected message —
  **exact match**. Omit the message (`^^^` only) to assert range
  only.
- **Multiple annotation lines under one source line** anchor to that
  source line. Use this for cops that fire several offenses on the
  same row (operator runs, multi-call expressions).
- Matching is exact-set: a cop emission without a matching
  annotation fails the test, and a missing annotation fails the
  test.
- On failure, the panic renders both expected and actual sides as
  caret-annotated source for a diff-style read.

Multi-line ranges (the cop's range spans past a newline) get carets
on the first line only and `(+ N more chars)` is appended to the
message. For block-level cops (whole `it … end`) drop to the
`run_cop` escape hatch — multi-line caret support is tracked as
`murphy-ac6`.

The method panics with a "use `expect_no_offenses` instead" message
if the annotated input has no caret lines (typo guard).

### `expect_no_offenses(&self, src: &str) -> &Self`

Counterpart to `expect_offense` — panics if the cop emits anything.
Rejects caret-bearing input as a symmetric typo guard ("use
`expect_offense` instead").

### `expect_correction(&self, annotated: &str, after: &str) -> &Self`

For cops that emit `RawEdit`s through `cx.emit_edit`. The first
argument is annotated source (same grammar as `expect_offense`); the
second is the exact source expected after applying every emitted
edit to the annotation-stripped input.

```rust
test::<SpaceInsideParens>()
    .expect_correction(
        indoc! {r#"
            foo( 1)
                ^ Space inside parentheses detected.
            bar(1 )
                 ^ Space inside parentheses detected.
        "#},
        "foo(1)\nbar(1)\n",
    );
```

The method first checks the exact offense set (same grammar as
`expect_offense`), then compares the corrected source. Use this
whenever a cop ships autocorrect — it pins both the offense set and
the fix in one assertion.

For tests that need to inspect raw edits directly, drop to
`run_cop_with_edits` (returns `CapturedRun { offenses, edits }`).

### `expect_no_corrections(&self, src: &str) -> &Self`

For shapes the cop may flag but must not autocorrect (unsafe
rewrite, judgement required), and for clean inputs that should
produce no edits. Constrains only the edit set — pair with
`expect_offense` / `expect_no_offenses` when the offense set also
matters.

```rust
test::<SpaceInsideParens>().expect_no_corrections("foo(1, 2)\nbar()\n");
```

### Chaining multiple expectations

The whole point of the tester builder: one tester, many cases,
shared setup.

```rust
test::<SpaceAroundOperators>()
    .with_options(&SpaceAroundOperatorsOptions {
        allow_for_alignment: false,
        ..Default::default()
    })
    .expect_offense(indoc! {r#"
        a&&b
         ^^ Surrounding space missing for operator `&&`.
    "#})
    .expect_correction(
        indoc! {r#"
            a+b
             ^ Surrounding space missing for operator `+`.
        "#},
        "a + b\n",
    );
```

When tests don't share setup, fall back to one-tester-per-`#[test]`
— a chain is a convenience, not a requirement.

## `indoc!`

Re-exported from the `indoc` crate (feature-gated). Strips the common
ASCII-whitespace prefix from each line of a `r#"…"#` literal at
compile time so Ruby fixtures can stay indented inside the test
function without affecting parse output or caret column math. Use it
on every multi-line fixture string.

## `run_cop` — escape hatch

For tests the caret grammar can't express cleanly — multi-line
offense ranges, asserting `cop_name` / `severity`, parametrised
loops over many inputs — drop down to `run_cop`:

```rust
use murphy_plugin_api::test_support::run_cop;

let offenses = run_cop::<MyCop>("def foo; end\n");
assert_eq!(offenses.len(), 1);
assert_eq!(offenses[0].cop_name, "Plugin/MyCop");
```

`run_cop` parses `src` as Ruby, runs the cop's dispatch (per-kind or
file-visit, mirroring host semantics), and returns
`Vec<CapturedOffense>`. `CapturedOffense` carries:

- `cop_name: String`
- `message: String`
- `range: Range`
- `severity: Option<Severity>` — `None` when the cop didn't
  override; the host's default chain applies in production.

For raw edits or for non-default options, use
`run_cop_with_edits` / `run_cop_with_options` /
`run_cop_with_options_and_edits`. The tester builder forwards to
these under the hood.

### Idiomatic `hits` helper

For positive tests with multi-line ranges, packs use a small `hits`
helper to keep test bodies focused on the assertion:

```rust
fn hits(source: &str) -> usize {
    run_cop::<MultipleExpectations>(source).len()
}

#[test]
fn flags_two_expects() {
    let src = indoc! {r#"
        it "works" do
          expect(a).to eq(1)
          expect(b).to eq(2)
        end
    "#};
    assert_eq!(hits(src), 1);
}
```

Since `run_cop` only dispatches the one cop type, every emission is
already from that cop — no per-name filter needed.

## Legacy `expect_*!` macros

`expect_offense!` / `expect_no_offenses!` / `expect_correction!` /
`expect_no_corrections!` macros are still exported and pass through
to the same internal helpers. The macro form is fine for one-off
single-expectation tests, but new cop tests prefer the tester
builder for the type-safe-options story and the chain. Existing
macro callsites do not have to migrate; both APIs will coexist.

```rust
// macro form (still supported)
expect_offense!(
    MyCop,
    indoc! {r#"
        x==0
         ^^ Surrounding space missing for operator `==`.
    "#}
);

// tester builder form (preferred for new tests)
test::<MyCop>().expect_offense(indoc! {r#"
    x==0
     ^^ Surrounding space missing for operator `==`.
"#});
```

## Test coverage checklist

For a ported cop, the test set should cover at minimum:

- **Positive case.** The exact shape RuboCop's first spec example
  flags. Use `expect_offense` for single-line emit ranges,
  `run_cop` + `hits` for multi-line.
- **Negative case at the threshold.** The just-OK shape. Use
  `expect_no_offenses`.
- **False-positive guards.** Every shape the cop deliberately
  ignores — explicit receivers (`Other.it ...`), empty bodies, hook
  blocks, aliased methods that aren't the target, etc. The RuboCop
  spec file for the original cop is the easiest checklist.
- **Syntactic variants.** `do…end` vs `{…}`, single- vs multi-line,
  aliases (`it` / `specify` / `example`).
- **Option boundaries** (when applicable). Body at `max`, body at
  `max + 1`. Pass non-default options through `with_options(&T::Options { … })`.
- **Autocorrect parity** (when applicable). Every shape the cop
  fixes via `cx.emit_edit` gets an `expect_correction` call pinning
  the corrected source. Every shape the cop deliberately reports
  without correcting gets an `expect_no_corrections` call.
  Idempotency — the fixture stays clean if the cop runs again on
  its own output — is asserted at the CLI level
  (`crates/murphy-cli/tests/cli.rs`), so a per-cop unit test is
  only needed when the corrected source is non-obvious.
