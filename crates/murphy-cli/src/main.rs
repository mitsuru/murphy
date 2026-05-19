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
//!   (`lint_one_file`'s read error) catches it, unchanged.
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
    Cop, NoReceiverPuts, Offense, SYNTAX_COP_NAME, Severity, aggregate, discover, parse, run_cops,
};
use rayon::prelude::*;
use std::collections::BTreeSet;
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

/// Lint a single file: read it, run the single-parse pipeline, and return
/// the offenses found in it (unaggregated — the caller aggregates *across*
/// all files so cross-file ordering/dedupe is one pass).
///
/// This is a pure extraction of what was the inline per-file block (Task 7/8)
/// — no behavior change for the one-file case. `Err` is a setup-class failure
/// (the file is missing/unreadable, design exit `2`). A *parse* failure is
/// NOT an `Err`: per design §6 it becomes exactly one synthetic
/// `Murphy/Syntax` offense in the returned `Vec` and the cop pass is skipped
/// (there is no AST). Returning `Ok` for a parse failure is what lets a
/// broken file in a multi-file run still exit `1` like any other offense
/// *without aborting the other files*.
fn lint_one_file(file: &str) -> Result<Vec<Offense>, AppError> {
    let source = std::fs::read_to_string(Path::new(file))
        .map_err(|e| AppError::setup(format!("cannot read {file:?}: {e}")))?;

    let mut sink: Vec<Offense> = Vec::new();
    match parse(&source) {
        Ok(ast) => {
            let cops: Vec<Box<dyn Cop>> = vec![Box::new(NoReceiverPuts)];
            run_cops(&ast, file, &cops, &mut sink);
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
    Ok(sink)
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

    // `lint_one_file` takes `&str` (unchanged — Task 7 refactors it, not us).
    // A non-UTF-8 path can't be losslessly passed; treat it as a setup error
    // rather than silently lossy-converting (paths here come from the FS walk
    // or CLI args — practically always UTF-8).
    let files: Vec<String> = targets
        .into_iter()
        .map(|p| {
            p.to_str().map(str::to_owned).ok_or_else(|| {
                AppError::setup(format!("non-UTF-8 path cannot be linted: {}", p.display()))
            })
        })
        .collect::<Result<_, AppError>>()?;

    // Lint every file in the explicit list IN PARALLEL across rayon's default
    // (core-sized) thread pool — `lint_one_file` parses its own file into an
    // immutable AST with no shared mutable state, and `Box<dyn Cop>` is
    // `Send + Sync` (Task 4), so this is an embarrassingly parallel map.
    //
    // Determinism does NOT depend on thread/arg/completion order: every file's
    // offenses land in one flat `Vec` whose final ordering comes entirely from
    // `aggregate`'s total order `(file, start, end, cop_name, message,
    // severity)` (Task 2). Thread interleaving cannot perturb the output
    // because `aggregate` re-sorts; `tests/parallel_determinism.rs` is the
    // permanent byte-identity guard for this.
    //
    // Abort-on-first-Err → exit 2 is preserved: collecting into
    // `Result<Vec<Vec<Offense>>, AppError>` short-circuits on the first `Err`
    // (a missing/unreadable file), and the `?` propagates it as a setup error.
    // Under parallelism *which* erroring file's message wins is
    // nondeterministic, but that is acceptable — only the exit *code* (2) is
    // contract; the stderr message is diagnostic-only (design §6).
    let per_file: Result<Vec<Vec<Offense>>, AppError> =
        files.par_iter().map(|file| lint_one_file(file)).collect();
    let offenses = aggregate(per_file?.into_iter().flatten().collect());

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
