//! `murphy` command-line entry point.
//!
//! Post-reboot (murphy-9cr.22): dispatch is over an arena AST through the
//! single `PluginCopV1` surface (ADR 0038). `parse(source, file)` returns
//! an owned `murphy_ast::Ast`; `dispatch::run_cops` walks it once and
//! routes matched nodes to every registered cop. The CLI's lint
//! pipeline, fixpoint loop, autocorrect write-back, and inline-directive
//! handling are unchanged in observable contract; only the engine
//! underneath has been replaced.
//!
//! Sub-commands:
//!
//! - `murphy lint [flags] [paths]…` — the main lint loop.
//! - `murphy migrate <.rubocop.yml>` — one-way config migration.
//! - `murphy lsp` — JSON-RPC LSP server (see `lsp.rs`).
//!
//! `murphy ast --format sexp` was dropped in murphy-9cr.22 — the
//! prism-based S-expression printer (`ast_sexp.rs`) is gone with the
//! legacy surface. A new arena S-expression printer is a follow-up
//! issue; the design (§3) explicitly accepts the temporary UX
//! degradation. `murphy lint --profile / --profile-format` likewise
//! await re-introduction once the new dispatcher carries its own per-cop
//! timing path (.22 perf-gate follow-up).

mod lsp;

use murphy_core::{
    CopRegistry, FixpointStatus, MurphyConfig, Offense, SYNTAX_COP_NAME, Severity,
    aggregate_with_config, discover_with_config, dispatch, migrate_rubocop_yml_to_murphy_toml,
    parse, run_to_fixpoint,
};
use murphy_plugin_api::PluginCopV1;
use murphy_reporting::{OutputFormat, format_lint_output};
use rayon::prelude::*;
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

/// Exit code: clean — zero offenses.
const EXIT_OK: u8 = 0;
/// Exit code: lint found one or more offenses.
const EXIT_OFFENSES: u8 = 1;
/// Exit code: config/cop/file-setup error.
const EXIT_SETUP_ERROR: u8 = 2;
/// Exit code: internal failure (a caught panic).
const EXIT_INTERNAL: u8 = 3;

/// Maximum autocorrect fixpoint iterations per file.
const MAX_FIX_ITERATIONS: u32 = 10;

/// Global monotonic counter for unique sibling-temp filenames.
static FIX_TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

const LINT_USAGE: &str =
    "murphy lint [--fix|-a] [--debug] [--format human|json|progress] [--] [path]...";

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
    let args: Vec<String> = std::env::args().collect();
    let outcome = catch_unwind(AssertUnwindSafe(|| run(&args)));
    let code = match outcome {
        Ok(Ok(code)) => code,
        Ok(Err(err)) => {
            let _ = writeln!(std::io::stderr(), "murphy: {}", err.message);
            err.code
        }
        Err(_panic) => {
            let _ = writeln!(
                std::io::stderr(),
                "murphy: internal failure (panic). \
                 Please file an issue."
            );
            EXIT_INTERNAL
        }
    };
    ExitCode::from(code)
}

fn read_source(path: &str) -> Result<String, AppError> {
    std::fs::read_to_string(Path::new(path))
        .map_err(|e| AppError::setup(format!("cannot read {path:?}: {e}")))
}

/// Read a bounded batch in parallel with a cancellation token. Returns
/// `Err` on the first setup error (the remaining workers stop ASAP).
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
        handle.join().expect("failed to join read worker thread");
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
        .collect::<Vec<_>>();
    // Restore input order. Workers race; results' order is arbitrary.
    let index_by_path: BTreeMap<&str, usize> = paths
        .iter()
        .enumerate()
        .map(|(i, p)| (p.as_str(), i))
        .collect();
    source_paths.sort_by_key(|(p, _)| index_by_path.get(p.as_str()).copied().unwrap_or(usize::MAX));
    Ok(source_paths)
}

/// Run every cop in `cops` over `source` (parsed for the given `file`),
/// applying inline-directive filtering. Syntax errors degrade to a single
/// `Murphy/Syntax` offense; cops are skipped on a parse failure.
fn lint_source(source: &str, file: &str, cops: &[&'static PluginCopV1]) -> Vec<Offense> {
    let mut offenses = match parse(source, file) {
        Ok(ast) => {
            let mut sink = dispatch::OffenseSink::new(file);
            dispatch::run_cops(&ast, cops, &mut sink);
            sink.into_offenses()
        }
        Err(err) => vec![Offense::new(
            file,
            SYNTAX_COP_NAME,
            err.range,
            Severity::Error,
            &err.message,
        )],
    };
    offenses = apply_inline_directive_filter(offenses, source);
    offenses
}

/// Per-file timed result used by `--debug` output. We measure parse +
/// dispatch totals only; per-cop timing requires a re-introduced timing
/// path that lands in a follow-up issue (.22 perf-gate follow-up).
struct TimedOffenses {
    offenses: Vec<Offense>,
    parse_micros: u128,
    cops_micros: u128,
}

fn lint_source_timed(source: &str, file: &str, cops: &[&'static PluginCopV1]) -> TimedOffenses {
    let parse_started = Instant::now();
    let parsed = parse(source, file);
    let parse_micros = parse_started.elapsed().as_micros();
    let cops_started = Instant::now();
    let offenses = match parsed {
        Ok(ast) => {
            let mut sink = dispatch::OffenseSink::new(file);
            dispatch::run_cops(&ast, cops, &mut sink);
            sink.into_offenses()
        }
        Err(err) => vec![Offense::new(
            file,
            SYNTAX_COP_NAME,
            err.range,
            Severity::Error,
            &err.message,
        )],
    };
    let cops_micros = cops_started.elapsed().as_micros();
    TimedOffenses {
        offenses: apply_inline_directive_filter(offenses, source),
        parse_micros,
        cops_micros,
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
                (InlineDirectiveKind::Disable, None) => disable_all = true,
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
                (InlineDirectiveKind::Todo, None) => todo_all = true,
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

/// Write `corrected` to `target` atomically using a sibling-temp + rename.
///
/// Resolves symlinks (`canonicalize`), preserves the real file's mode,
/// writes a sibling temp `.murphy-fix-<pid>-<N>.tmp` in the real file's
/// directory, sets permissions, then renames over the real path — no
/// truncation window. On any error, best-effort temp cleanup.
fn write_back_atomic(target: &Path, corrected: &str) -> Result<(), AppError> {
    let real = std::fs::canonicalize(target).map_err(|e| {
        AppError::setup(format!(
            "cannot resolve {} for --fix: {e}",
            target.display()
        ))
    })?;
    let perms = std::fs::metadata(&real)
        .map_err(|e| AppError::setup(format!("cannot stat {} for --fix: {e}", real.display())))?
        .permissions();
    let parent = real.parent().unwrap_or_else(|| Path::new("."));
    let counter = FIX_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let tmp_name = format!(".murphy-fix-{pid}-{counter}.tmp");
    let tmp_path = parent.join(&tmp_name);
    if let Err(e) = std::fs::write(&tmp_path, corrected) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(AppError::setup(format!(
            "cannot write temp file {}: {e}",
            tmp_path.display()
        )));
    }
    if let Err(e) = std::fs::set_permissions(&tmp_path, perms) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(AppError::setup(format!(
            "cannot set permissions on temp file {}: {e}",
            tmp_path.display()
        )));
    }
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

/// Build the lint closure for `run_to_fixpoint`: parse, dispatch, aggregate,
/// then collect every `autocorrect.edits` into one flat `Vec<Edit>`.
fn lint_closure_edits<'a>(
    source: &str,
    file: &'a str,
    cops: &'a [&'static PluginCopV1],
    config: &'a MurphyConfig,
) -> Vec<murphy_core::Edit> {
    let offenses = lint_source(source, file, cops);
    aggregate_with_config(offenses, config)
        .into_iter()
        .filter_map(|o| o.autocorrect.map(|ac| ac.edits))
        .flatten()
        .collect()
}

struct FileDebugInfo {
    path: String,
    iterations: u32,
    status: FixpointStatus,
}

/// Memoized lint over a batch of files. Identical source content is
/// linted exactly once; results are fanned out per path with `Offense.file`
/// rewritten to each contributor path (preserves ADR 0007 determinism).
fn lint_files_memoized(
    sources: &[(String, String)],
    cops: &[&'static PluginCopV1],
) -> Vec<Offense> {
    // Group paths by content so identical-content files share one lint.
    let mut groups: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (path, content) in sources {
        groups.entry(content.as_str()).or_default().push(path.as_str());
    }

    let groups_vec: Vec<(&str, Vec<&str>)> = groups.into_iter().collect();
    let mut out: Vec<Offense> = groups_vec
        .par_iter()
        .flat_map_iter(|(content, paths)| {
            // Lint once against the representative path. Then for every
            // additional path sharing the same content, clone the offense
            // list with `file` rewritten.
            let representative = paths[0];
            let base = lint_source(content, representative, cops);
            let mut all: Vec<Offense> = Vec::with_capacity(base.len() * paths.len());
            all.extend(base.iter().cloned());
            for &other in &paths[1..] {
                for o in &base {
                    let mut cloned = o.clone();
                    cloned.file = other.to_string();
                    all.push(cloned);
                }
            }
            all
        })
        .collect();
    // Per-thread order is non-deterministic; aggregator restores
    // determinism by its content-based sort. No sort needed here.
    out.shrink_to_fit();
    out
}

fn lint_files_memoized_debug(
    sources: &[(String, String)],
    cops: &[&'static PluginCopV1],
) -> (Vec<Offense>, Vec<(String, u128, u128)>) {
    // Debug variant: keep per-file (parse, cops) timings. No memoization
    // across content — `--debug` is for developer visibility, the cost
    // is acceptable.
    let mut all: Vec<Offense> = Vec::new();
    let mut timings: Vec<(String, u128, u128)> = Vec::new();
    for (path, content) in sources {
        let t = lint_source_timed(content, path, cops);
        timings.push((path.clone(), t.parse_micros, t.cops_micros));
        all.extend(t.offenses);
    }
    (all, timings)
}

fn run(args: &[String]) -> Result<u8, AppError> {
    let rest = args.get(1..).unwrap_or(&[]);
    let (subcommand, post_subcommand) = match rest {
        [subcommand, rest @ ..] => (subcommand.as_str(), rest),
        [] => {
            return Err(AppError::setup(
                "usage: murphy lint [flags] [path]... | \
                 murphy migrate <.rubocop.yml> | murphy lsp",
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

    if subcommand != "lint" {
        return Err(AppError::setup(format!(
            "unknown subcommand {subcommand:?} (usage: {LINT_USAGE} | \
             murphy migrate <.rubocop.yml> | murphy lsp)"
        )));
    }

    // ── flag extraction ────────────────────────────────────────────────────
    let mut fix = false;
    let mut debug = false;
    let mut output_format = OutputFormat::Human;
    let mut path_args: Vec<&str> = Vec::new();
    let mut flags_done = false;
    let mut pending_format = false;

    for token in post_subcommand {
        if flags_done {
            path_args.push(token.as_str());
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
            "--format" => pending_format = true,
            flag if flag.starts_with('-') => {
                return Err(AppError::setup(format!(
                    "unknown flag {flag:?} (usage: {LINT_USAGE}; use `--` before a path starting with `-`)"
                )));
            }
            path => path_args.push(path),
        }
    }
    if pending_format {
        return Err(AppError::setup(
            "missing value for --format (supported: human, json, progress)",
        ));
    }

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
            "murphy: debug: cop registry load done elapsed_ms={}, packs={:?}",
            run_started.elapsed().as_millis(),
            registry.pack_names()
        );
    }

    // ── path classification ────────────────────────────────────────────────
    let mut explicit_files: Vec<String> = Vec::new();
    let mut discover_roots: Vec<PathBuf> = Vec::new();
    if path_args.is_empty() {
        discover_roots.push(PathBuf::from("."));
    } else {
        for arg in &path_args {
            let p = Path::new(arg);
            if p.is_dir() {
                discover_roots.push(p.to_path_buf());
            } else {
                // existing file, missing file, or symlink — read_source handles
                explicit_files.push((*arg).to_string());
            }
        }
    }
    if debug {
        eprintln!(
            "murphy: debug: path classification: explicit_files={}, discover_roots={}, elapsed_ms={}",
            explicit_files.len(),
            discover_roots.len(),
            run_started.elapsed().as_millis()
        );
    }

    let mut all_paths: BTreeSet<String> = explicit_files.iter().cloned().collect();
    for root in &discover_roots {
        if debug {
            eprintln!(
                "murphy: debug: discover start root={:?} elapsed_ms={}",
                root,
                run_started.elapsed().as_millis()
            );
        }
        let discovered = if root == Path::new(".") {
            discover_with_config(root, &config).map_err(|e| AppError::setup(e.to_string()))?
        } else {
            // For non-cwd roots, load the root-local murphy.toml.
            let local_config =
                MurphyConfig::load(root).map_err(|e| AppError::setup(e.to_string()))?;
            discover_with_config(root, &local_config)
                .map_err(|e| AppError::setup(e.to_string()))?
        };
        for p in discovered {
            all_paths.insert(p.to_string_lossy().into_owned());
        }
        if debug {
            eprintln!(
                "murphy: debug: discover done root={:?} found={} elapsed_ms={}",
                root,
                all_paths.len(),
                run_started.elapsed().as_millis()
            );
        }
    }

    let paths: Vec<String> = all_paths.into_iter().collect();
    if debug {
        eprintln!(
            "murphy: debug: read batch start files={} elapsed_ms={}",
            paths.len(),
            run_started.elapsed().as_millis()
        );
    }
    let worker_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let sources = read_batch_sources(&paths, worker_count)?;
    if debug {
        eprintln!(
            "murphy: debug: read batch done files={} elapsed_ms={}",
            sources.len(),
            run_started.elapsed().as_millis()
        );
    }

    let cops = registry.cops();

    let mut fix_debug: Vec<FileDebugInfo> = Vec::new();
    let mut sources_for_lint = sources;

    // ── --fix: fixpoint autocorrect + write-back ───────────────────────────
    if fix {
        if debug {
            eprintln!(
                "murphy: debug: fixpoint start elapsed_ms={}",
                run_started.elapsed().as_millis()
            );
        }
        let mut next_sources: Vec<(String, String)> = Vec::with_capacity(sources_for_lint.len());
        for (path, source) in &sources_for_lint {
            let outcome = run_to_fixpoint(
                source,
                |s| lint_closure_edits(s, path, cops, &config),
                MAX_FIX_ITERATIONS,
            );
            if outcome.corrected != *source {
                write_back_atomic(Path::new(path), &outcome.corrected)?;
            }
            if debug {
                fix_debug.push(FileDebugInfo {
                    path: path.clone(),
                    iterations: outcome.iterations,
                    status: outcome.status,
                });
            }
            next_sources.push((path.clone(), outcome.corrected));
        }
        sources_for_lint = next_sources;
        if debug {
            eprintln!(
                "murphy: debug: fixpoint done elapsed_ms={}",
                run_started.elapsed().as_millis()
            );
            for info in &fix_debug {
                eprintln!(
                    "murphy: debug: fix {} iterations={} status={:?}",
                    info.path, info.iterations, info.status
                );
            }
        }
    }

    if debug {
        eprintln!(
            "murphy: debug: lint pass start files={} elapsed_ms={}",
            sources_for_lint.len(),
            run_started.elapsed().as_millis()
        );
    }
    let flat_offenses: Vec<Offense> = if debug {
        let (offenses, timings) = lint_files_memoized_debug(&sources_for_lint, cops);
        for (path, parse_us, cops_us) in &timings {
            eprintln!(
                "murphy: debug: lint {} parse_us={} cops_us={}",
                path, parse_us, cops_us
            );
        }
        offenses
    } else {
        lint_files_memoized(&sources_for_lint, cops)
    };
    let offenses = aggregate_with_config(flat_offenses, &config);
    if debug {
        eprintln!(
            "murphy: debug: lint pass done offenses={} elapsed_ms={}",
            offenses.len(),
            run_started.elapsed().as_millis()
        );
    }

    let exit = if offenses.is_empty() {
        EXIT_OK
    } else {
        EXIT_OFFENSES
    };

    let file_paths: Vec<String> = sources_for_lint.iter().map(|(p, _)| p.clone()).collect();
    let formatted =
        format_lint_output(&offenses, &file_paths, output_format).map_err(AppError::setup)?;
    let mut stdout = std::io::stdout().lock();
    if let Err(e) = writeln!(stdout, "{formatted}") {
        if e.kind() == std::io::ErrorKind::BrokenPipe {
            return Ok(exit);
        }
        return Err(AppError::setup(format!("failed to write stdout: {e}")));
    }

    Ok(exit)
}
