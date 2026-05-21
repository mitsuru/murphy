//! `murphy` command-line entry point (Task 7; multi-file Task 9; discovery
//! Phase 2 Task 6; --fix / -a / --debug Phase 4 Task 6).
//!
//! `murphy lint <path>...` runs the single-parse pipeline over a set of `.rb`
//! files, aggregates the offenses *across all files*, and prints human-readable
//! output by default. `--format json` preserves the machine-readable JSON array
//! contract. Argument parsing is hand-rolled (one subcommand, zero-or-more path
//! args — no `clap`; YAGNI until the CLI actually grows, design/plan).
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
//! The lint path prints the selected report format to stdout on success. The
//! `ast --format sexp` path prints sexp text to stdout on success.
//! Diagnostics (bad usage, unreadable file, parse error) always go to
//! **stderr** for both paths, so error exits print nothing to stdout.
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

mod lsp;
mod profile;
use murphy_core::{
    AstContext, Cop, CopRegistry, FixpointStatus, MurphyConfig, Offense, SYNTAX_COP_NAME, Severity,
    aggregate_with_config, ast_to_sexp, discover_with_config, migrate_rubocop_yml_to_murphy_toml,
    parse, run_cop_timed, run_cops, run_mruby_cop_isolated, run_to_fixpoint,
};
use murphy_reporting::{OutputFormat, format_lint_output};
use profile::{
    ProfileFormatter, ProfileOutputFormat, ProfileSummary, SpeedscopeFormatter, SummaryFormatter,
};
use rayon::prelude::*;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};
use std::thread;
use std::time::Instant;

// Convention inherited by Tasks 8/9: a closed stdout pipe (`murphy ... | head`)
// is NOT a setup error — the consumer hung up. A `BrokenPipe` failure writing
// the JSON to stdout exits `0` (see `run`), not `2`.
/// Exit code: clean — zero offenses.
const EXIT_OK: u8 = 0;

/// Maximum autocorrect fixpoint iterations per file (Phase 4 Task 6, APIN5).
///
/// Rationale: 10 rounds is ample for any realistic cop fixpoint — cops are
/// expected to converge in 1-2 iterations. `run_to_fixpoint` detects
/// oscillation independently of this cap, so a cyclic cop degrades gracefully
/// (Oscillation status) well before 10 rounds. A zero-budget (`0`) would call
/// `lint` never, so we use a positive value. See design §5 ("最大反復で打切り").
const MAX_FIX_ITERATIONS: u32 = 10;

/// Global monotonic counter for unique sibling-temp filenames.
///
/// Each `write_back_atomic` call increments this before constructing the temp
/// filename: `.murphy-fix-<pid>-<counter>.tmp`.  `AtomicU64` prevents
/// collisions across rayon worker threads writing different files in the same
/// run.
static FIX_TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);
/// Exit code: lint found one or more offenses.
const EXIT_OFFENSES: u8 = 1;
/// Exit code: config/cop/file-setup error (bad usage, missing/unreadable
/// file). A parse failure is NOT this — it is a `Murphy/Syntax` offense
/// (design §6), so a syntax-error file exits `1`.
const EXIT_SETUP_ERROR: u8 = 2;
/// Exit code: internal failure (a caught panic).
const EXIT_INTERNAL: u8 = 3;

const LINT_USAGE: &str = "murphy lint [--fix|-a] [--debug] [--format human|json|progress] [--profile] [--profile-format summary|speedscope] [--] [path]...";

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

fn read_ast_source(path: &str) -> Result<String, AppError> {
    if path == "-" {
        let mut source = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut source)
            .map_err(|e| AppError::setup(format!("cannot read stdin: {e}")))?;
        return Ok(source);
    }
    read_source(path)
}

/// A discovered user cop, loaded ONCE per run (P3 Task 7): its host-attributed
/// `cop_name` and the `.rb` source text. Reading the cop file is a
/// **setup-class** concern — a `cops/*.rb` that the registry enumerated but we
/// cannot read is a config/cop-setup error (exit 2, consistent with ADR 0004
/// trusted-cops + discovery's `ConfigError` → exit 2), so it is read here,
/// up-front, ONCE per run — never once per (file × cop).
struct MrubyCop {
    /// `Murphy/<PascalCase(stem)>` — feeds `Offense.cop_name` and the
    /// `aggregate` dedupe key (see [`mruby_cop_name`]).
    cop_name: String,
    /// The `.rb` cop source, read once. `run_mruby_cop_isolated` takes
    /// `&str`; this owns it for the whole run so every (file × cop) call
    /// borrows the same loaded text (no re-read).
    source: String,
}

/// Derive a stable host cop name from a `cops/<stem>.rb` path:
/// `Murphy/<PascalCase(stem)>`. snake_case `_`/`-` segments are dropped and
/// each non-empty segment is capitalized — `no_puts.rb` → `Murphy/NoPuts`,
/// `bad.rb` → `Murphy/Bad`. This is ONE pinned scheme (the e2e test asserts
/// it): it is what `Offense.cop_name` carries for a user cop and therefore
/// part of `aggregate`'s 4-tuple dedupe key. A file with no usable stem (no
/// file name / non-UTF-8) falls back to `Murphy/Cop` rather than panicking —
/// the registry only ever enumerates real `*.rb` regular files so this is
/// defensive, not a normal path.
fn mruby_cop_name(path: &Path) -> String {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let mut pascal = String::new();
    for seg in stem.split(['_', '-']) {
        let mut chars = seg.chars();
        if let Some(first) = chars.next() {
            pascal.extend(first.to_uppercase());
            pascal.push_str(chars.as_str());
        }
    }
    if pascal.is_empty() {
        pascal.push_str("Cop");
    }
    format!("Murphy/{pascal}")
}

/// Load every registry-enumerated `cops/*.rb` ONCE, up-front (NOT per file).
///
/// A read failure on a cop file the registry enumerated is a setup/config
/// error → `AppError::setup` (exit 2), exactly like `read_source` for a lint
/// target and like discovery's `ConfigError`. Doing it here (once per run, in
/// `run()`, before the per-file fan-out) — rather than inside the rayon
/// per-file worker — guarantees each cop file is read exactly once regardless
/// of how many lint targets there are.
///
/// ## Reserved-name collision guard (P3 Task 7 review I-1, ADR-0006 cop_name)
///
/// `mruby_cop_name` derives `Murphy/<PascalCase(stem)>` with no namespace
/// separation from engine-owned names. If a user drops `cops/no_receiver_puts.rb`
/// it derives `Murphy/NoReceiverPuts` — byte-identical to the native cop's
/// `name()` — or `cops/syntax.rb` → [`SYNTAX_COP_NAME`]. Because `aggregate`'s
/// dedupe key is the 4-tuple `(file, cop_name, range, message)`, a user offense
/// at the same range+message as the native/synthetic one would be silently
/// merged away with NO diagnostic — a contract hole the Task-8 gate must not
/// freeze. So: build the RESERVED set from the registry's own native cops
/// (every `registry.native_cops()[*].name()`, derived from the cop — NOT a
/// hardcoded string, so it tracks the registry and cannot drift) plus
/// `SYNTAX_COP_NAME`, and reject any user cop whose derived name collides with
/// a reserved name as a setup/config error (exit 2, ADR-0004 trusted-cop /
/// config-setup error class). The stderr message names the offending cop file
/// path and the reserved name so the user can rename the file.
///
/// NOTE: this guards ONLY collisions with the RESERVED engine names. Two
/// DISTINCT user cop files deriving the SAME `Murphy/...` name (e.g.
/// `foo_bar.rb` + `foo__bar.rb`) is a separate tracked issue (M-1) and is
/// deliberately NOT guarded here.
fn load_mruby_cops(
    registry: &CopRegistry,
    config: &MurphyConfig,
) -> Result<Vec<MrubyCop>, AppError> {
    // RESERVED = engine-owned names a user cop must not shadow. Derived from
    // the live registry (every native cop's own `name()`) + the synthetic
    // syntax-offense name — NOT hardcoded, so adding a native cop automatically
    // extends the reserved set with zero drift.
    let reserved: BTreeSet<String> = registry
        .native_cops()
        .iter()
        .map(|c| c.name().to_string())
        .chain(CopRegistry::native_cop_names())
        .chain(std::iter::once(SYNTAX_COP_NAME.to_string()))
        .collect();

    registry
        .mruby_cop_paths()
        .iter()
        .map(|p| {
            let cop_name = mruby_cop_name(p);
            if !config.cop_enabled(&cop_name) {
                return Ok(None);
            }
            if reserved.contains(&cop_name) {
                return Err(AppError::setup(format!(
                    "cop file {} derives the reserved engine cop name {:?}; \
                     rename the file (a user cop must not shadow an \
                     engine-owned name — its offenses would be silently \
                     deduped against the engine's)",
                    p.display(),
                    cop_name
                )));
            }
            let source = std::fs::read_to_string(p).map_err(|e| {
                AppError::setup(format!("cannot read cop file {}: {e}", p.display()))
            })?;
            Ok(Some(MrubyCop { cop_name, source }))
        })
        .collect::<Result<Vec<Option<MrubyCop>>, AppError>>()
        .map(|cops| cops.into_iter().flatten().collect())
}

/// Read a bounded batch in parallel with a cancellation token.
///
/// Returns `Err` on the first setup error and requests worker threads to
/// stop reading further files as soon as possible. In-flight workers may still
/// be finishing one file read, but we avoid scheduling / executing the next
/// iteration once cancellation is observed.
fn read_batch_sources(
    paths: &[String],
    worker_count: usize,
) -> Result<Vec<(String, String)>, AppError> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }

    let shared_paths = Arc::new(paths.to_vec());
    let shared_next = Arc::new(AtomicUsize::new(0));
    let cancel = Arc::new(AtomicBool::new(false));
    let results: Arc<Mutex<Vec<(String, String)>>> =
        Arc::new(Mutex::new(Vec::with_capacity(paths.len())));
    let first_error: Arc<Mutex<Option<AppError>>> = Arc::new(Mutex::new(None));

    let workers = worker_count.max(1).min(paths.len());
    let mut handles = Vec::with_capacity(workers);

    for _ in 0..workers {
        let shared_paths = Arc::clone(&shared_paths);
        let shared_next = Arc::clone(&shared_next);
        let cancel = Arc::clone(&cancel);
        let results = Arc::clone(&results);
        let first_error = Arc::clone(&first_error);

        let handle = thread::spawn(move || {
            while !cancel.load(Ordering::Acquire) {
                let index = shared_next.fetch_add(1, Ordering::AcqRel);
                if index >= shared_paths.len() {
                    return;
                }

                let path = &shared_paths[index];
                match read_source(path) {
                    Ok(source) => {
                        // If a setup error arrived while this worker was reading,
                        // keep side effects minimal and skip enqueueing the stale
                        // result.
                        if cancel.load(Ordering::Acquire) {
                            return;
                        }
                        results
                            .lock()
                            .expect("result sink lock poisoned")
                            .push((path.clone(), source));
                    }
                    Err(err) => {
                        let was_set = {
                            let mut lock = first_error.lock().expect("first error lock poisoned");
                            let was_set = lock.is_some();
                            if lock.is_none() {
                                *lock = Some(err);
                            }
                            was_set
                        };
                        if !was_set {
                            cancel.store(true, Ordering::Release);
                        }
                        return;
                    }
                }
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle
            .join()
            .expect("failed to join fast-abort read worker thread");
    }

    if let Some(error) = first_error
        .lock()
        .expect("first error lock poisoned")
        .take()
    {
        return Err(error);
    }

    let mut source_paths = results
        .lock()
        .expect("result sink lock poisoned")
        .drain(..)
        .collect::<Vec<(String, String)>>();

    // Keep file order deterministic for downstream grouping/replay logic.
    source_paths.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(source_paths)
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

    lint_source_impl(source, file, cops, false, false).offenses
}

struct TimedNativeOffenses {
    offenses: Vec<Offense>,
    parse_micros: u128,
    cops_micros: u128,
    cop_file_micros: Vec<(String, u64)>,
    cop_dispatch_micros: Vec<(String, u64)>,
}

fn lint_source_timed(
    source: &str,
    file: &str,
    cops: &[Box<dyn Cop>],
    profile: bool,
) -> TimedNativeOffenses {
    lint_source_impl(source, file, cops, true, profile)
}

fn write_lint_output(
    offenses: &[Offense],
    files: &[String],
    format: OutputFormat,
) -> Result<(), AppError> {
    let output = format_lint_output(offenses, files, format).map_err(AppError::setup)?;
    let mut stdout = std::io::stdout().lock();
    if let Err(e) = writeln!(stdout, "{output}") {
        if e.kind() == std::io::ErrorKind::BrokenPipe {
            return Ok(());
        }
        return Err(AppError::setup(format!("failed to write stdout: {e}")));
    }
    Ok(())
}

fn write_profile_output(
    profile_summary: &Option<ProfileSummary>,
    profile_format: ProfileOutputFormat,
) -> Result<(), AppError> {
    let profile_payload = match profile_summary.as_ref() {
        Some(summary) => match profile_format {
            ProfileOutputFormat::Summary => {
                let formatter = SummaryFormatter;
                formatter.format(summary)
            }
            ProfileOutputFormat::Speedscope => {
                let formatter = SpeedscopeFormatter;
                formatter.format(summary)
            }
        },
        None => Value::Null,
    };

    let mut stdout = std::io::stdout().lock();
    if let Err(e) = writeln!(stdout, "{}", profile_payload) {
        if e.kind() == std::io::ErrorKind::BrokenPipe {
            return Ok(());
        }
        return Err(AppError::setup(format!("failed to write stdout: {e}")));
    }

    Ok(())
}

fn lint_source_impl(
    source: &str,
    file: &str,
    cops: &[Box<dyn Cop>],
    measure: bool,
    profile: bool,
) -> TimedNativeOffenses {
    let mut sink: Vec<Offense> = Vec::new();
    let parse_started = measure.then(Instant::now);
    let parsed = parse(source);
    let parse_micros = parse_started
        .map(|started| started.elapsed().as_micros())
        .unwrap_or(0);
    let mut cops_micros: u128 = 0;
    let mut cop_file_micros: Vec<(String, u64)> = Vec::new();
    let mut cop_dispatch_micros: Vec<(String, u64)> = Vec::new();

    match parsed {
        Ok(ast) => {
            if profile {
                for cop in cops {
                    let cop_name = cop.name().to_string();
                    let mut cop_sink: Vec<Offense> = Vec::new();
                    let timings = run_cop_timed(&ast, file, cop.as_ref(), &mut cop_sink);
                    sink.extend(cop_sink);
                    cops_micros +=
                        u128::from(timings.inspect_file_micros + timings.dispatch_micros);
                    if timings.inspect_file_micros > 0 {
                        cop_file_micros.push((cop_name.clone(), timings.inspect_file_micros));
                    }
                    if timings.dispatch_micros > 0 {
                        cop_dispatch_micros.push((cop_name, timings.dispatch_micros));
                    }
                }
            } else {
                let cops_started = measure.then(Instant::now);
                run_cops(&ast, file, cops, &mut sink);
                cops_micros = cops_started
                    .map(|started| started.elapsed().as_micros())
                    .unwrap_or(0);
            }
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
    TimedNativeOffenses {
        offenses: apply_inline_directive_filter(sink, source),
        parse_micros,
        cops_micros,
        cop_file_micros,
        cop_dispatch_micros,
    }
}

#[derive(Debug, Clone)]
enum InlineDirectiveKind {
    Disable,
    Enable,
    Todo,
}

#[derive(Debug, Clone)]
struct InlineDirective {
    kind: InlineDirectiveKind,
    cop: Option<String>,
}

#[derive(Debug, Clone)]
struct DirectiveState {
    disable_all: bool,
    disabled_cops: BTreeSet<String>,
    todo_all: bool,
    todo_cops: BTreeSet<String>,
    line_start: usize,
    line_end: usize,
}

fn parse_inline_directive(line: &str) -> Option<InlineDirective> {
    let hash_pos = line.find('#')?;
    let comment = line[hash_pos + 1..].trim_start();

    let rest = comment.strip_prefix("murphy:")?;
    let mut parts = rest.split_whitespace();
    let keyword = parts.next()?;

    let cop = parts.next().map(str::to_string);

    let kind = match keyword {
        "disable" => InlineDirectiveKind::Disable,
        "enable" => InlineDirectiveKind::Enable,
        "todo" => InlineDirectiveKind::Todo,
        _ => return None,
    };

    Some(InlineDirective { kind, cop })
}

fn directive_states_by_line(source: &str) -> Vec<DirectiveState> {
    let mut states = Vec::new();
    let mut disable_all = false;
    let mut disabled_cops: BTreeSet<String> = BTreeSet::new();

    let mut offset = 0usize;
    for line in source.split_inclusive('\n') {
        let line_start = offset;
        let line_end = offset + line.len();
        let mut todo_all = false;
        let mut todo_cops: BTreeSet<String> = BTreeSet::new();

        if let Some(directive) = parse_inline_directive(line) {
            match (directive.kind, directive.cop) {
                (InlineDirectiveKind::Disable, Some(cop)) => {
                    disabled_cops.insert(cop);
                }
                (InlineDirectiveKind::Disable, None) => {
                    disable_all = true;
                }
                (InlineDirectiveKind::Enable, Some(cop)) => {
                    disabled_cops.remove(&cop);
                }
                (InlineDirectiveKind::Enable, None) => {
                    disable_all = false;
                    disabled_cops.clear();
                }
                (InlineDirectiveKind::Todo, Some(cop)) => {
                    todo_cops.insert(cop);
                }
                (InlineDirectiveKind::Todo, None) => {
                    todo_all = true;
                }
            }
        }

        states.push(DirectiveState {
            disable_all,
            disabled_cops: disabled_cops.clone(),
            todo_all,
            todo_cops,
            line_start,
            line_end,
        });

        offset = line_end;
    }

    states
}

fn is_directive_disabled(offense: &Offense, states: &[DirectiveState]) -> bool {
    if offense.cop_name == SYNTAX_COP_NAME {
        return false;
    }

    let start = offense.range.start_offset as usize;
    for state in states {
        if start >= state.line_start && start < state.line_end {
            return state.disable_all
                || state.disabled_cops.contains(&offense.cop_name)
                || state.todo_all
                || state.todo_cops.contains(&offense.cop_name);
        }
    }

    false
}

fn apply_inline_directive_filter(mut offenses: Vec<Offense>, source: &str) -> Vec<Offense> {
    if offenses.is_empty() {
        return Vec::new();
    }

    let states = directive_states_by_line(source);
    offenses.retain(|offense| !is_directive_disabled(offense, &states));
    offenses
}

/// Run every discovered mruby user cop over `source` ONCE, labeling every
/// offense (real OR error offense) with `file` (P3 Task 7).
///
/// One `AstContext` (Task 2) is built **once for this source** and shared (by
/// `Arc`) across all user cops — the user cops and the native pass see the
/// SAME bytes for this file (semantic "one parse per file": `AstContext::new`
/// is mruby's parse seam; native uses `parse` directly; both consume the
/// identical `source`, so offsets/ranges agree). Per (file × cop) we make
/// exactly ONE `run_mruby_cop_isolated` call — Task 5 OWNS the per-cop
/// OS-thread + wall-clock watchdog + abandon-on-timeout + Ruby-exception → one
/// error offense; this site does NOT re-implement any of that and does NOT
/// join/block the (possibly abandoned) cop thread. Each
/// `run_mruby_cop_isolated` call clones its OWN child `Arc<AstContext>` (ADR
/// 0009 rule 1), so an abandoned late-finishing cop keeps the AST alive on its
/// own clone independently of `ctx` here — dropping `ctx` at the end of this
/// function never pulls the arena out from under a zombie cop thread.
///
/// Determinism / memo interaction: this is called ONCE per UNIQUE content
/// (the [`lint_files_memoized`] caller fans the result out per path with
/// `Offense.file` rewritten, exactly as for the native pass), so duplicate
/// content is NOT re-run and the byte-identical-regardless-of-duplication
/// property holds. Final ordering/dedupe across native + every user cop is
/// `aggregate`'s sole responsibility (Task 6 total order + severity
/// precedence) — this function only collects.
///
/// A source that fails to parse: native already emitted the one
/// `Murphy/Syntax` offense and skipped its cops (design §6, no AST). The user
/// cops are likewise skipped here — there is no usable tree to traverse and
/// the syntax error is already reported once. (`AstContext::new` is only built
/// when `parse` succeeded, so this is also strictly less work.)
fn lint_source_mruby(
    source: &str,
    file: &str,
    mruby_cops: &[MrubyCop],
    profile: bool,
) -> (Vec<Offense>, Vec<(String, u64)>) {
    // I-2 (deferred to Phase 4, tracked): this `parse(source).is_err()` then
    // `AstContext::new` (which parses again) double-parses unique cop'd content.
    // It is NOT collapsed into a single `ctx.parse_result().errors()` check
    // because `crate::parse::parse` ALSO applies the `exceeds_offset_domain`
    // u32 byte-offset guard up front, which `ParseResult::errors()` does not
    // subsume — collapsing would silently drop that guard, so it is not
    // provably byte-identical (murphy-cql hard gate: any doubt ⇒ defer).
    if mruby_cops.is_empty() || parse(source).is_err() {
        return (Vec::new(), Vec::new());
    }
    let ctx: Arc<AstContext> = AstContext::new(source.as_bytes().to_vec());
    let mut sink: Vec<Offense> = Vec::new();
    let mut cop_micros: Vec<(String, u64)> = Vec::new();
    for cop in mruby_cops {
        let started = Instant::now();
        sink.extend(run_mruby_cop_isolated(
            &ctx,
            &cop.source,
            &cop.cop_name,
            file,
        ));
        let micros = started.elapsed().as_micros() as u64;
        if profile && micros > 0 {
            cop_micros.push((cop.cop_name.clone(), micros));
        }
    }
    let sink = apply_inline_directive_filter(sink, source);
    (sink, if profile { cop_micros } else { Vec::new() })
}

/// Write `corrected` to `target` atomically using a sibling-temp + rename.
///
/// # APIN2 (data-safety, BLOCKER)
///
/// `std::fs::write` truncates the target file before writing — if the process
/// is interrupted between truncate and write, the file is lost. Instead:
///
/// 1. **Resolve symlinks.** `read_source` reads *through* a symlink, so the
///    write must target the link's destination too. `canonicalize` resolves
///    `target` to the real file; the temp + rename happen in the **real
///    file's** directory. Renaming onto the link path itself would replace the
///    symlink with a regular file and leave the real destination stale
///    (roborev medium) — resolving first preserves the link and updates the
///    actual content.
/// 2. **Preserve mode.** A fresh temp gets umask permissions, so an executable
///    Ruby script or a group-writable file would silently lose its mode after
///    `--fix` (roborev medium). The real file's `Permissions` are captured
///    before writing and applied to the temp before the rename. (Owner/group
///    and timestamps are intentionally not replicated — out of scope for v1;
///    `rename` keeps the destination inode's owner on POSIX overwrite.)
/// 3. Write `corrected` to a sibling temp `.murphy-fix-<pid>-<N>.tmp` in the
///    real file's directory (same filesystem → `rename` is atomic on POSIX),
///    `set_permissions`, then `rename` over the real path — no truncation
///    window. On any error, best-effort temp cleanup.
///
/// The `tempfile` crate is in `[dev-dependencies]` only and MUST NOT be used
/// here. This manual implementation is the production write-back path.
fn write_back_atomic(target: &Path, corrected: &str) -> Result<(), AppError> {
    // Resolve symlinks: write the link's destination, not the link itself.
    let real = std::fs::canonicalize(target).map_err(|e| {
        AppError::setup(format!(
            "cannot resolve {} for --fix: {e}",
            target.display()
        ))
    })?;

    // Capture the real file's permissions so the rewritten file keeps its mode
    // (executable scripts, group-writable, …) instead of inheriting umask.
    let perms = std::fs::metadata(&real)
        .map_err(|e| AppError::setup(format!("cannot stat {} for --fix: {e}", real.display())))?
        .permissions();

    let parent = real.parent().unwrap_or_else(|| Path::new("."));
    let counter = FIX_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let tmp_name = format!(".murphy-fix-{pid}-{counter}.tmp");
    let tmp_path = parent.join(&tmp_name);

    // Write corrected content to the temp file.
    if let Err(e) = std::fs::write(&tmp_path, corrected) {
        // Best-effort cleanup — ignore errors (file may not exist yet).
        let _ = std::fs::remove_file(&tmp_path);
        return Err(AppError::setup(format!(
            "cannot write temp file {}: {e}",
            tmp_path.display()
        )));
    }

    // Restore the original file's mode onto the temp before renaming.
    if let Err(e) = std::fs::set_permissions(&tmp_path, perms) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(AppError::setup(format!(
            "cannot set permissions on temp file {}: {e}",
            tmp_path.display()
        )));
    }

    // Atomic rename: temp → real file (same directory → same filesystem).
    // Renaming onto the resolved real path (not `target`) keeps any symlink
    // pointing at it intact.
    if let Err(e) = std::fs::rename(&tmp_path, &real) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(AppError::setup(format!(
            "cannot rename {} → {}: {e}",
            tmp_path.display(),
            real.display()
        )));
    }

    Ok(())
}

/// Build the lint closure for `run_to_fixpoint`: parse `source`, run native +
/// mruby cops, aggregate, then collect every `autocorrect.edits` into
/// `Vec<Edit>`.
///
/// `aggregate` is called inside the closure so cross-engine determinism is
/// preserved on every fixpoint iteration, not just the final one.
fn lint_closure_edits<'a>(
    source: &str,
    file: &'a str,
    registry: &'a CopRegistry,
    mruby_cops: &'a [MrubyCop],
    config: &'a MurphyConfig,
) -> Vec<murphy_core::Edit> {
    let mut sink = lint_source(source, file, registry.native_cops());
    let (mruby_offenses, _) = lint_source_mruby(source, file, mruby_cops, false);
    sink.extend(mruby_offenses);
    aggregate_with_config(sink, config)
        .into_iter()
        .filter_map(|o| o.autocorrect.map(|ac| ac.edits))
        .flatten()
        .collect()
}

/// Per-file debug info collected during `--fix` (APIN4).
///
/// **Scope boundary (APIN4)**: this struct carries only autocorrect
/// observability (fixpoint iterations, status, conflicts). Plain lint progress
/// is emitted separately by `lint_files_memoized_debug`.
struct FileDebugInfo {
    path: String,
    iterations: u32,
    status: FixpointStatus,
    conflict_count: usize,
    conflict_reasons: Vec<murphy_core::ConflictReason>,
    was_written: bool,
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
/// 2. **Lint phase.** Group and lint in bounded-size batches to avoid
///    holding every discovered file body simultaneously: read a batch, dedupe by
///    content (exact equality via `String` key), lint only newly seen batch
///    contents in parallel, and keep parsed base offenses for fan-out.
///    `aggregate` remains the single determinism point.
///
/// The flattened per-path offenses go to `aggregate` UNCHANGED — it remains
/// the single determinism point (the total-order sort/dedupe, Task 2), so
/// neither read/lint parallelism nor the fan-out order can perturb output.
#[cfg(test)]
fn lint_files_memoized(
    files: &[String],
    registry: &CopRegistry,
    mruby_cops: &[MrubyCop],
    config: &MurphyConfig,
) -> Result<Vec<Offense>, AppError> {
    let mut profile_summary = None;
    lint_files_memoized_debug(
        files,
        registry,
        mruby_cops,
        config,
        false,
        &mut profile_summary,
    )
}

fn lint_files_memoized_debug(
    files: &[String],
    registry: &CopRegistry,
    mruby_cops: &[MrubyCop],
    config: &MurphyConfig,
    debug: bool,
    profile_summary: &mut Option<ProfileSummary>,
) -> Result<Vec<Offense>, AppError> {
    #[derive(Default)]
    struct ContentGroup {
        representative: String,
        paths: Vec<String>,
        base_offenses: Vec<Offense>,
        native_cop_file_micros: Vec<(String, u64)>,
        native_cop_dispatch_micros: Vec<(String, u64)>,
        mruby_cop_micros: Vec<(String, u64)>,
    }

    const BATCH_SIZE: usize = 128;
    let started = Instant::now();
    if debug {
        eprintln!(
            "murphy: debug: lint start files={} batch_size={} elapsed_ms={}",
            files.len(),
            BATCH_SIZE,
            started.elapsed().as_millis()
        );
    }

    // Phase 1+2 (streaming): read in bounded batches, then lint each batch's
    // newly-seen content once. This keeps the read-source peak memory bounded,
    // while still preserving byte-for-byte output when combined with the final
    // aggregate determinism point.

    // `available_parallelism` fallback is safe and keeps behavior deterministic
    // across platforms without external config.
    let worker_count = thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(1);

    let mut by_content: BTreeMap<String, ContentGroup> = BTreeMap::new();
    let batch_count = files.len().div_ceil(BATCH_SIZE);
    for (batch_index, chunk) in files.chunks(BATCH_SIZE).enumerate() {
        let read: Vec<(String, String)> = read_batch_sources(chunk, worker_count)?;
        if debug {
            eprintln!(
                "murphy: debug: batch {}/{} read files={} elapsed_ms={}",
                batch_index + 1,
                batch_count,
                read.len(),
                started.elapsed().as_millis()
            );
        }

        // Track newly introduced content keys so we can parse each exactly once.
        let mut newly_seen: Vec<String> = Vec::new();
        for (path, source) in read {
            match by_content.entry(source) {
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    entry.get_mut().paths.push(path);
                }
                std::collections::btree_map::Entry::Vacant(entry) => {
                    let content_key = entry.key().clone();
                    entry.insert(ContentGroup {
                        representative: path.clone(),
                        paths: vec![path],
                        base_offenses: Vec::new(),
                        native_cop_file_micros: Vec::new(),
                        native_cop_dispatch_micros: Vec::new(),
                        mruby_cop_micros: Vec::new(),
                    });
                    newly_seen.push(content_key);
                }
            }
        }

        // Parse/lint each newly introduced content exactly once, in parallel.
        let should_record_profile = profile_summary.is_some();
        let should_measure_timings = should_record_profile || debug;
        let parse_jobs: Vec<(String, String)> = newly_seen
            .iter()
            .map(|content| {
                let representative = by_content
                    .get(content)
                    .expect("newly discovered content must exist")
                    .representative
                    .clone();
                (content.clone(), representative)
            })
            .collect();

        let parsed = parse_jobs
            .par_iter()
            .map(|(content, representative)| {
                let (
                    mut base_offenses,
                    parse_micros,
                    cops_micros,
                    native_cop_file_micros,
                    native_cop_dispatch_micros,
                ) = if should_measure_timings {
                    let timed = lint_source_timed(
                        content,
                        representative,
                        registry.native_cops(),
                        should_record_profile,
                    );
                    (
                        timed.offenses,
                        timed.parse_micros,
                        timed.cops_micros,
                        timed.cop_file_micros,
                        timed.cop_dispatch_micros,
                    )
                } else {
                    (
                        lint_source(content, representative, registry.native_cops()),
                        0,
                        0,
                        Vec::new(),
                        Vec::new(),
                    )
                };

                let native_micros = parse_micros + cops_micros;

                let (mruby_offenses, mruby_cop_timings, mruby_micros) = if should_measure_timings {
                    let mruby_start = Instant::now();
                    let (offenses, timings) = lint_source_mruby(
                        content,
                        representative,
                        mruby_cops,
                        should_record_profile,
                    );
                    let elapsed = mruby_start.elapsed().as_micros();
                    let _cop_micros: u128 =
                        timings.iter().map(|(_, micros)| u128::from(*micros)).sum();
                    (offenses, timings, elapsed)
                } else {
                    (
                        lint_source_mruby(content, representative, mruby_cops, false).0,
                        Vec::new(),
                        0,
                    )
                };
                base_offenses.extend(mruby_offenses);
                let mruby_micros = if should_measure_timings {
                    mruby_micros
                } else {
                    0
                };

                (
                    content.clone(),
                    base_offenses,
                    native_micros,
                    parse_micros,
                    cops_micros,
                    mruby_micros,
                    native_cop_file_micros,
                    native_cop_dispatch_micros,
                    mruby_cop_timings,
                )
            })
            .collect::<Vec<(
                String,
                Vec<Offense>,
                u128,
                u128,
                u128,
                u128,
                Vec<(String, u64)>,
                Vec<(String, u64)>,
                Vec<(String, u64)>,
            )>>();
        if debug {
            let offense_count: usize = parsed
                .iter()
                .map(|(_, offenses, _, _, _, _, _, _, _)| offenses.len())
                .sum();
            let native_micros: u128 = parsed
                .iter()
                .map(|(_, _, native_micros, _, _, _, _, _, _)| native_micros)
                .sum();
            let parse_micros: u128 = parsed
                .iter()
                .map(|(_, _, _, parse_micros, _, _, _, _, _)| parse_micros)
                .sum();
            let cops_micros: u128 = parsed
                .iter()
                .map(|(_, _, _, _, cops_micros, _, _, _, _)| cops_micros)
                .sum();
            let mruby_micros: u128 = parsed
                .iter()
                .map(|(_, _, _, _, _, mruby_micros, _, _, _)| mruby_micros)
                .sum();
            eprintln!(
                "murphy: debug: batch {}/{} lint unique={} offenses={} native_ms={} parse_ms={} cops_ms={} mruby_ms={} elapsed_ms={}",
                batch_index + 1,
                batch_count,
                parsed.len(),
                offense_count,
                native_micros / 1_000,
                parse_micros / 1_000,
                cops_micros / 1_000,
                mruby_micros / 1_000,
                started.elapsed().as_millis()
            );
        }

        for (
            content,
            base_offenses,
            _native_micros,
            parse_micros,
            _cops_micros,
            _mruby_micros,
            native_cop_file_timings,
            native_cop_dispatch_timings,
            mruby_cop_timings,
        ) in parsed
        {
            let group = by_content
                .get_mut(&content)
                .expect("parsed content must still exist");
            group.base_offenses = base_offenses;

            if should_record_profile {
                let file = group.representative.clone();
                if let Some(summary) = profile_summary.as_mut() {
                    summary.record_parse(&file, parse_micros);
                    for (cop_name, micros) in &native_cop_file_timings {
                        summary.record_native_file(cop_name, &file, *micros);
                        group
                            .native_cop_file_micros
                            .push((cop_name.to_string(), *micros));
                    }
                    for (cop_name, micros) in &native_cop_dispatch_timings {
                        summary.record_native_dispatch(cop_name, &file, *micros);
                        group
                            .native_cop_dispatch_micros
                            .push((cop_name.to_string(), *micros));
                    }
                    for (cop_name, micros) in &mruby_cop_timings {
                        summary.record_mruby(cop_name, &file, *micros);
                        group.mruby_cop_micros.push((cop_name.to_string(), *micros));
                    }
                } else {
                    for (cop_name, micros) in &native_cop_file_timings {
                        group
                            .native_cop_file_micros
                            .push((cop_name.to_string(), *micros));
                    }
                    for (cop_name, micros) in &native_cop_dispatch_timings {
                        group
                            .native_cop_dispatch_micros
                            .push((cop_name.to_string(), *micros));
                    }
                    for (cop_name, micros) in &mruby_cop_timings {
                        group.mruby_cop_micros.push((cop_name.to_string(), *micros));
                    }
                }
            }
        }
    }

    let by_content_len = by_content.len();
    let per_unique: Vec<Vec<Offense>> = by_content
        .into_values()
        .map(|group| {
            group
                .paths
                .iter()
                .flat_map(|path| {
                    group.base_offenses.iter().map(|offense| {
                        let mut offense = offense.clone();
                        offense.file = (*path).to_owned();
                        offense
                    })
                })
                .collect::<Vec<Offense>>()
        })
        .collect();

    // `aggregate` is the single, unchanged determinism point (Task 2).
    let pre_aggregate: Vec<Offense> = per_unique.into_iter().flatten().collect();
    if debug {
        let mut by_cop: BTreeMap<&str, usize> = BTreeMap::new();
        for offense in &pre_aggregate {
            *by_cop.entry(offense.cop_name.as_str()).or_default() += 1;
        }
        let mut top_cops: Vec<(&str, usize)> = by_cop.into_iter().collect();
        top_cops.sort_by(|(left_name, left_count), (right_name, right_count)| {
            right_count
                .cmp(left_count)
                .then_with(|| left_name.cmp(right_name))
        });
        let top_cops = top_cops
            .into_iter()
            .take(10)
            .map(|(cop_name, count)| format!("{cop_name}={count}"))
            .collect::<Vec<_>>()
            .join(",");
        eprintln!("murphy: debug: top cops {top_cops}");
        eprintln!(
            "murphy: debug: aggregate input_offenses={} unique_contents={} elapsed_ms={}",
            pre_aggregate.len(),
            by_content_len,
            started.elapsed().as_millis()
        );
    }
    let offenses = aggregate_with_config(pre_aggregate, config);
    if debug {
        eprintln!(
            "murphy: debug: aggregate done offenses={} elapsed_ms={}",
            offenses.len(),
            started.elapsed().as_millis()
        );
    }
    Ok(offenses)
}

/// Parse args, run the pipeline, and return the exit code (or an [`AppError`]
/// carrying a non-success code + stderr message).
///
/// Returns `Ok(code)` for the *expected* outcomes (`0` clean / `1` offenses);
/// `Err` for setup-class failures (`2`). Panics propagate to the guard in
/// [`main`] and become `3`.
fn run(args: &[String]) -> Result<u8, AppError> {
    // args[0] is the program name. Expect: `lint [flags...] [path]...` or
    // `migrate <.rubocop.yml>` — the
    // subcommand then ZERO-or-more flag/path args. `get(1..)` instead of
    // `&args[1..]` so `run(&[])` yields a usage error, not a slice-index
    // panic→exit 3.
    //
    // Subcommand is extracted FIRST and validated independently of path
    // count: a missing subcommand (`murphy`) or a wrong one is bad usage →
    // exit 2 and never reaches discovery. ZERO paths is NOT bad usage (Task 6
    // change): `murphy lint` discovers from the cwd.
    let rest = args.get(1..).unwrap_or(&[]);
    let (subcommand, post_subcommand) = match rest {
        [subcommand, rest @ ..] => (subcommand.as_str(), rest),
        [] => {
            return Err(AppError::setup(
                "usage: murphy lint [flags] [path]... | murphy migrate <.rubocop.yml> | murphy lsp | murphy ast --format sexp <path|->",
            ));
        }
    };

    if subcommand == "migrate" {
        let path = match post_subcommand {
            [path] => path,
            _ => return Err(AppError::setup("usage: murphy migrate <.rubocop.yml>")),
        };
        let text = std::fs::read_to_string(path)
            .map_err(|e| AppError::setup(format!("cannot read {path:?}: {e}")))?;
        let toml = migrate_rubocop_yml_to_murphy_toml(&text)
            .map_err(|e| AppError::setup(e.to_string()))?;
        let mut stdout = std::io::stdout().lock();
        if let Err(e) = write!(stdout, "{toml}") {
            if e.kind() == std::io::ErrorKind::BrokenPipe {
                return Ok(EXIT_OK);
            }
            return Err(AppError::setup(format!("failed to write stdout: {e}")));
        }
        return Ok(EXIT_OK);
    }

    if subcommand == "lsp" {
        return lsp::run(post_subcommand);
    }

    if subcommand == "ast" {
        let path = match post_subcommand {
            [format, kind, path] if format == "--format" && kind == "sexp" => path,
            _ => return Err(AppError::setup("usage: murphy ast --format sexp <path|->")),
        };
        let source = read_ast_source(path)?;
        let ast = parse(&source).map_err(|err| AppError {
            code: EXIT_OFFENSES,
            message: err.to_string(),
        })?;
        let sexp = ast_to_sexp(&ast);
        let mut stdout = std::io::stdout().lock();
        if let Err(e) = writeln!(stdout, "{sexp}") {
            if e.kind() == std::io::ErrorKind::BrokenPipe {
                return Ok(EXIT_OK);
            }
            return Err(AppError::setup(format!("failed to write stdout: {e}")));
        }
        return Ok(EXIT_OK);
    }

    if subcommand != "lint" {
        return Err(AppError::setup(format!(
            "unknown subcommand {subcommand:?} (usage: {LINT_USAGE} | murphy migrate <.rubocop.yml> | murphy lsp | murphy ast --format sexp <path|->)"
        )));
    }

    // ── Phase 4 Task 6: flag extraction ────────────────────────────────────
    //
    // Strip `--fix` / `-a` / `--debug` from the post-subcommand token list
    // BEFORE path classification.  Any other token starting with `-` is an
    // unknown flag → setup error exit 2 (consistent with unknown-subcommand
    // handling; do NOT silently treat as a path).
    //
    // `--` is the end-of-flags separator (POSIX convention): every token
    // AFTER it is a path even if it starts with `-` (so `murphy lint --
    // -foo.rb` lints a file literally named `-foo.rb`). This restores the
    // pre-.6 ability to name `-`-prefixed files, which the flag check above
    // would otherwise reject as an unknown flag (roborev low: regression).
    let mut fix = false;
    let mut debug = false;
    let mut profile = false;
    let mut profile_format = ProfileOutputFormat::Summary;
    let mut output_format = OutputFormat::Human;
    let mut path_args: Vec<&str> = Vec::new();
    let mut flags_done = false;
    let mut pending_format = false;
    let mut pending_profile_format = false;
    let mut profile_format_set = false;

    for token in post_subcommand {
        if flags_done {
            path_args.push(token.as_str());
            continue;
        }
        if pending_profile_format {
            profile_format = ProfileOutputFormat::parse(token).ok_or_else(|| {
                AppError::setup(format!(
                    "unknown --profile-format value {token:?} (supported: summary, speedscope)"
                ))
            })?;
            pending_profile_format = false;
            profile_format_set = true;
            continue;
        }
        if pending_format {
            output_format = match token.as_str() {
                "human" => OutputFormat::Human,
                "json" => OutputFormat::Json,
                "progress" => OutputFormat::Progress,
                value => {
                    return Err(AppError::setup(format!(
                        "unknown format {value:?} (supported: human, json, progress)"
                    )));
                }
            };
            pending_format = false;
            continue;
        }
        match token.as_str() {
            "--" => flags_done = true,
            "--fix" | "-a" => fix = true,
            "--debug" => debug = true,
            "--profile" => profile = true,
            "--profile-format" => pending_profile_format = true,
            "--format" => pending_format = true,
            flag if flag.starts_with('-') => {
                return Err(AppError::setup(format!(
                    "unknown flag {flag:?} (usage: {LINT_USAGE}; use `--` before a path that starts with `-`)"
                )));
            }
            path => path_args.push(path),
        }
    }

    if pending_profile_format {
        return Err(AppError::setup(
            "missing value for --profile-format (supported: summary, speedscope)",
        ));
    }
    if pending_format {
        return Err(AppError::setup(
            "missing value for --format (supported: human, json, progress)",
        ));
    }
    if !profile && profile_format_set {
        return Err(AppError::setup(
            "--profile-format requires --profile (use --profile --profile-format summary|speedscope)",
        ));
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
    let run_started = Instant::now();
    if debug {
        eprintln!("murphy: debug: config load start elapsed_ms=0");
    }
    let config = MurphyConfig::load(Path::new(".")).map_err(|e| AppError::setup(e.to_string()))?;
    if debug {
        eprintln!(
            "murphy: debug: config load done elapsed_ms={}",
            run_started.elapsed().as_millis()
        );
        eprintln!(
            "murphy: debug: cop registry load start elapsed_ms={}",
            run_started.elapsed().as_millis()
        );
    }
    let registry = CopRegistry::discover_with_config(Path::new("."), &config)
        .map_err(|e| AppError::setup(e.to_string()))?;
    if debug {
        eprintln!(
            "murphy: debug: cop registry load done native_cops={} elapsed_ms={}",
            registry.native_cops().len(),
            run_started.elapsed().as_millis()
        );
    }

    // Load every enumerated `cops/*.rb` ONCE, up-front (P3 Task 7). Reading a
    // cop file the registry enumerated but cannot be read is a setup/config
    // error → exit 2 (ADR 0004 trusted-cops + discovery `ConfigError` → exit
    // 2), so it is surfaced HERE — before any linting and exactly once per
    // run, NOT once per (file × cop). An empty `cops/` (or none) → empty Vec,
    // so the native-only path (e.g. `sample_project`) is byte-identical: the
    // mruby pass is a no-op when there are no user cops.
    let mruby_cops = load_mruby_cops(&registry, &config)?;
    if debug {
        eprintln!(
            "murphy: debug: mruby cops load done cops={} elapsed_ms={}",
            mruby_cops.len(),
            run_started.elapsed().as_millis()
        );
        eprintln!(
            "murphy: debug: discovery start args={} elapsed_ms={}",
            path_args.len(),
            run_started.elapsed().as_millis()
        );
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
    if path_args.is_empty() {
        // Zero path args: discover from the cwd.
        for p in discover_with_config(Path::new("."), &config)
            .map_err(|e| AppError::setup(e.to_string()))?
        {
            targets.insert(p);
        }
    } else {
        for arg in &path_args {
            let path = Path::new(arg);
            if path.is_dir() {
                let dir_config = if path == Path::new(".") {
                    config.clone()
                } else {
                    MurphyConfig::load(path).map_err(|e| AppError::setup(e.to_string()))?
                };
                for p in discover_with_config(path, &dir_config)
                    .map_err(|e| AppError::setup(e.to_string()))?
                {
                    targets.insert(p);
                }
            } else {
                // Existing file OR non-existent path: explicit (Phase 1).
                targets.insert(path.to_path_buf());
            }
        }
    }
    if debug {
        eprintln!(
            "murphy: debug: discovery done files={} elapsed_ms={}",
            targets.len(),
            run_started.elapsed().as_millis()
        );
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

    let mut profile_summary = if profile {
        Some(ProfileSummary::default())
    } else {
        None
    };

    // ── --fix pipeline (Phase 4 Task 6) ────────────────────────────────────
    if fix {
        // Per-file fixpoint: read → run_to_fixpoint → write-back if changed.
        // Files are processed deterministically (files is already sorted from
        // BTreeSet). Debug info is collected in order and printed after all
        // writes (APIN4 scope: autocorrect observability only).
        let mut fixed_count: usize = 0;
        let mut processed: usize = 0;
        let mut debug_infos: Vec<FileDebugInfo> = Vec::with_capacity(files.len());
        // The first write-back error aborts the pass. The summary is still
        // emitted (APIN3: exactly one line whenever --fix runs a pass) but its
        // denominator is the number of files ACTUALLY processed, not the full
        // target count — claiming "N of <all>" when later files were never
        // touched is inaccurate (roborev medium).
        let mut write_error: Option<AppError> = None;

        for file in &files {
            if write_error.is_some() {
                break;
            }
            processed += 1;
            let original = read_source(file)?;
            let file_str: &str = file.as_str();
            let registry_ref = &registry;
            let mruby_cops_ref = &mruby_cops;

            let outcome = run_to_fixpoint(
                &original,
                |s| lint_closure_edits(s, file_str, registry_ref, mruby_cops_ref, &config),
                MAX_FIX_ITERATIONS,
            );

            let was_written = if outcome.corrected != original {
                // APIN2: sibling-temp + rename (never std::fs::write in-place).
                match write_back_atomic(Path::new(file), &outcome.corrected) {
                    Ok(()) => {
                        fixed_count += 1;
                        true
                    }
                    Err(e) => {
                        write_error = Some(e);
                        false
                    }
                }
            } else {
                false
            };

            if debug {
                debug_infos.push(FileDebugInfo {
                    path: file.clone(),
                    iterations: outcome.iterations,
                    status: outcome.status,
                    conflict_count: outcome.conflicts.len(),
                    conflict_reasons: outcome.conflicts.iter().map(|c| c.reason.clone()).collect(),
                    was_written,
                });
            }
        }

        // APIN3: ALWAYS emit exactly one summary line to stderr (regardless of
        // --debug). Denominator = files actually processed (== total_files on
        // a clean pass; < total_files if a write error aborted partway, so the
        // count never overstates work that did not happen).
        debug_assert!(processed <= files.len());
        eprintln!("murphy: fixed {fixed_count} of {processed} files");

        // APIN4 --debug: emit per-file autocorrect observability to STDERR.
        // Scope boundary: ONLY fixpoint iterations / status / conflicts.
        // Per-cop timing / deadline / exception observability is OUT of .6
        // scope — see APIN4 in the issue design.
        if debug {
            for info in &debug_infos {
                let status_label = match info.status {
                    FixpointStatus::Converged => "Converged",
                    FixpointStatus::MaxIterations => "MaxIterations",
                    FixpointStatus::Oscillation => "Oscillation",
                };
                eprintln!(
                    "murphy: debug: {} iterations={} status={} conflicts={} written={}",
                    info.path, info.iterations, status_label, info.conflict_count, info.was_written,
                );
                for reason in &info.conflict_reasons {
                    eprintln!("murphy: debug:   conflict reason={reason:?}");
                }
                if matches!(
                    info.status,
                    FixpointStatus::MaxIterations | FixpointStatus::Oscillation
                ) {
                    eprintln!(
                        "murphy: WARNING: {} did not converge (status={status_label})",
                        info.path
                    );
                } else if info.conflict_count > 0 {
                    // status == Converged but the final round still produced
                    // conflicts: the source is a stable fixed point (correct
                    // per run_to_fixpoint), yet some edits were never applied.
                    // Spell this out so a bare "Converged" is not read as
                    // "fully resolved" (roborev medium: Converged-with-
                    // conflicts must not look identical to a clean converge).
                    eprintln!(
                        "murphy: WARNING: {} converged with {} unresolved conflict(s) \
                         (source stable but some edits could not be applied)",
                        info.path, info.conflict_count
                    );
                }
            }
        }

        // Surface a deferred write error after the summary.
        if let Some(err) = write_error {
            return Err(err);
        }

        // APIN1: stdout = offenses on the NOW-ON-DISK source (post-fix lint).
        // Implemented by re-running `lint_files_memoized` on the same paths —
        // this IS the same call as a plain lint, so the invariant holds by
        // construction.
        let offenses = lint_files_memoized_debug(
            &files,
            &registry,
            &mruby_cops,
            &config,
            debug,
            &mut profile_summary,
        )?;

        if profile {
            write_profile_output(&profile_summary, profile_format)?;
        } else {
            write_lint_output(&offenses, &files, output_format)?;
        }

        return Ok(if offenses.is_empty() {
            EXIT_OK
        } else {
            EXIT_OFFENSES
        });
    }

    // ── Non-fix path (default): BYTE-IDENTICAL to pre-.6 behavior ──────────
    //
    // When NEITHER --fix NOR --debug is in effect, the pipeline is unchanged:
    // same `lint_files_memoized` call, same stdout, same exit codes.
    // This preserves ADR 0006/0007 frozen contract (integration_snapshot /
    // parallel_determinism unchanged).
    //
    // --debug without --fix: no fixpoint runs, but lint progress/timing goes to
    // stderr. stdout remains the JSON array.
    // Read + parse + lint + aggregate every file, with in-run content
    // memoization (Task 7): byte-identical content is parsed/linted ONCE and
    // fanned out per path. Parallelism (read and per-unique-content lint),
    // abort-on-first-read-Err → exit 2, and `aggregate` as the single
    // determinism point are all preserved inside `lint_files_memoized`; the
    // no-duplicate case is a 1:1 no-op so the snapshot stays byte-identical.
    // `tests/parallel_determinism.rs` is the permanent byte-identity guard.
    let offenses = lint_files_memoized_debug(
        &files,
        &registry,
        &mruby_cops,
        &config,
        debug,
        &mut profile_summary,
    )?;

    if profile {
        write_profile_output(&profile_summary, profile_format)?;
    } else {
        write_lint_output(&offenses, &files, output_format)?;
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
        std::fs::write(&dup_a, "# frozen_string_literal: true\n\nputs 'x'\n")
            .expect("write dup_a.rb");
        std::fs::write(&dup_b, "# frozen_string_literal: true\n\nputs 'x'\n")
            .expect("write dup_b.rb");
        std::fs::write(&other, "# frozen_string_literal: true\n\nputs 'y'\n")
            .expect("write other.rb");

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
        // No user cops here: this test pins the NATIVE memo (parse-count +
        // fan-out), so the mruby slice is empty (the mruby pass is then a
        // strict no-op and does not perturb `PARSE_CALLS`, which only
        // `lint_source` increments).
        let config = MurphyConfig::default();
        let offenses = expect_ok(lint_files_memoized(&files, &registry, &[], &config));

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
        std::fs::write(&nd_a, "# frozen_string_literal: true\n\nx = 1\n").expect("write a.rb");
        std::fs::write(&nd_b, "# frozen_string_literal: true\n\ny = 2\n").expect("write b.rb");

        let files2: Vec<String> = vec![
            nd_a.to_str().unwrap().to_owned(),
            nd_b.to_str().unwrap().to_owned(),
        ];

        PARSE_CALLS.store(0, Ordering::Relaxed);
        let offenses2 = expect_ok(lint_files_memoized(&files2, &registry, &[], &config));

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
        let config = MurphyConfig::default();
        let err = lint_files_memoized(&files, &registry, &[], &config)
            .expect_err("missing file must abort");
        assert_eq!(err.code, EXIT_SETUP_ERROR, "missing file → exit 2");
    }
}
