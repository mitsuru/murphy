# Testing cops — `expect_offense!` / `expect_no_offenses!` / `expect_correction!` / `expect_no_corrections!` / `run_cop`

Authoritative source: infra guide §11 plus
`crates/murphy-plugin-api/src/test_support.rs` and the design doc
`docs/plans/2026-05-24-expect-offense-macro-design.md`.

## Setup

Every pack already has the `test-support` feature enabled in
`[dev-dependencies]`:

```toml
[dev-dependencies]
murphy-plugin-api = { path = "../murphy-plugin-api", features = ["test-support"] }
```

(`murphy-std`, `murphy-rspec`, `murphy-rails`, `murphy-example-pack` are
all set up; new packs must add this line.)

Inside the cop file, import only the macros the test set actually uses:

```rust
#[cfg(test)]
mod tests {
    use super::MyCop;
    use murphy_plugin_api::test_support::{
        expect_correction, expect_no_corrections, expect_no_offenses, expect_offense, indoc,
        run_cop,
    };

    // tests…
}
```

The `test-support` cargo feature pulls `murphy-translate` (the parser)
into `murphy-plugin-api` at test time only — production cdylib builds
stay parser-free. `tests/dep_boundary.rs` checks runtime
`murphy-*` deps only, so the single-surface invariant still holds.

## `expect_offense!` — positive case

The default macro for "the cop emits exactly these offenses". Each
offense is described by a caret-annotated line directly below the
source line it points into.

```rust
#[test]
fn flags_two_expects_single_line_block() {
    expect_offense!(
        MultipleExpectations,
        indoc! {r#"
            it "x" do expect(a).to eq(1); expect(b).to eq(2) end
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Example has too many expectations (2/1)
        "#}
    );
}
```

### Annotation grammar

- A line whose first non-whitespace char is `^` is an annotation line;
  everything else is a source line. Annotation lines are stripped
  before the source is parsed as Ruby.
- The caret column is the **char index** start of the offense in the
  source line above. Number of carets = char length of the range.
- Caret math is multibyte-safe via `char_indices()` — count chars, not
  bytes, when computing the caret column for a fixture containing
  multibyte characters.
- Text after the last caret (whitespace-trimmed) is the expected
  message. **Exact-match** comparison. Omit the message (`^^^` only)
  to assert range only — useful for cops with dynamic message text.
- Matching is **exact-set**: any cop emission without a matching
  annotation fails the test, and any annotation without a matching
  emission fails the test.
- On failure, the panic renders **both** expected and actual sides as
  caret-annotated source for a diff-style read.

### Limitations

- One annotation line per source line in MVP. The parser already
  absorbs multiple; comparator/renderer support is tracked as
  `murphy-swo`.
- Multi-line ranges (offense spans past a newline) get carets on the
  first line only and `(+ N more chars)` is appended to the message.
  Block-level cops (whole `it … end`) are awkward to annotate this way
  — fall back to `run_cop` until multi-line caret support lands
  (tracked as `murphy-ac6`).
- The macro panics with a "use `expect_no_offenses!` instead" message
  if the input has no annotations (typo guard).

## `expect_no_offenses!` — negative case

```rust
#[test]
fn does_not_flag_single_expect() {
    expect_no_offenses!(
        MultipleExpectations,
        indoc! {r#"
            it "works" do
              expect(a).to eq(1)
            end
        "#}
    );
}
```

Companion to `expect_offense!`. Panics if the cop emits anything.
Rejects caret-bearing input as a symmetric typo guard ("use
`expect_offense!` instead").

## `expect_correction!` — assert offenses and autocorrect output

Use `expect_correction!` for cops that emit `RawEdit`s through
`cx.emit_edit`. The first fixture uses the same caret grammar as
`expect_offense!`; the third argument is the exact source expected
after applying all emitted edits to the annotation-stripped input.

```rust
#[test]
fn corrects_spaces_inside_parentheses() {
    expect_correction!(
        SpaceInsideParens,
        indoc! {r#"
            foo( 1)
                ^ Space inside parentheses detected.
            bar(1 )
                 ^ Space inside parentheses detected.
        "#},
        "foo(1)\nbar(1)\n"
    );
}
```

The macro first checks the exact offense set (same grammar and
exact-set semantics as `expect_offense!`), then compares the corrected
source string. Use this whenever a cop ships autocorrect: it pins both
the offense and the fix in one assertion. See
`crates/murphy-std/src/cops/layout/space_inside_parens.rs` for the canonical
in-tree usage.

For tests that need to inspect raw edits directly, drop to
`run_cop_with_edits` (returns `CapturedRun { offenses, edits }`).

## `expect_no_corrections!` — assert no autocorrect edits

Use `expect_no_corrections!` when a cop may emit offenses but must not
emit any autocorrect edits for the fixture. Common cases: an unsafe
shape where the cop deliberately reports without correcting, or a
clean input that should produce neither offense nor edit.

```rust
#[test]
fn leaves_clean_parentheses_without_corrections() {
    expect_no_corrections!(SpaceInsideParens, "foo(1, 2)\nbar()\n");
}
```

The macro rejects caret-bearing input as a typo guard ("use
`expect_correction!` instead"). It only checks the edit set — it does
not constrain offenses, so pair it with `expect_offense!` /
`expect_no_offenses!` when the offense set also matters.

## `indoc!`

Re-exported from the `indoc` crate (feature-gated). Strips the common
ASCII-whitespace prefix from each line of a `r#"…"#` literal at compile
time so Ruby fixtures can stay indented inside the test function
without affecting parse output or caret column math. Use it on every
fixture string the macros take.

## `run_cop` — escape hatch

For tests the caret grammar can't express cleanly — block-level
multi-line ranges, asserting `cop_name` / `severity`, parametrised
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

- `cop_name: &'static str`
- `message: String`
- `range: Range`
- `severity: Option<Severity>` — `None` when the cop didn't override;
  the host's default chain applies in production.

The two macros are thin layers on top of `run_cop`.

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

## Test coverage checklist

For a ported cop, the test set should cover at minimum:

- **Positive case.** The exact shape RuboCop's first spec example
  flags. Use `expect_offense!` for single-line emit ranges,
  `run_cop` + `hits` for multi-line.
- **Negative case at the threshold.** The just-OK shape. Use
  `expect_no_offenses!`.
- **False-positive guards.** Every shape the cop deliberately ignores
  — explicit receivers (`Other.it ...`), empty bodies, hook blocks,
  aliased methods that aren't the target, etc. The RuboCop spec file
  for the original cop is the easiest checklist.
- **Syntactic variants.** `do…end` vs `{…}`, single- vs multi-line,
  aliases (`it` / `specify` / `example`).
- **Option boundaries** (when applicable). Body at `max`, body at
  `max + 1`. Document the v1 default-only limitation when the test
  exercises non-default values via a hand-built struct (the live
  override path lands in murphy-9cr.9).
- **Autocorrect parity** (when applicable). Every shape the cop fixes
  via `cx.emit_edit` gets an `expect_correction!` test pinning the
  corrected source. Every shape the cop deliberately reports without
  correcting gets an `expect_no_corrections!` test. Idempotency — the
  fixture stays clean if the cop runs again on its own output — is
  asserted at the CLI level (`crates/murphy-cli/tests/cli.rs`), so a
  per-cop unit test is only needed when the corrected source is
  non-obvious.
