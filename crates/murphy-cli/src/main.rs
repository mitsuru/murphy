//! `murphy` command-line entry point (Task 7; multi-file Task 9; discovery
//! Phase 2 Task 6).
//!
//! `murphy lint <path>...` runs the single-parse pipeline over a set of `.rb`
//! files, aggregates the offenses *across all files*, and prints them as one
//! JSON array on stdout. Argument parsing is hand-rolled (one subcommand,
//! zero-or-more path args — no `clap`; YAGNI until the CLI actually grows,
//! design/plan).
//!
//! ## Path-arg precedence (Phase 2 Task 6, exact)
//!
//! Each path arg is classified by what it is on disk:
//!
//! - **Existing files** are linted *explicitly* — exactly the Phase 1
//!   behavior: NO directory walk, NO `murphy.toml`, NO `.murphyignore`. This
//!   keeps the frozen contract (ADR 0006) byte-identical: the snapshot /
//!   determinism tests pass explicit fixture filenames and are untouched.
//! - **Directory args** are *discovered* via `murphy_core::discover` (walks
//!   the tree, honoring an optional `<dir>/murphy.toml` `[files]`
//!   include/exclude and `.murphyignore`). An explicitly-passed directory arg
//!   roots the walk AT that dir, so a `.murphyignore` in a PARENT of it does
//!   not apply (explicit dir arg = "I mean this dir", consistent with
//!   explicit-file-bypasses-discovery).
//! - **Zero path args** (`murphy lint`) discovers from the cwd (`.`). This is
//!   the one Phase 1 behavior change: zero files used to be bad-usage→exit 2;
//!   it is now "discover cwd". A *missing* subcommand or a *wrong* subcommand
//!   is still bad usage → exit 2 (distinct: that path never reaches discovery).
//! - A non-existent path arg is neither a file nor a dir → it falls through
//!   the explicit-file path and the existing missing-file→exit 2 logic
//!   (`read_source`'s read error, hit in `lint_files_memoized`'s read phase)
//!   catches it, unchanged.
//! - Mixed args (some files, some dirs) → the explicit files plus everything
//!   discovered under the dirs, unioned (deduped, sorted).
//!
//! ## stdout / stderr split
//!
//! stdout is **only ever** a JSON array of offenses (design §5), so it stays
//! machine-parseable. Every diagnostic (bad usage, unreadable file, parse
//! error) goes to **stderr**; error exits print nothing to stdout.
//!
//! ## Exit codes (design doc + plan Task 7)
//!
//! - `0` — no offenses.
//! - `1` — one or more offenses.
//! - `2` — config/cop/file-setup error: a missing or unreadable file, or bad
//!   CLI usage. A *parse* failure is NOT exit 2 — per design §6 (Task 8) it
//!   becomes one `Murphy/Syntax` offense, so the file exits `1` like any other
//!   offense.
//! - `3` — internal failure: a panic anywhere in the run is caught and mapped
//!   here instead of aborting the process.

use murphy_core::{
    Cop, CopRegistry, Offense, SYNTAX_COP_NAME, Severity, aggregate, discover, parse, run_cops,
};
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

// Convention inherited by Tasks 8/9: a closed stdout pipe (`murphy ... | head`)
// is NOT a setup error — the consumer hung up. A `BrokenPipe` failure writing
// the JSON to stdout exits `0` (see `run`), not `2`.
/// Exit code: clean — zero offenses.
const EXIT_OK: u8 = 0;
/// Exit code: lint found one or more offenses.
const EXIT_OFFENSES: u8 = 1;
/// Exit code: config/cop/file-setup error (bad usage, missing/unreadable
/// file). A parse failure is NOT this — it is a `Murphy/Syntax` offense
/// (design §6), so a syntax-error file exits `1`.
const EXIT_SETUP_ERROR: u8 = 2;
/// Exit code: internal failure (a caught panic).
const EXIT_INTERNAL: u8 = 3;

/// A run failure that maps to a specific non-success exit code, carrying a
/// human-readable message destined for **stderr** (never stdout).
struct AppError {
    code: u8,
    message: String,
}

impl AppError {
    fn setup(message: impl Into<String>) -> AppError {
        AppError {
            code: EXIT_SETUP_ERROR,
            message: message.into(),
        }
    }
}

fn main() -> ExitCode {
    // Panic guard: any panic inside the run becomes exit `3` rather than the
    // default process abort. `&[String]` is already `RefUnwindSafe`; the
    // wrapper is needed only because an arbitrary closure is not auto-derived
    // `UnwindSafe`. Sound here: the process exits immediately after a caught
    // panic, so no post-panic (potentially broken) state is ever observed.
    let args: Vec<String> = std::env::args().collect();

    let outcome = catch_unwind(AssertUnwindSafe(|| run(&args)));

    let code = match outcome {
        Ok(Ok(code)) => code,
        Ok(Err(err)) => {
            eprintln!("murphy: {}", err.message);
            err.code
        }
        Err(_) => {
            // The panic message itself was already printed by the default
            // panic hook; add a one-line classification on stderr.
            eprintln!("murphy: internal failure (panic)");
            EXIT_INTERNAL
        }
    };

    ExitCode::from(code)
}

/// `#[cfg(test)]`-only counter of how many times the parse+cop pipeline
/// (`lint_source`) actually ran. The in-run memoization (Task 7) parses each
/// **unique content** once and fans the result out to every path that shares
/// it; this counter is the *deterministic* (not timing-based) proof of that:
/// a unit test drives `lint_files_memoized` over a synthetic set with
/// duplicate content and asserts the count equals the number of UNIQUE
/// contents, not the number of paths. Test-only so release builds carry zero
/// instrumentation.
///
/// Test-only instrumentation, NOT a runtime counter. **Exactly ONE** unit
/// test (`memoization_parses_unique_then_no_duplicate_in_one_test`) asserts
/// on this static; it `store(0)`s the counter at the top of EACH sub-scenario
/// it runs. Because only that single test ever touches `PARSE_CALLS`, there
/// is no cross-test ordering invariant and no global lock is needed. Do NOT
/// add a second counter-asserting test — extend the existing one with another
/// sub-scenario (reset the counter again at its top) instead, or the
/// implicit single-asserter invariant that lets us drop the lock breaks.
#[cfg(test)]
static PARSE_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Read a file's contents off disk (the **abort-on-Err → exit 2** path).
///
/// This is the ONLY concern of the old `lint_one_file` that can fail: a
/// missing or unreadable file is a setup-class error (design exit `2`).
/// Splitting it out (P2 Task 5 review Minor 1) is what makes [`lint_source`]
/// path-independent — and therefore safely memoizable: the same content read
/// from two different paths parses identically, so it need only parse once.
/// Behavior is byte-for-byte the old read: `std::fs::read_to_string` with the
/// exact same `AppError::setup` message shape, so the missing-file→exit-2
/// contract (`lint_multi_file_with_one_missing_exits_2`) is unchanged.
fn read_source(path: &str) -> Result<String, AppError> {
    std::fs::read_to_string(Path::new(path))
        .map_err(|e| AppError::setup(format!("cannot read {path:?}: {e}")))
}

/// Run the single-parse pipeline over already-read `source`, labeling every
/// offense with `file`.
///
/// `file` is used **only** as the `Offense.file` label (including the
/// synthetic syntax offense). Parsing and the cop pass are otherwise
/// completely path-independent — that path-independence is exactly what makes
/// in-run memoization correct: byte-identical content yields byte-identical
/// offenses modulo the `file` label, so two paths with the same content can
/// share one parse and differ only by a per-path `file` rewrite.
///
/// NEVER returns `Err`: a parse failure is the one synthetic `Murphy/Syntax`
/// offense (design §6, cops skipped — there is no AST), not a setup error.
/// The setup-class failure (missing/unreadable file) is [`read_source`]'s.
///
/// `cops` is the registry-owned native cop slice (P3 Task 1): the per-call
/// inline `vec![Box::new(NoReceiverPuts)]` was lifted into a once-built
/// [`CopRegistry`] threaded down from [`run`]. Behavior is byte-identical —
/// only the *source* of the cop vector changed (inline literal →
/// `registry.native_cops()`); the same one native cop still runs.
fn lint_source(source: &str, file: &str, cops: &[Box<dyn Cop>]) -> Vec<Offense> {
    #[cfg(test)]
    PARSE_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    let mut sink: Vec<Offense> = Vec::new();
    match parse(source) {
        Ok(ast) => {
            run_cops(&ast, file, cops, &mut sink);
        }
        Err(e) => {
            // Use `e.message` verbatim (prism's first-error text); the Display
            // impl would prepend "parse error at bytes ..", which §6 does not
            // ask for. Range is the prism first-error byte span (ADR 0001).
            sink.push(Offense::new(
                file,
                SYNTAX_COP_NAME,
                e.range,
                Severity::Error,
                &e.message,
            ));
        }
    }
    sink
}

/// Read, parse+lint, and aggregate every file — with **in-run content
/// memoization** (Phase 2 Task 7, Scope Fence 3: in-memory, single run only).
///
/// Two-phase, output byte-identical to the non-memoized Task 5 result for
/// *any* input (the common no-duplicate case is a 1:1 no-op):
///
/// 1. **Read phase.** Read every path. Reading is the only I/O that can
///    `Err` → exit 2; collecting into `Result<Vec<_>, AppError>` keeps the
///    Task-5 abort-on-first-`Err` short-circuit (a missing file anywhere
///    aborts the whole run), so `lint_multi_file_with_one_missing_exits_2`
///    is preserved. Reads are independent → done in parallel.
/// 2. **Lint phase.** Group paths by their **content `String`** (keyed on the
///    content itself, not a hash — zero collision risk, zero new dependency;
///    the plan explicitly prefers this and Phase-2 scale makes holding the
///    contents a non-concern). `par_iter` over the UNIQUE contents (this
///    preserves Task 5's parallelism — the unique set has the same length as
///    `files` when nothing is duplicated), running `lint_source` ONCE per
///    unique content against a deterministic representative path (the first
///    path in `BTreeMap`/sorted order — deterministic for defense-in-depth
///    only; `lint_source` never writes stderr, a parse failure becomes a
///    `Murphy/Syntax` OFFENSE whose `file` is rewritten per-path in the
///    fan-out and `aggregate` re-sorts, so the representative choice is not
///    observable). Then fan out: every path sharing that content gets that
///    content's offenses with `Offense.file` rewritten to its own path
///    (offsets/cop_name/severity/message are identical because the bytes are).
///
/// The flattened per-path offenses go to `aggregate` UNCHANGED — it remains
/// the single determinism point (the total-order sort/dedupe, Task 2), so
/// neither read/lint parallelism nor the fan-out order can perturb output.
fn lint_files_memoized(files: &[String], registry: &CopRegistry) -> Result<Vec<Offense>, AppError> {
    // Phase 1: read every path. `?` on the collected Result short-circuits on
    // the first read error (missing/unreadable → exit 2), exactly as the
    // Task-5 `par_iter().collect::<Result<_,_>>()` did.
    let contents: Vec<String> = files
        .par_iter()
        .map(|f| read_source(f))
        .collect::<Result<_, AppError>>()?;

    // Group paths by content. `BTreeMap` keyed on the owned content `String`
    // gives a deterministic representative (the first path, since `files` is
    // already sorted upstream) and zero collision risk vs a hash. Values are
    // the paths (in `files` order) that share that exact content.
    let mut by_content: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (path, content) in files.iter().zip(contents.iter()) {
        by_content
            .entry(content.as_str())
            .or_default()
            .push(path.as_str());
    }

    // Phase 2: lint each UNIQUE content ONCE, in parallel (same parallelism
    // as Task 5 when there are no dups), then fan out per path with the
    // `Offense.file` rewritten. The representative is the first path for that
    // content (deterministic for defense-in-depth only — `lint_source` never
    // writes stderr; a parse failure is a `Murphy/Syntax` offense whose `file`
    // is rewritten per real path below and `aggregate` re-sorts, so the
    // representative choice is not observable).
    let unique: Vec<(&&str, &Vec<&str>)> = by_content.iter().collect();
    let per_unique: Vec<Vec<Offense>> = unique
        .par_iter()
        .map(|(content, paths)| {
            let representative = paths[0];
            let base = lint_source(content, representative, registry.native_cops());
            // Fan out: one offense list per path sharing this content, with
            // `file` set to that path. Identical content ⇒ identical
            // offsets/cop/severity/message; only `file` differs.
            paths
                .iter()
                .flat_map(|p| {
                    base.iter().map(move |o| {
                        let mut o = o.clone();
                        o.file = (*p).to_owned();
                        o
                    })
                })
                .collect::<Vec<Offense>>()
        })
        .collect();

    // `aggregate` is the single, unchanged determinism point (Task 2).
    Ok(aggregate(per_unique.into_iter().flatten().collect()))
}

/// Parse args, run the pipeline, and return the exit code (or an [`AppError`]
/// carrying a non-success code + stderr message).
///
/// Returns `Ok(code)` for the *expected* outcomes (`0` clean / `1` offenses);
/// `Err` for setup-class failures (`2`). Panics propagate to the guard in
/// [`main`] and become `3`.
fn run(args: &[String]) -> Result<u8, AppError> {
    // args[0] is the program name. Expect: `lint [path]...` — the subcommand
    // then ZERO-or-more path args (a path is a file OR a dir; precedence in
    // the module doc). `get(1..)` instead of `&args[1..]` so `run(&[])` yields
    // a usage error, not a slice-index panic→exit 3.
    //
    // Subcommand is extracted FIRST and validated independently of path
    // count: a missing subcommand (`murphy`) or a wrong one is bad usage →
    // exit 2 and never reaches discovery. ZERO paths is NOT bad usage (Task 6
    // change): `murphy lint` discovers from the cwd. This distinction is the
    // whole point — `bad_usage_exits_2` invokes `murphy` with no subcommand,
    // which still exits 2; `murphy lint` (zero paths) now discovers cwd.
    let rest = args.get(1..).unwrap_or(&[]);
    let (subcommand, paths) = match rest {
        [subcommand, paths @ ..] => (subcommand.as_str(), paths),
        [] => {
            return Err(AppError::setup("usage: murphy lint [path]..."));
        }
    };

    if subcommand != "lint" {
        return Err(AppError::setup(format!(
            "unknown subcommand {subcommand:?} (usage: murphy lint [path]...)"
        )));
    }

    // Build the cop registry ONCE per run (P3 Task 1) and share it: the
    // per-call inline `vec![Box::new(NoReceiverPuts)]` is lifted here. The
    // registry holds the native cops (the only cops that RUN today) and
    // ENUMERATES (does not load) `./cops/*.rb` for P3 Task 3/4.
    //
    // `cops/` is resolved CWD-RELATIVE: `Path::new(".")` = the project root =
    // the directory `murphy` was invoked from. This is DELIBERATELY and
    // permanently INDEPENDENT of the lint-target path arg(s) parsed below:
    // `cd /repo && murphy lint subproject/` lints `subproject/` files but
    // loads cops from `/repo/cops/`, NOT `/repo/subproject/cops/`. That is the
    // intended reading of ADR 0004 mitigation 2 — cops come from the project's
    // own `cops/` (the dir you ran `murphy` in), never auto-discovered from
    // whatever sub-path (possibly a dependency tree) you point the linter at.
    // It is also consistent with the zero-arg `discover(Path::new("."))`
    // convention used below for file discovery. The decision is pinned in
    // `registry.rs` (test
    // `cops_dir_is_resolved_at_the_given_root_not_a_lint_subdir`): invisible
    // in Task 1 (enumerate-only) but observable once Task 3/4 RUN the cops, so
    // it is locked NOW. "Walk up for the nearest `cops/`" is out of scope for
    // v1.
    //
    // An absent `cops/` is fine (no user cops), a real I/O error on an
    // existing one is a setup error → exit 2. The registry is `Sync`, so a
    // borrow of its native slice safely crosses the rayon `par_iter` boundary
    // inside `lint_files_memoized`. Behavior is byte-identical to Phase 2:
    // only the *source* of the cop vector moved (inline literal → registry).
    let registry =
        CopRegistry::discover(Path::new(".")).map_err(|e| AppError::setup(e.to_string()))?;

    // Classify each path arg (module doc precedence). Existing files go to the
    // explicit list (Phase 1 path, frozen-contract preserving). Directories
    // are discovered. Zero path args → discover the cwd. A non-existent path
    // is neither file nor dir → it stays in the explicit list so the existing
    // missing-file→exit 2 read error in `lint_one_file` catches it unchanged.
    //
    // The combined list is a `BTreeSet<PathBuf>` so mixed args (`lint d/ d/x.rb`)
    // dedupe and the rayon input is sorted (deterministic — defense in depth;
    // `aggregate` re-sorts the output anyway).
    let mut targets: BTreeSet<PathBuf> = BTreeSet::new();
    if paths.is_empty() {
        // Zero path args: discover from the cwd.
        for p in discover(Path::new(".")).map_err(|e| AppError::setup(e.to_string()))? {
            targets.insert(p);
        }
    } else {
        for arg in paths {
            let path = Path::new(arg);
            if path.is_dir() {
                for p in discover(path).map_err(|e| AppError::setup(e.to_string()))? {
                    targets.insert(p);
                }
            } else {
                // Existing file OR non-existent path: explicit (Phase 1).
                targets.insert(path.to_path_buf());
            }
        }
    }

    // The memo pipeline takes `&[String]`. A non-UTF-8 path can't be
    // losslessly passed; treat it as a setup error rather than silently
    // lossy-converting (paths here come from the FS walk or CLI args —
    // practically always UTF-8).
    let files: Vec<String> = targets
        .into_iter()
        .map(|p| {
            p.to_str().map(str::to_owned).ok_or_else(|| {
                AppError::setup(format!("non-UTF-8 path cannot be linted: {}", p.display()))
            })
        })
        .collect::<Result<_, AppError>>()?;

    // Read + parse + lint + aggregate every file, with in-run content
    // memoization (Task 7): byte-identical content is parsed/linted ONCE and
    // fanned out per path. Parallelism (read and per-unique-content lint),
    // abort-on-first-read-Err → exit 2, and `aggregate` as the single
    // determinism point are all preserved inside `lint_files_memoized`; the
    // no-duplicate case is a 1:1 no-op so the snapshot stays byte-identical.
    // `tests/parallel_determinism.rs` is the permanent byte-identity guard.
    let offenses = lint_files_memoized(&files, &registry)?;

    // stdout is exclusively the JSON array (design §5). `serde_json` cannot
    // fail serializing `Vec<Offense>` (all fields are plain owned data), but
    // map a hypothetical failure to a setup error rather than unwrap-panic.
    let json = serde_json::to_string(&offenses)
        .map_err(|e| AppError::setup(format!("failed to serialize offenses: {e}")))?;
    let mut stdout = std::io::stdout().lock();
    if let Err(e) = writeln!(stdout, "{json}") {
        // A closed pipe (`murphy lint x.rb | head`) is the consumer hanging
        // up, not a failure: exit `0` (conventional). Any other stdout write
        // error is a genuinely broken stdout → setup error (exit 2).
        if e.kind() == std::io::ErrorKind::BrokenPipe {
            return Ok(EXIT_OK);
        }
        return Err(AppError::setup(format!("failed to write stdout: {e}")));
    }

    Ok(if offenses.is_empty() {
        EXIT_OK
    } else {
        EXIT_OFFENSES
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    /// `AppError` deliberately isn't `Debug` (it carries a stderr message,
    /// not a programmer-facing repr). Unwrap it in tests via its `code`
    /// rather than adding a derive to a non-test type just for `.expect()`.
    fn expect_ok(r: Result<Vec<Offense>, AppError>) -> Vec<Offense> {
        match r {
            Ok(v) => v,
            Err(e) => panic!("expected Ok, got AppError {{ code: {} }}", e.code),
        }
    }

    /// In-run memoization, both scenarios in ONE test (the ONLY test that
    /// asserts on the process-global `PARSE_CALLS`). Because exactly one test
    /// touches the counter there is no cross-test ordering invariant and no
    /// global lock is needed; the counter is `store(0)`-reset at the top of
    /// EACH sub-scenario below. The parse count is proven by the deterministic
    /// `#[cfg(test)]` atomic counter incremented inside `lint_source`, NOT by
    /// timing. To add coverage, add another sub-scenario here (reset the
    /// counter again at its top) — do NOT add a second counter-asserting test.
    ///
    /// Sub-scenario 1 — duplicate fan-out: a 3-path set whose first two paths
    /// have byte-identical content parses+lints ONCE per UNIQUE content (2),
    /// not once per path (3), and still fans out to 3 paths' worth of offenses
    /// with the correct per-path `file`.
    ///
    /// Sub-scenario 2 — no-duplicate 1:1 no-op: N distinct contents → exactly
    /// N parse calls (full Task-5 parallelism, no memo win), output unchanged.
    /// This is the equivalence guard's unit-level companion (the snapshot /
    /// `parallel_determinism` integration tests are the byte-identity guard).
    #[test]
    fn memoization_parses_unique_then_no_duplicate_in_one_test() {
        // ---- Sub-scenario 1: duplicate content fans out, parsed once. ----
        let dir = tempfile::tempdir().expect("create tempdir");
        // Two byte-identical dirty files + one distinct dirty file. All dirty
        // so every path contributes an offense (proves the fan-out, not just
        // the parse count).
        let dup_a = dir.path().join("dup_a.rb");
        let dup_b = dir.path().join("dup_b.rb");
        let other = dir.path().join("other.rb");
        std::fs::write(&dup_a, "puts \"x\"\n").expect("write dup_a.rb");
        std::fs::write(&dup_b, "puts \"x\"\n").expect("write dup_b.rb");
        std::fs::write(&other, "puts \"y\"\n").expect("write other.rb");

        let files: Vec<String> = vec![
            dup_a.to_str().unwrap().to_owned(),
            dup_b.to_str().unwrap().to_owned(),
            other.to_str().unwrap().to_owned(),
        ];

        // Native-only registry: these tests assert the native cop pipeline
        // (parse-count memo + fan-out), not `cops/` discovery, so no tempdir
        // root / cwd dependence is wanted here.
        let registry = CopRegistry::native_only();

        PARSE_CALLS.store(0, Ordering::Relaxed);
        let offenses = expect_ok(lint_files_memoized(&files, &registry));

        // (a) UNIQUE content parsed once each: 2 unique contents → exactly 2
        // parse calls, NOT 3 (the duplicate is NOT re-parsed).
        assert_eq!(
            PARSE_CALLS.load(Ordering::Relaxed),
            2,
            "two unique contents must parse exactly twice (dup parsed once), got {}",
            PARSE_CALLS.load(Ordering::Relaxed)
        );

        // (b) Output has 3 paths' worth of offenses (one NoReceiverPuts per
        // dirty file) with the correct per-path `file` label.
        assert_eq!(
            offenses.len(),
            3,
            "3 dirty paths → 3 offenses (dup fanned out), got {offenses:?}"
        );
        let mut files_seen: Vec<&str> = offenses.iter().map(|o| o.file.as_str()).collect();
        files_seen.sort_unstable();
        let mut expected = vec![
            dup_a.to_str().unwrap(),
            dup_b.to_str().unwrap(),
            other.to_str().unwrap(),
        ];
        expected.sort_unstable();
        assert_eq!(
            files_seen, expected,
            "each path gets its own offense with its own `file`"
        );
        for o in &offenses {
            assert_eq!(o.cop_name, "Murphy/NoReceiverPuts");
        }

        // The two duplicate paths' offenses are identical modulo `file`
        // (offsets/cop/severity/message), proving the fan-out is a pure
        // per-path `file` rewrite of one parse.
        let a = offenses
            .iter()
            .find(|o| o.file == dup_a.to_str().unwrap())
            .expect("dup_a offense");
        let b = offenses
            .iter()
            .find(|o| o.file == dup_b.to_str().unwrap())
            .expect("dup_b offense");
        assert_eq!(a.range, b.range);
        assert_eq!(a.cop_name, b.cop_name);
        assert_eq!(a.severity, b.severity);
        assert_eq!(a.message, b.message);
        assert_ne!(a.file, b.file);

        // ---- Sub-scenario 2: no duplicates → 1:1 no-op, N parses for N. ----
        // Same test body (no second #[test], no global lock): reset the
        // counter again at the top of this sub-scenario.
        let dir2 = tempfile::tempdir().expect("create tempdir");
        let nd_a = dir2.path().join("a.rb");
        let nd_b = dir2.path().join("b.rb");
        std::fs::write(&nd_a, "x = 1\n").expect("write a.rb");
        std::fs::write(&nd_b, "y = 2\n").expect("write b.rb");

        let files2: Vec<String> = vec![
            nd_a.to_str().unwrap().to_owned(),
            nd_b.to_str().unwrap().to_owned(),
        ];

        PARSE_CALLS.store(0, Ordering::Relaxed);
        let offenses2 = expect_ok(lint_files_memoized(&files2, &registry));

        assert_eq!(
            PARSE_CALLS.load(Ordering::Relaxed),
            2,
            "2 distinct contents → 2 parse calls (1:1, no memo win)"
        );
        assert!(
            offenses2.is_empty(),
            "both clean → zero offenses, got {offenses2:?}"
        );
    }

    /// Read phase preserves abort-on-first-Err → exit 2: a missing path
    /// anywhere in the set makes `lint_files_memoized` return the setup
    /// `AppError` (code 2), even though a sibling file is readable+clean.
    /// This is the unit-level mirror of `lint_multi_file_with_one_missing_exits_2`.
    #[test]
    fn missing_file_in_set_aborts_with_setup_error() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let good = dir.path().join("good.rb");
        std::fs::write(&good, "x = 1\n").expect("write good.rb");
        let missing = dir.path().join("does_not_exist.rb");

        let files: Vec<String> = vec![
            good.to_str().unwrap().to_owned(),
            missing.to_str().unwrap().to_owned(),
        ];

        let registry = CopRegistry::native_only();
        let err = lint_files_memoized(&files, &registry).expect_err("missing file must abort");
        assert_eq!(err.code, EXIT_SETUP_ERROR, "missing file → exit 2");
    }
}
