# AllowedPatterns — real regex matching infrastructure — design

Issue: `murphy-b3n5`
Scope: `crates/murphy-plugin-api`, `crates/murphy-std`
Status: design approved 2026-06-04

## 1. Why

RuboCop's `AllowedPatterns` cop option compiles each entry with `Regexp.new`
and matches it (unanchored) against a candidate string — usually a method
name. Murphy's `#[derive(CopOptions)]` only supports `Vec<String>`, so cops
that expose `AllowedPatterns` currently fake the match with a **substring
scan** (`name.contains(pattern)`). This is a known v1 limitation: a user who
writes an anchored or metacharacter pattern (`\Atest_`, `eq.*l`, `^foo`) does
not get regex semantics.

The regex engine is already present — `regex = "1"` (Rust RE2) is a dependency
of `murphy-plugin-api` and re-exported (`pub use regex`) for `def_node_matcher!`.
What is missing is the wiring from an `AllowedPatterns` string list to a real
regex match.

Cops currently faking it with substring:

- `crates/murphy-std/src/cops/style/class_equality_comparison.rs:228`
- `crates/murphy-std/src/cops/style/format_string_token.rs:321`

**Already does real regex** (discovered during planning):
`crates/murphy-std/src/cops/style/optional_boolean_parameter.rs:97-114` already
compiles `regex::Regex::new(pat)` behind an inline `thread_local!` cache and
matches unanchored — its existing tests pin `^respond_to` (anchor) and
`[invalid` (invalid → silently skipped). For this cop the work is a **DRY
refactor**: replace the inline cache with the shared helper. It is the de-facto
reference implementation, and its `thread_local!` cache is the precedent this
design generalizes.

`Style/SymbolProc` wants `AllowedPatterns` too but has not implemented even the
substring stand-in; it becomes a consumer of this infrastructure (out of scope
here — see §6).

## 2. Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Regex engine | **RE2 (Rust `regex`)** | Already a dependency; linear-time, no C dep, fits the "fast native Rust" thesis. Lookahead/backreferences are unsupported and rare for method-name matching. |
| API shape | **`cx` helper + cache** | Keeps options as `Vec<String>`; cops swap one line. No derive/option-cache changes (minimal churn). |
| Cache | **`thread_local!` per-thread** | Matches existing precedent in `optional_boolean_parameter`; lock-free hot path. A separate global `Mutex<HashSet>` dedups the invalid-pattern diagnostic across threads. |
| Invalid pattern | **Diagnostic + skip** | RE2-incompatible patterns emit a one-time stderr warning and are treated as non-matching; processing continues. |
| Scope | **Helper + migrate 2 substring cops + refactor 1** | `class_equality_comparison` and `format_string_token` switch from substring to regex; `optional_boolean_parameter` swaps its inline cache for the shared helper. `SymbolProc` is a follow-up. |

## 3. API surface

New helper on `Cx` (`crates/murphy-plugin-api/src/cx.rs`):

```rust
impl Cx<'_> {
    /// True if `name` matches any of `patterns` as an unanchored RE2 regex.
    /// Each distinct pattern is compiled once per process and cached.
    /// A pattern that fails to compile is reported once (stderr) and skipped
    /// (never counts as a match).
    pub fn matches_any_pattern(&self, name: &str, patterns: &[String]) -> bool
}
```

Cop change (one line):

```rust
// before — substring stand-in
opts.allowed_patterns.iter().any(|p| name.contains(p.as_str()))
// after — real regex
cx.matches_any_pattern(name, &opts.allowed_patterns)
```

The option type stays `Vec<String>`. The derive macro, `from_config_json`, and
the option-decode path are untouched. `AllowedMethods` (exact match) remains a
cop-side check; the helper covers only the pattern list.

**Match semantics**: `Regex::is_match` is unanchored, matching RuboCop's
`match?`/`=~`. No implicit flags (`i`/`m`/`x`) — same as `Regexp.new`. A
pattern with no metacharacters behaves identically to the old `.contains()`;
only metacharacter patterns change meaning (that is the fix).

## 4. Cache and concurrency

Cops run under rayon across all cores
(`crates/murphy-core/src/mruby/proxy.rs`). The cache is **per-thread**, matching
the precedent already in `optional_boolean_parameter`:

```rust
thread_local! {
    static PATTERN_CACHE: RefCell<HashMap<String, Option<Regex>>> =
        RefCell::new(HashMap::new());
}
// Dedups the invalid-pattern diagnostic across threads (cold path only).
static REPORTED_INVALID: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));
```

- Key: the pattern string. Value: `Some(Regex)` on success, `None` on compile
  failure — failures are cached per-thread so a bad pattern is compiled at most
  once per thread.
- **Lock-free hot path**: matching only touches the `thread_local` cache. The
  global `Mutex` is taken only on the rare compile-failure path, to ensure the
  diagnostic is emitted once across the whole process.
- The cache is bounded by the finite set of config-derived patterns, so no
  eviction is needed. Per-thread duplication is negligible (a handful of short
  patterns × core count).

Contrast with `def_node_matcher!`, which emits a `static LazyLock<Regex>` per
**compile-time-known** literal pattern (`node_pattern.rs:3286`). `AllowedPatterns`
are **config-derived (runtime-known)**, so a static gensym is impossible — hence
the string-keyed dynamic cache.

Match procedure in `matches_any_pattern`:

1. Iterate `patterns`.
2. Look each up in the thread-local cache (miss → compile + store
   `Some`/`None`; on failure also report via the global dedup set).
3. `Some(re)` → `re.is_match(name)`; `true` returns early.
4. `None` → skip (already diagnosed).
5. No match across all → `false`.

## 5. Error handling

On first compile failure for a pattern (e.g. lookahead `(?=...)`, backreference
`\1`):

- Emit a single stderr warning and store `Err(())` in the cache. Subsequent
  encounters of the same pattern are silent.
- Treat the pattern as non-matching; the cop continues.

**stderr, not stdout**: ADR 0006 freezes the stdout offense-JSON contract;
diagnostics must not pollute it. `Cx` has no existing warn/diagnostic channel,
so the minimal implementation is a deduped `eprintln!` confined to one function
(`report_invalid_pattern(pattern, err)`) — easy to reroute if a structured
`cx.warn()` channel is added later.

Message shape:

```
murphy: AllowedPatterns: invalid regex `(?=foo)`: <regex crate error>; pattern ignored
```

**Why not a config-load ConfigError**: Approach A keeps options as `Vec<String>`
compiled at runtime; the config loader does not know which options are pattern
lists. Load-time validation (the rejected "Approach C") cleanly separates
validation from matching but couples the loader to per-option semantics. It is
left as a documented future option on the issue.

## 6. Migration and testing

**Migrate the 2 substring cops** (`class_equality_comparison`,
`format_string_token`): swap `.iter().any(|p| name.contains(p))` for
`cx.matches_any_pattern(name, &opts.allowed_patterns)`. Update each
`murphy-parity` metadata block: "substring match, known v1 limitation" → "full
regex (RE2 subset); lookahead/backreferences unsupported, diagnosed and
skipped". Where `AllowedPatterns` was the sole reason for `status: partial`,
revisit the wording (keep `partial` if other gaps remain).

**Refactor `optional_boolean_parameter`**: replace the inline `thread_local!`
regex cache (lines 97-114) with `cx.matches_any_pattern`. Behavior is unchanged
except that an invalid pattern now also emits the shared stderr diagnostic
(previously silent) — its existing `[invalid` test still passes (assertions read
offenses/stdout, not stderr).

**Tests (TDD — failing test first)**:

- Helper unit tests (`cx.rs` `#[cfg(test)]`):
  - regex semantics: `^equal` anchors and matches `equal?`; mid-string match;
    `eq.*l` metacharacter match — cases substring cannot distinguish.
  - invalid pattern `(?=x)` does not panic and returns `false`.
  - a plain (metacharacter-free) pattern behaves identically to `.contains()`
    (backward compatibility).
- Per-cop: add metacharacter/anchor cases to existing `AllowedPatterns` tests
  (e.g. `class_equality_comparison` honoring `\Aequal\z`). Existing plain-string
  tests stay green (no regression).
- Determinism/idempotence: existing snapshot/determinism tests unchanged — the
  offense JSON does not change.

**Out of scope (follow-up)**: adding `AllowedPatterns` to `Style/SymbolProc` is
done as part of the SymbolProc gap-fill (alongside `itblock` etc.). After this
issue lands, the SymbolProc side just calls the helper.
