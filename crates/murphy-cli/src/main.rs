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
    AstContext, Cop, CopRegistry, Offense, SYNTAX_COP_NAME, Severity, aggregate, discover, parse,
    run_cops, run_mruby_cop_isolated,
};
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::thread;

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
fn load_mruby_cops(registry: &CopRegistry) -> Result<Vec<MrubyCop>, AppError> {
    // RESERVED = engine-owned names a user cop must not shadow. Derived from
    // the live registry (every native cop's own `name()`) + the synthetic
    // syntax-offense name — NOT hardcoded, so adding a native cop automatically
    // extends the reserved set with zero drift.
    let reserved: BTreeSet<&str> = registry
        .native_cops()
        .iter()
        .map(|c| c.name())
        .chain(std::iter::once(SYNTAX_COP_NAME))
        .collect();

    registry
        .mruby_cop_paths()
        .iter()
        .map(|p| {
            let cop_name = mruby_cop_name(p);
            if reserved.contains(cop_name.as_str()) {
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
            Ok(MrubyCop { cop_name, source })
        })
        .collect()
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
                        let should_capture = {
                            let mut lock = first_error.lock().expect("first error lock poisoned");
                            let was_set = lock.is_some();
                            if lock.is_none() {
                                *lock = Some(err);
                            }
                            was_set
                        };
                        if !should_capture {
                            cancel.store(true, Ordering::Release);
                            return;
                        }
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
fn lint_source_mruby(source: &str, file: &str, mruby_cops: &[MrubyCop]) -> Vec<Offense> {
    // I-2 (deferred to Phase 4, tracked): this `parse(source).is_err()` then
    // `AstContext::new` (which parses again) double-parses unique cop'd content.
    // It is NOT collapsed into a single `ctx.parse_result().errors()` check
    // because `crate::parse::parse` ALSO applies the `exceeds_offset_domain`
    // u32 byte-offset guard up front, which `ParseResult::errors()` does not
    // subsume — collapsing would silently drop that guard, so it is not
    // provably byte-identical (murphy-cql hard gate: any doubt ⇒ defer).
    if mruby_cops.is_empty() || parse(source).is_err() {
        return Vec::new();
    }
    let ctx: Arc<AstContext> = AstContext::new(source.as_bytes().to_vec());
    let mut sink: Vec<Offense> = Vec::new();
    for cop in mruby_cops {
        sink.extend(run_mruby_cop_isolated(
            &ctx,
            &cop.source,
            &cop.cop_name,
            file,
        ));
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
/// 2. **Lint phase.** Group and lint in bounded-size batches to avoid
///    holding every discovered file body simultaneously: read a batch, dedupe by
///    content (exact equality via `String` key), lint only newly seen batch
///    contents in parallel, and keep parsed base offenses for fan-out.
///    `aggregate` remains the single determinism point.
///
/// The flattened per-path offenses go to `aggregate` UNCHANGED — it remains
/// the single determinism point (the total-order sort/dedupe, Task 2), so
/// neither read/lint parallelism nor the fan-out order can perturb output.
fn lint_files_memoized(
    files: &[String],
    registry: &CopRegistry,
    mruby_cops: &[MrubyCop],
) -> Result<Vec<Offense>, AppError> {
    #[derive(Default)]
    struct ContentGroup {
        representative: String,
        paths: Vec<String>,
        base_offenses: Vec<Offense>,
    }

    const BATCH_SIZE: usize = 128;

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
    for chunk in files.chunks(BATCH_SIZE) {
        let read: Vec<(String, String)> = read_batch_sources(chunk, worker_count)?;

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
                    });
                    newly_seen.push(content_key);
                }
            }
        }

        // Parse/lint each newly introduced content exactly once, in parallel.
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
                let mut base = lint_source(content, representative, registry.native_cops());
                base.extend(lint_source_mruby(content, representative, mruby_cops));
                (content.clone(), base)
            })
            .collect::<Vec<(String, Vec<Offense>)>>();

        for (content, base_offenses) in parsed {
            by_content
                .get_mut(&content)
                .expect("parsed content must still exist")
                .base_offenses = base_offenses;
        }
    }

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

    // Load every enumerated `cops/*.rb` ONCE, up-front (P3 Task 7). Reading a
    // cop file the registry enumerated but cannot be read is a setup/config
    // error → exit 2 (ADR 0004 trusted-cops + discovery `ConfigError` → exit
    // 2), so it is surfaced HERE — before any linting and exactly once per
    // run, NOT once per (file × cop). An empty `cops/` (or none) → empty Vec,
    // so the native-only path (e.g. `sample_project`) is byte-identical: the
    // mruby pass is a no-op when there are no user cops.
    let mruby_cops = load_mruby_cops(&registry)?;

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
    let offenses = lint_files_memoized(&files, &registry, &mruby_cops)?;

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
        // No user cops here: this test pins the NATIVE memo (parse-count +
        // fan-out), so the mruby slice is empty (the mruby pass is then a
        // strict no-op and does not perturb `PARSE_CALLS`, which only
        // `lint_source` increments).
        let offenses = expect_ok(lint_files_memoized(&files, &registry, &[]));

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
        let offenses2 = expect_ok(lint_files_memoized(&files2, &registry, &[]));

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
        let err = lint_files_memoized(&files, &registry, &[]).expect_err("missing file must abort");
        assert_eq!(err.code, EXIT_SETUP_ERROR, "missing file → exit 2");
    }
}
