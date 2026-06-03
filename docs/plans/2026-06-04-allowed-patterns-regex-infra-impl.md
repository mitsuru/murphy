# AllowedPatterns regex matching infrastructure — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Give cops a real regex matcher for the `AllowedPatterns` option, replacing today's substring stand-in, via one cached `Cx` helper.

**Architecture:** Add `Cx::matches_any_pattern(name, &[String]) -> bool` in `murphy-plugin-api`, backed by a free function with a `thread_local!` compiled-regex cache (RE2 / Rust `regex`, already a dependency). Invalid patterns emit a one-time stderr diagnostic and are skipped. Then migrate the two substring cops and DRY-refactor the one cop that already inlines its own regex cache.

**Tech Stack:** Rust, `regex` crate (RE2), `thread_local!`, `LazyLock<Mutex<HashSet>>`, Murphy plugin ABI (`Cx`).

**Design:** `docs/plans/2026-06-04-allowed-patterns-regex-infra-design.md`

**Conventions (read before starting):**
- TDD is mandatory — failing test first (project CLAUDE.md).
- Worktree already set up; `mise trust` + `eval "$(mise activate bash)"` if tools are missing (worktree setup note in CLAUDE.md).
- Gates: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo +nightly fmt --check`.

---

### Task 1: `matches_any_pattern` helper + thread_local cache + diagnostic

**Files:**
- Modify: `crates/murphy-plugin-api/src/cx.rs` (add imports near top; add free fn + `Cx` method; add tests in the existing `#[cfg(test)] mod tests`)

**Step 1: Add the imports**

At the top of `crates/murphy-plugin-api/src/cx.rs`, after the existing `use std::marker::PhantomData;` (line 3), add:

```rust
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};
```

(`regex` is already a direct dependency of this crate — `regex::Regex` resolves without a `use`.)

**Step 2: Write the failing tests**

Add to the `#[cfg(test)] mod tests` block at the bottom of `cx.rs` (the module that already contains `options_or_default_decodes_current_cop_options`). These target the free function directly, so no `Cx` fixture is needed:

```rust
#[test]
fn matches_any_pattern_anchored_regex() {
    // `^equal` is a valid regex that the old substring scan could never match
    // (the method name contains no `^`), but an anchored regex matches.
    assert!(super::allowed_pattern_match("equal?", &["^equal".to_string()]));
    assert!(!super::allowed_pattern_match("not_equal?", &["^equal".to_string()]));
}

#[test]
fn matches_any_pattern_metacharacters() {
    assert!(super::allowed_pattern_match("eql?", &["eq.?l".to_string()]));
    assert!(!super::allowed_pattern_match("xyz", &["eq.?l".to_string()]));
}

#[test]
fn matches_any_pattern_plain_string_is_substring() {
    // A metacharacter-free pattern behaves like the old `.contains()`.
    assert!(super::allowed_pattern_match("respond_to_missing?", &["respond_to".to_string()]));
    assert!(!super::allowed_pattern_match("foo", &["respond_to".to_string()]));
}

#[test]
fn matches_any_pattern_invalid_is_skipped_not_panicking() {
    // Unbalanced `[` fails to compile; must not panic and must not match.
    assert!(!super::allowed_pattern_match("anything", &["[invalid".to_string()]));
}

#[test]
fn matches_any_pattern_empty_list_is_false() {
    assert!(!super::allowed_pattern_match("anything", &[]));
}
```

**Step 3: Run tests to verify they fail**

Run: `cargo test -p murphy-plugin-api matches_any_pattern`
Expected: FAIL to compile — `cannot find function 'allowed_pattern_match' in module 'super'`.

**Step 4: Write the implementation**

Add to `cx.rs` at module scope (outside `impl Cx`, e.g. just above the `#[cfg(test)]` module). Place the `Cx` method inside the main `impl Cx<'_>` block near `options_or_default` (around line 2782):

```rust
// --- AllowedPatterns regex matching (murphy-b3n5) ---

thread_local! {
    /// Per-thread compiled-regex cache for AllowedPatterns. `None` caches a
    /// compile failure so a bad pattern is compiled at most once per thread.
    static PATTERN_CACHE: RefCell<HashMap<String, Option<regex::Regex>>> =
        RefCell::new(HashMap::new());
}

/// Dedups the invalid-pattern diagnostic across threads (cold path only).
static REPORTED_INVALID: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

/// True if `name` matches any entry in `patterns` as an unanchored RE2 regex.
/// Each distinct pattern is compiled once per thread and cached. A pattern
/// that fails to compile is reported once (stderr) and skipped.
pub(crate) fn allowed_pattern_match(name: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|pat| {
        PATTERN_CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            let compiled = cache.entry(pat.clone()).or_insert_with(|| {
                match regex::Regex::new(pat) {
                    Ok(re) => Some(re),
                    Err(err) => {
                        report_invalid_pattern(pat, &err);
                        None
                    }
                }
            });
            compiled.as_ref().is_some_and(|re| re.is_match(name))
        })
    })
}

/// Emit a one-time stderr diagnostic for a pattern that failed to compile.
/// stderr (not stdout) because ADR 0006 freezes the stdout offense-JSON
/// contract. Confined to one function so a future structured `cx.warn()`
/// channel only needs to change here.
fn report_invalid_pattern(pattern: &str, err: &regex::Error) {
    if REPORTED_INVALID
        .lock()
        .expect("REPORTED_INVALID poisoned")
        .insert(pattern.to_string())
    {
        eprintln!(
            "murphy: AllowedPatterns: invalid regex `{pattern}`: {err}; pattern ignored"
        );
    }
}
```

And the thin `Cx` method (inside `impl<'a> Cx<'a>` / `impl Cx<'_>`, next to `options_or_default`):

```rust
    /// True if `name` matches any of `patterns` as an unanchored RE2 regex.
    /// Compiled regexes are cached; invalid patterns are reported once and
    /// skipped. See `allowed_pattern_match`.
    pub fn matches_any_pattern(&self, name: &str, patterns: &[String]) -> bool {
        allowed_pattern_match(name, patterns)
    }
```

**Step 5: Run tests to verify they pass**

Run: `cargo test -p murphy-plugin-api matches_any_pattern`
Expected: PASS (5 tests).

**Step 6: Gate + commit**

```bash
cargo clippy -p murphy-plugin-api --all-targets -- -D warnings
cargo +nightly fmt
git add crates/murphy-plugin-api/src/cx.rs
git commit -m "feat(plugin-api): add Cx::matches_any_pattern regex helper (murphy-b3n5)"
```

---

### Task 2: Migrate `class_equality_comparison` to regex

**Files:**
- Modify: `crates/murphy-std/src/cops/style/class_equality_comparison.rs:226-228` (call site), `:54-90` (parity metadata block)
- Test: same file, `#[cfg(test)] mod tests`

**Step 1: Write the failing test**

Add right after the existing `respects_allowed_patterns` test (line 630). Use the
**exact builder harness that test already uses** (`test::<T>().with_options(&ClassEqualityComparisonOptions{...})`)
and an anchored pattern that substring matching cannot satisfy:

```rust
#[test]
fn allowed_patterns_uses_regex_anchors() {
    use super::ClassEqualityComparisonOptions;
    // `\Aequal` anchors to the start of the method name and matches `equal?`.
    // The old substring scan can never match — the name contains no `\A`.
    test::<ClassEqualityComparison>()
        .with_options(&ClassEqualityComparisonOptions {
            allowed_methods: vec![],
            allowed_patterns: vec![r"\Aequal".to_string()],
        })
        .expect_no_offenses(indoc! {r#"
            def equal?(other)
              self.class == other.class
            end
        "#});
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p murphy-std allowed_patterns_uses_regex_anchors`
Expected: FAIL — `"equal?".contains(r"\Aequal")` is false, so `self.class == other.class` is still flagged and `expect_no_offenses` fails.

**Step 3: Implement — swap substring for the helper**

In `is_allowed_context` (line ~226), replace:

```rust
                if opts
                    .allowed_patterns
                    .iter()
                    .any(|p| method_name.contains(p.as_str()))
                {
                    return true;
                }
```

with:

```rust
                if cx.matches_any_pattern(method_name, &opts.allowed_patterns) {
                    return true;
                }
```

(`cx` is already a parameter of `is_allowed_context`.)

**Step 4: Run test to verify it passes**

Run: `cargo test -p murphy-std class_equality_comparison`
Expected: PASS (new test + all existing tests green; `respects_allowed_patterns` with plain `"equal"` still matches as substring-equivalent).

**Step 5: Update the parity metadata block**

In the `//! ## AllowedMethods and AllowedPatterns` doc section (and the `murphy-parity` block if it states the limitation), change wording from "simple substring match (RuboCop uses Regexp), which is a known v1 limitation" to:

```
//!   AllowedPatterns now uses real regex matching (RE2 / Rust `regex`, via
//!   `cx.matches_any_pattern`). Lookahead/backreferences are unsupported and
//!   such patterns are diagnosed (stderr) and skipped.
```

**Step 6: Commit**

```bash
git add crates/murphy-std/src/cops/style/class_equality_comparison.rs
git commit -m "feat(style): ClassEqualityComparison AllowedPatterns uses regex (murphy-b3n5)"
```

---

### Task 3: Migrate `format_string_token` to regex

**Files:**
- Modify: `crates/murphy-std/src/cops/style/format_string_token.rs:319-321` (call site in `is_allowed_method`), parity/doc comment at `:317` ("simple substring match")
- Test: same file, `#[cfg(test)] mod tests`

**Step 1: Write the failing test**

This file has **no** existing `allowed_patterns` test (confirmed). Model the new
test on the file's existing `with_options` builder tests (e.g.
`flags_annotated_token_in_template_mode`, line 671). In default mode the cop
flags the template token `%{greeting}` in `format('%{greeting}', ...)`; the
enclosing method is `format`. An anchored `^format$` exempts it under regex but
not under substring (`"format".contains("^format$")` is false). Add:

```rust
#[test]
fn allowed_patterns_uses_regex() {
    // `^format$` anchors to the enclosing method name `format`; the old
    // substring scan can never match (the name contains no `^`/`$`).
    test::<FormatStringToken>()
        .with_options(&FormatStringTokenOptions {
            allowed_patterns: vec!["^format$".to_string()],
            ..FormatStringTokenOptions::default()
        })
        .expect_no_offenses("format('%{greeting}', greeting: 'Hello')\n");
}
```

`FormatStringTokenOptions` is already imported in the test module (line 607).

**Step 2: Run test to verify it fails**

Run: `cargo test -p murphy-std allowed_patterns_uses_regex`
Expected: FAIL — substring scan does not match `^format$`, so the template token
is still flagged and `expect_no_offenses` fails.

**Step 3: Implement — swap substring for the helper**

In `is_allowed_method` (line ~319), replace:

```rust
            if opts
                .allowed_patterns
                .iter()
                .any(|p| name.contains(p.as_str()))
            {
                return true;
            }
```

with:

```rust
            if cx.matches_any_pattern(name, &opts.allowed_patterns) {
                return true;
            }
```

(`cx` is already a parameter of `is_allowed_method`.)

**Step 4: Run tests to verify they pass**

Run: `cargo test -p murphy-std format_string_token`
Expected: PASS (new test + existing tests green).

**Step 5: Update the doc/parity comment**

Change the `:317` comment "Pattern matching: simple substring match (RuboCop uses Regexp)." to note real regex via `cx.matches_any_pattern`, lookahead/backref skipped with a diagnostic (mirror Task 2 Step 5 wording). Update the `murphy-parity` block if it lists AllowedPatterns substring as a gap.

**Step 6: Commit**

```bash
git add crates/murphy-std/src/cops/style/format_string_token.rs
git commit -m "feat(style): FormatStringToken AllowedPatterns uses regex (murphy-b3n5)"
```

---

### Task 4: DRY-refactor `optional_boolean_parameter` to the shared helper

**Files:**
- Modify: `crates/murphy-std/src/cops/style/optional_boolean_parameter.rs:97-114` (replace inline `thread_local!` regex block)
- Test: same file — existing `^respond_to`, `_missing`, and `skips_invalid_pattern_silently` (line 329, `[invalid` → `expect_offense`) tests must stay green (behavior-preserving refactor; no new test required).

> NOTE: `skips_invalid_pattern_silently` asserts `expect_offense` (invalid pattern ignored, offense still fires) — that still holds. But after the swap the invalid pattern is no longer *silent*: the shared helper emits a one-time stderr diagnostic. Update the test name to `skips_invalid_pattern` and its comment to "...is diagnosed (stderr) and skipped" so the name stays truthful. The assertion itself is unchanged.

**Step 1: Confirm the existing tests are green first**

Run: `cargo test -p murphy-std optional_boolean_parameter`
Expected: PASS (baseline before refactor).

**Step 2: Replace the inline cache with the helper**

Replace lines 97-114 (the `thread_local! { static REGEX_CACHE ... }` block through the `if opts.allowed_patterns.iter().any(...) { return; }`) with:

```rust
        // AllowedPatterns: unanchored regex match (mirrors RuboCop's
        // `pattern.match?(name)`), via the shared cached helper.
        if cx.matches_any_pattern(name, &opts.allowed_patterns) {
            return;
        }
```

Verify `cx` and `name` are in scope at this point (they are — `name` is the def name already bound above this block, `cx` is the visitor param). Remove now-unused imports if the `thread_local`/`RefCell`/`regex` were only used here.

**Step 3: Run tests to verify they still pass**

Run: `cargo test -p murphy-std optional_boolean_parameter`
Expected: PASS — `^respond_to` still allows, `[invalid` still skips (now also emits a stderr diagnostic, which the assertions ignore).

**Step 4: Update the parity/doc comment**

If the file's metadata or comments describe the inline cache, update to "AllowedPatterns via shared `cx.matches_any_pattern` helper".

**Step 5: Commit**

```bash
git add crates/murphy-std/src/cops/style/optional_boolean_parameter.rs
git commit -m "refactor(style): OptionalBooleanParameter uses shared pattern helper (murphy-b3n5)"
```

---

### Task 5: Full workspace gate

**Step 1: Run the full suite and gates**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo +nightly fmt --check
```

Expected: all green. If `fmt --check` fails, run `cargo +nightly fmt` and amend the relevant commit.

**Step 2: Verify the offense-JSON contract is unchanged**

All three migrated cops ship `AllowedPatterns: []` by default
(`default.yml:3658` ClassEqualityComparison, `:4231` FormatStringToken, `:5187`
OptionalBooleanParameter — all empty; the only non-empty default is
`Lint/UnreachableLoop:2596`, which is out of scope). An empty pattern list
matches nothing under both substring and regex, so **default-config behavior is
unchanged** and no determinism/snapshot test should move — only stderr
diagnostics were added.

Run: `cargo test --workspace -- determinism snapshot` (best-effort filter)
Expected: PASS with no snapshot updates.

**Step 3: Final commit (if fmt/clippy required changes)**

```bash
git add -A
git commit -m "chore: workspace gate for AllowedPatterns regex infra (murphy-b3n5)"
```

---

## Done criteria

- `Cx::matches_any_pattern` exists with unit tests covering regex semantics, invalid-skip, and plain-string equivalence.
- `class_equality_comparison` and `format_string_token` match `AllowedPatterns` via regex; new anchored-pattern tests pass.
- `optional_boolean_parameter` uses the shared helper; its existing tests stay green.
- Full workspace `cargo test` / `clippy` / `fmt` green; offense JSON unchanged.
- `murphy-parity` blocks updated to drop the "substring v1 limitation" note.
- Follow-up noted: add `AllowedPatterns` to `Style/SymbolProc` during its gap-fill.
