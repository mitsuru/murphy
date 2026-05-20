//! Cop registry: the single source of the cop set for a run (Phase 3 Task 1).
//!
//! Phase 1/2 built the cop vector inline at every `lint_source` call
//! (`vec![Box::new(NoReceiverPuts)]`). Phase 3 introduces user cops written as
//! `.rb` files; the set is no longer a literal. [`CopRegistry`] is the one
//! place that owns it: the **native** cops (reimplemented in Rust, run on all
//! cores) and the **discovered `cops/*.rb` paths** (user/mruby cops).
//!
//! ## Task 1 scope — enumerate, do NOT load
//!
//! This task ONLY enumerates `cops/*.rb` paths. It does **not** read, parse,
//! embed, or run any `.rb` cop — that is Phase 3 Tasks 3/4, which consume
//! [`CopRegistry::mruby_cop_paths`]. The load-bearing invariant for Task 1:
//!
//! > Enumerating `cops/*.rb` cannot change linter output. Adding a `.rb` cop
//! > to a project under test is inert until Task 3/4 wires loading. Only the
//! > configured native cops run, so this phase's snapshot contract stays tied
//! > to the native set explicitly registered below.
//!
//! ## `cops/` location & enumeration rule (ADR 0004 mitigation 2)
//!
//! Cops are loaded ONLY from the project's own `<root>/cops/` directory
//! ([`CopRegistry::discover`] takes the project root explicitly — the CLI
//! passes `.`, mirroring [`crate::discover`]'s zero-arg convention; tests pass
//! a tempdir so no process-cwd mutation / global state is involved).
//!
//! Enumeration is **flat** (`<root>/cops/*.rb`, non-recursive) — ADR 0004
//! mitigation 2 describes a flat per-project `cops/`; nested layout is
//! deferred (YAGNI until a cop needs it). Entries are filtered to regular
//! files with a `.rb` extension and returned **sorted** (deterministic,
//! defense-in-depth like the rest of the pipeline). An absent `cops/` →
//! empty list, **no error** (a project simply having no user cops is normal,
//! not a setup failure); a genuine I/O error reading an existing `cops/` is a
//! [`ConfigError::Io`] (mapped to exit 2 by the CLI, like the rest of
//! discovery).

use crate::ConfigError;
use crate::Cop;
use crate::MurphyConfig;
use crate::NoReceiverPuts;
use crate::cops::layout::{EmptyLines, SpaceInsideParens, TrailingWhitespace};
use crate::cops::lint::{Debugger, DeprecatedClassMethods};
use crate::cops::style::{
    AndOr, FrozenStringLiteralComment, IfUnlessModifier, NilComparison, RedundantReturn,
    StringLiterals, SymbolArray, WordArray,
};
use std::path::{Path, PathBuf};

/// The cop set for a run: native cops (run on all cores) plus the discovered
/// (Task 1: **enumerated, not loaded**) `cops/*.rb` user-cop paths.
///
/// Built ONCE per run and shared. It is `Send + Sync` because every field is:
/// `Vec<Box<dyn Cop>>` is `Send + Sync` (`Cop: Send + Sync`, ADR 0002) and
/// `Vec<PathBuf>` is `Send + Sync`. That matters: the native-cop slice crosses
/// the rayon `par_iter` boundary in the CLI's memoized lint phase.
pub struct CopRegistry {
    /// The native cops reimplemented in Rust. This is the slice `run_cops`
    /// consumes; discovered mruby paths are kept separately and loaded by the
    /// mruby pipeline.
    native: Vec<Box<dyn Cop>>,
    /// Discovered `cops/*.rb` paths, sorted. Populated by [`Self::discover`]
    /// but **not loaded/run in Task 1** — consumed in P3 Task 3/4 (mruby
    /// loading). Exposed via [`Self::mruby_cop_paths`] (exercised by tests so
    /// the field is not dead code under `-D warnings`).
    mruby_cop_paths: Vec<PathBuf>,
}

impl CopRegistry {
    /// Returns the built-in native cops. Centralized here so adding a native
    /// cop is a one-line change in one place instead of at every `run_cops`
    /// call site.
    fn native_cops_list() -> Vec<Box<dyn Cop>> {
        vec![
            Box::new(NoReceiverPuts),
            Box::new(TrailingWhitespace),
            Box::new(EmptyLines),
            Box::new(SpaceInsideParens),
            Box::new(Debugger),
            Box::new(DeprecatedClassMethods),
            Box::new(AndOr),
            Box::new(FrozenStringLiteralComment),
            Box::new(IfUnlessModifier),
            Box::new(NilComparison),
            Box::new(RedundantReturn),
            Box::new(StringLiterals),
            Box::new(SymbolArray),
            Box::new(WordArray),
        ]
    }

    pub fn native_cop_names() -> Vec<String> {
        Self::native_cops_list()
            .into_iter()
            .map(|cop| cop.name().to_string())
            .collect()
    }

    /// Build a registry whose mruby-cop path list is empty (no `cops/`
    /// discovery). The native cops are still present. Useful for callers /
    /// tests that only need the native set and have no project root.
    pub fn native_only() -> Self {
        CopRegistry {
            native: Self::native_cops_list(),
            mruby_cop_paths: Vec::new(),
        }
    }

    /// Build a registry for the project rooted at `root`: the native cops plus
    /// the **enumerated** (not loaded) `<root>/cops/*.rb` paths, sorted.
    ///
    /// `<root>/cops/` absent → no mruby paths, no error. An I/O error reading
    /// an existing `cops/` (other than not-found) is a [`ConfigError::Io`].
    ///
    /// ## `cops/` root is CWD-RELATIVE, deliberately decoupled from the lint target
    ///
    /// `root` here is the **project root = the directory `murphy` was invoked
    /// from** — the CLI passes `Path::new(".")` (the process cwd), mirroring
    /// the zero-arg [`crate::discover`] convention. This is **deliberately and
    /// permanently independent of the lint-target path(s)**: `murphy lint
    /// subproject/` lints files under `subproject/` but still loads cops from
    /// `./cops/` (the invocation dir), NOT `subproject/cops/`. That is the
    /// intended reading of ADR 0004 mitigation 2 — cops come from *"the
    /// project's own configured `cops/` path"*, i.e. the project you ran
    /// `murphy` in, not from whichever sub-path you happened to point the
    /// linter at (which could be a dependency's tree — exactly what mitigation
    /// 2 forbids auto-loading from).
    ///
    /// This decision is **pinned NOW** (test
    /// `cops_dir_is_resolved_at_the_given_root_not_a_lint_subdir`): Task 1 only
    /// enumerates so it is invisible, but Phase 3 Task 3/4 actually RUN the
    /// discovered cops, making it observable — pinning here prevents Task 3/4
    /// from silently changing it. A future "walk up the tree for the nearest
    /// `cops/`" (RuboCop-style) is **explicitly out of scope for v1** (YAGNI;
    /// would also widen the ADR 0004 trust surface).
    pub fn discover(root: &Path) -> Result<Self, ConfigError> {
        let config = MurphyConfig::load(root)?;
        Self::discover_with_config(root, &config)
    }

    pub fn discover_with_config(root: &Path, config: &MurphyConfig) -> Result<Self, ConfigError> {
        let mruby_cop_paths = enumerate_cop_paths(root, &config.cops.path)?;
        let native = Self::native_cops_list()
            .into_iter()
            .filter(|cop| config.cop_enabled(cop.name()))
            .collect();
        Ok(CopRegistry {
            native,
            mruby_cop_paths,
        })
    }

    /// The native cops, as the `&[Box<dyn Cop>]` slice `run_cops` takes.
    pub fn native_cops(&self) -> &[Box<dyn Cop>] {
        &self.native
    }

    /// The discovered `cops/*.rb` paths, sorted. **Enumerated, not loaded** in
    /// Task 1 — consumed in P3 Task 3/4.
    pub fn mruby_cop_paths(&self) -> &[PathBuf] {
        &self.mruby_cop_paths
    }
}

/// Enumerate `<root>/cops/*.rb` (flat, non-recursive), filtered to regular
/// files with a `.rb` extension, sorted. Absent `cops/` → empty (no error);
/// a real I/O error on an existing `cops/` → [`ConfigError::Io`].
fn enumerate_cop_paths(root: &Path, cops_path: &Path) -> Result<Vec<PathBuf>, ConfigError> {
    let cops_dir = root.join(cops_path);
    let entries = match std::fs::read_dir(&cops_dir) {
        Ok(entries) => entries,
        // No `cops/` directory is the normal "this project has no user cops"
        // case, not a setup error. Any other I/O error (permissions, not a
        // directory, ...) IS a setup error → exit 2 like the rest of
        // discovery.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(ConfigError::Io(format!(
                "cannot read cops directory {}: {e}",
                cops_dir.display()
            )));
        }
    };

    let mut paths: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| {
            ConfigError::Io(format!(
                "cannot read an entry in cops directory {}: {e}",
                cops_dir.display()
            ))
        })?;
        let path = entry.path();
        // Only regular files ending in `.rb`. Subdirectories (flat-cops/ is
        // the v1 layout — nesting deferred) and non-`.rb` files are skipped
        // silently: they are not user cops, not an error.
        let is_file = entry.file_type().map(|ft| ft.is_file()).unwrap_or(false);
        if is_file && path.extension().and_then(|e| e.to_str()) == Some("rb") {
            paths.push(path);
        }
    }
    // Deterministic order (defense-in-depth, like the rest of the pipeline;
    // Task 3/4 will load in this order).
    paths.sort();
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXPECTED_NATIVE_COPS: [&str; 14] = [
        "Murphy/NoReceiverPuts",
        "Layout/TrailingWhitespace",
        "Layout/EmptyLines",
        "Layout/SpaceInsideParens",
        "Lint/Debugger",
        "Lint/DeprecatedClassMethods",
        "Style/AndOr",
        "Style/FrozenStringLiteralComment",
        "Style/IfUnlessModifier",
        "Style/NilComparison",
        "Style/RedundantReturn",
        "Style/StringLiterals",
        "Style/SymbolArray",
        "Style/WordArray",
    ];

    /// The registry is `Send + Sync` — it crosses the rayon `par_iter`
    /// boundary in the CLI's memoized lint phase. Compile-time assertion.
    #[test]
    fn registry_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CopRegistry>();
    }

    /// The registry yields the native cop set in ADR order.
    #[test]
    fn registry_exposes_native_cops_in_adr_order() {
        let reg = CopRegistry::native_only();
        let names: Vec<&str> = reg.native_cops().iter().map(|c| c.name()).collect();
        assert_eq!(
            names, EXPECTED_NATIVE_COPS,
            "native cops should run in registry order"
        );

        // Same native set when discovered against a root with no `cops/`.
        let dir = tempfile::tempdir().expect("create tempdir");
        let reg = CopRegistry::discover(dir.path()).expect("discover with no cops/ is Ok");
        let names: Vec<&str> = reg.native_cops().iter().map(|c| c.name()).collect();
        assert_eq!(names, EXPECTED_NATIVE_COPS);
    }

    #[test]
    fn native_cop_names_are_unique_and_include_existing_cop() {
        let names = CopRegistry::native_cop_names();
        let unique: std::collections::BTreeSet<_> = names.iter().cloned().collect();

        assert_eq!(names.len(), unique.len(), "native cop names must be unique");
        assert!(names.iter().any(|name| name == "Murphy/NoReceiverPuts"));
    }

    /// Absent `cops/` directory → empty mruby-cop path list, NOT an error
    /// (a project with no user cops is normal).
    #[test]
    fn absent_cops_dir_yields_no_paths_and_no_error() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let reg = CopRegistry::discover(dir.path()).expect("absent cops/ must not error");
        assert!(
            reg.mruby_cop_paths().is_empty(),
            "no cops/ → no enumerated paths, got {:?}",
            reg.mruby_cop_paths()
        );
    }

    /// An existing but empty `cops/` → empty path list, no error.
    #[test]
    fn empty_cops_dir_yields_no_paths() {
        let dir = tempfile::tempdir().expect("create tempdir");
        std::fs::create_dir(dir.path().join("cops")).expect("mkdir cops");
        let reg = CopRegistry::discover(dir.path()).expect("empty cops/ must not error");
        assert!(reg.mruby_cop_paths().is_empty());
        // Native cops still present.
        assert_eq!(reg.native_cops().len(), EXPECTED_NATIVE_COPS.len());
    }

    /// `cops/*.rb` paths are enumerated **sorted**, non-`.rb` files and
    /// subdirectories are skipped, and (Task 1 invariant) they are enumerated
    /// only — not loaded/run.
    #[test]
    fn cops_rb_paths_enumerated_sorted_filtered() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let cops = dir.path().join("cops");
        std::fs::create_dir(&cops).expect("mkdir cops");
        // Created out of order: enumeration must return them SORTED.
        std::fs::write(cops.join("b.rb"), "# cop b").expect("write b.rb");
        std::fs::write(cops.join("a.rb"), "# cop a").expect("write a.rb");
        // Non-`.rb` file and a subdirectory must be skipped.
        std::fs::write(cops.join("README.md"), "not a cop").expect("write README.md");
        std::fs::create_dir(cops.join("nested")).expect("mkdir nested");
        std::fs::write(cops.join("nested").join("c.rb"), "# nested cop")
            .expect("write nested/c.rb");

        let reg = CopRegistry::discover(dir.path()).expect("discover Ok");
        let got: Vec<PathBuf> = reg.mruby_cop_paths().to_vec();
        let expected = vec![cops.join("a.rb"), cops.join("b.rb")];
        assert_eq!(
            got, expected,
            "cops/*.rb enumerated sorted, flat only, non-.rb skipped"
        );

        // Task 1 invariant: enumerating cops does not change what RUNS — the
        // native cop set is unaffected by the presence of `.rb` files.
        let names: Vec<&str> = reg.native_cops().iter().map(|c| c.name()).collect();
        assert_eq!(names, EXPECTED_NATIVE_COPS);
    }

    /// PINS THE `cops/` ROOT DECISION (ADR 0004 mitigation 2): discovery is
    /// rooted at the GIVEN root's `<root>/cops/`, NOT at any lint-target
    /// subdir of it. Layout under root `R`:
    ///
    /// ```text
    /// R/cops/x.rb     <- the project's cop (the ONLY thing discover(R) sees)
    /// R/sub/          <- a sibling subdir (a plausible `murphy lint sub/`
    /// R/sub/y.rb         target) with NO `R/sub/cops/`
    /// ```
    ///
    /// `discover(R)` must enumerate exactly `[R/cops/x.rb]` — it must NOT
    /// descend into / re-root at `R/sub/` just because that is where the lint
    /// target would be. This is invisible in Task 1 (enumerate-only) but
    /// Task 3/4 RUN the cops; pinning here makes it impossible for Task 3/4 to
    /// silently switch to a lint-target-relative `cops/` root.
    #[test]
    fn cops_dir_is_resolved_at_the_given_root_not_a_lint_subdir() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let root = dir.path();

        // The project's own cops/ at the root.
        let cops = root.join("cops");
        std::fs::create_dir(&cops).expect("mkdir R/cops");
        std::fs::write(cops.join("x.rb"), "# project cop x").expect("write R/cops/x.rb");

        // A sibling subdir that a user might point the linter at
        // (`murphy lint sub/`). It deliberately has NO `sub/cops/`.
        let sub = root.join("sub");
        std::fs::create_dir(&sub).expect("mkdir R/sub");
        std::fs::write(sub.join("y.rb"), "puts \"y\"\n").expect("write R/sub/y.rb");

        // discover() is rooted at the GIVEN root, NOT at the lint-target
        // subdir: it sees R/cops/x.rb and nothing under R/sub/.
        let reg = CopRegistry::discover(root).expect("discover at root Ok");
        assert_eq!(
            reg.mruby_cop_paths(),
            &[cops.join("x.rb")],
            "cops/ is resolved at the GIVEN root (R/cops/), NOT a lint subdir \
             (R/sub/) — ADR 0004 mitigation 2; pinned for P3 Task 3/4"
        );
    }

    /// PINS the existing-but-unreadable-`cops/` → [`ConfigError::Io`] mapping
    /// (the CLI turns this into exit 2). Here `cops` exists at the root but as
    /// a REGULAR FILE, not a directory: `read_dir` fails with an error whose
    /// kind is NOT `NotFound`, so `discover` must return `Err(Io(_))` — never
    /// `Ok` (which would swallow a real setup failure), never panic.
    /// (`ConfigError` is not `PartialEq`, so the variant is asserted via
    /// `matches!`.)
    #[test]
    fn cops_is_a_regular_file_yields_config_error_io() {
        let dir = tempfile::tempdir().expect("create tempdir");
        // `cops` exists but is a file, not a directory.
        std::fs::write(dir.path().join("cops"), "i am not a directory")
            .expect("write cops as a regular file");

        // `CopRegistry` (the Ok type) is intentionally not `Debug`, so
        // `.expect_err()` won't compile; match the Result directly instead of
        // adding a derive to a non-test type just for this assertion.
        match CopRegistry::discover(dir.path()) {
            Ok(_) => panic!("an existing-but-unreadable cops/ must be an Err, not Ok"),
            Err(ConfigError::Io(_)) => {}
            Err(other) => panic!(
                "existing-but-not-a-directory cops/ → ConfigError::Io (→ exit 2), got {other:?}"
            ),
        }
    }
}
