# RuboCop Rails Default Options Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Wire Rails-pack cop defaults into murphy-std while preserving built-in RuboCop defaults.

**Architecture:** Decode options at each cop's existing Cx options boundary. Add typed list options for the two safe-navigation cops, add AllowedPatterns to NumberConversion, and manually decode the InverseMethods map for InvertibleUnlessCondition. Merge configured entries with intrinsic defaults.

**Tech Stack:** Rust 2024, murphy-plugin-api CopOptions, serde_json, Rust regex, parser-driven cop tests, Cargo.

## Global Constraints

- Do not change MURPHY_PLUGIN_ABI_VERSION.
- Do not change crates/murphy-rails/config/default.yml.
- Use PascalCase option names.
- Write failing tests before production code.
- Preserve the existing .beads/.gitignore change.
- Use cx.options_or_default inside cop investigations.

## File Map

- safe_navigation_chain.rs: AllowedMethods options and additive nil-safe lookup.
- redundant_safe_navigation.rs: AllowedMethods options and additive nil-safe lookup.
- number_conversion.rs: AllowedPatterns option and regex matching.
- invertible_unless_condition.rs: manual InverseMethods decoding and effective map.
- symbol_proc.rs: Rails allowlist regression tests.
- collection_compact.rs and and_or.rs: retain existing Rails option guards.
- Four newly wired cop parity comments: remove stale unconfigurable claims.

## Task 1: Lint/SafeNavigationChain

**Files:** Modify crates/murphy-std/src/cops/lint/safe_navigation_chain.rs.

**Produces:** SafeNavigationChainOptions with allowed_methods: Vec<String>; is_nil_safe_method accepts the options.

- [ ] Write failing tests for configured presence_in and for preserving built-in nil?.
- [ ] Run cargo test -p murphy-std --lib safe_navigation_chain and confirm compilation fails because the options type is absent.
- [ ] Add a CopOptions struct with AllowedMethods defaulting to an empty list; read it with cx.options_or_default and union it with NIL_SAFE_METHODS.
  The target option shape is:

      #[derive(CopOptions)]
      pub struct SafeNavigationChainOptions {
          #[option(name = "AllowedMethods", default = [])]
          pub allowed_methods: Vec<String>,
      }

  The lookup must remain additive:

      NIL_SAFE_METHODS.contains(&method)
          || opts.allowed_methods.iter().any(|allowed| allowed == method)
- [ ] Run the focused test and confirm all existing and new tests pass.
- [ ] Commit with git commit -m "feat(std): wire SafeNavigationChain allowed methods".

## Task 2: Lint/RedundantSafeNavigation

**Files:** Modify crates/murphy-std/src/cops/lint/redundant_safe_navigation.rs.

**Produces:** RedundantSafeNavigationOptions with allowed_methods: Vec<String>; configured presence is nil-safe in a condition and configured values do not remove intrinsic respond_to?.

- [ ] Write the two failing parser-driven tests.
- [ ] Run cargo test -p murphy-std --lib redundant_safe_navigation and confirm compilation fails because the options type is absent.
- [ ] Add CopOptions with AllowedMethods, decode in check_csend, and pass options through is_redundant_safe_navigation and is_nil_safe_method.
  Keep the helper boundary explicit:

      fn is_redundant_safe_navigation(
          node: NodeId,
          receiver: NodeId,
          opts: &RedundantSafeNavigationOptions,
          cx: &Cx<'_>,
      ) -> bool
- [ ] Run the focused test and confirm all tests pass.
- [ ] Commit with git commit -m "feat(std): wire RedundantSafeNavigation allowed methods".

## Task 3: Lint/NumberConversion

**Files:** Modify crates/murphy-std/src/cops/lint/number_conversion.rs.

**Produces:** Options.allowed_patterns: Vec<String>, decoded from AllowedPatterns and matched against the receiver method name with Cx::matches_any_pattern.

- [ ] Write failing tests for a matching minutes pattern and a non-matching hours pattern.
- [ ] Run cargo test -p murphy-std --lib number_conversion and confirm the missing field causes the expected compile failure.
- [ ] Add the AllowedPatterns field with default empty list and return early when the receiver method matches a configured regex.
  Add this field beside allowed_methods and allowed_classes:

      #[option(name = "AllowedPatterns", default = [])]
      pub allowed_patterns: Vec<String>,

  Apply it only after identifying receiver_method:

      if cx.matches_any_pattern(receiver_method, &opts.allowed_patterns) {
          return;
      }
- [ ] Update the parity note to mark AllowedPatterns as supported.
- [ ] Run the focused test and confirm all tests pass.
- [ ] Commit with git commit -m "feat(std): honor NumberConversion allowed patterns".

## Task 4: Style/InvertibleUnlessCondition

**Files:** Modify crates/murphy-std/src/cops/style/invertible_unless_condition.rs.

**Produces:** InvertibleUnlessConditionOptions with inverse_methods: BTreeMap<String, String>, manual CopOptions decoding, and effective_inverse_methods that overlays configured entries on the built-in map.

- [ ] Write failing tests that decode colon-prefixed present/blank entries and verify the built-in even/odd entry remains active.
- [ ] Run cargo test -p murphy-std --lib invertible_unless_condition and confirm the absent options type causes the expected compile failure.
- [ ] Implement from_config_json and to_config_json using serde_json::Value; require an object for InverseMethods and strip a leading colon from keys and values.
  The decoded value is:

      #[derive(Default, Debug, Clone, PartialEq, Eq)]
      pub struct InvertibleUnlessConditionOptions {
          pub inverse_methods: BTreeMap<String, String>,
      }

  Normalize each entry with trim_start_matches(':') before storing it.
- [ ] Decode options once in check, build the effective map, and pass it to invertible, preferred_condition, build_send_condition, autocorrect_condition, and autocorrect_send.
  Construct the effective map with built-ins first and configured entries second:

      fn effective_inverse_methods(
          opts: &InvertibleUnlessConditionOptions,
      ) -> BTreeMap<String, String> {
          let mut methods = built_in_inverse_methods();
          methods.extend(opts.inverse_methods.clone());
          methods
      }
- [ ] Run the focused test and confirm all tests pass.
- [ ] Commit with git commit -m "feat(std): wire InvertibleUnlessCondition methods".

## Task 5: Existing option-aware cops

**Files:** Modify crates/murphy-std/src/cops/style/symbol_proc.rs; retain existing tests in collection_compact.rs and and_or.rs.

- [ ] Add a SymbolProc test covering both Rails allowlist entries mail and respond_to through with_options.
- [ ] Run the focused CollectionCompact, SymbolProc, and AndOr test groups and confirm they pass without production changes.
- [ ] Commit with git commit -m "test(std): guard Rails cop option defaults".

## Task 6: Metadata and quality gates

**Files:** Update parity comments in the four changed cop files.

- [ ] State that configured AllowedMethods and InverseMethods are merged with intrinsic defaults; retain unrelated parity gaps.
- [ ] Run cargo test -p murphy-std --test cop_parity_metadata.
- [ ] Run cargo test -p murphy-std --lib.
- [ ] Run cargo fmt --all -- --check.
- [ ] Run cargo clippy -p murphy-std --all-targets -- -D warnings.
- [ ] Run git diff --check, git status --short, and bd show murphy-tatp; verify .beads/.gitignore is untouched.
- [ ] Run bd close murphy-tatp --reason="Rails pack cop defaults are decoded and applied by std cops" after all gates pass.
- [ ] Commit final parity metadata with git commit -m "docs(std): update Rails cop option parity".
