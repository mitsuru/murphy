//! `murphy` command-line entry point (Task 7).
//!
//! Phase 1 shape: `murphy lint <file>` runs the single-parse pipeline over
//! one file and prints the aggregated offenses as a JSON array on stdout.
//! Argument parsing is hand-rolled (one subcommand, one arg — no `clap`;
//! YAGNI until the CLI actually grows, design/plan).
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
//! - `2` — config/cop/file-setup error. For Task 7 that is a missing or
//!   unreadable file, a parse failure (Task 8 will turn this into a
//!   syntax-error *offense* instead), or bad CLI usage.
//! - `3` — internal failure: a panic anywhere in the run is caught and mapped
//!   here instead of aborting the process.

use murphy_core::{Cop, NoReceiverPuts, Offense, aggregate, parse, run_cops};
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
/// file, parse failure in Phase 1).
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

/// Parse args, run the pipeline, and return the exit code (or an [`AppError`]
/// carrying a non-success code + stderr message).
///
/// Returns `Ok(code)` for the *expected* outcomes (`0` clean / `1` offenses);
/// `Err` for setup-class failures (`2`). Panics propagate to the guard in
/// [`main`] and become `3`.
fn run(args: &[String]) -> Result<u8, AppError> {
    // args[0] is the program name. Expect exactly: `lint <file>`.
    // `get(1..)` instead of `&args[1..]` so `run(&[])` (a future unit test /
    // refactor) yields a usage error, not a slice-index panic→exit 3.
    let rest = args.get(1..).unwrap_or(&[]);
    let (subcommand, file) = match rest {
        [subcommand, file] => (subcommand.as_str(), file.as_str()),
        _ => {
            return Err(AppError::setup("usage: murphy lint <file>"));
        }
    };

    if subcommand != "lint" {
        return Err(AppError::setup(format!(
            "unknown subcommand {subcommand:?} (usage: murphy lint <file>)"
        )));
    }

    let source = std::fs::read_to_string(Path::new(file))
        .map_err(|e| AppError::setup(format!("cannot read {file:?}: {e}")))?;

    // Phase 1: a parse failure is a setup-class error (exit 2). Task 8 will
    // replace this with emitting a syntax-error offense instead.
    let ast = parse(&source).map_err(|e| AppError::setup(format!("{file}: {e}")))?;

    let cops: Vec<Box<dyn Cop>> = vec![Box::new(NoReceiverPuts)];
    let mut sink: Vec<Offense> = Vec::new();
    run_cops(&ast, file, &cops, &mut sink);
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
