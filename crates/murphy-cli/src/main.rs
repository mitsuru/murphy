//! `murphy` command-line entry point (Task 7; multi-file in Task 9).
//!
//! Phase 1 shape: `murphy lint <file>...` runs the single-parse pipeline
//! over each explicitly-listed file (no directory discovery yet — plan
//! Task 9: "loop over the explicit file list"), aggregates the offenses
//! *across all files*, and prints them as one JSON array on stdout.
//! Argument parsing is hand-rolled (one subcommand, one-or-more file args —
//! no `clap`; YAGNI until the CLI actually grows, design/plan).
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
    Cop, NoReceiverPuts, Offense, SYNTAX_COP_NAME, Severity, aggregate, parse, run_cops,
};
use std::io::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
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
    // args[0] is the program name. Expect: `lint <file>...` (one or more
    // files; processed in arg order, but final output ordering is from
    // `aggregate`, not arg order). `get(1..)` instead of `&args[1..]` so
    // `run(&[])` yields a usage error, not a slice-index panic→exit 3.
    let rest = args.get(1..).unwrap_or(&[]);
    let (subcommand, files) = match rest {
        // `files @ ..` must be non-empty: `lint` with zero files is bad usage.
        [subcommand, files @ ..] if !files.is_empty() => (subcommand.as_str(), files),
        _ => {
            return Err(AppError::setup("usage: murphy lint <file>..."));
        }
    };

    if subcommand != "lint" {
        return Err(AppError::setup(format!(
            "unknown subcommand {subcommand:?} (usage: murphy lint <file>...)"
        )));
    }

    // Loop over the explicit file list (single-threaded; no discovery yet —
    // plan Task 9 / YAGNI). Collect every file's offenses into one flat sink,
    // then aggregate ACROSS all files in a single pass — the cross-file
    // (file, start_offset) sort is what makes the multi-file output
    // deterministic regardless of arg order. A missing/unreadable file is
    // still a setup error (`?` → exit 2); a parse failure on one file does
    // not abort the others (it is an offense in that file's Vec, not `Err`).
    let mut sink: Vec<Offense> = Vec::new();
    for file in files {
        sink.extend(lint_one_file(file)?);
    }
    let offenses = aggregate(sink);

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
