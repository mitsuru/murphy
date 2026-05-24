# expect_offense! / expect_no_offenses! — design

Issue: `murphy-ac6`
Scope: `crates/murphy-plugin-api/src/test_support.rs`
Status: design approved 2026-05-24

## 1. Why

`murphy-plugin-api` already exposes a `test_support` feature with
`run_cop::<T>(src) -> Vec<CapturedOffense>` (introduced for
`murphy-6bv` / `murphy-6tq`). All current RSpec pack tests use it via
a `hits(src) -> usize` helper that asserts only the **count** of
emitted offenses. Range and message regressions slip through silently.

`expect_offense!` adds rubocop-style inline caret annotations so each
test asserts `(range, message)` pairs without per-cop boilerplate.

## 2. Scope

In:

- `expect_offense!(Cop, src)` — assert exact set of `(range, message)`
  pairs against carets parsed from `src`.
- `expect_no_offenses!(Cop, src)` — assert that `Cop` emits nothing.
- Parser / renderer / comparator unit tests in `test_support`'s own
  `#[cfg(test)] mod tests`.
- 2–3 of the existing 11 RSpec tests rewritten as a smoke migration.

Out (follow-up issues):

- `expect_correction!` (autocorrect-aware assert).
- Full migration of remaining 8–9 RSpec tests off `hits()`.
- Multiple annotation lines per source line (parser ready, comparator /
  renderer not).
- Multi-cop dispatch harness — `run_cop::<T>` is single-cop only.
- Editor-visual alignment helpers for non-ASCII source lines.

## 3. Surface

Both macros live in `crates/murphy-plugin-api/src/test_support.rs`
under the existing `test-support` feature. Production cdylibs never
touch this code path.

User-visible paths:

- `murphy_plugin_api::test_support::expect_offense!`
- `murphy_plugin_api::test_support::expect_no_offenses!`

`macro_rules!` with `#[macro_export]` lands a macro at the crate root,
so the module-scoped path is provided via
`pub use crate::expect_offense;` inside `test_support`. The crate-root
form is intentionally not advertised — `test_support` is the single
documented entry.

Call sites:

```rust
use murphy_plugin_api::test_support::{expect_offense, expect_no_offenses, indoc};

expect_offense!(MyCop, indoc! {r#"
    expect(a).to eq 1
    expect(b).to eq 2
    ^^^^^^^^^^^^^^^^^ Example has too many expects
"#});

expect_no_offenses!(MyCop, indoc! {r#"
    expect(a).to eq 1
"#});
```

Match semantics: **exact set**. Any offense the cop emits without a
matching annotation is a failure; any annotation without a matching
emission is a failure. Order is normalised before comparison so the
test author writes annotations in source order.

Empty message (carets only, no text after) ⇒ **range-only assert**.
Used for cops with dynamic message text like `(6/5)` where the range
is the stable part.

`run_cop::<T>` stays public. `expect_offense!` is a thin layer on top
of it; direct callers using `run_cop` for ad-hoc inspection are
unaffected.

## 4. Annotation grammar

**Annotation line:** any line whose first non-whitespace character is
`^`. All other lines are **source lines**.

**Caret column = char index of the directly-preceding source line.**
Not byte offset, not display column. `indoc!` strips the common
ASCII-whitespace prefix from every line before the macro sees it, so
the leading-space count of an annotation line equals the caret start
column in the source line above. Conversion to a byte range happens
via `source_line.char_indices().nth(col)`.

```text
expect(a).to eq 1
expect(b).to eq 2
^^^^^^^^^^^^^^^^^ Example has too many expects
```

- 17 carets, starting at char column 0 of `expect(b).to eq 2`
  → `Range { start = byte_offset_of(line, 0), end = byte_offset_of(line, 17) }`
  in the cleaned source.
- Message is everything after the last `^`, with surrounding
  whitespace trimmed.

Non-ASCII source lines are supported. Caret count is char count
(matches rubocop's behaviour); editors with East Asian Width will
render the carets visually offset against the wide chars but the
assert still computes correctly. Only the source line's bytes vs
chars matter — the annotation line itself is always ASCII (spaces,
`^`, optional message).

**Pairing rule:** each annotation line attaches to **the source line
immediately above it**. Two consecutive annotation lines both attach
to the same source line (parser-side extension point for future
multi-offense-per-line support). MVP comparator / renderer only
support ≤ 1 annotation per source line and panic with a clear
message if a test uses more.

**Edge cases:**

- A source line whose first non-whitespace char is `^` would be
  misclassified; Ruby syntax effectively never starts a logical line
  with `^`, and the docstring calls it out.
- Heredoc bodies containing `^` at start-of-line: rare in fixtures;
  documented limitation.
- Trailing newline of the input is preserved unchanged (Ruby parser
  is whitespace-sensitive at EOF).

## 5. Algorithm

Input: `annotated: &str`, `T: NodeCop + Default`.

1. **Scan** `annotated.lines()` in order:
   - Annotation line → record
     `Annotation { src_line_idx, char_col, char_len, msg: Option<String> }`,
     attached to the most recent source line index.
   - Source line → append to `cleaned_lines: Vec<&str>`, assign a new
     index.
   - Two annotation lines in a row attached to the same source line:
     MVP panics with `"multiple annotations per source line not yet
     supported"`.
   - An annotation appearing before any source line: panic
     `"annotation precedes any source line"`.

2. **Build cleaned source:**
   `cleaned = cleaned_lines.join("\n")`, plus a trailing `\n` if
   `annotated` ended with one. Cache `line_byte_starts: Vec<usize>`.

3. **Expand annotations to expected ranges:**
   ```rust
   let line = cleaned_lines[ann.src_line_idx];
   let s_byte = nth_char_byte(line, ann.char_col);
   let e_byte = nth_char_byte(line, ann.char_col + ann.char_len);
   Expected {
       range: Range {
           start: (line_byte_starts[ann.src_line_idx] + s_byte) as u32,
           end:   (line_byte_starts[ann.src_line_idx] + e_byte) as u32,
       },
       message: ann.msg,
   }
   ```
   `nth_char_byte(line, n)` returns `line.char_indices().nth(n).map(|(b,_)| b).unwrap_or(line.len())`.

4. **Run cop:** `let actuals = run_cop::<T>(&cleaned);`.

5. **Sort both sides** by `(range.start, range.end, message)` for
   stable comparison. Tie-break on message lexicographically.

6. **Compare:**
   - If lengths differ → fail.
   - Pairwise: `expected.range == actual.range` AND
     (`expected.message.is_none()` OR `expected.message.as_deref() == Some(actual.message.as_str())`).
   - Any mismatch → fail.

7. **On failure:** render both sides via §6 and `panic!` with the
   formatted diff. On success: nothing.

`expect_no_offenses!` shares scanning/cleaning but asserts:
- Annotations parsed: 0 (else panic `"expect_no_offenses! must not
  contain annotations; use expect_offense! instead"`).
- `actuals.is_empty()`. If not, render the actual side and panic.

## 6. Failure rendering

`fn render(src: &str, items: &[(Range, Option<&str>)]) -> String`.

- Re-split `src` into lines, recompute `line_byte_starts`.
- For each item: find the source line where `range.start` falls.
  Compute caret column = char index from the line start to
  `range.start` (inverse of step 3). Caret count = char count from
  `range.start` to `min(range.end, line_end)`.
- Emit one annotation line immediately under its source line. Two
  items on the same source line are emitted as two consecutive
  annotation lines (consistent with the parser's extension point).
- Multi-line ranges (`range.end` falls in a later line than `range.start`):
  emit the caret only on the first line, append
  `(+ N more chars)` to the message so the reader knows the range
  extends.

Final panic shape:

```text
expect_offense! mismatch

expected:
  <render(cleaned_src, expected items)>

actual:
  <render(cleaned_src, actual items)>
```

- Indent: 2 spaces.
- No colour, no ANSI escapes (stderr may not be a tty).
- `expect_no_offenses!` uses the `actual:` block only with a different
  header (`expect_no_offenses! found N offense(s)`).

## 7. Frozen contract

Surface choices that are expensive to change after migration:

- Macro syntax: `expect_offense!(Cop, src)`, `expect_no_offenses!(Cop, src)`.
- Module path: `murphy_plugin_api::test_support::*`.
- Annotation grammar: caret on line below source, `^` count = char
  count, half-open range, message is text after the last `^` trimmed.
- Empty message ⇒ range-only assert.
- Exact-set match (no extras, no misses).

Things explicitly allowed to evolve without a breaking change:

- Multi-annotation per source line (parser already absorbs it).
- Richer failure rendering (colour, terminal width, snippet markers).
- `expect_correction!` and `expect_no_corrections!` siblings.

## 8. Tests for the harness itself

`test_support`'s own `#[cfg(test)] mod tests` will cover:

- Parser: single annotation, empty message, two consecutive annotation
  lines (panics), annotation with no source above (panics).
- Char-index conversion: ASCII line, multibyte line (`expect("あ")`
  with carets on `"あ"`).
- Comparator: extra emit fails, missing emit fails, range mismatch
  fails, message mismatch fails, message-skip mode passes regardless
  of dynamic content.
- Renderer: round-trip — feed `(cleaned, expected_items)` back into
  `render`, verify the output matches the original annotated input
  line-for-line.

Plus 2–3 RSpec tests rewritten end-to-end as smoke coverage —
remaining migration is its own follow-up.
