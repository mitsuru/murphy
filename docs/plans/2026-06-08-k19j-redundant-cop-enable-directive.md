# Redundant Cop Enable Directive Tracking — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement `Lint/RedundantCopEnableDirective` by adding a host primitive `Cx::extra_enabled_directives()` (mirroring RuboCop's `CommentConfig#extra_enabled_comments`) on top of the existing `comment_directives()` infra, including the config-disabled-cop seed threaded through the ABI.

**Architecture:** RuboCop computes redundant enables host-side in `CommentConfig` and the cop is a thin consumer. Mirror that: `Cx::extra_enabled_directives()` walks the existing `comment_directives()` with a per-cop disable-count map seeded by config-disabled cops, returning the redundant enable comments. The config-disabled seed reaches `Cx` via two tail-appended `CxRaw` fields filled host-side. The cop (`RedundantCopEnableDirective`, currently a noop stub) iterates the primitive and emits offenses + autocorrect.

**Tech Stack:** Rust (murphy-plugin-api ABI + Cx, murphy-core dispatch/config, murphy-cli, murphy-std cop). TDD via `cargo test`. mise-activated shell (`eval "$(mise activate bash)"`).

**Reference (read before Task 5):**
- RuboCop cop: `/home/ubuntu/.local/share/mise/installs/ruby/3.3.5/lib/ruby/gems/3.3.0/gems/rubocop-1.87.0/lib/rubocop/cop/lint/redundant_cop_enable_directive.rb`
- RuboCop CommentConfig: same gem `lib/rubocop/comment_config.rb` (`extra_enabled_comments`, `handle_switch`, `handle_enable_all`, lines 56-83, 232-256)

**Working directory:** `/home/ubuntu/projects/murphy/.worktrees/k19j-redundant-enable` (run `eval "$(mise activate bash)"` per shell).

---

## Design decisions (from beads murphy-k19j design)

- **Boundary = host primitive** `Cx::extra_enabled_directives()` (Option B), not cop-internal — the config seed needs host config access and centralizes the count-pass for future reuse (RedundantCopDisableDirective).
- **Full parity including config seed** — `# rubocop:enable Foo` is NOT redundant when `Foo` is `Enabled: false` in `.murphy.yml`.
- **Department-level directives deferred** — `# rubocop:disable Lint` (department) expansion is out of scope; documented as a parity gap + follow-up issue (Task 7).

## Primitive contract

```rust
/// One redundant `# rubocop:enable` comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedundantEnable<'a> {
    /// Range of the whole enable comment (`# rubocop:enable A, B`).
    pub comment_range: Range,
    /// The redundant cop names within that comment, in source order.
    /// `enable all` redundancy is represented as the single sentinel `"all"`.
    pub cop_names: Vec<&'a str>,
    /// True when EVERY cop named by the directive is redundant
    /// (RuboCop's `directive.match?(cop_names)`); drives whole-comment removal.
    pub all_in_directive: bool,
}
```

`Cx::extra_enabled_directives(&self) -> Vec<RedundantEnable<'a>>`.

---

## Task 1: ABI — tail-append `config_disabled_cops` to `CxRaw`

**Files:**
- Modify: `crates/murphy-plugin-api/src/abi.rs` (CxRaw struct ~155-212, doc ~236-240, offset assertions ~399-430)
- Modify (CxRaw construction sites, add the two fields = null/0):
  - `crates/murphy-core/src/dispatch.rs:195` (`build_cx_raw`)
  - `crates/murphy-plugin-api/src/internal.rs:239` (`cx_raw_for` test helper)
  - `crates/murphy-plugin-api/src/cx.rs` (`cx_raw_for` test helper — grep `active_support_extensions_enabled:`)
  - `crates/murphy-plugin-api/src/test_support.rs` (grep `active_support_extensions_enabled:`)
  - `crates/murphy-plugin-macros/tests/{register_modes_equivalence,cop_attr_behavior,node_pattern_behavior,cross_backend_conformance}.rs`

**Step 1: Add the fields to `CxRaw`**

In `abi.rs`, after `pub active_support_extensions_enabled: bool,` (line 211):

```rust
    /// Cop names disabled by config (`Enabled: false` in `.murphy.yml`), used
    /// as the seed for `Cx::extra_enabled_directives()` — RuboCop's
    /// `registry.disabled(config)`. Run-wide; the same slice is shared by every
    /// cop in a run. Tail-appended under ABI v4 lockstep (murphy-k19j); empty
    /// (`null`/`0`) when no config-disabled cops or on option-only entry points.
    pub config_disabled_cops: *const RawSlice,
    pub config_disabled_cops_len: usize,
```

Extend the ABI-evolution doc block (~236-239) with a `murphy-k19j` line noting the tail-append. Do NOT bump `MURPHY_PLUGIN_ABI_VERSION` (project policy for tail-appended CxRaw fields).

**Step 2: Add offset assertions**

Find the CxRaw `offset_of!` test block in `abi.rs` (near line 414). Add assertions for the two new fields at the tail (mirror the existing pattern — assert each new field's offset follows the prior field). Keep the existing `size_of::<CxRaw>()` guard consistent (the two pointer-sized + usize fields grow the struct; update any hard-coded size assertion accordingly).

**Step 3: Update every CxRaw construction site**

Append to each CxRaw literal:

```rust
        config_disabled_cops: std::ptr::null(),
        config_disabled_cops_len: 0,
```

(In `dispatch.rs build_cx_raw` this becomes wired in Task 4; for now null/0.)

**Step 4: Build**

Run: `eval "$(mise activate bash)" && cargo build 2>&1 | tail -5`
Expected: compiles clean (the macros tests are compiled on `cargo test`; run `cargo test -p murphy-plugin-macros --no-run` to confirm those literals too).

**Step 5: Commit**

```bash
git add crates/murphy-plugin-api/src/abi.rs crates/murphy-core/src/dispatch.rs crates/murphy-plugin-api/src/internal.rs crates/murphy-plugin-api/src/cx.rs crates/murphy-plugin-api/src/test_support.rs crates/murphy-plugin-macros/tests/
git commit -m "feat(abi): tail-append config_disabled_cops to CxRaw (murphy-k19j)"
```

---

## Task 2: `Cx::extra_enabled_directives()` — core count pass (no seed yet)

**Files:**
- Modify: `crates/murphy-plugin-api/src/cx.rs` (add `RedundantEnable` struct near `CommentDirective` ~70; add method near `comment_directives()` ~2628; add tests near line 3271)

**Step 1: Write the failing test**

Add to the `#[cfg(test)]` module in `cx.rs` (mirror `comment_directives_expose_same_line_and_block_ranges` for Cx construction via `cx_raw_for`):

```rust
#[test]
fn extra_enabled_directives_flags_enable_without_disable() {
    let source = concat!(
        "foo = 1\n",
        "# rubocop:enable Layout/LineLength\n",
    );
    let ast = murphy_translate::translate(source, "t.rb");
    let fns = FnTable { emit_offense: noop_offense, emit_edit: noop_edit };
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };

    let extras = cx.extra_enabled_directives();
    assert_eq!(extras.len(), 1);
    assert_eq!(extras[0].cop_names, vec!["Layout/LineLength"]);
    assert!(extras[0].all_in_directive);
}

#[test]
fn extra_enabled_directives_ignores_paired_disable_enable() {
    let source = concat!(
        "# rubocop:disable Style/StringLiterals\n",
        "foo = \"1\"\n",
        "# rubocop:enable Style/StringLiterals\n",
    );
    let ast = murphy_translate::translate(source, "t.rb");
    let fns = FnTable { emit_offense: noop_offense, emit_edit: noop_edit };
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };
    assert!(cx.extra_enabled_directives().is_empty());
}

#[test]
fn extra_enabled_directives_partial_redundancy() {
    // A disabled, B never disabled -> only B redundant, not all_in_directive.
    let source = concat!(
        "# rubocop:disable A\n",
        "foo\n",
        "# rubocop:enable A, B\n",
    );
    let ast = murphy_translate::translate(source, "t.rb");
    let fns = FnTable { emit_offense: noop_offense, emit_edit: noop_edit };
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };
    let extras = cx.extra_enabled_directives();
    assert_eq!(extras.len(), 1);
    assert_eq!(extras[0].cop_names, vec!["B"]);
    assert!(!extras[0].all_in_directive);
}

#[test]
fn extra_enabled_directives_enable_all_with_nothing_disabled() {
    let source = "foo\n# rubocop:enable all\n";
    let ast = murphy_translate::translate(source, "t.rb");
    let fns = FnTable { emit_offense: noop_offense, emit_edit: noop_edit };
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };
    let extras = cx.extra_enabled_directives();
    assert_eq!(extras.len(), 1);
    assert_eq!(extras[0].cop_names, vec!["all"]);
    assert!(extras[0].all_in_directive);
}

#[test]
fn extra_enabled_directives_skips_inline_directives() {
    // Same-line (inline) enable is not a comment-only line -> skipped.
    let source = "foo # rubocop:enable Layout/LineLength\n";
    let ast = murphy_translate::translate(source, "t.rb");
    let fns = FnTable { emit_offense: noop_offense, emit_edit: noop_edit };
    let raw = cx_raw_for(&ast, &fns);
    let cx = unsafe { Cx::from_raw(&raw) };
    assert!(cx.extra_enabled_directives().is_empty());
}
```

**Step 2: Run to verify it fails**

Run: `eval "$(mise activate bash)" && cargo test -p murphy-plugin-api extra_enabled_directives 2>&1 | tail -15`
Expected: FAIL — `no method named extra_enabled_directives`.

**Step 3: Implement the struct + method**

Near `CommentDirective` (cx.rs ~70) add the `RedundantEnable<'a>` struct (see Primitive contract above). Add to the `impl<'a> Cx<'a>` block (near `comment_directives()` ~2628):

```rust
/// Redundant `# rubocop:enable` comments — Murphy's analog of RuboCop's
/// `CommentConfig#extra_enabled_comments`. Walks `comment_directives()` in
/// source order with a per-cop disable-count map (seeded by
/// `config_disabled_cops`); an enable for a cop whose count is zero is
/// redundant. Inline (same-line) directives are skipped, matching RuboCop's
/// `comment_only_line?`.
pub fn extra_enabled_directives(&self) -> Vec<RedundantEnable<'a>> {
    use std::collections::HashMap;
    let directives = self.comment_directives();

    // Seed: config-disabled cops start with count 1 (RRuboCop registry.disabled).
    let mut count: HashMap<&str, i32> = HashMap::new();
    for name in self.config_disabled_cops() {
        *count.entry(name).or_insert(0) += 1;
    }

    // Group consecutive directives by comment_range so per-comment redundancy
    // and `all_in_directive` can be decided. comment_directives() emits the
    // per-cop split for one comment contiguously and in source order.
    let mut out: Vec<RedundantEnable<'a>> = Vec::new();
    let mut i = 0;
    while i < directives.len() {
        let d0 = &directives[i];
        // Collect the run sharing this comment_range.
        let mut j = i;
        while j < directives.len() && directives[j].comment_range == d0.comment_range {
            j += 1;
        }
        let group = &directives[i..j];
        i = j;

        // Skip inline directives (RuboCop comment_only_line? == false).
        if d0.scope == CommentDirectiveScope::SameLine {
            continue;
        }

        match d0.kind {
            CommentDirectiveKind::Disable => {
                for d in group {
                    if let Some(cop) = d.cop {
                        *count.entry(cop).or_insert(0) += 1;
                    }
                }
            }
            CommentDirectiveKind::Enable => {
                // enable all (cop == None): decrement every positive count;
                // redundant iff none were positive.
                if group.iter().any(|d| d.cop.is_none()) {
                    let mut enabled_any = false;
                    for v in count.values_mut() {
                        if *v > 0 {
                            *v -= 1;
                            enabled_any = true;
                        }
                    }
                    if !enabled_any {
                        out.push(RedundantEnable {
                            comment_range: d0.comment_range,
                            cop_names: vec!["all"],
                            all_in_directive: true,
                        });
                    }
                } else {
                    let mut redundant: Vec<&str> = Vec::new();
                    let mut total = 0;
                    for d in group {
                        let Some(cop) = d.cop else { continue };
                        total += 1;
                        let slot = count.entry(cop).or_insert(0);
                        if *slot > 0 {
                            *slot -= 1;
                        } else {
                            redundant.push(cop);
                        }
                    }
                    if !redundant.is_empty() {
                        let all_in_directive = redundant.len() == total;
                        out.push(RedundantEnable {
                            comment_range: d0.comment_range,
                            cop_names: redundant,
                            all_in_directive,
                        });
                    }
                }
            }
            CommentDirectiveKind::Todo => {}
        }
    }
    out
}
```

Add a temporary `fn config_disabled_cops(&self) -> impl Iterator<Item = &'a str>` returning `std::iter::empty()` for now (Task 3 implements it against the raw fields). To keep Task 2 self-contained, stub it:

```rust
fn config_disabled_cops(&self) -> std::vec::IntoIter<&'a str> {
    Vec::new().into_iter() // replaced in Task 3
}
```

**Step 4: Run to verify it passes**

Run: `eval "$(mise activate bash)" && cargo test -p murphy-plugin-api extra_enabled_directives 2>&1 | tail -15`
Expected: PASS (5 tests).

**Step 5: Commit**

```bash
git add crates/murphy-plugin-api/src/cx.rs
git commit -m "feat(cx): add extra_enabled_directives count-pass primitive (murphy-k19j)"
```

---

## Task 3: Config seed wiring into `Cx`

**Files:**
- Modify: `crates/murphy-plugin-api/src/cx.rs` (real `config_disabled_cops()`; seed test + a seed-aware `cx_raw_for`)

**Step 1: Write the failing seed test**

The cx.rs test `cx_raw_for(&ast, &fns)` builds a CxRaw with `config_disabled_cops: null`. Add a seed-aware helper and a test:

```rust
fn cx_raw_with_disabled<'a>(ast: &'a Ast, fns: &'a FnTable, disabled: &'a [RawSlice]) -> CxRaw {
    let mut raw = cx_raw_for(ast, fns);
    raw.config_disabled_cops = disabled.as_ptr();
    raw.config_disabled_cops_len = disabled.len();
    raw
}

#[test]
fn extra_enabled_directives_seed_suppresses_config_disabled_enable() {
    // Foo is disabled in config -> `# rubocop:enable Foo` is NOT redundant.
    let source = "foo\n# rubocop:enable Foo\n";
    let ast = murphy_translate::translate(source, "t.rb");
    let fns = FnTable { emit_offense: noop_offense, emit_edit: noop_edit };
    let disabled = [RawSlice::from_str("Foo")];
    let raw = cx_raw_with_disabled(&ast, &fns, &disabled);
    let cx = unsafe { Cx::from_raw(&raw) };
    assert!(cx.extra_enabled_directives().is_empty());
}
```

**Step 2: Run to verify it fails**

Run: `eval "$(mise activate bash)" && cargo test -p murphy-plugin-api extra_enabled_directives_seed 2>&1 | tail -15`
Expected: FAIL — `# rubocop:enable Foo` flagged because the stub seed is empty.

**Step 3: Implement `config_disabled_cops()` against the raw fields**

Replace the Task 2 stub:

```rust
fn config_disabled_cops(&self) -> impl Iterator<Item = &'a str> {
    let slices = unsafe {
        slice(self.raw.config_disabled_cops, self.raw.config_disabled_cops_len)
    };
    slices.iter().filter_map(|s| std::str::from_utf8(unsafe { s.as_bytes() }).ok())
}
```

(Confirm the existing private `slice` helper + `RawSlice::as_bytes` signatures; mirror how `comments()` / `sorted_tokens()` read their ptr+len pairs.)

**Step 4: Run to verify it passes**

Run: `eval "$(mise activate bash)" && cargo test -p murphy-plugin-api extra_enabled_directives 2>&1 | tail -15`
Expected: PASS (6 tests).

**Step 5: Commit**

```bash
git add crates/murphy-plugin-api/src/cx.rs
git commit -m "feat(cx): seed extra_enabled_directives from config_disabled_cops (murphy-k19j)"
```

---

## Task 4: Host wiring — enumerate config-disabled cops + thread into dispatch

**Files:**
- Modify: `crates/murphy-core/src/config.rs` (add `disabled_cop_names()`)
- Modify: `crates/murphy-core/src/dispatch.rs` (`build_cx_raw` + `run_cops_with_options_and_context` gain a `config_disabled_cops: &[RawSlice]` param; default wrapper + tests pass `&[]`)
- Modify: `crates/murphy-cli/src/main.rs` (`lint_source` ~421, `lint_source_timed` ~560 build the RawSlice vec and pass it)

**Step 1: Write the failing host test (config enumeration)**

In `config.rs` tests:

```rust
#[test]
fn disabled_cop_names_lists_enabled_false_rules() {
    let cfg = MurphyConfig::from_yaml_str(
        "Style/StringLiterals:\n  Enabled: false\nLint/Debugger:\n  Enabled: true\n"
    ).unwrap();
    let names: Vec<&str> = cfg.disabled_cop_names().collect();
    assert!(names.contains(&"Style/StringLiterals"));
    assert!(!names.contains(&"Lint/Debugger"));
}
```

(Confirm the actual config constructor name used in existing config.rs tests — grep `fn from_yaml` / how tests build `MurphyConfig`.)

**Step 2: Run to verify it fails**

Run: `eval "$(mise activate bash)" && cargo test -p murphy-core disabled_cop_names 2>&1 | tail -15`
Expected: FAIL — no method `disabled_cop_names`.

**Step 3: Implement `disabled_cop_names()`**

In `config.rs` (near `allcops_context`):

```rust
/// Cop names the resolved config disables (`Enabled: false`), for seeding
/// `Cx::extra_enabled_directives()`. Mirrors RuboCop's `registry.disabled(config)`
/// for the explicitly-configured subset (default-disabled-but-unconfigured cops
/// are out of scope — see murphy-k19j parity note).
pub fn disabled_cop_names(&self) -> impl Iterator<Item = &str> {
    self.cops.rules.iter()
        .filter(|(_, rule)| rule.enabled == Some(false))
        .map(|(name, _)| name.as_str())
}
```

(Verify field path `self.cops.rules` + `rule.enabled: Option<bool>` against config.rs:62/85; adjust if base_defaults layering must be merged. If a merged/effective view is needed, reuse the existing resolution path rather than re-implementing.)

**Step 4: Thread through dispatch**

`run_cops_with_options_and_context` + `build_cx_raw` gain `config_disabled_cops: &[RawSlice]`. In `build_cx_raw`, set:

```rust
        config_disabled_cops: config_disabled_cops.as_ptr(),
        config_disabled_cops_len: config_disabled_cops.len(),
```

`run_cops_with_options` (the no-context wrapper, ~258) and the dispatch.rs tests (~456/508/525) pass `&[]`.

**Step 5: Wire the CLI**

In `lint_source` and `lint_source_timed`, before the dispatch call build the seed slice from config (config owns the name strings → RawSlices borrow them for the call's duration):

```rust
let disabled_names: Vec<RawSlice> = config
    .disabled_cop_names()
    .map(|n| RawSlice { ptr: n.as_ptr(), len: n.len() })
    .collect();
dispatch::run_cops_with_options_and_context(
    &ast,
    &scoped_cops,
    &mut sink,
    config.allcops_context(),
    &disabled_names,
    |name| config.cop_options_json(name),
);
```

(Confirm `RawSlice` is importable in main.rs / dispatch.rs; reuse `RawSlice::from_str`-style construction if a borrow helper exists.)

**Step 6: Run to verify it passes + no regressions**

Run: `eval "$(mise activate bash)" && cargo test -p murphy-core -p murphy-cli 2>&1 | grep -E "test result|error\[" | tail`
Expected: all PASS.

**Step 7: Commit**

```bash
git add crates/murphy-core/src/config.rs crates/murphy-core/src/dispatch.rs crates/murphy-cli/src/main.rs
git commit -m "feat(core): thread config-disabled cop seed into dispatch (murphy-k19j)"
```

---

## Task 5: Implement the `RedundantCopEnableDirective` cop

**Files:**
- Modify: `crates/murphy-std/src/cops/lint/redundant_cop_enable_directive.rs` (replace noop)
- Read first: RuboCop `redundant_cop_enable_directive.rb` (range helpers `range_with_comma`, `range_to_remove`, `range_of_offense`).

**Step 1: Write the failing cop tests** (port from RuboCop spec)

```rust
#[cfg(test)]
mod tests {
    use super::RedundantCopEnableDirective;
    use indoc::indoc;
    use murphy_plugin_api::test_support::test;

    #[test]
    fn flags_enable_without_disable() {
        test::<RedundantCopEnableDirective>().expect_offense(indoc! {r#"
            foo = 1
            # rubocop:enable Layout/LineLength
                             ^^^^^^^^^^^^^^^^^ Unnecessary enabling of Layout/LineLength.
        "#});
    }

    #[test]
    fn accepts_enable_matching_a_disable() {
        test::<RedundantCopEnableDirective>().expect_no_offenses(indoc! {r#"
            # rubocop:disable Style/StringLiterals
            foo = "1"
            # rubocop:enable Style/StringLiterals
        "#});
    }

    #[test]
    fn corrects_whole_comment_when_all_redundant() {
        test::<RedundantCopEnableDirective>().expect_correction(
            indoc! {r#"
                foo = 1
                # rubocop:enable Layout/LineLength
            "#},
            indoc! {r#"
                foo = 1
            "#},
        );
    }

    #[test]
    fn corrects_partial_redundancy_removing_one_cop() {
        // A disabled, B not -> remove only B (with its comma).
        test::<RedundantCopEnableDirective>().expect_correction(
            indoc! {r#"
                # rubocop:disable A
                foo
                # rubocop:enable A, B
            "#},
            indoc! {r#"
                # rubocop:disable A
                foo
                # rubocop:enable A
            "#},
        );
    }
}
```

Cross-check the exact caret column and corrected whitespace against RuboCop by running the real gem on the same snippets:
`echo '<snippet>' | (cd /tmp && rubocop --require rubocop --only Lint/RedundantCopEnableDirective -a -)` — adjust expectations to match RuboCop byte-for-byte.

**Step 2: Run to verify it fails**

Run: `eval "$(mise activate bash)" && cargo test -p murphy-std redundant_cop_enable 2>&1 | tail -20`
Expected: FAIL — noop emits nothing.

**Step 3: Implement the cop**

Replace the `noop` body. Sketch (port RuboCop `register_offense` / `range_*`):

```rust
impl RedundantCopEnableDirective {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        // Fast path mirrors RuboCop's `raw_source.include?('enable')`.
        if !cx.source().contains("enable") {
            return;
        }
        for extra in cx.extra_enabled_directives() {
            let comment_src = cx.raw_source(extra.comment_range);
            for name in &extra.cop_names {
                let Some(idx) = comment_src.find(name) else { continue };
                let start = extra.comment_range.start + idx as u32;
                let offense_range = Range { start, end: start + name.len() as u32 };
                let shown = if *name == "all" { "all cops" } else { name };
                cx.emit_offense(offense_range, &format!("Unnecessary enabling of {shown}."), None);

                if extra.all_in_directive {
                    // Remove the whole enable comment + trailing newline/space.
                    cx.emit_edit(whole_comment_removal_range(extra.comment_range, cx), "");
                } else {
                    cx.emit_edit(comma_removal_range(extra.comment_range, comment_src, idx, name.len(), cx), "");
                }
            }
        }
    }
}
```

Implement `whole_comment_removal_range` (comment range expanded right through the newline — see RuboCop `range_with_surrounding_space(side: :right)`; reuse `cx.range_with_surrounding_space` if available, else compute) and `comma_removal_range` (port `range_to_remove`/`range_with_comma_before`/`range_with_comma_after`). Follow `.claude/rules/autocorrect-pattern.md` (surgical edits) and `.claude/rules/offense-messages.md` (`format!`, done above). When `all_in_directive`, emit the whole-comment removal once per comment, not once per name (guard with a `seen` flag or emit before the name loop).

**Step 4: Run to verify it passes**

Run: `eval "$(mise activate bash)" && cargo test -p murphy-std redundant_cop_enable 2>&1 | tail -20`
Expected: PASS. Verify idempotence (expect_correction reaching fixpoint is checked by the harness).

**Step 5: Update parity metadata**

In the cop's `//! ```murphy-parity` block: `status: partial` → `status: complete`; remove the "cannot be implemented" note; add a department-directive gap note referencing the Task 7 follow-up issue. Remove the obsolete "Known v1 limitation" doc section.

**Step 6: Commit**

```bash
git add crates/murphy-std/src/cops/lint/redundant_cop_enable_directive.rs
git commit -m "feat(murphy-std): implement Lint/RedundantCopEnableDirective (murphy-k19j)"
```

---

## Task 6: CLI seed-parity integration test

**Files:**
- Modify: `crates/murphy-cli/tests/cli.rs` (add a test)

**Step 1: Write the test**

A fixture with `.murphy.yml` disabling a cop and a source containing `# rubocop:enable <that cop>` on its own line must NOT report RedundantCopEnableDirective for it; the same enable for a NON-disabled cop must report. Use the existing cli.rs harness pattern (grep an existing multi-file/`.murphy.yml` test for the scaffold).

**Step 2: Run**

Run: `eval "$(mise activate bash)" && cargo test -p murphy-cli --test cli redundant_enable 2>&1 | tail -15`
Expected: PASS.

**Step 3: Commit**

```bash
git add crates/murphy-cli/tests/cli.rs
git commit -m "test(cli): config-disabled seed parity for RedundantCopEnableDirective (murphy-k19j)"
```

---

## Task 7: Department-directive follow-up issue + final gates

**Step 1: File the follow-up beads issue**

```bash
bd create --title="Infra: expand department-level rubocop:enable/disable directives to cop names" \
  --type=task --priority=3 \
  --description="RedundantCopEnableDirective (murphy-k19j) handles cop-name-level enable/disable only. RuboCop expands department directives (e.g. # rubocop:disable Lint) to member cop names via the registry. extra_enabled_directives() treats a department token as an opaque cop name, so department-scoped enable/disable pairing is not parity-correct. Needs registry department->cops resolution exposed to the directive count-pass."
```

Note the new issue ID in the cop's parity gap note (Task 5 Step 5) — amend if needed.

**Step 2: Full quality gates**

```bash
eval "$(mise activate bash)"
cargo test --workspace 2>&1 | grep -E "test result|error" | tail -40
cargo +nightly fmt --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -20
```
Expected: all green.

**Step 3: Commit any fmt/clippy fixes**

```bash
git add -A && git commit -m "chore(murphy-k19j): fmt + clippy"
```

---

## Acceptance checklist (from beads murphy-k19j)

- [ ] `Cx::extra_enabled_directives()` returns redundant enables (`"all"` sentinel for enable-all), seeded by config-disabled cops.
- [ ] `CxRaw` gains tail-appended `config_disabled_cops` fields; murphy-core fills them; ABI back-compat preserved (offset asserts updated, no version bump).
- [ ] Cop flags `# rubocop:enable Foo` with no active disable and `# rubocop:enable all` when nothing disabled; no longer a noop.
- [ ] Inline (same-line) directives skipped (matches `comment_only_line?`).
- [ ] Autocorrect: whole-comment removal when all redundant; comma-aware single-cop removal when partial; idempotent.
- [ ] Config seed parity: `# rubocop:enable Foo` NOT flagged when `Foo` is `Enabled: false`.
- [ ] Department directives out of scope (follow-up issue filed, parity note updated).
- [ ] `cargo test --workspace`, `cargo +nightly fmt --check`, `cargo clippy --all-targets -D warnings` all green.
